//! Permission-gated tool runtime (extracted from `production.rs`, task-01 structure).
//!
//! `PermissionGatedTools` is the `EngineToolRuntime` the AgentEngine drives: it
//! enforces the allowlist, ProjectIdentity verification, permission Ask/Allow/Deny,
//! subagent budgets, checkpoints, and the task/mcp orchestration tools. Behavior is
//! unchanged from the pre-split single-file version.

use agent_core::{
    cap_child_permission, default_subagent_tool_allowlist, AgentEngine, EngineToolRuntime,
    EventSequencer, PermissionManager, PermissionProfile, SubAgentManager, SubAgentStatus,
    ToolExecutionResult, ToolSchema,
};
use assistant_protocol::v2::RunEventKind;
use capability_gateway::{CapabilityGateway, SideEffect};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::production::{normalize_permission_scope, ProductionRuntime, TaskRecord};

/// Tools with permission gate + real task orchestration.
pub struct PermissionGatedTools {
    pub gateway: Arc<CapabilityGateway>,
    pub permissions: Arc<PermissionManager>,
    pub events: EventSequencer,
    pub interactions: Arc<crate::runtime::InteractionHub>,
    pub subagents: Arc<SubAgentManager>,
    pub task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    /// Shared with ProductionRuntime so kill_task / cascade can cancel live engines.
    pub engines: Arc<Mutex<HashMap<String, Arc<AgentEngine>>>>,
    pub runtime: Option<Arc<ProductionRuntime>>,
    pub provider_id: String,
    /// Real parent run key_id when known (never `"auto"`).
    pub key_id: Option<String>,
    pub parent_run_id: String,
    pub conversation_id: String,
    pub model_id: String,
    pub permission_profile: String,
    /// `None` = parent/unrestricted surface (permission profile still applies).
    /// `Some` = hard allowlist; tools outside the list are hidden and denied.
    pub tool_allowlist: Option<Vec<String>>,
}
impl PermissionGatedTools {
    fn tool_allowed(&self, name: &str) -> bool {
        match &self.tool_allowlist {
            None => true,
            Some(list) => {
                if list.iter().any(|t| t == name) {
                    return true;
                }
                // MCP surface: allow only when explicitly listed as `mcp_call` or exact name.
                if name.starts_with("mcp__") {
                    return list.iter().any(|t| t == "mcp_call" || t == name);
                }
                false
            }
        }
    }

