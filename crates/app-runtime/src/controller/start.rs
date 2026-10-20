use super::Controller;
use crate::http::{bind_loopback_listener, serve_loopback_http};
use crate::http::{write_error, write_json_response};
use crate::{ControllerState, ReadyRuntime, CONTROLLER_STATE, LOOPBACK_PORT};
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
        let surface_id = req
            .params
            .get("surfaceId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // 1. 快照状态（短暂持锁：只做状态判定与 Arc clone，不在锁内执行业务）。
        let (module, instance_id) = {
            let state = CONTROLLER_STATE.lock().unwrap();
            let bound_generation = match &*state {
                ControllerState::Bound(bound) => Some(bound.activation_generation),
                ControllerState::Ready(ready) => Some(ready.bound.activation_generation),
                _ => None,
            };
            if req
                .params
                .get("expectedActivationGeneration")
                .and_then(|v| v.as_u64())
                .is_some_and(|expected| Some(expected) != bound_generation)
            {
                drop(state);
                write_error(
                    &req.id,
                    AppErrorCode::InstallationChanged,
                    "product generation changed; reopen the module",
                    false,
                );
                return;
            }
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
                ControllerState::Ready(ready) => {
                    let port = LOOPBACK_PORT.load(Ordering::SeqCst);
                    let sessions = ready.sessions.clone();
                    let instance_id = ready.bound.instance_id.clone();
                    drop(state);
                    let generation =
                        match sessions.lock().unwrap().generation_for_surface(surface_id) {
                            Ok(generation) => generation,
                            Err(error) => {
                                write_json_response::<serde_json::Value>(
                                    &req.id,
                                    false,
                                    None,
                                    Some(error),
                                );
                                return;
                            }
                        };
                    write_json_response(
                        &req.id,
                        true,
                        Some(StartResult {
                            generation,
                            instance_id,
                            port,
                            state: "ready".into(),
                        }),
                        None,
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

        // 2. prepare module（业务启动并产出无锁服务对象；失败 → 回滚为 Stopping）。
        let service = {
            let mut module_guard = module.lock().unwrap();
            match module_guard.start() {
                Ok(s) => s,
                Err(err) => {
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
        };

        // 3. prepare session。
        let Some(mut sessions) = SessionManager::new() else {
            self.rollback_failed_start(&module);
            write_error(
                &req.id,
                AppErrorCode::StartFailed,
                "failed to initialize session manager",
                true,
            );
            return;
        };
        let generation = match sessions.generation_for_surface(surface_id) {
            Ok(generation) => generation,
            Err(error) => {
                self.rollback_failed_start(&module);
                write_json_response::<serde_json::Value>(&req.id, false, None, Some(error));
                return;
            }
        };
        let sessions = Arc::new(Mutex::new(sessions));

        // 4. prepare listener。ADR-0035：统一绑定系统分配的动态 loopback 端口。
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
                *state = ControllerState::Ready(ReadyRuntime {
                    bound,
                    service,
                    sessions,
                });
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
