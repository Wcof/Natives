//! natives-app-runtime：统一官方内置应用运行时（ADR-0031）。
//!
//! 职责：Native Messaging framing、Origin/协议/激活校验、编译期模块注册、
//! Runtime 状态机（单次 appId 绑定）、事务式 Start、有界 loopback HTTP、
//! 会话鉴权、协作式取消与确定性关闭。
//! 禁止承担任何具体模块（Fund 等）的业务逻辑。

use app_runtime_core::cancellation::CancellationToken;
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::framing::{read_frame, write_frame};
use app_runtime_core::http::{write_preflight, write_response as write_http_response, HttpRequest};
use app_runtime_core::lock::{acquire_runtime, RuntimeLease, RuntimeUnavailable};
#[cfg(feature = "test-fixture")]
use app_runtime_core::module::{AppHealth, ModuleDescriptor, ModuleHttpResponse};
use app_runtime_core::module::{BuiltInAppModule, ModuleContext, ModuleRegistry};
use app_runtime_core::origin::chrome_extension_origin;
use app_runtime_core::protocol::{
    AppRequest, AppResponse, HandshakeParamsV2, HandshakeResultV2, StartResult, StatusResult,
    APP_PROTOCOL_VERSION_V2, MAX_FRAME_BYTES,
};
use app_runtime_core::session::{random_id, SessionManager};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Runtime 状态机（计划 §10）：合法迁移
//   BOOTING → handshake → BOUND → initialize(同握手) → INITIALIZED
//           → start(事务式) → READY → stop/EOF → STOPPING → STOPPED
// 其余迁移一律拒绝（APP_RUNTIME_* 错误码）。
// ---------------------------------------------------------------------------

/// 已绑定 appId 的运行时（握手成功，模块已 initialize）。
struct BoundRuntime {
    app_id: String,
    instance_id: String,
    module: Arc<Mutex<Box<dyn BuiltInAppModule>>>,
    _lease: RuntimeLease,
}

/// 就绪运行时：会话管理器与 loopback 监听已提交（端口经 LOOPBACK_PORT 共享）。
struct ReadyRuntime {
    bound: BoundRuntime,
    sessions: Arc<Mutex<SessionManager>>,
}

enum ControllerState {
    Booting,
    Bound(BoundRuntime),
    Ready(ReadyRuntime),
    Stopping,
    Stopped,
}

struct Controller {
    /// 关闭取消信号：注入 ModuleContext，业务长任务必须轮询。
    cancellation: Arc<CancellationToken>,
    /// loopback 服务停止旗标（关闭 accept 循环）。
    server_shutdown: Arc<AtomicBool>,
    /// 主循环是否应退出（stop 请求或启动失败后）。
    stop_requested: AtomicBool,
}

impl Controller {
    fn new() -> Self {
        Self {
            cancellation: Arc::new(CancellationToken::new()),
            server_shutdown: Arc::new(AtomicBool::new(false)),
            stop_requested: AtomicBool::new(false),
        }
    }
}

static LOOPBACK_PORT: AtomicU16 = AtomicU16::new(0);
/// 当前处理中的 HTTP 请求数（有界并发硬门）。
static ACTIVE_HTTP_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static HTTP_LIMITS: app_runtime_core::limits::RuntimeLimits =
    app_runtime_core::limits::RuntimeLimits::hard_gate();

/// 全局 Runtime 状态（进程级单实例；HTTP 线程与主循环共享同一状态机）。
static CONTROLLER_STATE: Mutex<ControllerState> = Mutex::new(ControllerState::Booting);

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn write_json_response<T: serde::Serialize>(
    id: &str,
    ok: bool,
    result: Option<T>,
    error: Option<AppErrorBody>,
) {
    let body = AppResponse {
        id: id.to_string(),
        ok,
        result,
        error,
    };
    match serde_json::to_vec(&body) {
        Ok(bytes) if bytes.len() <= MAX_FRAME_BYTES => {
            if let Err(err) = write_frame(&mut std::io::stdout().lock(), &bytes) {
                eprintln!("natives-app-runtime: write frame failed: {err:?}");
                std::process::exit(1);
            }
        }
        Ok(_) => {
            eprintln!("natives-app-runtime: response exceeds frame budget");
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("natives-app-runtime: serialize response failed: {err}");
            std::process::exit(1);
        }
    }
}

use app_runtime_core::error::AppErrorBody;