    fn deny_not_allowlisted(name: &str) -> ToolExecutionResult {
        ToolExecutionResult {
            output: serde_json::json!({
                "error": format!("tool `{name}` not in subagent tool_allowlist"),
                "denied": true,
                "code": "tool_not_allowlisted",
            }),
            is_error: true,
            duration_ms: 0,
        }
    }
}
#[async_trait::async_trait]
impl EngineToolRuntime for PermissionGatedTools {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        self.gateway
            .list_tools()
            .into_iter()
            .filter(|t| self.tool_allowed(t.name))
            .map(|t| ToolSchema {
                name: t.name.to_string(),
                description: t.description.to_string(),
                input_schema: t.schema.clone(),
            })
            .collect()
    }

    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
    ) -> ToolExecutionResult {
        if cancel.is_cancelled() {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "cancelled"}),
                is_error: true,
                duration_ms: 0,
            };
        }

        // Hard allowlist gate before permission / orchestration (Phase 0).
        if !self.tool_allowed(name) {
            return Self::deny_not_allowlisted(name);
        }

        // ProjectIdentity fail-closed for mutating/process/network/MCP tools.
        if let Err(err) = self.ensure_verified_project_for_tool(name).await {
            return ToolExecutionResult {
                output: serde_json::json!({"error": err, "denied": true, "code": "PROJECT_IDENTITY_REQUIRED"}),
                is_error: true,
                duration_ms: 0,
            };
        }

        // Subagent tool-call budget (production hook).
        if let Some((child_id, tree_root)) = self.subagent_budget_ids().await {
            if let Err(e) = self
                .subagents
                .consume_tool_call(&child_id, &tree_root)
                .await
            {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": e,
                        "denied": true,
                        "code": "subagent_tool_budget_exhausted",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        let tool = self.gateway.get_tool(name);
        let is_mcp = name == "mcp_call" || name.starts_with("mcp__");
        if tool.is_none()
            && !matches!(name, "task" | "task_output" | "kill_task" | "mcp_call")
            && !name.starts_with("mcp__")
        {
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("unknown tool: {name}")}),
                is_error: true,
                duration_ms: 0,
            };
        }

        // Permission gate: PermissionClass + SideEffect (G5).
        // Orchestration tools always go through this gate (never early-return around it).
        let profile_str = match self.permission_profile.as_str() {
            "full_access" | "autonomous" | "full" => "autonomous",
            "readonly" => "readonly",
            _ => "ask",
        };
        let (perm_class, side_effect) = if let Some(tool) = tool {
            (tool.permission_class, tool.side_effect)
        } else if is_mcp {
            (
                capability_gateway::PermissionClass::ExternalWrite,
                SideEffect::Network,
            )
        } else {
            // Fail-closed defaults if somehow unregistered.
            match name {
                "task_output" => (
                    capability_gateway::PermissionClass::AlwaysAllowed,
                    SideEffect::ReadOnly,
                ),
                "task" | "kill_task" => (
                    capability_gateway::PermissionClass::ProjectWrite,
                    SideEffect::Process,
                ),
                _ => (
                    capability_gateway::PermissionClass::Elevation,
                    SideEffect::Destructive,
                ),
            }
        };
        let class_result = capability_gateway::policy::check_permission(perm_class, profile_str);
        let needs_ask = match class_result {
            capability_gateway::policy::PolicyResult::Allowed => false,
            capability_gateway::policy::PolicyResult::NeedsApproval(_)
            | capability_gateway::policy::PolicyResult::Denied(_) => true,
        };

        if profile_str == "readonly" && !matches!(side_effect, SideEffect::ReadOnly) {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "readonly profile denies side effects", "denied": true}),
                is_error: true,
                duration_ms: 0,
            };
        }

        if needs_ask {
            if let Some(denied) = self.await_tool_permission(name, &input).await {
                return denied;
            }
        }

        // Orchestration tools after permission.
        if name == "task" {
            return self.execute_task(input).await;
        }
        if name == "task_output" {
            let id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let rec = self.task_outputs.lock().await.get(id).cloned();
            return ToolExecutionResult {
                output: serde_json::to_value(rec)
                    .unwrap_or(serde_json::json!({"status":"unknown"})),
                is_error: false,
                duration_ms: 0,
            };
        }
        if name == "kill_task" {
            let id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let ok = self.kill_task_tree(id).await;
            return ToolExecutionResult {
                output: serde_json::json!({"cancelled": ok, "task_id": id}),
                is_error: !ok,
                duration_ms: 0,
            };
        }
        // MCP tools: always after permission gate (ExternalWrite / Network).
        if name == "mcp_call" || name.starts_with("mcp__") {
            return self.execute_mcp_call(name, input).await;
        }

        let Some(_tool) = tool else {
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("unknown tool: {name}")}),
                is_error: true,
                duration_ms: 0,
            };
        };

        // Inject project cwd for terminal when missing (sandbox).
        let mut input = input;
        if name == "run_terminal" {
            if input.get("cwd").and_then(|v| v.as_str()).is_none() {
                if let Some(root) = &self.gateway.project_root {
                    if let Some(obj) = input.as_object_mut() {
                        obj.insert("cwd".into(), Value::String(root.clone()));
                    }
                }
            }
        }

        // Phase 3: lazy before-image for write tools (write_file / apply_patch).
        let write_paths = extract_write_paths(name, &input);
        for rel in &write_paths {
            let _ = crate::checkpoint::global_checkpoint_manager()
                .capture_before(&self.parent_run_id, rel);
        }

        let started = Instant::now();
        // Stable id for tool_output_delta correlation (engine also emits its own
        // tool_call_* ids; UI merges by tool_call_id when present on deltas).
        let stream_tool_call_id = uuid::Uuid::new_v4().to_string();

        // Create tool call context
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(CancellationToken::new)
        } else {
            CancellationToken::new()
        };
        let tool_context = self
            .build_tool_call_context(stream_tool_call_id.clone(), cancel)
            .await;

        match self
            .gateway
            .execute(name, input.clone(), &tool_context)
            .await
        {
            Ok(out) => {
                // Side-effect ledger for restore coverage honesty.
                let cat = crate::side_effect_ledger::category_for_tool(name);
                let reversible = cat == "workspace_file";
                let _ = crate::side_effect_ledger::record_tool_effect(
                    &self.parent_run_id,
                    name,
                    cat,
                    &input,
                    reversible,
                    None,
                );
                if name == "run_terminal" {
                    emit_terminal_output_deltas(
                        &self.events,
                        &self.parent_run_id,
                        &stream_tool_call_id,
                        &out.result,
                    );
                    // Background shell tasks: surface on Activity task list.
                    if out
                        .result
                        .get("background")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        let task_id = out
                            .result
                            .get("task_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&stream_tool_call_id)
                            .to_string();
                        let label = out
                            .result
                            .get("display_command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("terminal")
                            .to_string();
                        self.events.append(
                            &self.parent_run_id,
                            RunEventKind::TaskStarted {
                                task_id: task_id.clone(),
                                run_id: self.parent_run_id.clone(),
                                label,
                            },
                        );
                        if let Some(rt) = &self.runtime {
                            rt.task_outputs.lock().await.insert(
                                task_id,
                                TaskRecord {
                                    run_id: self.parent_run_id.clone(),
                                    status: "running".into(),
                                    output: out
                                        .result
                                        .get("output")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                },
                            );
                        }
                    }
                }
                for rel in &write_paths {
                    let _ = crate::checkpoint::global_checkpoint_manager()
                        .capture_after(&self.parent_run_id, rel);
                    // Best-effort FileChanged with before/after from checkpoint live map.
                    if let Ok(preview) = crate::checkpoint::global_checkpoint_manager()
                        .checkpoint_for_run_public(&self.parent_run_id)
                    {
                        if let Some(snap) = preview.files.iter().find(|f| &f.path == rel) {
                            self.events.append(
                                &self.parent_run_id,
                                RunEventKind::FileChanged {
                                    path: rel.clone(),
                                    change_type: if !snap.existed_before {
                                        "created".into()
                                    } else {
                                        "modified".into()
                                    },
                                    before: snap.before_content.clone(),
                                    after: snap.after_content.clone(),
                                    before_hash: snap.before_hash.clone(),
                                    after_hash: snap.after_hash.clone(),
                                    diff_artifact_id: None,
                                },
                            );
                        }
                    }
                }
                ToolExecutionResult {
                    output: out.result,
                    is_error: false,
                    duration_ms: out.duration_ms.max(started.elapsed().as_millis() as u64),
                }
            }
            Err(err) => ToolExecutionResult {
                output: serde_json::json!({"error": err.message, "code": err.code}),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            },
        }
    }
}
/// Emit batched terminal stdout/stderr as ToolOutputDelta (≤8KB chunks, ≤1MB total).
fn emit_terminal_output_deltas(
    events: &EventSequencer,
    run_id: &str,
    tool_call_id: &str,
    result: &Value,
) {
    const CHUNK: usize = 8 * 1024;
    const MAX_PERSIST: usize = 1024 * 1024;
    let stdout = result
        .get("stdout")
        .and_then(|v| v.as_str())
        .or_else(|| result.get("output").and_then(|v| v.as_str()))
        .unwrap_or("");
    let stderr = result.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
    let mut persisted = 0usize;
    for (stream, text) in [("stdout", stdout), ("stderr", stderr)] {
        if text.is_empty() {
            continue;
        }
        let bytes = text.as_bytes();
        let mut offset = 0usize;
        while offset < bytes.len() {
            if persisted >= MAX_PERSIST {
                events.append(
                    run_id,
                    RunEventKind::ToolOutputDelta {
                        tool_call_id: tool_call_id.to_string(),
                        stream: stream.into(),
                        text: String::new(),
                        truncated: true,
                    },
                );
                return;
            }
            let end = (offset + CHUNK).min(bytes.len());
            let take = (end - offset).min(MAX_PERSIST - persisted);
            let end = offset + take;
            let chunk = String::from_utf8_lossy(&bytes[offset..end]).into_owned();
            persisted += chunk.len();
            events.append(
                run_id,
                RunEventKind::ToolOutputDelta {
                    tool_call_id: tool_call_id.to_string(),
                    stream: stream.into(),
                    text: chunk,
                    truncated: persisted >= MAX_PERSIST || end < bytes.len() && take < CHUNK,
                },
            );
            offset = end;
            if take == 0 {
                break;
            }
        }
    }
}
fn extract_write_paths(name: &str, input: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    match name {
        "write_file" | "edit_file" => {
            if let Some(p) = input.get("path").and_then(|v| v.as_str()) {
                if !p.is_empty() && !p.contains("..") {
                    // Prefer project-relative: strip absolute if possible is caller's job.
                    paths.push(p.to_string());
                }
            }
        }
        "apply_patch" => {
            if let Some(arr) = input.get("files").and_then(|v| v.as_array()) {
                for f in arr {
                    if let Some(p) = f.get("path").and_then(|v| v.as_str()) {
                        if !p.is_empty() && !p.contains("..") {
                            paths.push(p.to_string());
                        }
                    }
                }
            }
            if let Some(p) = input.get("path").and_then(|v| v.as_str()) {
                if !p.is_empty() && !p.contains("..") {
                    paths.push(p.to_string());
                }
            }
        }
        _ => {}
    }
    paths
}
fn tool_pattern(name: &str, input: &Value) -> String {
    if name == "run_terminal" {
        input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    }
}
impl PermissionGatedTools {
    /// If this tools instance is running as a subagent child, return (child_run_id, tree_root).
    async fn subagent_budget_ids(&self) -> Option<(String, String)> {
        // parent_run_id field is the current run for this tools instance.
        let child_run = self.parent_run_id.clone();
        let run = crate::run_manager::global_run_manager().get_run(&child_run)?;
        let parent = run.parent_run_id.clone()?;
        // tree root = walk parents until none
        let mut root = parent.clone();
        let mut guard = 0;
        while guard < 32 {
            guard += 1;
            let Some(r) = crate::run_manager::global_run_manager().get_run(&root) else {
                break;
            };
            match r.parent_run_id {
                Some(p) => root = p,
                None => break,
            }
        }
        Some((child_run, root))
    }

