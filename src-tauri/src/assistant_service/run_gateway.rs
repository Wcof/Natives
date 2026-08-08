//! Host preflight for run.start and long-poll subscribe forwarding.
use crate::daemon::data::DataStore;
use crate::daemon_authority;
use crate::execution_engine_settings::{
    build_runtime_descriptors, load_execution_engine_settings, resolve_execution_policy,
    store_policy_snapshot, ResolvedExecutionPolicyV1,
};
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
    /// Explicit per-run maxSteps override (S3 policy: explicit > settings).
    max_steps: Option<u32>,
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
    // Explicit per-run step budget override (S3 policy priority: explicit
    // run override > Settings V2 > safe default). Clamped into daemon bounds
    // during policy resolution.
    let max_steps = params
        .get("max_steps")
        .or_else(|| params.get("maxSteps"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
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
        max_steps,
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

/// Resolve the S3 execution policy for a NEW top-level Run.
///
/// Priority chain (EXECUTION-POLICY-V1): explicit run override →
/// conversation override (no producer yet; kept as a seam) → application
/// Settings V2 → safe default native. An unavailable explicit/conversation
/// external runtime is a hard failure; only the application default may fall
/// back to native (`externalUnavailablePolicy == "fallback_native"`).
fn resolve_policy_for_run(req: &RunStartRequest) -> Result<ResolvedExecutionPolicyV1, RpcResponse> {
    let settings = load_execution_engine_settings().map_err(|e| {
        error_response(
            "SETTINGS_UNAVAILABLE",
            &format!("execution settings unavailable: {e}"),
        )
    })?;
    let runtimes = build_runtime_descriptors(&settings);
    // Conversation-level runtime override does not exist yet; the hook is
    // wired so the priority chain is complete when a producer arrives.
    let conversation_runtime_id: Option<&str> = None;
    resolve_execution_policy(
        &settings,
        &runtimes,
        req.runtime_id.as_deref(),
        conversation_runtime_id,
        req.max_steps,
    )
    .map_err(|e| error_response("RUNTIME_UNAVAILABLE", &e))
}

/// Build the typed create request from the resolved policy (pure; unit-tested).
fn build_create_run_request(
    req: &RunStartRequest,
    policy: &ResolvedExecutionPolicyV1,
    permission_profile: &str,
    user_content: &str,
    attachments: Option<Vec<assistant_protocol::v2::AttachmentRef>>,
    idempotency_key: String,
) -> assistant_protocol::v2::CreateRunRequest {
    assistant_protocol::v2::CreateRunRequest {
        conversation_id: req.conversation_id.clone(),
        provider_id: req.provider_id.clone(),
        model_id: req.model_id.clone(),
        key_id: None,
        agent_profile_id: req.agent_profile_id.clone(),
        permission_profile: Some(permission_profile.to_string()),
        content: Some(user_content.to_string()),
        attachments,
        // maxSteps comes from the resolved policy (never a hardcoded 50).
        max_steps: Some(policy.max_steps),
        parent_run_id: None,
        project_path: req.project_path.clone(),
        idempotency_key: Some(idempotency_key),
        effort: req.effort.clone(),
        // runtimeId comes from the resolved policy (never a silent default).
        runtime_id: Some(policy.runtime_id.clone()),
        capability_selection: req.capability_selection.clone(),
    }
}

/// Build the typed start request from the resolved policy (pure; unit-tested).
fn build_start_run_request(
    req: &RunStartRequest,
    policy: &ResolvedExecutionPolicyV1,
    permission_profile: &str,
    user_content: String,
    attachments: Option<Vec<assistant_protocol::v2::AttachmentRef>>,
    run_id: &str,
) -> assistant_protocol::v2::StartRunRequest {
    assistant_protocol::v2::StartRunRequest {
        run_id: Some(run_id.to_string()),
        conversation_id: Some(req.conversation_id.clone()),
        provider_id: Some(req.provider_id.clone()),
        model_id: Some(req.model_id.clone()),
        key_id: None,
        content: Some(user_content),
        attachments,
        trigger_message_id: None,
        permission_profile: Some(permission_profile.to_string()),
        max_steps: Some(policy.max_steps),
        project_path: req.project_path.clone(),
        idempotency_key: None,
        effort: req.effort.clone(),
        runtime_id: Some(policy.runtime_id.clone()),
        agent_profile_id: req.agent_profile_id.clone(),
        capability_selection: req.capability_selection.clone(),
    }
}

async fn create_and_start_run(
    _data_store: &Arc<DataStore>,
    req: &RunStartRequest,
    normalized_attachments: Vec<Value>,
) -> RpcResponse {
    // S3 Execution Policy: resolved ONCE, before any daemon call. Run 创建后
    // snapshot 固化 — later Settings edits never affect this Run.
    let policy = match resolve_policy_for_run(req) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

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
    let create_req = build_create_run_request(
        req,
        &policy,
        &permission_profile,
        &user_content,
        daemon_attachments_opt.clone(),
        idempotency_key.clone(),
    );

    // Send create over the raw request path so `disabled_tools` (the S3
    // subtract-only deny list) rides in the create payload. The protocol's
    // CreateRunRequest does not yet carry the field (NEEDS-INTEGRATION: daemon
    // final subtraction), but the payload already contains it for when it does.
    let mut create_params = match serde_json::to_value(&create_req) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                "DAEMON_CREATE_FAILED",
                &format!("serialize create request: {e}"),
            )
        }
    };
    if let Some(obj) = create_params.as_object_mut() {
        obj.insert(
            "disabled_tools".into(),
            serde_json::to_value(&policy.disabled_tools).unwrap_or_default(),
        );
    }
    let daemon_run = match daemon_authority::request("run.create", create_params).await {
        Ok(value) => match serde_json::from_value::<assistant_protocol::v2::RunV2>(value) {
            Ok(run) => run,
            Err(e) => {
                return error_response("DAEMON_CREATE_FAILED", &format!("invalid run payload: {e}"))
            }
        },
        Err(e) => return error_response("DAEMON_CREATE_FAILED", &e),
    };

    // 固化 policy snapshot: the Run's immutable execution policy is persisted
    // so Resume/Retry and audits inherit it and later Settings edits cannot
    // change an existing Run. A snapshot write failure is surfaced as a
    // warning but does not fail the already-created Run (its maxSteps /
    // runtimeId are baked into the daemon row).
    if let Err(e) = store_policy_snapshot(&daemon_run.id, &policy) {
        eprintln!(
            "[run_gateway] policy snapshot for {} not persisted: {e}",
            daemon_run.id
        );
    }

    let start_req = build_start_run_request(
        req,
        &policy,
        &permission_profile,
        user_content,
        daemon_attachments_opt,
        &daemon_run.id,
    );
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
        "execution_policy": policy,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_engine_settings::{resolve_execution_policy, ExecutionEngineSettingsV2};

    fn req() -> RunStartRequest {
        RunStartRequest {
            conversation_id: "c1".into(),
            provider_id: "openai".into(),
            model_id: "gpt".into(),
            content: Some("hello".into()),
            effort: None,
            runtime_id: None,
            max_steps: None,
            attachments: Vec::new(),
            project_path: Some("/tmp".into()),
            agent_profile_id: None,
            capability_selection: None,
        }
    }

    fn native_descriptors(
        _settings: &ExecutionEngineSettingsV2,
    ) -> Vec<crate::execution_engine_settings::RuntimeDescriptor> {
        // Deterministic: native ready, claude degraded. No external process
        // probing in unit tests.
        use crate::execution_engine_settings::RuntimeDescriptor;
        let mut native = RuntimeDescriptor {
            id: crate::execution_engine_settings::RUNTIME_NATIVE.into(),
            display_name: "Native".into(),
            status: "ready".into(),
            version: None,
            authority: "native".into(),
            reason_code: "native_ready".into(),
            reason: "test".into(),
            capabilities: Default::default(),
            controllable: Vec::new(),
        };
        native
            .capabilities
            .insert("tools".into(), "supported".into());
        let claude = RuntimeDescriptor {
            id: crate::execution_engine_settings::RUNTIME_CLAUDE_CLI.into(),
            display_name: "Claude CLI".into(),
            status: "degraded".into(),
            version: None,
            authority: "external_bridge".into(),
            reason_code: "claude_cli_not_installed".into(),
            reason: "test".into(),
            capabilities: Default::default(),
            controllable: Vec::new(),
        };
        vec![native, claude]
    }

    /// Settings maxSteps must reach the created Run via the resolved policy →
    /// CreateRunRequest chain (S3 authority, no hardcoded 50).
    #[test]
    fn settings_max_steps_reaches_created_run() {
        let mut settings = ExecutionEngineSettingsV2::default().normalized();
        settings.native.max_steps = 137;
        let runtimes = native_descriptors(&settings);
        let policy =
            resolve_execution_policy(&settings, &runtimes, None, None, None).expect("resolve");
        assert_eq!(policy.max_steps, 137);

        let r = req();
        let create = build_create_run_request(&r, &policy, "ask", "hello", None, "k1".into());
        assert_eq!(
            create.max_steps,
            Some(137),
            "settings maxSteps must reach create request"
        );
        assert_ne!(create.max_steps, Some(50), "hardcoded 50 must be gone");

        let start = build_start_run_request(&r, &policy, "ask", "hello".into(), None, "run-1");
        assert_eq!(
            start.max_steps,
            Some(137),
            "settings maxSteps must reach start request"
        );
    }

    /// Settings default runtime must reach the created Run (runtime_id on both
    /// create and start), and an explicit run override wins over settings.
    #[test]
    fn settings_default_runtime_reaches_created_run() {
        let mut settings = ExecutionEngineSettingsV2::default().normalized();
        settings.default_runtime = crate::execution_engine_settings::RUNTIME_CLAUDE_CLI.to_string();
        let runtimes = native_descriptors(&settings);
        // Application default resolves; claude is degraded → fail policy errors.
        // Use fallback_native to prove the source/fallback chain is honored and
        // the policy's resolved runtime (native) reaches the request.
        settings.external_unavailable_policy = "fallback_native".to_string();
        let policy =
            resolve_execution_policy(&settings, &runtimes, None, None, None).expect("resolve");
        assert!(policy.fallback_used);
        assert_eq!(
            policy.runtime_id,
            crate::execution_engine_settings::RUNTIME_NATIVE
        );

        let r = req();
        let create = build_create_run_request(&r, &policy, "ask", "hello", None, "k1".into());
        assert_eq!(
            create.runtime_id.as_deref(),
            Some(crate::execution_engine_settings::RUNTIME_NATIVE),
            "resolved runtime must reach create request"
        );
        let start = build_start_run_request(&r, &policy, "ask", "hello".into(), None, "run-1");
        assert_eq!(
            start.runtime_id.as_deref(),
            Some(crate::execution_engine_settings::RUNTIME_NATIVE)
        );

        // Explicit run override wins over the application default.
        let explicit_policy = resolve_execution_policy(
            &settings,
            &runtimes,
            Some(crate::execution_engine_settings::RUNTIME_NATIVE),
            None,
            None,
        )
        .expect("explicit native resolves");
        assert_eq!(explicit_policy.runtime_source, "explicit_run");
        let create2 =
            build_create_run_request(&r, &explicit_policy, "ask", "hello", None, "k2".into());
        assert_eq!(
            create2.runtime_id.as_deref(),
            Some(crate::execution_engine_settings::RUNTIME_NATIVE)
        );
    }

    /// Explicit external runtime unavailable → hard fail BEFORE any create
    /// request is built (never a silent fallback).
    #[test]
    fn explicit_external_unavailable_fails_before_create() {
        let mut settings = ExecutionEngineSettingsV2::default().normalized();
        settings.external_unavailable_policy = "fallback_native".to_string();
        let runtimes = native_descriptors(&settings);
        let err = resolve_execution_policy(
            &settings,
            &runtimes,
            Some(crate::execution_engine_settings::RUNTIME_CLAUDE_CLI),
            None,
            None,
        )
        .expect_err("explicit unavailable must fail even with fallback_native");
        assert!(err.contains(crate::execution_engine_settings::RUNTIME_CLAUDE_CLI));
    }
}

// ─── Permission (host still dual-writes until task-04 integration) ───
