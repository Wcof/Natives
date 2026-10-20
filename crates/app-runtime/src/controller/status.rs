use super::Controller;
use crate::http::{write_error, write_json_response};
use crate::{ControllerState, CONTROLLER_STATE};
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::protocol::{AppRequest, StatusResult};

impl Controller {
    pub(super) fn handle_status(&self, req: &AppRequest) {
        let state = CONTROLLER_STATE.lock().unwrap();
        let (instance_id, runtime_state, service) = match &*state {
            ControllerState::Ready(ready) => (
                ready.bound.instance_id.clone(),
                "ready",
                Some(ready.service.clone()),
            ),
            ControllerState::Bound(bound) => (bound.instance_id.clone(), "initialized", None),
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
            active_requests: crate::ACTIVE_HTTP_REQUESTS.load(std::sync::atomic::Ordering::Acquire),
            active_tasks: service.map_or(0, |service| service.protected_task_count()),
        };
        write_json_response(&req.id, true, Some(result), None);
    }

    pub(super) fn handle_data_status(&self, req: &AppRequest) {
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

    /// app:card_snapshot（ADR-0036）：仅 Ready 状态接受；参数白名单只允许
    /// 本次 app:start 返回的 instanceId/generation；响应 ≤64 KiB。只调用已
    /// 绑定模块的窄只读接口，不透传任意 route/字段。
    pub(super) fn handle_card_snapshot(&self, req: &AppRequest) {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CardSnapshotParams {
            #[serde(rename = "instanceId")]
            instance_id: String,
            generation: String,
            #[serde(rename = "surfaceId", default)]
            surface_id: String,
        }
        let params: CardSnapshotParams = match serde_json::from_value(req.params.clone()) {
            Ok(p) => p,
            Err(e) => {
                write_error(
                    &req.id,
                    AppErrorCode::ProtocolMismatch,
                    format!("invalid card_snapshot params: {e}"),
                    false,
                );
                return;
            }
        };

        let (instance_id, service) = {
            let state = CONTROLLER_STATE.lock().unwrap();
            match &*state {
                ControllerState::Ready(ready) => {
                    (ready.bound.instance_id.clone(), ready.service.clone())
                }
                ControllerState::Bound(_) => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::InvalidState,
                        "runtime not started; card snapshot requires app:start",
                        false,
                    );
                    return;
                }
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
            }
        };

        if params.instance_id != instance_id {
            write_error(
                &req.id,
                AppErrorCode::SessionInvalid,
                "card snapshot instance mismatch",
                false,
            );
            return;
        }
        let generation_matches = match &*CONTROLLER_STATE.lock().unwrap() {
            ControllerState::Ready(ready) => ready
                .sessions
                .lock()
                .map(|sessions| {
                    sessions.surface_generation_matches(&params.surface_id, &params.generation)
                })
                .unwrap_or(false),
            _ => false,
        };
        if !generation_matches {
            write_error(
                &req.id,
                AppErrorCode::SessionInvalid,
                "card snapshot generation mismatch",
                false,
            );
            return;
        }

        let snapshot = service.card_snapshot();
        match snapshot {
            Ok(snapshot) => {
                let size = serde_json::to_vec(&snapshot)
                    .map(|bytes| bytes.len())
                    .unwrap_or(0);
                if size > app_runtime_core::protocol::MAX_CARD_SNAPSHOT_BYTES {
                    write_error(
                        &req.id,
                        AppErrorCode::InvalidState,
                        "card snapshot response too large",
                        false,
                    );
                    return;
                }
                write_json_response(&req.id, true, Some(snapshot), None);
            }
            Err((code, message)) => {
                let error_code =
                    AppErrorCode::from_code(&message).unwrap_or(AppErrorCode::StartFailed);
                write_error(&req.id, error_code, message, code >= 500 && code < 600);
            }
        }
    }
}