    /// Resolve verified ProjectIdentity for the parent run (None if unbound/orphan).
    async fn verified_project_identity(&self) -> Option<crate::project_identity::ProjectIdentity> {
        let run = crate::run_manager::global_run_manager().get_run(&self.parent_run_id)?;
        let project_id = run.project_id.as_deref()?;
        // Prefer daemon DataStore used by RunManager (same assistant.db as create_run).
        let store = crate::run_manager::global_run_manager().data_store_ref()?;
        let conn = store.conn().ok()?;
        crate::project_identity::store::verify_for_invocation(&conn, project_id).ok()
    }

    async fn ensure_verified_project_for_tool(&self, name: &str) -> Result<(), String> {
        if !crate::runtime::tool_requires_verified_project(name) {
            return Ok(());
        }
        // Production: require verified ProjectIdentity when RunManager has a DataStore.
        // Fixture/unit tests (no data_store on global RunManager) keep gateway root as
        // workspace bound and are not fail-closed here — host create_run always binds
        // identity when a project path is provided.
        let has_store = crate::run_manager::global_run_manager()
            .data_store_ref()
            .is_some();
        if !has_store {
            return Ok(());
        }
        match self.verified_project_identity().await {
            Some(_) => Ok(()),
            None => Err(format!(
                "tool `{name}` requires a verified ProjectIdentity on the run (unbound/orphaned/fingerprint mismatch)"
            )),
        }
    }

    async fn build_tool_invocation(
        &self,
        name: &str,
        input: &Value,
    ) -> crate::runtime::ToolInvocation {
        if let Some(identity) = self.verified_project_identity().await {
            return crate::runtime::invocation_from_verified_identity(
                name,
                input,
                &self.conversation_id,
                &self.parent_run_id,
                Some(&self.conversation_id),
                &identity,
            );
        }
        crate::runtime::invocation_from_gate(
            name,
            input,
            &self.conversation_id,
            &self.parent_run_id,
            self.gateway.project_root.as_deref(),
        )
    }

    async fn build_tool_call_context(
        &self,
        tool_call_id: String,
        cancel: CancellationToken,
    ) -> capability_gateway::ToolCallContext {
        if let Some(identity) = self.verified_project_identity().await {
            return capability_gateway::ToolCallContext::from_verified_identity_with_cancel(
                identity.project_id.clone(),
                identity.identity_version,
                std::path::PathBuf::from(&identity.canonical_path),
                self.parent_run_id.clone(),
                self.conversation_id.clone(),
                tool_call_id,
                self.permission_profile.clone(),
                cancel,
            );
        }
        let project_root = self
            .gateway
            .project_root
            .as_ref()
            .map(|s| std::path::PathBuf::from(s))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        capability_gateway::ToolCallContext::with_cancel(
            project_root,
            self.parent_run_id.clone(),
            self.conversation_id.clone(),
            tool_call_id,
            self.permission_profile.clone(),
            cancel,
        )
    }

