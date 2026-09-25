use super::Controller;
use crate::http::{write_error, write_json_response};
use crate::{CONTROLLER_STATE, ControllerState};
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::protocol::{AppRequest, StatusResult};

impl Controller {
    pub(super) fn handle_status(&self, req: &AppRequest) {
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
}
