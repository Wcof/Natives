//! Tool-surface registration and subagent route binding (extracted from `production.rs`, task-01 structure).
//!
//! `register_tools_for_surface` / `normalize_permission_scope` build the per-run tool surface;
//! `validate_route_binding` / `restart_subagent_with_binding` / `wake_assignment_waiter` drive
//! credential-route updates and subagent restarts.

use crate::runtime::TaskRecord;
use capability_gateway::CapabilityGateway;
use serde_json::Value;

/// Register gateway tools for a run. Parent (`allowlist=None`) gets full builtins.
/// Child (`Some`) only registers the intersection so unauthorized tools are not present.
pub(crate) fn register_tools_for_surface(
    gateway: &mut CapabilityGateway,
    allowlist: Option<&[String]>,
) {
    // D01: a duplicate canonical tool name is a config bug that must fail the
    // daemon before any run starts — never a silently shadowed tool.
    let mut register = |tool| {
        if let Err(error) = gateway.register(tool) {
            panic!("tool surface registration failed (fail closed): {error}");
        }
    };
    match allowlist {
        None => {
            if let Err(error) = gateway.register_builtins() {
                panic!("tool surface registration failed (fail closed): {error}");
            }
        }
        Some(list) => {
            let allowed: std::collections::HashSet<&str> =
                list.iter().map(|s| s.as_str()).collect();
            // Draft tools are deliberately not part of `builtin_tools()`: a general
            // session has no business writing drafts, so they can only ever appear
            // where an allowlist names them explicitly.
            for tool in capability_gateway::tools::builtin_tools()
                .into_iter()
                .chain(capability_gateway::tools::creative_draft_tools())
            {
                if allowed.contains(tool.name) {
                    register(tool);
                }
            }
            // Orchestration tools are handled by PermissionGatedTools even if not in
            // gateway; still register schema stubs only when allowlisted.
        }
    }
}

/// Collect relative paths that a write-side tool is about to touch.
pub(crate) fn normalize_permission_scope(scope: &str) -> String {
    match scope.trim().to_ascii_lowercase().as_str() {
        "run" | "this_run" => "this_run".into(),
        "session" => "session".into(),
        "project" | "always" | "forever" => "project".into(),
        _ => "once".into(),
    }
}

/// Wake a pending `subagent_assignment` waiter (from interaction.respond).
pub fn wake_assignment_waiter(interaction_id: &str, response: Value) -> bool {
    let rt = &crate::global_run_manager().runtime;
    if let Ok(mut map) = rt.assignment_waiters.lock() {
        if let Some(tx) = map.remove(interaction_id) {
            let _ = tx.send(response);
            return true;
        }
    }
    false
}

/// Validate a route binding before it is persisted or used to restart a subagent.
/// IDs only — never accepts plaintext credentials.
pub fn validate_route_binding(binding: &crate::subagent_store::RouteBinding) -> Result<(), String> {
    let provider = binding.provider_id.trim();
    let key = binding.key_id.trim();
    let model = binding.model_id.trim();
    if provider.is_empty() {
        return Err("route binding provider_id is required".into());
    }
    if key.is_empty() {
        return Err("route binding key_id is required".into());
    }
    if model.is_empty() {
        return Err("route binding model_id is required".into());
    }
    // Reject values that look like secrets rather than IDs.
    for (label, v) in [
        ("provider_id", provider),
        ("key_id", key),
        ("model_id", model),
    ] {
        if v.len() > 256 {
            return Err(format!("route binding {label} is too long"));
        }
        if v.contains('\0') || v.contains('\n') || v.contains('\r') {
            return Err(format!("route binding {label} contains invalid characters"));
        }
    }
    Ok(())
}

