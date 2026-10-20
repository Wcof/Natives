//! app:secret（ADR-0038）：Extension 可信表单专属的模块 Secret 窄写入。
//! 仅 Ready 会话可用；参数、generation 与能力声明任一不匹配都在任何
//! Keychain 副作用前拒绝。协议没有读取明文的 op，响应永不回显 value。

use super::Controller;
use crate::http::{write_error, write_json_response};
use crate::{ControllerState, CONTROLLER_STATE};
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::protocol::AppRequest;
use app_runtime_core::secrets::{SecretError, SECRET_CAPABILITY};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretParams {
    #[serde(rename = "instanceId")]
    instance_id: String,
    generation: String,
    #[serde(rename = "surfaceId", default)]
    surface_id: String,
    op: String,
    #[serde(rename = "accountId")]
    account_id: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    confirm: Option<bool>,
}

fn secret_error(err: SecretError) -> &'static str {
    match err.code {
        AppErrorCode::KeychainLocked => "keychain locked or unavailable; unlock and retry",
        AppErrorCode::KeychainUnsupported => "secret backend unsupported on this platform",
        _ => "invalid secret request",
    }
}

impl Controller {
    pub(super) fn handle_secret(&self, req: &AppRequest) {
        let params: SecretParams = match serde_json::from_value(req.params.clone()) {
            Ok(p) => p,
            Err(e) => {
                write_error(
                    &req.id,
                    AppErrorCode::ProtocolMismatch,
                    format!("invalid secret params: {e}"),
                    false,
                );
                return;
            }
        };

        let (instance_id, sessions, capabilities, secrets) = {
            let state = CONTROLLER_STATE.lock().unwrap();
            match &*state {
                ControllerState::Ready(ready) => (
                    ready.bound.instance_id.clone(),
                    ready.sessions.clone(),
                    ready.bound.capabilities.clone(),
                    ready.bound.secrets.clone(),
                ),
                ControllerState::Bound(_) => {
                    drop(state);
                    write_error(
                        &req.id,
                        AppErrorCode::InvalidState,
                        "runtime not started; secret requires app:start",
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
                "secret instance mismatch",
                false,
            );
            return;
        }
        let generation_matches = match sessions.lock() {
            Ok(sessions) => {
                sessions.surface_generation_matches(&params.surface_id, &params.generation)
            }
            Err(_) => false,
        };
        if !generation_matches {
            write_error(
                &req.id,
                AppErrorCode::SessionInvalid,
                "secret generation mismatch",
                false,
            );
            return;
        }
        if !capabilities.iter().any(|c| c == SECRET_CAPABILITY) {
            write_error(
                &req.id,
                AppErrorCode::CapabilityDenied,
                "module does not declare platform.secret capability",
                false,
            );
            return;
        }

        let outcome = match params.op.as_str() {
            "set" => {
                let Some(value) = params.value.as_deref() else {
                    write_error(
                        &req.id,
                        AppErrorCode::ProtocolMismatch,
                        "secret set requires a value",
                        false,
                    );
                    return;
                };
                match secrets.set(&params.account_id, value) {
                    Ok(()) => serde_json::json!({"op": "set", "accountId": params.account_id}),
                    Err(err) => {
                        write_error(&req.id, err.code, secret_error(err), err.retryable);
                        return;
                    }
                }
            }
            "status" => match secrets.status(&params.account_id) {
                Ok(status) => serde_json::json!({
                    "op": "status",
                    "accountId": params.account_id,
                    "exists": status.exists,
                    "updatedAt": status.updated_at,
                }),
                Err(err) => {
                    write_error(&req.id, err.code, secret_error(err), err.retryable);
                    return;
                }
            },
            "delete" => {
                if params.confirm != Some(true) {
                    write_error(
                        &req.id,
                        AppErrorCode::ProtocolMismatch,
                        "secret delete requires confirm: true",
                        false,
                    );
                    return;
                }
                match secrets.delete(&params.account_id) {
                    Ok(()) => serde_json::json!({"op": "delete", "accountId": params.account_id}),
                    Err(err) => {
                        write_error(&req.id, err.code, secret_error(err), err.retryable);
                        return;
                    }
                }
            }
            other => {
                write_error(
                    &req.id,
                    AppErrorCode::ProtocolMismatch,
                    format!("unknown secret op '{other}'"),
                    false,
                );
                return;
            }
        };
        write_json_response(&req.id, true, Some(outcome), None);
    }
}
