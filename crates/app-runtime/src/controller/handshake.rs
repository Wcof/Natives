use super::Controller;
use crate::http::{write_error, write_json_response};
use crate::{BoundRuntime, ControllerState, CONTROLLER_STATE};
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::lock::{acquire_runtime, RuntimeUnavailable};
use app_runtime_core::module::{ModuleContext, ModuleRegistry};
use app_runtime_core::protocol::{
    AppRequest, HandshakeParamsV2, HandshakeResultV2, APP_PROTOCOL_VERSION_V2,
};
use app_runtime_core::session::random_id;
use std::path::Path;
use std::sync::{Arc, Mutex};

impl Controller {
    /// 握手：一个 Runtime Process 只能绑定一次 appId（计划 §11）。
    /// 校验顺序（计划 §34.1）：origin → 协议版本 → 激活 → registry → 绑定 → initialize。
    pub(super) fn handle_handshake(
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

        // 状态机预检与多 Surface 握手支持（ADR-0042 / F04）：
        // 若本进程已处于 Bound 或 Ready 状态且 appId 一致，直接复用既有模块实例返回当前状态；
        // 跨模块混用或进程处于停止状态时才严格拒绝。
        {
            let state = CONTROLLER_STATE.lock().unwrap();
            let bound_generation = match &*state {
                ControllerState::Bound(bound) => Some(bound.activation_generation),
                ControllerState::Ready(ready) => Some(ready.bound.activation_generation),
                _ => None,
            };
            if bound_generation.is_some_and(|generation| generation != activation.generation) {
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
                ControllerState::Bound(bound) if bound.app_id == *app_id => {
                    let descriptor = bound.module.lock().unwrap().descriptor();
                    let res = HandshakeResultV2 {
                        protocol_version: APP_PROTOCOL_VERSION_V2,
                        app_id: app_id.clone(),
                        module_api_version: descriptor.module_api_version,
                        data_schema_version: descriptor.data_schema_version,
                        capability_version: descriptor.capability_version,
                        state: "initialized".into(),
                    };
                    drop(state);
                    write_json_response(&req.id, true, Some(res), None);
                    return;
                }
                ControllerState::Ready(ready) if ready.bound.app_id == *app_id => {
                    let descriptor = ready.bound.module.lock().unwrap().descriptor();
                    let res = HandshakeResultV2 {
                        protocol_version: APP_PROTOCOL_VERSION_V2,
                        app_id: app_id.clone(),
                        module_api_version: descriptor.module_api_version,
                        data_schema_version: descriptor.data_schema_version,
                        capability_version: descriptor.capability_version,
                        state: "initialized".into(),
                    };
                    drop(state);
                    write_json_response(&req.id, true, Some(res), None);
                    return;
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
                    write_error(&req.id, AppErrorCode::Stopped, "runtime stopped", false);
                    return;
                }
                ControllerState::Booting => {}
            }
        }

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
                        activation_generation: activation.generation,
                        instance_id: instance_id.clone(),
                        module: Arc::new(Mutex::new(module)),
                        capabilities: activation.capabilities.clone(),
                        secrets: Arc::new(app_runtime_core::secrets::SecretStore::for_app(app_id)),
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
}
