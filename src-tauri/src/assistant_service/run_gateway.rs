//! Host preflight for run.start and long-poll subscribe forwarding.
use crate::daemon::data::DataStore;
use crate::daemon_authority;
use serde_json::Value;
use std::sync::Arc;

use super::provider_catalog::provider_model_pair_available;
use super::{error_response, success_response, RpcResponse};

struct RunStartRequest {
    conversation_id: String,
    provider_id: String,
    model_id: String,
    content: Option<String>,
    effort: Option<String>,
    runtime_id: Option<String>,
    attachments: Vec<Value>,
    project_path: Option<String>,
    /// Expert / agent persona for this run (ADR-0016 seam A).
    agent_profile_id: Option<String>,
    /// Capability library selection override (ADR-0016). None = conversation default.
    capability_selection: Option<assistant_protocol::v2::CapabilitySelection>,
}

pub(crate) async fn handle_run_start(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let req = match parse_run_start_request(params) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let normalized = match normalize_attachments(&req.attachments) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    if let Err(resp) = preflight_run_start(data_store, &req).await {
        return resp;
    }
    create_and_start_run(data_store, &req, normalized).await
}

fn parse_run_start_request(params: &Value) -> Result<RunStartRequest, RpcResponse> {
    let conversation_id = params
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| error_response("MISSING_PARAM", "conversation_id is required"))?
        .to_string();
    let provider_id = params
        .get("provider_id")
        .and_then(Value::as_str)
        .ok_or_else(|| error_response("MISSING_PARAM", "provider_id is required"))?
        .to_string();
    let model_id = params
        .get("model_id")
        .and_then(Value::as_str)
        .ok_or_else(|| error_response("MISSING_PARAM", "model_id is required"))?
        .to_string();
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
        .map(str::to_string);
    let effort = params
        .get("effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let runtime_id = params
        .get("runtime_id")
        .or_else(|| params.get("runtimeId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let attachments = params
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if attachments.len() > 10 {
        return Err(error_response(
            "INVALID_INPUT",
            "At most 10 attachments are allowed",
        ));
    }
    let project_path = params
        .get("project_path")
        .or_else(|| params.get("workspace_path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("NATIVES_PROJECT_PATH").ok())
        .filter(|path| !path.trim().is_empty());
    let agent_profile_id = params
        .get("agent_profile_id")
        .or_else(|| params.get("agentProfileId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let capability_selection = match params
        .get("capability_selection")
        .or_else(|| params.get("capabilitySelection"))
    {
        Some(raw) if !raw.is_null() => {
            let selection: assistant_protocol::v2::CapabilitySelection =
                serde_json::from_value(raw.clone()).map_err(|e| {
                    error_response("INVALID_INPUT", &format!("capability_selection: {e}"))
                })?;
            selection
                .validate()
                .map_err(|e| error_response("INVALID_INPUT", e))?;
            Some(selection)
        }
        _ => None,
    };
    Ok(RunStartRequest {
        conversation_id,
        provider_id,
        model_id,
        content,
        effort,
        runtime_id,
        attachments,
        project_path,
        agent_profile_id,
        capability_selection,
    })
}

fn normalize_attachments(attachments: &[Value]) -> Result<Vec<Value>, RpcResponse> {
    let mut normalized_attachments = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let path = match attachment.get("path").and_then(Value::as_str) {
            Some(path) if !path.trim().is_empty() => path.trim(),
            _ => {
                return Err(error_response(
                    "INVALID_INPUT",
                    "attachment path is required",
                ))
            }
        };
        let metadata = match crate::file_manager::read_file(path) {
            Ok(metadata) if !metadata.truncated => metadata,
            Ok(_) => {
                return Err(error_response(
                    "INVALID_INPUT",
                    "attachments must be UTF-8 files smaller than 2 MB",
                ))
            }
            Err(error) => {
                return Err(error_response(
                    "INVALID_INPUT",
                    &format!("Attachment cannot be read: {error}"),
                ))
            }
        };
        let name = attachment
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path)
                    .to_string()
            });
        let mime_type = attachment
            .get("mime_type")
            .or_else(|| attachment.get("mimeType"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("text/plain");
        let size = attachment
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or(metadata.size);
        normalized_attachments.push(serde_json::json!({
            "path": path,
            "name": name,
            "mime_type": mime_type,
            "size": size,
        }));
    }
    Ok(normalized_attachments)
}

async fn preflight_run_start(
    data_store: &Arc<DataStore>,
    req: &RunStartRequest,
) -> Result<(), RpcResponse> {
    if req.project_path.is_none()
        && std::env::var("NATIVES_REQUIRE_PROJECT_PATH")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true)
    {
        return Err(error_response(
            "PROJECT_PATH_REQUIRED",
            "project_path must be provided by UI (daemon cwd is not a valid default)",
        ));
    }
    {
        let conn = data_store.conn();
        if !provider_model_pair_available(&req.provider_id, &req.model_id, &conn) {
            return Err(error_response(
                "INVALID_PARAM",
                "Provider/model pair is not available",
            ));
        }
    }
    if let Ok(runs) = daemon_authority::list_runs(Some(&req.conversation_id)).await {
        if runs.iter().any(|r| !r.status.is_terminal()) {
            return Err(error_response(
                "RUN_ALREADY_ACTIVE",
                "This conversation already has an active run",
            ));
        }
    }
    Ok(())
}

async fn create_and_start_run(
    _data_store: &Arc<DataStore>,
    req: &RunStartRequest,
    normalized_attachments: Vec<Value>,
) -> RpcResponse {
    let permission_profile = match daemon_authority::request(
        "conversation.get",
        serde_json::json!({ "id": req.conversation_id }),
    )
    .await
    {
        Ok(conv) => conv
            .get("permission_profile_id")
            .and_then(Value::as_str)
            .filter(|p| matches!(*p, "readonly" | "ask" | "full_access"))
            .unwrap_or("ask")
            .to_string(),
        Err(_) => "ask".to_string(),
    };

    let user_content = req.content.clone().unwrap_or_default();
    let daemon_attachments: Vec<assistant_protocol::v2::AttachmentRef> = normalized_attachments
        .iter()
        .filter_map(|attachment| {
            let path = attachment.get("path")?.as_str()?.to_string();
            Some(assistant_protocol::v2::AttachmentRef {
                path,
                name: attachment
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                mime_type: attachment
                    .get("mime_type")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                size: attachment.get("size").and_then(Value::as_u64),
            })
        })
        .collect();
    let daemon_attachments_opt = if daemon_attachments.is_empty() {
        None
    } else {
        Some(daemon_attachments)
    };

    let idempotency_key = uuid::Uuid::new_v4().to_string();
    let daemon_run = match daemon_authority::create_run(assistant_protocol::v2::CreateRunRequest {
        conversation_id: req.conversation_id.clone(),
        provider_id: req.provider_id.clone(),
        model_id: req.model_id.clone(),
        key_id: None,
        agent_profile_id: req.agent_profile_id.clone(),
        permission_profile: Some(permission_profile.clone()),
        content: Some(user_content.clone()),
        attachments: daemon_attachments_opt.clone(),
        max_steps: Some(50),
        parent_run_id: None,
        project_path: req.project_path.clone(),
        idempotency_key: Some(idempotency_key.clone()),
        effort: req.effort.clone(),
        runtime_id: req.runtime_id.clone(),
        capability_selection: req.capability_selection.clone(),
    })
    .await
    {
        Ok(r) => r,
        Err(e) => return error_response("DAEMON_CREATE_FAILED", &e),
    };
    let start_req = assistant_protocol::v2::StartRunRequest {
        run_id: Some(daemon_run.id.clone()),
        conversation_id: Some(req.conversation_id.clone()),
        provider_id: Some(req.provider_id.clone()),
        model_id: Some(req.model_id.clone()),
        key_id: None,
        content: Some(user_content),
        attachments: daemon_attachments_opt,
        trigger_message_id: None,
        permission_profile: Some(permission_profile.clone()),
        max_steps: Some(50),
        project_path: req.project_path.clone(),
        idempotency_key: None,
        effort: req.effort.clone(),
        runtime_id: req.runtime_id.clone(),
        agent_profile_id: req.agent_profile_id.clone(),
        capability_selection: req.capability_selection.clone(),
    };
    let mode_label = daemon_authority::authority_mode_label();
    let started_daemon = match daemon_authority::start_run(start_req).await {
        Ok(run) => run,
        Err(error) => return error_response("DAEMON_START_FAILED", &error),
    };
    let status = if started_daemon.status.is_terminal() {
        started_daemon.status.as_str().to_string()
    } else {
        "running".to_string()
    };
    let started_at = started_daemon
        .started_at
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    success_response(serde_json::json!({
        "id": started_daemon.id,
        "conversation_id": req.conversation_id,
        "status": status,
        "provider_id": req.provider_id,
        "model_id": req.model_id,
        "permission_profile": started_daemon.permission_profile,
        "started_at": started_at,
        "execution": "agent_daemon_run_manager",
        "authority_mode": mode_label,
        "daemon_run_id": started_daemon.id,
    }))
}

pub(crate) async fn handle_run_subscribe(
    _data_store: &Arc<DataStore>,
    params: &Value,
) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };
    let after_sequence = params
        .get("after_sequence")
        .or_else(|| params.get("last_sequence"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let wait_ms = params
        .get("wait_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .clamp(0, 30_000);
    let want_push = params
        .get("mode")
        .and_then(Value::as_str)
        .map(|m| m == "push" || m == "long_poll")
        .unwrap_or(false)
        || wait_ms > 0;

    // Forward long-poll to daemon authority; then project host state before terminal:true.
    let mut params_forward = params.clone();
    if let Some(obj) = params_forward.as_object_mut() {
        obj.insert("run_id".into(), Value::String(run_id.to_string()));
        obj.insert("after_sequence".into(), serde_json::json!(after_sequence));
        if want_push && wait_ms > 0 {
            obj.insert("wait_ms".into(), serde_json::json!(wait_ms));
            obj.insert("mode".into(), Value::String("push".into()));
        }
    }

    // Forward only — no host projection of events/messages/runs.
    let data = match daemon_authority::request("run.subscribe", params_forward).await {
        Ok(data) => data,
        Err(error) => match daemon_authority::replay_events(run_id, after_sequence).await {
            Ok(events) => {
                let daemon_terminal = daemon_authority::get_run(run_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|r| r.status.is_terminal())
                    .unwrap_or(false);
                let event_values: Vec<Value> = events
                    .into_iter()
                    .map(|e| {
                        serde_json::json!({
                            "run_id": e.run_id,
                            "sequence": e.effective_run_sequence(),
                            "timestamp": e.timestamp.to_rfc3339(),
                            "type": e.payload.type_name(),
                            "payload": e.payload,
                        })
                    })
                    .collect();
                return success_response(serde_json::json!({
                    "run_id": run_id,
                    "events": event_values,
                    "terminal": daemon_terminal,
                    "mode": "subscribe_fallback_replay",
                    "error": error,
                }));
            }
            Err(e2) => return error_response("DAEMON_RPC_ERROR", &format!("{error}; {e2}")),
        },
    };

    let daemon_terminal = data
        .get("terminal")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || daemon_authority::get_run(run_id)
            .await
            .ok()
            .flatten()
            .map(|r| r.status.is_terminal())
            .unwrap_or(false);

    let out_events = data
        .get("events")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));

    success_response(serde_json::json!({
        "run_id": run_id,
        "events": out_events,
        "terminal": daemon_terminal,
        "mode": data.get("mode").cloned().unwrap_or(Value::String("subscribe_host".into())),
    }))
}

// ─── Permission (host still dual-writes until task-04 integration) ───
