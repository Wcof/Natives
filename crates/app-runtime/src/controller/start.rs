use super::Controller;
use crate::http::{write_error, write_json_response};
use crate::http::{bind_loopback_listener, serve_loopback_http};
use crate::{CONTROLLER_STATE, ControllerState, LOOPBACK_PORT, ReadyRuntime};
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::module::BuiltInAppModule;
use app_runtime_core::protocol::{AppRequest, StartResult};
use app_runtime_core::session::SessionManager;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

impl Controller {
    /// 事务式 Start（计划 §12）：prepare module → prepare session → prepare
    /// listener → commit Ready；任何一步失败回滚全部资源并进入 Stopping。
    pub(super) fn handle_start(&self, req: &AppRequest) {
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
    pub(super) fn rollback_failed_start(&self, module: &Arc<Mutex<Box<dyn BuiltInAppModule>>>) {
        if let Ok(mut module_guard) = module.lock() {
            let _ = module_guard.shutdown();
        }
        self.enter_stopping();
    }
}
