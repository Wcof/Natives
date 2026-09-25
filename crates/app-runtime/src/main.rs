//! natives-app-runtime：统一官方内置应用运行时（ADR-0031）。
//!
//! 职责：Native Messaging framing、Origin/协议/激活校验、编译期模块注册、
//! Runtime 状态机（单次 appId 绑定）、事务式 Start、有界 loopback HTTP、
//! 会话鉴权、协作式取消与确定性关闭。
//! 禁止承担任何具体模块（Fund 等）的业务逻辑。

mod controller;
mod fixtures;
mod http;

use app_runtime_core::cancellation::CancellationToken;
use app_runtime_core::framing::read_frame;
use app_runtime_core::lock::RuntimeLease;
use app_runtime_core::module::{BuiltInAppModule, ModuleRegistry};
use app_runtime_core::origin::chrome_extension_origin;
use app_runtime_core::protocol::{AppRequest, APP_PROTOCOL_VERSION_V2};
use app_runtime_core::session::SessionManager;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(feature = "test-fixture")]
use fixtures::SecondFixtureModule;
use http::{apps_root};

// ---------------------------------------------------------------------------
// Runtime 状态机（计划 §10）：合法迁移
//   BOOTING → handshake → BOUND → initialize(同握手) → INITIALIZED
//           → start(事务式) → READY → stop/EOF → STOPPING → STOPPED
// 其余迁移一律拒绝（APP_RUNTIME_* 错误码）。
// ---------------------------------------------------------------------------

/// 已绑定 appId 的运行时（握手成功，模块已 initialize）。
pub(crate) struct BoundRuntime {
    pub(crate) app_id: String,
    pub(crate) instance_id: String,
    pub(crate) module: Arc<Mutex<Box<dyn BuiltInAppModule>>>,
    pub(crate) _lease: RuntimeLease,
}

/// 就绪运行时：会话管理器与 loopback 监听已提交（端口经 LOOPBACK_PORT 共享）。
pub(crate) struct ReadyRuntime {
    pub(crate) bound: BoundRuntime,
    pub(crate) sessions: Arc<Mutex<SessionManager>>,
}

pub(crate) enum ControllerState {
    Booting,
    Bound(BoundRuntime),
    Ready(ReadyRuntime),
    Stopping,
    Stopped,
}

struct Controller {
    /// 关闭取消信号：注入 ModuleContext，业务长任务必须轮询。
    pub(crate) cancellation: Arc<CancellationToken>,
    /// loopback 服务停止旗标（关闭 accept 循环）。
    pub(crate) server_shutdown: Arc<AtomicBool>,
    /// 主循环是否应退出（stop 请求或启动失败后）。
    pub(crate) stop_requested: AtomicBool,
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

pub(crate) static LOOPBACK_PORT: AtomicU16 = AtomicU16::new(0);
/// 当前处理中的 HTTP 请求数（有界并发硬门）。
pub(crate) static ACTIVE_HTTP_REQUESTS: AtomicUsize = AtomicUsize::new(0);
pub(crate) static HTTP_LIMITS: app_runtime_core::limits::RuntimeLimits =
    app_runtime_core::limits::RuntimeLimits::hard_gate();

/// 全局 Runtime 状态（进程级单实例；HTTP 线程与主循环共享同一状态机）。
pub(crate) static CONTROLLER_STATE: Mutex<ControllerState> = Mutex::new(ControllerState::Booting);

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