    async fn await_tool_permission(
        &self,
        name: &str,
        input: &Value,
    ) -> Option<ToolExecutionResult> {
        let pattern = tool_pattern(name, input);
        let inv = self.build_tool_invocation(name, input).await;
        // Skip ask when structured grant already covers this invocation.
        if let Some(rt) = &self.runtime {
            if matches!(
                rt.check_tool_grant_invocation(&inv).await,
                crate::runtime::GrantDecision::Allowed { .. }
            ) {
                return None;
            }
        }

        let tool_call_id = uuid::Uuid::new_v4().to_string();
        let permission_id = self
            .permissions
            .request_permission_for_profile(
                match self.permission_profile.as_str() {
                    "readonly" | "read_only" => PermissionProfile::ReadOnly,
                    "full_access" | "autonomous" | "full" => PermissionProfile::Autonomous,
                    _ => PermissionProfile::ConfirmEach,
                },
                &self.parent_run_id,
                &tool_call_id,
                name,
                format!("Approve {name}?"),
                input.clone(),
            )
            .await
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

        if permission_id == "auto-approved" {
            return None;
        }
        // Install the waiter before publishing the event. Otherwise a fast UI
        // (or test responder) can observe PermissionRequested, respond, and
        // lose the race before the channel exists, leaving the engine blocked.
        let (tx, rx) = oneshot::channel::<(bool, String)>();
        self.interactions
            .register_permission(&permission_id, &self.parent_run_id, name, tx)
            .await;
        // Best-effort: persist interaction row for restart recovery.
        let _ = crate::interaction_store::insert_pending(
            &permission_id,
            Some(&self.parent_run_id),
            Some(&self.conversation_id),
            "tool_permission",
            serde_json::json!({
                "tool_call_id": tool_call_id,
                "tool_name": name,
                "reason": format!("Approve tool `{name}`"),
                "input": input,
            }),
        );
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, Some(permission_id.clone()));
        let _ = crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id);
        self.events.append(
            &self.parent_run_id,
            RunEventKind::PermissionRequested {
                tool_call_id: tool_call_id.clone(),
                tool_name: name.to_string(),
                reason: format!("Approve tool `{name}`"),
                permission_id: permission_id.clone(),
                input: input.clone(),
            },
        );
        // Select permission response, timeout, and run cancel token (task-03).
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(CancellationToken::new)
        } else {
            CancellationToken::new()
        };
        let (approved, scope) = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = self.interactions.resolve_permission(&permission_id).await;
                (false, "cancelled".into())
            }
            res = tokio::time::timeout(Duration::from_secs(120), rx) => {
                res.ok().and_then(|r| r.ok()).unwrap_or((false, "once".into()))
            }
        };
        let scope = normalize_permission_scope(&scope);
        if approved {
            if let Some(rt) = &self.runtime {
                // Structured grant only — empty write_file pattern no longer means any path.
                rt.remember_tool_grant_invocation(&inv, &scope).await;
                let _ = pattern; // kept for legacy audit trails if needed
            }
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, None);
        let _ = crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id);
        self.events.append(
            &self.parent_run_id,
            RunEventKind::PermissionResponded {
                permission_id,
                approved,
                scope: scope.clone(),
            },
        );
        // Phase 2: AfterPermissionResolved is a documented safe point. Message
        // mutation lives in AgentEngine (apply_safe_point); the tool layer cannot
        // push into provider history here. Call the harness so the seam is live,
        // then re-queue any interjection so Engine's AfterTool/ProviderBatch can
        // inject it into messages (on_safe_point consumes pending).
        match crate::prompt_queue_store::on_safe_point(
            &self.conversation_id,
            agent_core::SafePoint::AfterPermissionResolved,
        ) {
            agent_core::HarnessAction::InjectInterjection { content } => {
                crate::prompt_queue_store::global_harness()
                    .interject(&self.conversation_id, content);
            }
            _ => {}
        }
        if approved {
            None
        } else {
            Some(ToolExecutionResult {
                output: serde_json::json!({"error": "permission denied", "denied": true}),
                is_error: true,
                duration_ms: 0,
            })
        }
    }

    /// Same tree-cancel rules as ProductionRuntime::cancel_run_tree, using
    /// the shared engines/subagents/events maps held by this tool runtime.
    async fn cancel_run_tree_local(&self, run_id: &str) {
        if let Some(rt) = &self.runtime {
            rt.cancel_run_tree(run_id).await;
            return;
        }
        let descendants = self.subagents.list_descendants(run_id).await;
        let mut run_ids: Vec<String> = vec![run_id.to_string()];
        for d in &descendants {
            if !run_ids.contains(&d.run_id) {
                run_ids.push(d.run_id.clone());
            }
        }
        {
            let engines = self.engines.lock().await;
            for rid in &run_ids {
                if let Some(engine) = engines.get(rid) {
                    engine.request_cancel();
                }
            }
        }
        for d in &descendants {
            let _ = self
                .subagents
                .update_status(&d.id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(&d.id) {
                rec.status = "cancelled".into();
            }
        }
        let _ = self.subagents.cascade_cancel_metadata(run_id).await;
        // No terminal lifecycle events here — RunManager::cancel is the sole committer.
    }

    async fn kill_task_tree(&self, task_id: &str) -> bool {
        let child_run_id = self
            .task_outputs
            .lock()
            .await
            .get(task_id)
            .map(|r| r.run_id.clone());
        let Some(rid) = child_run_id else {
            return false;
        };
        self.cancel_run_tree_local(&rid).await;
        let _ = self
            .subagents
            .update_status(task_id, SubAgentStatus::Cancelled)
            .await;
        if let Some(rec) = self.task_outputs.lock().await.get_mut(task_id) {
            rec.status = "cancelled".into();
        }
        true
    }

    /// Route `mcp_call` / namespaced `mcp__server__tool` through daemon MCP runtime.
    async fn execute_mcp_call(&self, name: &str, input: Value) -> ToolExecutionResult {
        let started = Instant::now();
        let (server_id, tool_name, arguments) = if name == "mcp_call" {
            let server = input
                .get("server")
                .or_else(|| input.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool = input
                .get("tool")
                .or_else(|| input.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = input
                .get("arguments")
                .or_else(|| input.get("input"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            (server, tool, args)
        } else {
            // mcp__{server}__{tool}
            let rest = name.strip_prefix("mcp__").unwrap_or(name);
            let mut parts = rest.splitn(2, "__");
            let server = parts.next().unwrap_or("").to_string();
            let tool = parts.next().unwrap_or("").to_string();
            (server, tool, input)
        };
        if server_id.is_empty() || tool_name.is_empty() {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": "mcp_call requires server and tool",
                    "code": "invalid_input",
                }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
        let call_id = format!("mcp-{server_id}-{tool_name}");
        let display_name = format!("mcp_call:{server_id}/{tool_name}");
        // Structured audit event (permission already passed).
        self.events.append(
            &self.parent_run_id,
            RunEventKind::ToolCallStarted {
                id: call_id.clone(),
                name: display_name.clone(),
            },
        );
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(CancellationToken::new)
        } else {
            CancellationToken::new()
        };
        match crate::runtime::mcp_invocation::invoke_mcp_tool(
            &server_id,
            &tool_name,
            arguments,
            &cancel,
            Some(&self.parent_run_id),
        )
        .await
        {
            Ok(result) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                self.events.append(
                    &self.parent_run_id,
                    RunEventKind::ToolCallCompleted {
                        id: call_id,
                        name: display_name,
                        output: result.clone(),
                        is_error: false,
                        duration_ms,
                    },
                );
                // MCP is not auto-rollbackable — record for restore coverage honesty.
                let _ = crate::side_effect_ledger::record_tool_effect(
                    &self.parent_run_id,
                    "mcp_call",
                    "mcp",
                    &serde_json::json!({ "server": server_id, "tool": tool_name }),
                    false,
                    None,
                );
                ToolExecutionResult {
                    output: serde_json::json!({
                        "server": server_id,
                        "tool": tool_name,
                        "ok": true,
                        "result": result,
                    }),
                    is_error: false,
                    duration_ms,
                }
            }
            Err(e) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                self.events.append(
                    &self.parent_run_id,
                    RunEventKind::ToolCallCompleted {
                        id: call_id,
                        name: display_name,
                        output: serde_json::json!({"error": e}),
                        is_error: true,
                        duration_ms,
                    },
                );
                ToolExecutionResult {
                    output: serde_json::json!({
                        "server": server_id,
                        "tool": tool_name,
                        "ok": false,
                        "error": e,
                        "code": "mcp_call_failed",
                    }),
                    is_error: true,
                    duration_ms,
                }
            }
        }
    }

    async fn execute_task(&self, input: Value) -> ToolExecutionResult {
        let prompt = input
            .get("prompt")
            .or_else(|| input.get("task"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if prompt.trim().is_empty() {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "prompt required for task"}),
                is_error: true,
                duration_ms: 0,
            };
        }
        // Credential fields from the model are intentionally ignored (route policy assigns).
        let _ignored_provider = input.get("provider_id");
        let _ignored_key = input.get("key_id");
        let _ignored_model = input.get("model_id");

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Child permission never exceeds parent; default request is ask (not full_access).
        let requested_perm = input
            .get("permission_profile")
            .and_then(|v| v.as_str())
            .unwrap_or("ask");
        let child_perm = cap_child_permission(&self.permission_profile, requested_perm);
        // Explicit tool_allowlist on task input, else default readonly surface.
        let child_allowlist: Vec<String> = if let Some(arr) = input.get("tool_allowlist") {
            arr.as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(default_subagent_tool_allowlist)
        } else {
            default_subagent_tool_allowlist()
        };

        // Prefer binding injected by execute_task_batch; else resolve (single-task path).
        let binding = if let Some(b) = input
            .get("_resolved_binding")
            .cloned()
            .and_then(|v| serde_json::from_value::<crate::subagent_store::RouteBinding>(v).ok())
        {
            b
        } else {
            match self.resolve_task_binding(&input).await {
                Ok(b) => b,
                Err(e) => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": e,
                            "code": "subagent_assignment_failed",
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            }
        };
        let child_provider = binding.provider_id.clone();
        let child_key = binding.key_id.clone();
        let child_model = binding.model_id.clone();

        // Persist hidden child conversation + subagent_session (real IDs).
        let _ = crate::conversation_store::ensure_conversation_stub(
            &self.conversation_id,
            &self.provider_id,
            &self.model_id,
            Some(&self.permission_profile),
            None,
        );
        let task_call_id = input
            .get("task_call_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let (session_id, child_conversation_id) =
            match crate::subagent_store::create_hidden_child_session(
                &self.conversation_id,
                Some(&self.parent_run_id),
                task_call_id,
                &name,
                &prompt,
                &binding,
                Some(&child_perm),
                self.gateway.project_root.as_deref(),
            ) {
                Ok(v) => v,
                Err(e) => {
                    if use_fixture_flag(&input) {
                        let sid = uuid::Uuid::new_v4().to_string();
                        let cid = uuid::Uuid::new_v4().to_string();
                        (sid, cid)
                    } else {
                        return ToolExecutionResult {
                            output: serde_json::json!({
                                "error": format!("create child session failed: {e}"),
                            }),
                            is_error: true,
                            duration_ms: 0,
                        };
                    }
                }
            };

        // Standard RunManager path: create_run + start_detached (no embedded Engine).
        let project_path = self.gateway.project_root.clone();
        let created = match crate::global_run_manager().create_run(
            assistant_protocol::v2::CreateRunRequest {
                conversation_id: child_conversation_id.clone(),
                provider_id: child_provider.clone(),
                model_id: child_model.clone(),
                key_id: Some(child_key.clone()),
                agent_profile_id: None,
                permission_profile: Some(child_perm.clone()),
                content: Some(prompt.clone()),
                attachments: None,
                max_steps: Some(15),
                parent_run_id: Some(self.parent_run_id.clone()),
                project_path: project_path.clone(),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            },
        ) {
            Ok(r) => r,
            Err(e) => {
                let _ =
                    crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&e));
                return ToolExecutionResult {
                    output: serde_json::json!({"error": format!("create child run failed: {e}")}),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        };
        let child_run_id = created.id.clone();

        // Metadata shares real run_id + persistent session id as task_id.
        // Depth from parent chain — never hardcode 1 (task-11).
        let depth = self.subagents.depth_for_child(&self.parent_run_id).await;
        let child = match self
            .subagents
            .register(
                session_id.clone(),
                child_run_id.clone(),
                &self.parent_run_id,
                prompt.clone(),
                depth,
                child_provider.clone(),
                child_key.clone(),
                child_model.clone(),
                child_perm.clone(),
                child_allowlist.clone(),
                None,
                Some("none".into()),
                project_path.clone(),
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                let _ =
                    crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&e));
                return ToolExecutionResult {
                    output: serde_json::json!({"error": e}),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        };
        let _ = self
            .subagents
            .update_status(&child.id, SubAgentStatus::Running)
            .await;
        let _ = crate::subagent_store::update_subagent_session_status(&session_id, "running", None);

        // Apply child tool surface before RunManager starts the engine.
        crate::global_run_manager()
            .runtime
            .set_run_tool_allowlist(&child_run_id, child_allowlist.clone())
            .await;

        self.events.append(
            &self.parent_run_id,
            RunEventKind::SubagentCreated {
                sub_run_id: child_run_id.clone(),
                agent_profile_id: None,
                task: prompt.clone(),
            },
        );

        let task_id = session_id.clone();
        self.task_outputs.lock().await.insert(
            task_id.clone(),
            TaskRecord {
                run_id: child_run_id.clone(),
                status: "running".into(),
                output: None,
            },
        );

        let start_result = crate::run_manager::RunManager::start_detached_global(
            assistant_protocol::v2::StartRunRequest {
                run_id: Some(child_run_id.clone()),
                conversation_id: Some(child_conversation_id.clone()),
                provider_id: Some(child_provider.clone()),
                model_id: Some(child_model.clone()),
                key_id: Some(child_key.clone()),
                content: Some(prompt.clone()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some(child_perm.clone()),
                max_steps: Some(15),
                project_path: project_path.clone(),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            },
        );
        if let Err(e) = start_result {
            let _ = crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&e));
            if let Some(rec) = self.task_outputs.lock().await.get_mut(&task_id) {
                rec.status = "failed".into();
                rec.output = Some(e.clone());
            }
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("start child run failed: {e}")}),
                is_error: true,
                duration_ms: 0,
            };
        }

        // Background watcher: when RunManager marks the run terminal, update session/task.
        let session_id_bg = session_id.clone();
        let task_id_bg = task_id.clone();
        let child_run_id_bg = child_run_id.clone();
        let parent_run_id = self.parent_run_id.clone();
        let events = self.events.clone();
        let subagents = self.subagents.clone();
        let task_outputs = self.task_outputs.clone();
        let mem_task_id_bg = child.id.clone();
        let child_timeout_ms = self.subagents.config().child_timeout_ms.max(1);
        let tree_root_for_budget = self.parent_run_id.clone();
        let subagents_for_budget = self.subagents.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_millis(child_timeout_ms);
            for _ in 0..3_600 {
                if tokio::time::Instant::now() >= deadline {
                    // Timeout → unified cancel tree for the child.
                    crate::global_run_manager()
                        .runtime
                        .cancel_run(&child_run_id_bg)
                        .await;
                    let _ = crate::global_run_manager()
                        .cancel(assistant_protocol::v2::CancelRunRequest {
                            run_id: child_run_id_bg.clone(),
                        })
                        .await;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
                let Some(run) = crate::global_run_manager().get_run(&child_run_id_bg) else {
                    continue;
                };
                let status = run.status.as_str().to_string();
                if !run.status.is_terminal() {
                    continue;
                }
                let text = events
                    .replay_after(&child_run_id_bg, 0)
                    .into_iter()
                    .filter_map(|e| match e.payload {
                        RunEventKind::TextDelta { text } => Some(text),
                        _ => None,
                    })
                    .collect::<String>();
                if status == "completed" {
                    // Best-effort token settle from usage events + text estimate.
                    let usage_tokens: u64 = events
                        .replay_after(&child_run_id_bg, 0)
                        .into_iter()
                        .filter_map(|e| match e.payload {
                            RunEventKind::UsageUpdated {
                                input_tokens,
                                output_tokens,
                                ..
                            } => Some(input_tokens.saturating_add(output_tokens)),
                            _ => None,
                        })
                        .max()
                        .unwrap_or_else(|| (text.len() as u64 / 4).max(1));
                    let _ = subagents_for_budget
                        .settle_tokens(&child_run_id_bg, &tree_root_for_budget, usage_tokens)
                        .await;
                    let _ = subagents
                        .update_status(&mem_task_id_bg, SubAgentStatus::Completed)
                        .await;
                    let _ = crate::subagent_store::update_subagent_session_status(
                        &session_id_bg,
                        "completed",
                        None,
                    );
                    events.append(
                        &parent_run_id,
                        RunEventKind::SubagentCompleted {
                            sub_run_id: child_run_id_bg.clone(),
                            result: text.clone(),
                        },
                    );
                } else {
                    let err_msg = run.error_code.clone().unwrap_or_else(|| status.clone());
                    let _ = subagents
                        .update_status(&mem_task_id_bg, SubAgentStatus::Failed(err_msg.clone()))
                        .await;
                    let _ = crate::subagent_store::close_subagent_session(
                        &session_id_bg,
                        if status == "cancelled" || status == "interrupted" {
                            "cancelled"
                        } else {
                            "failed"
                        },
                        Some(&err_msg),
                    );
                    events.append(
                        &parent_run_id,
                        RunEventKind::SubagentFailed {
                            sub_run_id: child_run_id_bg.clone(),
                            error: err_msg,
                        },
                    );
                }
                let rec = TaskRecord {
                    run_id: child_run_id_bg.clone(),
                    status,
                    output: if text.is_empty() { None } else { Some(text) },
                };
                task_outputs.lock().await.insert(task_id_bg, rec);
                break;
            }
        });

        ToolExecutionResult {
            output: serde_json::json!({
                "task_id": task_id,
                "run_id": child_run_id,
                "conversation_id": child_conversation_id,
                "session_id": session_id,
                "status": "running",
                "provider_id": child_provider,
                "key_id": child_key,
                "model_id": child_model,
                "permission_profile": child_perm,
            }),
            is_error: false,
            duration_ms: 0,
        }
    }

    fn default_binding(&self) -> crate::subagent_store::RouteBinding {
        crate::subagent_store::RouteBinding {
            provider_id: self.provider_id.clone(),
            key_id: self
                .key_id
                .clone()
                .filter(|k| !k.trim().is_empty() && !k.eq_ignore_ascii_case("auto"))
                .unwrap_or_default(),
            model_id: self.model_id.clone(),
        }
    }

    /// One assignment interaction for the entire task batch (full tasks[] + default_binding).
    /// Returns call_id → binding map (default mode maps every call_id to default_binding).
    async fn resolve_batch_assignment(
        &self,
        tasks: &[(String, String, String)],
    ) -> Result<std::collections::HashMap<String, crate::subagent_store::RouteBinding>, String>
    {
        use std::collections::HashMap;

        let use_fixture = std::env::var("NATIVES_DAEMON_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let default_binding = self.default_binding();

        // Existing policy: assign from pool without UI.
        if let Some(policy) = crate::subagent_store::get_route_policy(&self.conversation_id)
            .ok()
            .flatten()
        {
            let mut map = HashMap::new();
            let mut attempted = Vec::new();
            for (call_id, _, _) in tasks {
                let b = crate::subagent_store::pick_binding(&policy, &attempted)?;
                attempted.push(b.clone());
                // Prefer not repeating until pool exhausted (pick_binding already cycles).
                map.insert(call_id.clone(), b);
            }
            return Ok(map);
        }

        if use_fixture {
            // Offline unit tests: still ignore model-supplied credentials unless
            // NATIVES_DAEMON_FIXTURE_HONOR_TASK_CREDS=1 (legacy identity tests).
            let honor = std::env::var("NATIVES_DAEMON_FIXTURE_HONOR_TASK_CREDS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let mut map = HashMap::new();
            for (call_id, _, _) in tasks {
                let b = if honor {
                    // Caller may pass creds via ambient: use default_binding only;
                    // honor path is for resolve_task_binding single-task tests that
                    // still set fixture env — fall through to default.
                    default_binding.clone()
                } else {
                    crate::subagent_store::RouteBinding {
                        provider_id: if default_binding.provider_id.is_empty() {
                            "fixture".into()
                        } else {
                            default_binding.provider_id.clone()
                        },
                        key_id: if default_binding.key_id.is_empty() {
                            "fixture-key".into()
                        } else {
                            default_binding.key_id.clone()
                        },
                        model_id: if default_binding.model_id.is_empty() {
                            "fixture-model".into()
                        } else {
                            default_binding.model_id.clone()
                        },
                    }
                };
                map.insert(call_id.clone(), b);
            }
            return Ok(map);
        }

        let Some(rt) = self.runtime.clone() else {
            return Err(
                "no route policy and no runtime for subagent_assignment interaction".into(),
            );
        };

        // Only one assignment interaction per parent conversation at a time.
        let interaction_id = {
            let mut inflight = rt
                .assignment_inflight
                .lock()
                .map_err(|e| format!("assignment_inflight lock: {e}"))?;
            if let Some(existing) = inflight.get(&self.conversation_id) {
                // Another batch already waiting — still wait on same interaction.
                existing.clone()
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                inflight.insert(self.conversation_id.clone(), id.clone());
                id
            }
        };

        let (tx, rx) = oneshot::channel::<Value>();
        let mut installed = false;
        {
            let mut waiters = rt
                .assignment_waiters
                .lock()
                .map_err(|e| format!("assignment_waiters lock: {e}"))?;
            if !waiters.contains_key(&interaction_id) {
                waiters.insert(interaction_id.clone(), tx);
                installed = true;
                let batch_id = uuid::Uuid::new_v4().to_string();
                let tasks_payload: Vec<Value> = tasks
                    .iter()
                    .map(|(call_id, name, prompt)| {
                        serde_json::json!({
                            "call_id": call_id,
                            "name": name,
                            "prompt": prompt,
                        })
                    })
                    .collect();
                let payload = serde_json::json!({
                    "kind": "subagent_assignment",
                    "batch_id": batch_id,
                    "parent_conversation_id": self.conversation_id,
                    "parent_run_id": self.parent_run_id,
                    "conversation_id": self.conversation_id,
                    "run_id": self.parent_run_id,
                    "default_binding": {
                        "provider_id": default_binding.provider_id,
                        "key_id": default_binding.key_id,
                        "model_id": default_binding.model_id,
                    },
                    "tasks": tasks_payload,
                    "reason": "Assign provider/key/model for subagent tasks in this conversation",
                });
                let _ = crate::interaction_store::insert_pending(
                    &interaction_id,
                    Some(&self.parent_run_id),
                    Some(&self.conversation_id),
                    "subagent_assignment",
                    payload.clone(),
                );
                self.events.append(
                    &self.parent_run_id,
                    RunEventKind::InteractionRequested {
                        interaction_id: interaction_id.clone(),
                        kind: "subagent_assignment".into(),
                        payload,
                    },
                );
            } else {
                drop(tx);
            }
        }

        let response = if installed {
            match tokio::time::timeout(Duration::from_secs(120), rx).await {
                Ok(Ok(v)) => v,
                Ok(Err(_)) => {
                    if let Ok(mut i) = rt.assignment_inflight.lock() {
                        i.remove(&self.conversation_id);
                    }
                    return Err("subagent assignment cancelled".into());
                }
                Err(_) => {
                    if let Ok(mut w) = rt.assignment_waiters.lock() {
                        w.remove(&interaction_id);
                    }
                    if let Ok(mut i) = rt.assignment_inflight.lock() {
                        i.remove(&self.conversation_id);
                    }
                    return Err("subagent assignment timed out".into());
                }
            }
        } else {
            // Wait for policy written by the owner of the oneshot.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            loop {
                if let Some(policy) = crate::subagent_store::get_route_policy(&self.conversation_id)
                    .ok()
                    .flatten()
                {
                    let mut map = HashMap::new();
                    let mut attempted = Vec::new();
                    for (call_id, _, _) in tasks {
                        let b = crate::subagent_store::pick_binding(&policy, &attempted)?;
                        attempted.push(b.clone());
                        map.insert(call_id.clone(), b);
                    }
                    return Ok(map);
                }
                let still = rt
                    .assignment_inflight
                    .lock()
                    .map(|g| g.contains_key(&self.conversation_id))
                    .unwrap_or(false);
                if !still {
                    return Err("subagent assignment cancelled".into());
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err("subagent assignment timed out".into());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };

        // Cancel / deny → fail the batch (no silent default key).
        if response
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || response.get("approved").and_then(|v| v.as_bool()) == Some(false)
        {
            if let Ok(mut i) = rt.assignment_inflight.lock() {
                i.remove(&self.conversation_id);
            }
            return Err("subagent assignment cancelled".into());
        }

        let mode = response
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("default");

        // Prefer per-call assignments; fall back to pool/bindings.
        let assignments: Vec<Value> = response
            .get("assignments")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut map = HashMap::new();
        if mode == "default" {
            if default_binding.key_id.trim().is_empty() {
                if let Ok(mut i) = rt.assignment_inflight.lock() {
                    i.remove(&self.conversation_id);
                }
                return Err(
                    "default_binding.key_id missing on parent run; cannot confirm default mode"
                        .into(),
                );
            }
            crate::production::validate_route_binding(&default_binding)?;
            for (call_id, _, _) in tasks {
                map.insert(call_id.clone(), default_binding.clone());
            }
        } else if !assignments.is_empty() {
            for a in assignments {
                let call_id = a
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let b = crate::subagent_store::RouteBinding {
                    provider_id: a
                        .get("provider_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    key_id: a
                        .get("key_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    model_id: a
                        .get("model_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                };
                crate::production::validate_route_binding(&b)?;
                if !call_id.is_empty() {
                    map.insert(call_id, b);
                }
            }
            // Fill any missing call_ids from pool if present.
            if map.len() < tasks.len() {
                let pool: Vec<crate::subagent_store::RouteBinding> = response
                    .get("pool")
                    .or_else(|| response.get("bindings"))
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                let mut pi = 0usize;
                for (call_id, _, _) in tasks {
                    if map.contains_key(call_id) {
                        continue;
                    }
                    if pool.is_empty() {
                        break;
                    }
                    let b = pool[pi % pool.len()].clone();
                    crate::production::validate_route_binding(&b)?;
                    map.insert(call_id.clone(), b);
                    pi += 1;
                }
            }
        } else {
            let bindings: Vec<crate::subagent_store::RouteBinding> = response
                .get("pool")
                .or_else(|| response.get("bindings"))
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            if bindings.is_empty() {
                if let Ok(mut i) = rt.assignment_inflight.lock() {
                    i.remove(&self.conversation_id);
                }
                return Err("subagent assignment response missing bindings".into());
            }
            for b in &bindings {
                crate::production::validate_route_binding(b)?;
            }
            let mut pi = 0usize;
            for (call_id, _, _) in tasks {
                map.insert(call_id.clone(), bindings[pi % bindings.len()].clone());
                pi += 1;
            }
        }

        if map.is_empty() {
            if let Ok(mut i) = rt.assignment_inflight.lock() {
                i.remove(&self.conversation_id);
            }
            return Err("subagent assignment produced empty binding map".into());
        }

        // Persist pool for subsequent subagents (random/custom pool or default singleton).
        let pool_for_policy: Vec<crate::subagent_store::RouteBinding> = response
            .get("pool")
            .or_else(|| response.get("bindings"))
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| {
                map.values().cloned().fold(Vec::new(), |mut acc, b| {
                    if !acc.iter().any(|x| x == &b) {
                        acc.push(b);
                    }
                    acc
                })
            });
        if !pool_for_policy.is_empty() {
            let _ = crate::subagent_store::upsert_route_policy(
                &self.conversation_id,
                mode,
                &pool_for_policy,
            );
        }

        if let Ok(mut i) = rt.assignment_inflight.lock() {
            i.remove(&self.conversation_id);
        }
        Ok(map)
    }

    /// Single-task path: resolve one binding (uses batch assignment with one task).
    async fn resolve_task_binding(
        &self,
        _input: &Value,
    ) -> Result<crate::subagent_store::RouteBinding, String> {
        let call_id = _input
            .get("task_call_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let name = _input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let prompt = _input
            .get("prompt")
            .or_else(|| _input.get("task"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let map = self
            .resolve_batch_assignment(&[(call_id.clone(), name, prompt)])
            .await?;
        map.get(&call_id)
            .cloned()
            .or_else(|| map.into_values().next())
            .ok_or_else(|| "subagent assignment produced no binding".into())
    }
}
fn use_fixture_flag(input: &Value) -> bool {
    input
        .get("fixture")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || std::env::var("NATIVES_DAEMON_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}