fn write_error(id: &str, code: AppErrorCode, message: impl Into<String>, retryable: bool) {
    write_json_response::<serde_json::Value>(
        id,
        false,
        None,
        Some(AppErrorBody::new(code, message, retryable)),
    );
}

fn dirs_home() -> PathBuf {
    std::env::var("NATIVES_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(PathBuf::from))
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn apps_root() -> PathBuf {
    std::env::var("NATIVES_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // 与 Core 的 natives_dir_name()（app_signing.rs）保持一致：
            // debug 构建（本地候选）用 ~/.natives-local，否则 ~/.natives。
            let dir = if cfg!(debug_assertions) {
                ".natives-local"
            } else {
                ".natives"
            };
            dirs_home().join(dir).join("apps")
        })
}

fn create_module_registry() -> ModuleRegistry {
    let mut registry = ModuleRegistry::new();
    register_builtin_modules(&mut registry);
    registry
}

/// 模块静态注入入口：只注册真实官方模块（modules/registry.json，ADR-0031）。
/// test-fixture feature（计划 §44）仅用于 CI 验证多模块架构，禁止进入 Release Runtime。
fn register_builtin_modules(registry: &mut ModuleRegistry) {
    registry.register(
        "fund",
        Box::new(|| Box::new(fund_module::FundModule::new())),
    );
    registry.register(
        "tokenusage",
        Box::new(|| Box::new(tokenusage_module::TokenUsageModule::new())),
    );
    #[cfg(feature = "test-fixture")]
    registry.register("fixture-second", Box::new(|| Box::new(SecondFixtureModule)));
}

/// 计划 §44 test-only 第二模块：证明架构不是"只对 Fund 有效"——
/// 两个模块可同时在 Registry、独立进程绑定、独立数据根、独立关闭。
#[cfg(feature = "test-fixture")]
struct SecondFixtureModule;

#[cfg(feature = "test-fixture")]
impl BuiltInAppModule for SecondFixtureModule {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            app_id: "fixture-second".into(),
            display_name: "Fixture Second".into(),
            module_api_version: 1,
            data_schema_version: 1,
            capability_version: 1,
        }
    }
    fn initialize(&mut self, _ctx: &ModuleContext) -> Result<(), String> {
        Ok(())
    }
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn handle_ui(&self, path: &str) -> Result<Option<ModuleHttpResponse>, String> {
        if path == "/" || path == "/index.html" {
            Ok(Some(ModuleHttpResponse::ok_html("<h1>fixture</h1>")))
        } else {
            Ok(None)
        }
    }
    fn handle_api(
        &self,
        _req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        if route == "/api/ping" {
            Ok(ModuleHttpResponse::ok_json("{\"pong\":true}"))
        } else {
            Err((404, "not found".into()))
        }
    }
    fn data_status(&self) -> Result<app_runtime_core::protocol::DataStatusResult, String> {
        Ok(app_runtime_core::protocol::DataStatusResult {
            current_schema: 1,
            migration_state: "ready".into(),
            last_data_writer_version: "fixture".into(),
            has_committed_new_writes: false,
            previous_version_compatible: true,
        })
    }
    fn health(&self) -> Result<AppHealth, String> {
        Ok(AppHealth {
            ok: true,
            detail: "fixture-second ready".into(),
        })
    }
    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let registry = create_module_registry();

    if args.iter().any(|a| a == "--health") {
        let modules = registry.registered_app_ids();
        println!(
            "{{\"ok\":true,\"runtime\":\"natives-app-runtime\",\"protocolVersion\":{},\"modules\":{}}}",
            APP_PROTOCOL_VERSION_V2,
            serde_json::to_string(&modules).unwrap_or_else(|_| "[]".into())
        );
        return;
    }

    // 校验 Chrome Native Messaging 调用的 origin
    let origin = match chrome_extension_origin(&args) {
        Ok(origin) => origin,
        Err(error) => {
            eprintln!(
                "natives-app-runtime: origin check failed: {}",
                error.message
            );
            std::process::exit(2);
        }
    };

    let apps_root = apps_root();
    let controller = Arc::new(Controller::new());
    let mut stdin = std::io::stdin().lock();

    while !controller.stop_requested.load(Ordering::SeqCst) {
        let buffer = match read_frame(&mut stdin) {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => break, // EOF 或读取异常：进入标准退出路径
        };

        let Ok(req) = serde_json::from_slice::<AppRequest>(&buffer) else {
            continue;
        };

        controller.handle_request(&req, &origin, &apps_root, &registry);
    }

    // 统一退出回收：2 秒内释放全部资源
    controller.shutdown();
    std::process::exit(0);
}

