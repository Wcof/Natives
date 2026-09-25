use super::Controller;
use crate::http::{write_error, write_json_response};
use crate::{CONTROLLER_STATE, ControllerState};
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::protocol::AppRequest;
use std::sync::atomic::Ordering;

impl Controller {
    pub(super) fn handle_session(&self, req: &AppRequest) {
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

    pub(super) fn handle_stop(&self, req: &AppRequest) {
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
    pub(crate) fn shutdown(&self) {
        self.enter_stopping();
        self.stop_requested.store(true, Ordering::SeqCst);
    }
}