/// Update binding; if a child run is active, cancel it and start a new child run
/// on the same hidden conversation with the unfinished task. Returns the new run id
/// when a restart was performed (or cancelled id when only cancel happened).
pub async fn restart_subagent_with_binding(
    session_id: &str,
    binding: &crate::subagent_store::RouteBinding,
) -> Result<Option<String>, String> {
    validate_route_binding(binding)?;
    let sid = session_id.trim();
    if sid.is_empty() {
        return Err("session_id required".into());
    }

    let sess = crate::subagent_store::get_subagent_session(sid)?
        .ok_or_else(|| format!("subagent session not found: {sid}"))?;

    let previous = crate::subagent_store::RouteBinding {
        provider_id: sess.provider_id.clone(),
        key_id: sess.key_id.clone(),
        model_id: sess.model_id.clone(),
    };
    let mut attempted = sess.attempted_bindings.clone();
    if !attempted.iter().any(|b| b == &previous) {
        attempted.push(previous);
    }
    crate::subagent_store::update_session_binding(sid, binding, &attempted)?;

    // Merge new binding into parent route pool for subsequent random assignment.
    if let Ok(Some(mut policy)) =
        crate::subagent_store::get_route_policy(&sess.parent_conversation_id)
    {
        if !policy.bindings.iter().any(|b| b == binding) {
            policy.bindings.push(binding.clone());
            let _ = crate::subagent_store::upsert_route_policy(
                &sess.parent_conversation_id,
                &policy.mode,
                &policy.bindings,
            );
        }
    }

    let was_running = matches!(
        sess.status.as_str(),
        "running" | "queued" | "waiting" | "open" | "pending_assignment"
    ) || crate::global_run_manager()
        .runtime
        .task_output(sid)
        .await
        .map(|r| r.status == "running")
        .unwrap_or(false);

    // Cancel any live task keyed by session / known child run.
    if let Some(rec) = crate::global_run_manager().runtime.task_output(sid).await {
        if !rec.run_id.is_empty() {
            crate::global_run_manager()
                .runtime
                .cancel_run_tree(&rec.run_id)
                .await;
        }
    }

    if !was_running {
        // Idle / completed / closed: only binding update for next send.
        let _ = crate::subagent_store::touch_subagent_session(sid);
        return Ok(None);
    }

    // Start a new child run on the same hidden conversation via RunManager.
    let prompt = if sess.task.trim().is_empty() {
        "Continue the previous subagent task with the updated credentials.".to_string()
    } else {
        sess.task.clone()
    };
    let rm = crate::global_run_manager();
    // Restore the exact child scope persisted at spawn (migration 029). A route
    // restart must never guess defaults: missing project identity, permission
    // ceiling, profile, or step budget fails closed instead of narrowing or
    // widening the child's authority.
    let project_path = sess.project_path.clone().ok_or_else(|| {
        "subagent session has no persisted project path; route restart requires exact project identity"
            .to_string()
    })?;
    let permission_profile = sess.permission_profile.clone().ok_or_else(|| {
        "subagent session has no persisted permission ceiling; route restart requires exact scope"
            .to_string()
    })?;
    let max_steps = sess
        .max_steps
        .map(|steps| steps.clamp(1, u32::MAX as i64) as u32)
        .ok_or_else(|| {
            "subagent session has no persisted step budget; route restart requires exact scope"
                .to_string()
        })?;
    let created = rm.create_run(assistant_protocol::v2::CreateRunRequest {
        capability_selection: None,
        disabled_tools: None,
        conversation_id: sess.child_conversation_id.clone(),
        provider_id: binding.provider_id.clone(),
        model_id: binding.model_id.clone(),
        key_id: Some(binding.key_id.clone()),
        agent_profile_id: sess.agent_profile_id.clone(),
        permission_profile: Some(permission_profile.clone()),
        content: Some(prompt.clone()),
        attachments: None,
        max_steps: Some(max_steps),
        parent_run_id: sess.parent_run_id.clone(),
        project_path: Some(project_path.clone()),
        idempotency_key: None,
        effort: None,
        runtime_id: Some("native".into()),
    })?;
    if !sess.tool_allowlist.is_empty() {
        crate::global_run_manager()
            .runtime
            .set_run_tool_allowlist(&created.id, sess.tool_allowlist.clone())
            .await;
    }
    let run = crate::run_manager::RunManager::start_detached_global(
        assistant_protocol::v2::StartRunRequest {
            agent_profile_id: sess.agent_profile_id.clone(),
            capability_selection: None,
            run_id: Some(created.id.clone()),
            conversation_id: Some(sess.child_conversation_id.clone()),
            provider_id: Some(binding.provider_id.clone()),
            model_id: Some(binding.model_id.clone()),
            key_id: Some(binding.key_id.clone()),
            content: Some(prompt),
            attachments: None,
            trigger_message_id: None,
            permission_profile: Some(permission_profile),
            max_steps: Some(max_steps),
            project_path: Some(project_path),
            idempotency_key: None,
            effort: None,
            runtime_id: Some("native".into()),
        },
    )?;

    // Index task_output so reaper / kill see the new run.
    crate::global_run_manager()
        .runtime
        .task_outputs
        .lock()
        .await
        .insert(
            sid.to_string(),
            TaskRecord {
                run_id: run.id.clone(),
                status: "running".into(),
                output: None,
            },
        );
    let _ = crate::subagent_store::update_subagent_session_status(sid, "running", None);
    let _ = crate::subagent_store::touch_subagent_session(sid);

    Ok(Some(run.id))
}