impl Controller {
    fn handle_request(
        &self,
        req: &AppRequest,
        origin: &str,
        apps_root: &Path,
        registry: &ModuleRegistry,
    ) {
        match req.method.as_str() {
            "app:handshake" => self.handle_handshake(req, origin, apps_root, registry),
            "app:start" => self.handle_start(req),
            "app:status" => self.handle_status(req),
            "app:data_status" => self.handle_data_status(req),
            "app:session" => self.handle_session(req),
            "app:stop" => self.handle_stop(req),
            _ => write_error(
                &req.id,
                AppErrorCode::ProtocolMismatch,
                format!("unknown method '{}'", req.method),
                false,
            ),
        }
    }

    /// 握手：一个 Runtime Process 只能绑定一次 appId（计划 §11）。
    /// 校验顺序（计划 §34.1）：origin → 协议版本 → 激活 → registry → 绑定 → initialize。
    fn handle_handshake(
        &self,
        req: &AppRequest,
        origin: &str,
        apps_root: &Path,
        registry: &ModuleRegistry,
    ) {
        let params: HandshakeParamsV2 = match serde_json::from_value(req.params.clone()) {
            Ok(p) => p,
            Err(e) => {
                write_error(
                    &req.id,
                    AppErrorCode::ProtocolMismatch,
                    format!("invalid handshake params: {e}"),
                    false,
                );
                return;
            }
        };

        if params.protocol_version != APP_PROTOCOL_VERSION_V2 {
            write_error(
                &req.id,
                AppErrorCode::ProtocolMismatch,
                format!(
                    "expected protocol version {APP_PROTOCOL_VERSION_V2}, got {}",
                    params.protocol_version
                ),
                false,
            );
            return;
        }

        let app_id = &params.app_id;
        if !registry.contains(app_id) {
            write_error(
                &req.id,
                AppErrorCode::ModuleNotFound,
                format!("module '{app_id}' not registered in app-runtime"),
                false,
            );
            return;
        }

        // 状态机预检（计划 §11/§41）：本进程已绑定（或正在停止/已停止）时，
        // 重复握手一律 APP_RUNTIME_*，不得先去抢运行锁返回误导性错误。
        {
            let state = CONTROLLER_STATE.lock().unwrap();
            let code = match &*state {
                ControllerState::Bound(_) | ControllerState::Ready(_) => {
                    Some(AppErrorCode::AlreadyBound)
                }
                ControllerState::Stopping => Some(AppErrorCode::Stopping),
                ControllerState::Stopped => Some(AppErrorCode::Stopped),
                ControllerState::Booting => None,
            };
            if let Some(code) = code {
                drop(state);
                write_error(
                    &req.id,
                    code,
                    format!("runtime already bound to another handshake (app '{app_id}')"),
                    false,
                );
                return;
            }
        }

        // 校验激活投影
        let product_version = params.product_version.as_deref().unwrap_or("0.1.0");
        let activation = match app_runtime_core::verify_activation_with_generation(
            apps_root,
            app_id,
            product_version,
            Some(origin),
            params.activation_generation,
        ) {
            Ok(act) => act,
            Err((code, msg)) => {
                let retryable = code == AppErrorCode::Busy;
                write_error(&req.id, code, msg, retryable);
                return;
            }
        };

        // 获取运行时独占锁
        let lease = match acquire_runtime(apps_root, app_id) {
            Ok(Ok(lease)) => lease,
            Ok(Err(RuntimeUnavailable::Busy)) => {
                write_error(&req.id, AppErrorCode::Busy, "应用正在安装或更新中", true);
                return;
            }
            Ok(Err(RuntimeUnavailable::AlreadyRunning)) => {
                write_error(
                    &req.id,
                    AppErrorCode::RunningElsewhere,
                    "应用正在另一会话中使用",
                    false,
                );
                return;
            }
            Ok(Err(RuntimeUnavailable::Limit)) => {
                write_error(
                    &req.id,
                    AppErrorCode::RuntimeLimit,
                    "运行槽已满，请先停止其他应用",
                    false,
                );
                return;
            }
            Err(err) => {
                write_error(
                    &req.id,
                    AppErrorCode::StartFailed,
                    format!("获取运行锁失败: {err}"),
                    true,
                );
                return;
            }
        };

        // 实例化模块
        let Some(mut module) = registry.create(app_id) else {
            drop(lease);
            write_error(
                &req.id,
                AppErrorCode::ModuleNotFound,
                "failed to create module instance",
                false,
            );
            return;
        };

        let ctx = ModuleContext::for_app(apps_root, app_id, product_version, activation.generation);
        // 取消信号与运行时共享：关闭时 cancel() 直达模块长任务。
        let shared_ctx = ModuleContext {
            cancellation: self.cancellation.clone(),
            ..ctx
        };
        if let Err(err) = module.initialize(&shared_ctx) {
            drop(lease);
            write_error(
                &req.id,
                AppErrorCode::StartFailed,
                format!("module initialize failed: {err}"),
                false,
            );
            return;
        }

        let descriptor = module.descriptor();
        let Some(instance_id) = random_id() else {
            drop(lease);
            write_error(&req.id, AppErrorCode::StartFailed, "no OS randomness", true);
            return;
        };

        // 提交绑定：仅 Booting 允许；重复握手一律 APP_RUNTIME_ALREADY_BOUND。
        {
            let mut state = CONTROLLER_STATE.lock().unwrap();
            match &*state {
                ControllerState::Booting => {
                    *state = ControllerState::Bound(BoundRuntime {
                        app_id: app_id.to_string(),
                        instance_id: instance_id.clone(),
                        module: Arc::new(Mutex::new(module)),
                        _lease: lease,
                    });
                }
                ControllerState::Bound(_) | ControllerState::Ready(_) => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::AlreadyBound,
                        format!("runtime already bound to another handshake (app '{app_id}')"),
                        false,
                    );
                    return;
                }
                ControllerState::Stopping => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::Stopping,
                        "runtime is stopping",
                        false,
                    );
                    return;
                }
                ControllerState::Stopped => {
                    drop(state);
                    write_error(&req.id, AppErrorCode::Stopped, "runtime has stopped", false);
                    return;
                }
            }
        }

        let result = HandshakeResultV2 {
            protocol_version: APP_PROTOCOL_VERSION_V2,
            app_id: app_id.to_string(),
            state: "initialized".into(),
            module_api_version: descriptor.module_api_version,
            data_schema_version: descriptor.data_schema_version,
            capability_version: descriptor.capability_version,
        };
        write_json_response(&req.id, true, Some(result), None);
    }

    /// 事务式 Start（计划 §12）：prepare module → prepare session → prepare
    /// listener → commit Ready；任何一步失败回滚全部资源并进入 Stopping。
    fn handle_start(&self, req: &AppRequest) {
        // 1. 快照状态（短暂持锁：只做状态判定与 Arc clone，不在锁内执行业务）。
        let (module, instance_id) = {
            let state = CONTROLLER_STATE.lock().unwrap();
            match &*state {
                ControllerState::Booting => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::NotInitialized,
                        "runtime not bound; call app:handshake first",
                        false,
                    );
                    return;
                }
                ControllerState::Ready(_) => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::AlreadyStarted,
                        "runtime already started",
                        false,
                    );
                    return;
                }
                ControllerState::Stopping => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::Stopping,
                        "runtime is stopping",
                        false,
                    );
                    return;
                }
                ControllerState::Stopped => {
                    drop(state);
                    write_error(&req.id, AppErrorCode::Stopped, "runtime has stopped", false);
                    return;
                }
                ControllerState::Bound(bound) => (bound.module.clone(), bound.instance_id.clone()),
            }
        };

        // 2. prepare module（业务启动；失败 → 回滚为 Stopping）。
        {
            let mut module_guard = module.lock().unwrap();
            if let Err(err) = module_guard.start() {
                drop(module_guard);
                self.enter_stopping();
                write_error(
                    &req.id,
                    AppErrorCode::StartFailed,
                    format!("module start failed: {err}"),
                    false,
                );
                return;
            }
        }

        // 3. prepare session。
        let Some(sessions) = SessionManager::new() else {
            self.rollback_failed_start(&module);
            write_error(
                &req.id,
                AppErrorCode::StartFailed,
                "failed to initialize session manager",
                true,
            );
            return;
        };
        let generation = sessions.generation().to_string();
        let sessions = Arc::new(Mutex::new(sessions));

        // 4. prepare listener。ADR-0032：默认绑定固定端口 8765（可经
        // NATIVES_APP_RUNTIME_PORT 覆盖，设 0 强制动态端口）；被占用时回退动态端口。
        let listener = match bind_loopback_listener() {
            Ok(l) => l,
            Err(e) => {
                self.rollback_failed_start(&module);
                write_error(
                    &req.id,
                    AppErrorCode::StartFailed,
                    format!("failed to bind loopback: {e}"),
                    true,
                );
                return;
            }
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        LOOPBACK_PORT.store(port, Ordering::SeqCst);

        // 5. 全部成功 → commit Ready 并启动服务线程。
        {
            let mut state = CONTROLLER_STATE.lock().unwrap();
            if let ControllerState::Bound(bound) =
                std::mem::replace(&mut *state, ControllerState::Stopped)
            {
                *state = ControllerState::Ready(ReadyRuntime { bound, sessions });
            }
        }
        let shutdown = self.server_shutdown.clone();
        std::thread::spawn(move || serve_loopback_http(listener, shutdown));

        let result = StartResult {
            instance_id,
            port,
            generation,
            state: "ready".into(),
        };
        write_json_response(&req.id, true, Some(result), None);
    }

    /// Start 失败回滚：关闭模块并进入 Stopping（不留半启动 Runtime，计划 §12）。
    fn rollback_failed_start(&self, module: &Arc<Mutex<Box<dyn BuiltInAppModule>>>) {
        if let Ok(mut module_guard) = module.lock() {
            let _ = module_guard.shutdown();
        }
        self.enter_stopping();
    }

    fn enter_stopping(&self) {
        self.cancellation.cancel();
        self.server_shutdown.store(true, Ordering::SeqCst);
        // 计划 §18 关闭顺序：cancel → stop accept →（进入 STOPPING 拒绝新请求）
        // → revoke bearer → shutdown module → 释放 runtime lease → STOPPED。
        let taken = {
            let mut state = CONTROLLER_STATE.lock().unwrap();
            match *state {
                ControllerState::Booting | ControllerState::Stopping | ControllerState::Stopped => {
                    None
                }
                _ => Some(std::mem::replace(&mut *state, ControllerState::Stopping)),
            }
        };
        match taken {
            Some(ControllerState::Ready(ready)) => {
                eprintln!(
                    "natives-app-runtime: event=shutdown app_id={} phase=ready",
                    ready.bound.app_id
                );
                if let Ok(mut sessions) = ready.sessions.lock() {
                    sessions.revoke();
                }
                if let Ok(mut module) = ready.bound.module.lock() {
                    let _ = module.shutdown();
                }
            }
            Some(ControllerState::Bound(bound)) => {
                eprintln!(
                    "natives-app-runtime: event=shutdown app_id={} phase=bound",
                    bound.app_id
                );
                if let Ok(mut module) = bound.module.lock() {
                    let _ = module.shutdown();
                }
            }
            _ => {}
        }
        // 资源释放完成 → 最终 STOPPED（此后 handshake/start 均拒绝）。
        let mut state = CONTROLLER_STATE.lock().unwrap();
        if matches!(*state, ControllerState::Stopping) {
            *state = ControllerState::Stopped;
        }
    }

    fn handle_status(&self, req: &AppRequest) {
        let state = CONTROLLER_STATE.lock().unwrap();
        let (instance_id, runtime_state) = match &*state {
            ControllerState::Ready(ready) => (ready.bound.instance_id.clone(), "ready"),
            ControllerState::Bound(bound) => (bound.instance_id.clone(), "initialized"),
            ControllerState::Booting => {
                drop(state);
                write_error(
                    &req.id,
                    AppErrorCode::NotInitialized,
                    "no active instance",
                    false,
                );
                return;
            }
            ControllerState::Stopping => {
                drop(state);
                write_error(
                    &req.id,
                    AppErrorCode::Stopping,
                    "runtime is stopping",
                    false,
                );
                return;
            }
            ControllerState::Stopped => {
                drop(state);
                write_error(&req.id, AppErrorCode::Stopped, "runtime has stopped", false);
                return;
            }
        };
        drop(state);
        let result = StatusResult {
            instance_id,
            state: runtime_state.into(),
            operation: None,
        };
        write_json_response(&req.id, true, Some(result), None);
    }

    fn handle_data_status(&self, req: &AppRequest) {
        let module = {
            let state = CONTROLLER_STATE.lock().unwrap();
            match &*state {
                ControllerState::Bound(bound) => bound.module.clone(),
                ControllerState::Ready(ready) => ready.bound.module.clone(),
                _ => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::NotInitialized,
                        "no active instance",
                        false,
                    );
                    return;
                }
            }
        };
        let status = match module.lock() {
            Ok(module_guard) => module_guard.data_status(),
            Err(_) => Err("module lock poisoned".into()),
        };
        match status {
            Ok(status) => write_json_response(&req.id, true, Some(status), None),
            Err(err) => write_error(&req.id, AppErrorCode::StartFailed, err, false),
        }
    }

    fn handle_session(&self, req: &AppRequest) {
        let op = req.params.get("op").and_then(|v| v.as_str()).unwrap_or("");
        let challenge = req
            .params
            .get("challenge")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let sessions = {
            let state = CONTROLLER_STATE.lock().unwrap();
            match &*state {
                ControllerState::Ready(ready) => ready.sessions.clone(),
                _ => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::NotInitialized,
                        "no active session",
                        false,
                    );
                    return;
                }
            }
        };
        let mut sessions_guard = match sessions.lock() {
            Ok(guard) => guard,
            Err(_) => {
                write_error(
                    &req.id,
                    AppErrorCode::StartFailed,
                    "session lock poisoned",
                    false,
                );
                return;
            }
        };
        match op {
            "issue" => match sessions_guard.issue(challenge) {
                Ok(res) => write_json_response(
                    &req.id,
                    true,
                    Some(serde_json::json!({
                        "token": res.token,
                        "generation": res.generation,
                        "expiresAt": res.expires_at,
                    })),
                    None,
                ),
                Err(err) => {
                    write_json_response::<serde_json::Value>(&req.id, false, None, Some(err))
                }
            },
            "rotate" => match sessions_guard.rotate() {
                Ok(gen) => write_json_response(
                    &req.id,
                    true,
                    Some(serde_json::json!({
                        "generation": gen
                    })),
                    None,
                ),
                Err(err) => {
                    write_json_response::<serde_json::Value>(&req.id, false, None, Some(err))
                }
            },
            "revoke" => {
                sessions_guard.revoke();
                write_json_response(
                    &req.id,
                    true,
                    Some(serde_json::json!({"revoked": true})),
                    None,
                );
            }
            _ => write_error(
                &req.id,
                AppErrorCode::ProtocolMismatch,
                "unknown session op",
                false,
            ),
        }
    }

    fn handle_stop(&self, req: &AppRequest) {
        self.enter_stopping();
        self.stop_requested.store(true, Ordering::SeqCst);
        write_json_response(
            &req.id,
            true,
            Some(serde_json::json!({"stopped": true})),
            None,
        );
    }

    /// 确定性关闭（计划 §18）：cancel → stop accept → revoke → shutdown
    /// module → 释放锁；超时由进程退出兜底。
    fn shutdown(&self) {
        self.enter_stopping();
        self.stop_requested.store(true, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// 有界 loopback HTTP 服务（计划 §17）：Nonblocking-ish accept 循环 +
// 并发硬门（ACTIVE_HTTP_REQUESTS <= max_http_concurrency），超限立即 503；
// 请求处理不持有 Controller 状态锁——短暂锁取 Arc<Module> 后立即释放。
// ---------------------------------------------------------------------------

/// ADR-0032：默认绑定固定端口 8765（可用环境变量 `NATIVES_APP_RUNTIME_PORT`
/// 覆盖，设为 `0` 强制动态端口）；默认端口被占用时回退动态端口（`:0`），
/// 实际端口经 `StartResult.port` 上报。绑定始终限于 127.0.0.1。
fn bind_loopback_listener() -> std::io::Result<TcpListener> {
    const DEFAULT_PORT: u16 = 8765;
    let port = std::env::var("NATIVES_APP_RUNTIME_PORT")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => Ok(l),
        Err(e) if port != 0 && e.kind() == std::io::ErrorKind::AddrInUse => {
            TcpListener::bind(("127.0.0.1", 0))
        }
        Err(e) => Err(e),
    }
}

fn serve_loopback_http(listener: TcpListener, shutdown: Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(s) => {
                // 并发硬门：超过 max_http_concurrency 直接拒绝，不无限建线程。
                if ACTIVE_HTTP_REQUESTS.load(Ordering::SeqCst) >= HTTP_LIMITS.max_http_concurrency {
                    let mut busy = s;
                    let _ = write_http(
                        &mut busy,
                        503,
                        "Service Unavailable",
                        "application/json",
                        b"{\"error\":\"APP_RUNTIME_BUSY\"}",
                    );
                    continue;
                }
                ACTIVE_HTTP_REQUESTS.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    handle_http_client(s);
                    ACTIVE_HTTP_REQUESTS.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(_) => break,
        }
    }
}

fn handle_http_client(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let request = match HttpRequest::read(&mut stream) {
        Ok(req) => req,
        Err(_) => return,
    };

    let port = LOOPBACK_PORT.load(Ordering::SeqCst);
    if port != 0 && !request.has_loopback_host(port) {
        let _ = write_http(
            &mut stream,
            403,
            "Forbidden",
            "text/plain",
            b"bad host header",
        );
        return;
    }

    if request.method == "OPTIONS" {
        let _ = write_preflight(&mut stream, &request);
        return;
    }

    let route = request.path.split('?').next().unwrap_or(&request.path);
    if route == "/healthz" {
        let _ = write_http(&mut stream, 200, "OK", "application/json", b"{\"ok\":true}");
        return;
    }

    // 短暂锁：只 clone Arc<Module> 与会话 Arc，绝不在锁内执行业务请求（计划 §17.2）。
    let (module, sessions) = {
        // 采用编译期已知的最小闭包持有锁作用域。
        let guard = CONTROLLER_STATE.lock().unwrap();
        match &*guard {
            ControllerState::Ready(ready) => {
                (ready.bound.module.clone(), Some(ready.sessions.clone()))
            }
            _ => return, // 未就绪：静默关闭连接（端口未对外公布）
        }
    };

    // 1. UI 静态资源（无需 token）
    if let Ok(Some(resp)) = module.lock().unwrap().handle_ui(route) {
        let _ = write_http(
            &mut stream,
            resp.status_code,
            &resp.reason,
            &resp.content_type,
            &resp.body,
        );
        return;
    }

    // 2. 业务 API：验证 sandbox origin 与 Bearer token
    // 例外（ADR-0032）：/api/tray/state 与 /api/limits 为只读聚合，供进程外系统组件
    // （macOS 顶栏）在无 Native Messaging 会话时轮询，免 Bearer 鉴权与
    // sandbox origin 要求，但仍要求 loopback Host 头（上方已校验）。
    let tray_readonly = route == "/api/tray/state" || route == "/api/limits";
    let session_ok = tray_readonly
        || match &sessions {
            Some(sessions) => match sessions.lock() {
                Ok(sessions_guard) => sessions_guard
                    .authorize(
                        sessions_guard.generation(),
                        request.header("authorization").unwrap_or(""),
                    )
                    .is_ok(),
                Err(_) => false,
            },
            None => false,
        };

    if !tray_readonly && (!request.sandbox_origin() || !session_ok) {
        let _ = write_http(
            &mut stream,
            401,
            "Unauthorized",
            "application/json",
            b"{\"error\":\"APP_SESSION_INVALID\"}",
        );
        return;
    }

    let api_result = {
        let module_guard = match module.lock() {
            Ok(guard) => guard,
            Err(_) => {
                let _ = write_http(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    "application/json",
                    b"{\"error\":\"APP_RUNTIME_INVALID_STATE\"}",
                );
                return;
            }
        };
        module_guard.handle_api(&request, route)
    };
    match api_result {
        Ok(resp) => {
            let _ = write_http(
                &mut stream,
                resp.status_code,
                &resp.reason,
                &resp.content_type,
                &resp.body,
            );
        }
        Err((code, msg)) => {
            let body = format!("{{\"error\":{}}}", json_str(&msg));
            let _ = write_http(
                &mut stream,
                code,
                "Error",
                "application/json",
                body.as_bytes(),
            );
        }
    }
}

/// 全局 Runtime 状态（进程级单实例；HTTP 线程与主循环共享同一状态机）。
/// 定义于文件头部：main 循环与 loopback HTTP 线程共享同一 `Mutex<ControllerState>`。

fn write_http(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()> {
    write_http_response(
        stream,
        code,
        reason,
        ctype,
        body,
        "default-src 'none'; script-src 'self' 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'; form-action 'none'; base-uri 'none'; frame-ancestors chrome-extension:",
    )
}
