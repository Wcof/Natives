//! Production execution seams for the Agent Daemon Run Authority.
//!
//! Real providers (no Echo mock on production path), capability-gateway tools,
//! permission Ask/Allow/Deny, hooks, and subagent child runs.

use agent_core::assemble_context;
use agent_core::{
    cap_child_permission, default_subagent_tool_allowlist, AgentEngine, EngineError, EngineMessage,
    EngineProvider, EngineProviderEvent, EngineProviderEventStream, EngineRunConfig,
    EventSequencer, HookEvent, HookRegistry, HookRequest, PermissionManager, PermissionProfile,
    SubAgentConfig, SubAgentManager, SubAgentStatus, ToolSchema,
};
use assistant_protocol::v2::RunEventKind;
use capability_gateway::CapabilityGateway;
use futures_util::StreamExt;
use provider_adapters::capabilities::{
    history_message_to_provider, HistoryMessage, HistoryToolCall, ProviderAdapter, ProviderError,
    ProviderRequest, ProviderTool,
};
use provider_adapters::stream::ProviderEvent;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

/// Force-kill terminal processes owned by the global ProcessSupervisor (task-03).
struct GlobalProcessCancelHook;

#[async_trait::async_trait]
impl crate::runtime::execution_registry::ProcessCancelHook for GlobalProcessCancelHook {
    async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        use capability_gateway::ProcessSupervisor;
        capability_gateway::global_process_supervisor()
            .cancel(task_id)
            .await
            .map(|_| ())
    }

    async fn cancel_tasks_for_run(&self, run_id: &str) -> Result<(), String> {
        use capability_gateway::ProcessSupervisor;
        let sup = capability_gateway::global_process_supervisor();
        let snaps = sup.list_for_run(run_id).await;
        for s in snaps {
            let _ = sup.cancel(&s.task_id).await;
        }
        Ok(())
    }
}

/// Production runtime facade owned by the Daemon.
///
/// Deep state owners (task-01):
/// - `execution` → [`crate::runtime::ExecutionRegistry`]
/// - `tool_policy` → [`crate::runtime::ToolPolicyState`]
/// - permission and assignment waits → InteractionHub
/// - `subagents` / `task_outputs` → TaskSupervisor maps
/// - `engines` → migrating into ExecutionRegistry
///
/// Callers should use methods (`cancel_run`, `respond_permission`, `set_run_tool_allowlist`)
/// rather than reaching into maps when possible.
pub struct ProductionRuntime {
    pub events: EventSequencer,
    pub permissions: Arc<PermissionManager>,
    pub subagents: Arc<SubAgentManager>,
    // hooks: removed dead shared state — each start builds HookRegistry per project (task-01).
    /// Sole interaction waiter owner (permission + assignment).
    pub interactions: Arc<crate::runtime::InteractionHub>,
    /// task_id → child run status/output
    pub task_outputs: Arc<Mutex<HashMap<String, crate::runtime::TaskRecord>>>,
    pub engines: Arc<Mutex<HashMap<String, Arc<AgentEngine>>>>,
    /// Sole cancel-token + join/resource registry (task-03). Agent C relocates in task-01.
    pub execution: Arc<crate::runtime::ExecutionRegistry>,
    /// Structured tool grant policy (task-09). Agent C relocates in task-01.
    pub tool_policy: Arc<crate::runtime::ToolPolicyState>,
    /// interaction_id → oneshot for subagent_assignment batch waits.
    /// std mutex so interaction.respond can wake without re-entering tokio runtime.
    pub(crate) assignment_waiters: Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    /// Ensure only one assignment interaction is pending per parent conversation.
    pub(crate) assignment_inflight: Arc<std::sync::Mutex<HashMap<String, String>>>,
    /// Per-run tool allowlist registered before RunManager starts a child run.
    /// `Some(list)` = hard allowlist; entry removed once the run starts.
    pub run_tool_allowlists: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

#[cfg(test)]
pub use crate::production_credentials::clear_credential_broker_for_tests;
pub use crate::production_credentials::{
    install_credential_broker, resolve_credential, resolve_credential_for_run, CredentialBrokerFn,
};
pub use crate::production_hooks::{build_production_hooks, build_production_hooks_for_project};
pub use crate::production_tools::PermissionGatedTools;
pub use crate::runtime::TaskRecord;

// Hook assembly moved to `production_hooks.rs` (task-01 structure).

impl ProductionRuntime {
    pub fn new() -> Self {
        Self::new_with_events(EventSequencer::new())
    }

    pub fn new_with_event_store(data_store: Arc<crate::storage::DataStore>) -> Self {
        Self::new_with_events(EventSequencer::with_persistence(Arc::new(
            crate::event_log::EventLog::new(data_store),
        )))
    }

    fn new_with_events(events: EventSequencer) -> Self {
        let rt = Self {
            events,
            permissions: Arc::new(PermissionManager::new(PermissionProfile::ConfirmEach)),
            subagents: Arc::new(SubAgentManager::new(SubAgentConfig::default())),
            interactions: Arc::new(crate::runtime::InteractionHub::new()),
            task_outputs: Arc::new(Mutex::new(HashMap::new())),
            engines: Arc::new(Mutex::new(HashMap::new())),
            execution: Arc::new(crate::runtime::ExecutionRegistry::new()),
            tool_policy: Arc::new(crate::runtime::ToolPolicyState::new()),
            assignment_waiters: Arc::new(std::sync::Mutex::new(HashMap::new())),
            assignment_inflight: Arc::new(std::sync::Mutex::new(HashMap::new())),
            run_tool_allowlists: Arc::new(Mutex::new(HashMap::new())),
        };
        // Assignment bridges remain until the subagent interaction protocol is moved.
        let mut rt = rt;
        rt.assignment_waiters = rt.interactions.assignment_waiters_arc();
        rt.assignment_inflight = rt.interactions.assignment_inflight_arc();
        // Wire process supervisor force-kill into cancel tree (task-03).
        rt.execution
            .set_process_cancel_hook(Arc::new(GlobalProcessCancelHook));
        // Background reaper: idle subagent sessions.
        spawn_subagent_reaper();
        rt
    }

    /// Register a hard tool allowlist for a run that will be started via RunManager.
    /// Consumed once by [`Self::start_run`] / fixture start path.
    pub async fn set_run_tool_allowlist(&self, run_id: &str, allowlist: Vec<String>) {
        self.run_tool_allowlists
            .lock()
            .await
            .insert(run_id.to_string(), allowlist);
    }

    pub async fn take_run_tool_allowlist(&self, run_id: &str) -> Option<Vec<String>> {
        self.run_tool_allowlists.lock().await.remove(run_id)
    }

    // ─── Engine registry facade (task-01) ───

    /// Register an engine handle for a run.
    pub async fn register_engine(&self, run_id: &str, engine: Arc<AgentEngine>) {
        self.engines.lock().await.insert(run_id.to_string(), engine);
    }

    /// Remove engine handle. Returns true if it existed.
    pub async fn remove_engine(&self, run_id: &str) -> bool {
        self.engines.lock().await.remove(run_id).is_some()
    }

    /// Check if a run has a live engine.
    pub async fn has_engine(&self, run_id: &str) -> bool {
        self.engines.lock().await.contains_key(run_id)
    }

    /// Clone engine handles for PermissionGatedTools construction.
    pub async fn engine_handles(&self) -> Arc<Mutex<HashMap<String, Arc<AgentEngine>>>> {
        self.engines.clone()
    }

    // ─── Permission waiter facade (task-01 / task-04) ───

    /// Insert a permission waiter.
    pub async fn insert_permission_waiter(
        &self,
        id: &str,
        run_id: &str,
        tool_name: &str,
        tx: oneshot::Sender<(bool, String)>,
    ) {
        self.interactions
            .register_permission(id, run_id, tool_name, tx)
            .await;
    }

    /// Remove a permission waiter (on respond or cancel).
    pub async fn remove_permission_waiter(
        &self,
        id: &str,
    ) -> Option<(String, String, oneshot::Sender<(bool, String)>)> {
        self.interactions.resolve_permission(id).await
    }

    // ─── Task output facade (task-01) ───

    /// Clone task output map reference for PermissionGatedTools.
    pub fn task_outputs_ref(&self) -> Arc<Mutex<HashMap<String, TaskRecord>>> {
        self.task_outputs.clone()
    }

    /// Insert a task output record.
    pub async fn insert_task_output(&self, id: &str, record: TaskRecord) {
        self.task_outputs
            .lock()
            .await
            .insert(id.to_string(), record);
    }

    /// Remove a task output record.
    pub async fn remove_task_output(&self, id: &str) -> Option<TaskRecord> {
        self.task_outputs.lock().await.remove(id)
    }

    // ─── Assignment facade (task-01 / task-11) ───

    /// Clone assignment waiters map for PermissionGatedTools.
    pub fn assignment_waiters_ref(
        &self,
    ) -> Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>> {
        self.interactions.assignment_waiters_arc()
    }

    /// Clone assignment inflight map for subagent store.
    pub fn assignment_inflight_ref(&self) -> Arc<std::sync::Mutex<HashMap<String, String>>> {
        self.interactions.assignment_inflight_arc()
    }

    pub async fn set_permission_profile(&self, profile: &str) {
        let p = match profile {
            "readonly" | "read_only" => PermissionProfile::ReadOnly,
            "full_access" | "autonomous" | "full" => PermissionProfile::Autonomous,
            _ => PermissionProfile::ConfirmEach,
        };
        self.permissions.set_profile(p).await;
    }

    pub async fn respond_permission(
        &self,
        request_id: &str,
        approved: bool,
        run_id: Option<&str>,
        scope: Option<&str>,
    ) -> Result<(), String> {
        if request_id.trim().is_empty() {
            return Err("request_id required".into());
        }
        let scope = normalize_permission_scope(scope.unwrap_or("once"));
        let (_bound_run, _tool_name, tx) = self
            .interactions
            .resolve_permission_for_run(request_id, run_id)
            .await?;
        let _ = tx.send((approved, scope.clone()));
        // Best-effort: resolve any matching interaction row for restart recovery.
        let _ = crate::interaction_store::mark_resolved(
            request_id,
            serde_json::json!({ "approved": approved, "scope": scope }),
        );
        Ok(())
    }

    /// Record a durable structured grant after approval (task-09).
    ///
    /// `pattern` is treated as a hint for terminal command when input is not passed;
    /// prefer [`Self::remember_tool_grant_invocation`] when full input is available.
    pub async fn remember_tool_grant(
        &self,
        conversation_id: &str,
        run_id: &str,
        tool_name: &str,
        pattern: &str,
        scope: &str,
    ) {
        let input = if tool_name == "run_terminal" && !pattern.is_empty() {
            serde_json::json!({ "command": pattern, "cwd": "." })
        } else if matches!(tool_name, "write_file" | "read_file" | "apply_patch")
            && !pattern.is_empty()
        {
            serde_json::json!({ "path": pattern })
        } else {
            serde_json::json!({})
        };
        let inv =
            crate::runtime::invocation_from_gate(tool_name, &input, conversation_id, run_id, None);
        let _ = self
            .tool_policy
            .remember(&inv, scope, Some("permission_respond"))
            .await;
    }

    /// Remember grant from a full tool invocation (structured constraints).
    pub async fn remember_tool_grant_invocation(
        &self,
        inv: &crate::runtime::ToolInvocation,
        scope: &str,
    ) {
        let _ = self
            .tool_policy
            .remember(inv, scope, Some("permission_respond"))
            .await;
    }

    pub async fn has_tool_grant(
        &self,
        conversation_id: &str,
        run_id: &str,
        tool_name: &str,
        pattern: &str,
    ) -> bool {
        let input = if tool_name == "run_terminal" && !pattern.is_empty() {
            serde_json::json!({ "command": pattern, "cwd": "." })
        } else if matches!(tool_name, "write_file" | "read_file" | "apply_patch")
            && !pattern.is_empty()
        {
            serde_json::json!({ "path": pattern })
        } else {
            // Empty input for non-pattern tools: only exact empty-constraint grants match.
            serde_json::json!({})
        };
        let inv =
            crate::runtime::invocation_from_gate(tool_name, &input, conversation_id, run_id, None);
        matches!(
            self.tool_policy.check(&inv).await,
            crate::runtime::GrantDecision::Allowed { .. }
        )
    }

    pub async fn check_tool_grant_invocation(
        &self,
        inv: &crate::runtime::ToolInvocation,
    ) -> crate::runtime::GrantDecision {
        self.tool_policy.check(inv).await
    }

    /// Execute a production engine turn. Returns `EngineOutcome` only — never commits
    /// Run lifecycle status or terminal lifecycle events. RunManager is the sole committer.
    pub async fn start_run(
        &self,
        run_id: String,
        conversation_id: String,
        provider_id: String,
        model_id: String,
        key_id: Option<String>,
        permission_profile: String,
        agent_profile_id: Option<String>,
        user_content: String,
        max_steps: u32,
        project_path: Option<std::path::PathBuf>,
    ) -> Result<agent_core::EngineOutcome, String> {
        let project_root = project_path.ok_or_else(|| {
            "project_path is required for daemon runs; process cwd fallback is disabled".to_string()
        })?;
        // Full production hook set + project hooks for this workspace.
        let hooks = build_production_hooks_for_project(Some(&project_root));
        // Context budget: min(Profile tokenBudget, model context_window); default 128K.
        // chars/4 is only used when Provider usage is unavailable (engine estimate path).
        let profile = agent_profile_id
            .as_deref()
            .and_then(|id| agent_core::load_agent_profile(id, Some(&project_root)));
        let model_window = lookup_model_context_window(&provider_id, &model_id);
        let budget = agent_core::ContextBudget::resolve(
            profile.as_ref().and_then(|profile| profile.token_budget),
            model_window,
        );
        let cancel = self.ensure_execution_token(&run_id, None).await?;
        let engine = Arc::new(
            AgentEngine::new(self.events.clone())
                .with_cancel_token(cancel.clone())
                .with_hooks(hooks)
                .with_session_harness(crate::prompt_queue_store::global_harness())
                .with_context_budget(budget.history_compact_chars, budget.tool_output_max_chars),
        );
        self.engines
            .lock()
            .await
            .insert(run_id.clone(), engine.clone());

        // Phase 3: logical checkpoint at run start (lazy before-images on writes).
        if let Ok(cp_id) = crate::checkpoint::global_checkpoint_manager().begin_run(
            &run_id,
            &conversation_id,
            &project_root,
        ) {
            self.events.append(
                &run_id,
                RunEventKind::CheckpointCreated {
                    checkpoint_id: cp_id,
                    label: Some("run_start".into()),
                },
            );
        }
        // Mark coordinator running so terminal drain / cancel-and-send are scoped.
        crate::prompt_queue_store::global_harness().mark_running(
            &conversation_id,
            &run_id,
            &user_content,
        );
        if let Err(e) = crate::prompt_queue_store::persist_actor_snapshot(&conversation_id) {
            eprintln!("[production] persist_actor_snapshot on run start: {e}");
        }

        let provider = crate::routing::RoutedProvider::new(crate::routing::load_plan(
            provider_id.clone(),
            key_id.clone(),
            model_id.clone(),
        ));
        // Child subagent runs may have pre-registered a readonly (or custom) surface.
        // A built-in surface name (e.g. the creative session) resolves next; it has
        // no profile on disk, so this is the only place its allowlist can come from.
        let mut tool_allowlist = self
            .take_run_tool_allowlist(&run_id)
            .await
            .or_else(|| agent_profile_id.as_deref().and_then(builtin_surface_allowlist))
            .or_else(|| profile.as_ref().and_then(|profile| profile.tools.clone()));
        if let (Some(allowlist), Some(disallowed)) = (
            tool_allowlist.as_mut(),
            profile
                .as_ref()
                .and_then(|profile| profile.disallowed_tools.as_ref()),
        ) {
            allowlist.retain(|tool| !disallowed.iter().any(|denied| denied == tool));
        }
        let tools = PermissionGatedTools {
            gateway: {
                let mut g = CapabilityGateway::new();
                g.set_project_root(project_root.to_string_lossy().to_string());
                register_tools_for_surface(&mut g, tool_allowlist.as_deref());
                Arc::new(g)
            },
            permissions: self.permissions.clone(),
            events: self.events.clone(),
            interactions: self.interactions.clone(),
            subagents: self.subagents.clone(),
            task_outputs: self.task_outputs.clone(),
            engines: self.engines.clone(),
            // Assignment waiters live on the process-wide runtime.
            runtime: Some(crate::global_run_manager().runtime.clone()),
            provider_id: provider_id.clone(),
            key_id: key_id.clone(),
            parent_run_id: run_id.clone(),
            conversation_id: conversation_id.clone(),
            model_id: model_id.clone(),
            permission_profile: permission_profile.clone(),
            tool_allowlist,
        };

        let skill_prompt = crate::skill_store::prompt_for_project(&project_root);
        let mut assembled = assemble_context(
            profile.as_ref(),
            Some(&project_root),
            (!skill_prompt.is_empty()).then_some(skill_prompt.as_str()),
        );
        // Built-in surfaces have no profile on disk, so their working
        // instructions are prepended here. Project/skill context still applies.
        if let Some(surface_prompt) = agent_profile_id
            .as_deref()
            .and_then(builtin_surface_system_prompt)
        {
            assembled.system_prompt = if assembled.system_prompt.is_empty() {
                surface_prompt.to_string()
            } else {
                format!("{surface_prompt}\n\n{}", assembled.system_prompt)
            };
        }
        // Compact history against resolved token budget (chars/4 fallback estimate).
        let raw_history =
            crate::conversation_store::engine_history(&conversation_id).unwrap_or_default();
        let history_pairs: Vec<(String, String)> = raw_history
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        let (compacted, _) = agent_core::compact_messages(&history_pairs, budget.token_budget);
        // Map compacted (role, content) back to EngineMessage, preserving tool fields
        // for messages still present (match by role+content).
        let messages: Vec<EngineMessage> = compacted
            .into_iter()
            .map(|(role, content)| {
                if let Some(orig) = raw_history
                    .iter()
                    .find(|m| m.role == role && m.content == content)
                {
                    orig.clone()
                } else {
                    EngineMessage {
                        role,
                        content,
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    }
                }
            })
            .collect();
        let config = EngineRunConfig {
            run_id: run_id.clone(),
            conversation_id: conversation_id.clone(),
            model: model_id,
            system_prompt: if assembled.system_prompt.is_empty() {
                None
            } else {
                Some(assembled.system_prompt)
            },
            messages,
            user_content,
            max_steps,
        };

        let outcome = match engine.run(config, &provider, &tools).await {
            Ok(o) => o,
            Err(e) => agent_core::EngineOutcome::failed(e.code(), e.to_string(), e.retryable()),
        };
        // Finalize checkpoint — failure closes related side effects (no silent half-state).
        if let Err(e) = crate::checkpoint::global_checkpoint_manager().finalize_run(&run_id) {
            eprintln!("[production] checkpoint finalize_run failed: {e}");
        }
        let success = matches!(outcome, agent_core::EngineOutcome::Completed { .. });
        if success {
            crate::conversation_store::append_assistant_turn_from_events(
                &conversation_id,
                &run_id,
                &self.events.replay_after(&run_id, 0),
            )?;
        }
        // Do NOT commit_outcome or append terminal lifecycle events here.
        // RunManager is the sole lifecycle committer after this returns.
        self.engines.lock().await.remove(&run_id);

        // SessionCoordinator: drain next prompt / cancel-and-send after real terminal.
        // Never re-executes the just-finished run — only starts a *new* queued item.
        let _ =
            crate::prompt_queue_store::on_run_terminal(&conversation_id, &run_id, success).await;
        Ok(outcome)
    }

    /// Sole production cancel API: cancel `run_id` and every nested descendant
    /// child run via the ExecutionRegistry token tree (task-03).
    ///
    /// Order: signal tokens → wake waiters → grace → force process/join cleanup.
    /// Lifecycle status (`Cancelling`/`Cancelled`) is committed by RunManager,
    /// not here. Domain cleanup only.
    ///
    /// All cancel entry points (RPC, UI, kill_task, parent cancel) must call this.
    pub async fn cancel_run_tree(&self, run_id: &str) {
        // Prefer registry tree; fall back to subagent metadata for legacy paths.
        let mut run_ids = self.execution.list_tree(run_id).await;
        let descendants = self.subagents.list_descendants(run_id).await;
        for d in &descendants {
            if !run_ids.contains(&d.run_id) {
                run_ids.push(d.run_id.clone());
            }
        }
        if run_ids.is_empty() {
            run_ids.push(run_id.to_string());
        }

        // Signal cooperative cancel on registry tokens + engines.
        let _ = self.execution.signal_tree(run_id).await;
        {
            let engines = self.engines.lock().await;
            for rid in &run_ids {
                if let Some(engine) = engines.get(rid) {
                    engine.request_cancel();
                }
            }
        }

        // Wake permission / assignment waiters bound to this tree.
        self.cancel_waiters_for_runs(&run_ids).await;

        // Metadata + task_outputs for every descendant task.
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

        // Grace + force via registry (process kill, join abort).
        let outcome = self.execution.cancel_tree(run_id).await;
        if !outcome.quiet {
            eprintln!(
                "[production] cancel_tree cleanup incomplete for {run_id}: {:?}",
                outcome.errors
            );
        }

        // Drop engine handles for this tree after force phase.
        {
            let mut engines = self.engines.lock().await;
            for rid in &outcome.run_ids {
                engines.remove(rid);
            }
        }

        // No terminal lifecycle events here — RunManager::cancel commits Cancelled/Failed
        // after cleanup quiet. Domain cleanup only.
    }

    /// Alias for tree cancel — never cancel a single node without descendants.
    pub async fn cancel_run(&self, run_id: &str) {
        self.cancel_run_tree(run_id).await;
    }

    /// Daemon shutdown entry (task-13 wiring): cancel every root and wait quiet.
    pub async fn cancel_all_execution_roots(&self) -> Vec<crate::runtime::CancelCleanupOutcome> {
        self.execution.cancel_all_execution_roots().await
    }

    async fn cancel_waiters_for_runs(&self, run_ids: &[String]) {
        let stale = self.interactions.cancel_runs(run_ids).await;
        for pid in stale {
            let _ = crate::interaction_store::mark_resolved(
                &pid,
                serde_json::json!({ "approved": false, "scope": "once", "reason": "cancelled" }),
            );
        }
    }

    /// Ensure a run is registered in the cancel tree; returns its token.
    pub async fn ensure_execution_token(
        &self,
        run_id: &str,
        parent_run_id: Option<&str>,
    ) -> Result<CancellationToken, String> {
        let reg = if let Some(parent) = parent_run_id {
            if self.execution.is_registered(parent).await {
                self.execution.register_child(run_id, parent).await?
            } else {
                // Parent missing (legacy): still create child root but record parent link.
                self.execution
                    .register_with_token(run_id, Some(parent.to_string()), CancellationToken::new())
                    .await?
            }
        } else {
            self.execution.register_root(run_id).await?
        };
        Ok(reg.token)
    }

    pub async fn spawn_child_task(
        &self,
        parent_run_id: &str,
        prompt: String,
        provider_id: String,
        key_id: String,
        model_id: String,
        permission_profile: String,
        parent_permission_profile: &str,
        project_root: Option<String>,
    ) -> Result<String, String> {
        let child_perm = cap_child_permission(parent_permission_profile, &permission_profile);
        let child_allowlist = default_subagent_tool_allowlist();
        let subagent_hooks =
            build_production_hooks_for_project(project_root.as_deref().map(std::path::Path::new));
        let start_responses = subagent_hooks
            .dispatch(HookRequest {
                event: HookEvent::SubagentStart,
                run_id: parent_run_id.to_string(),
                tool_name: Some("task".into()),
                input: serde_json::json!({
                    "prompt": prompt.clone(),
                    "provider_id": provider_id.clone(),
                    "model_id": model_id.clone(),
                }),
            })
            .await;
        HookRegistry::aggregate_allow(&start_responses)
            .map_err(|reason| format!("subagent hook denied: {reason}"))?;
        // Depth from parent chain — never hardcode 1 (task-11).
        let depth = self.subagents.depth_for_child(parent_run_id).await;
        let child = self
            .subagents
            .spawn(
                parent_run_id,
                prompt.clone(),
                depth,
                provider_id.clone(),
                key_id.clone(),
                model_id.clone(),
                child_perm.clone(),
                child_allowlist.clone(),
                None,
                Some("none".into()),
                None,
            )
            .await?;
        self.subagents
            .update_status(&child.id, SubAgentStatus::Running)
            .await?;
        self.events.append(
            parent_run_id,
            RunEventKind::SubagentCreated {
                sub_run_id: child.run_id.clone(),
                agent_profile_id: child.agent_profile_id.clone(),
                task: prompt.clone(),
            },
        );
        let child_conversation_id = {
            // Best-effort persist; fall back to UUID (never subagent-* pseudo id).
            let binding = crate::subagent_store::RouteBinding {
                provider_id: provider_id.clone(),
                key_id: key_id.clone(),
                model_id: model_id.clone(),
            };
            match crate::subagent_store::create_hidden_child_session(
                // parent conversation unknown here — use synthetic parent stub from run if needed
                "orphan-parent",
                Some(parent_run_id),
                None,
                "",
                &prompt,
                &binding,
                Some(&child_perm),
                project_root.as_deref(),
            ) {
                Ok((_sid, cid)) => cid,
                Err(_) => uuid::Uuid::new_v4().to_string(),
            }
        };
        let child_conversation_bg = child_conversation_id.clone();
        let task_id = child.id.clone();
        let child_run_id = child.run_id.clone();
        let parent_owned = parent_run_id.to_string();
        let events = self.events.clone();
        let subagents = self.subagents.clone();
        let task_outputs = self.task_outputs.clone();
        let permissions = self.permissions.clone();
        let interactions = self.interactions.clone();
        let engines = self.engines.clone();
        let task_id_bg = task_id.clone();
        let child_allowlist_bg = child_allowlist;
        let child_perm_bg = child_perm;
        let project_root_bg = project_root;

        task_outputs.lock().await.insert(
            task_id.clone(),
            TaskRecord {
                run_id: child_run_id.clone(),
                status: "running".into(),
                output: None,
            },
        );

        tokio::spawn(async move {
            let child_project_root = project_root_bg.as_deref().map(std::path::PathBuf::from);
            let provider = RealProvider {
                provider_id: provider_id.clone(),
                key_id: Some(key_id.clone()),
            };
            let tools = PermissionGatedTools {
                gateway: {
                    let mut g = CapabilityGateway::new();
                    if let Some(root) = &project_root_bg {
                        g.set_project_root(root.clone());
                    }
                    register_tools_for_surface(&mut g, Some(&child_allowlist_bg));
                    Arc::new(g)
                },
                permissions,
                events: events.clone(),
                interactions,
                subagents: subagents.clone(),
                task_outputs: task_outputs.clone(),
                engines: engines.clone(),
                runtime: None,
                provider_id: provider_id.clone(),
                key_id: None,
                parent_run_id: child_run_id.clone(),
                conversation_id: child_conversation_bg.clone(),
                model_id: model_id.clone(),
                permission_profile: child_perm_bg,
                tool_allowlist: Some(child_allowlist_bg),
            };
            // Same production hook set as parent (M4) — not a reduced AllowAll-only registry.
            let hooks = build_production_hooks_for_project(child_project_root.as_deref());
            // Child cancel token is parent.child_token when registry has parent.
            let child_cancel = if let Some(parent_tok) = engines
                .lock()
                .await
                .get(&parent_owned)
                .map(|e| e.cancel_token())
            {
                parent_tok.child_token()
            } else {
                CancellationToken::new()
            };
            // Best-effort register under shared runtime if available via engines map only.
            let engine = Arc::new(
                AgentEngine::new(events.clone())
                    .with_cancel_token(child_cancel)
                    .with_hooks(hooks)
                    .with_session_harness(crate::prompt_queue_store::global_harness()),
            );
            engines
                .lock()
                .await
                .insert(child_run_id.clone(), engine.clone());
            let mut child_system =
                "You are a subagent with independent credentials. Complete the task.".to_string();
            if let Some(root) = child_project_root.as_deref() {
                let skills = crate::skill_store::prompt_for_project(root);
                if !skills.is_empty() {
                    child_system.push_str("\n\n");
                    child_system.push_str(&skills);
                }
            }
            let child_context =
                assemble_context(None, child_project_root.as_deref(), Some(&child_system));
            let config = EngineRunConfig {
                run_id: child_run_id.clone(),
                conversation_id: child_conversation_bg,
                model: model_id,
                system_prompt: Some(child_context.system_prompt),
                messages: Vec::new(),
                user_content: prompt,
                max_steps: 20,
            };
            let result = engine.run(config, &provider, &tools).await;
            engines.lock().await.remove(&child_run_id);
            let (status, output) = match &result {
                Ok(o) => {
                    let text = events
                        .replay_after(&child_run_id, 0)
                        .into_iter()
                        .filter_map(|e| match e.payload {
                            RunEventKind::TextDelta { text } => Some(text),
                            _ => None,
                        })
                        .collect::<String>();
                    let status = match o {
                        agent_core::EngineOutcome::Completed { .. } => "completed".to_string(),
                        agent_core::EngineOutcome::Failed { .. } => "failed".to_string(),
                        agent_core::EngineOutcome::Cancelled => "cancelled".to_string(),
                        agent_core::EngineOutcome::Interrupted { .. } => "interrupted".to_string(),
                    };
                    let _ =
                        crate::run_manager::global_run_manager().commit_outcome(&child_run_id, o);
                    (status, Some(text))
                }
                Err(e) => {
                    let outcome =
                        agent_core::EngineOutcome::failed(e.code(), e.to_string(), e.retryable());
                    let _ = crate::run_manager::global_run_manager()
                        .commit_outcome(&child_run_id, &outcome);
                    ("failed".into(), Some(e.to_string()))
                }
            };
            if status == "completed" {
                let _ = subagents
                    .update_status(&task_id_bg, SubAgentStatus::Completed)
                    .await;
                events.append(
                    &parent_owned,
                    RunEventKind::SubagentCompleted {
                        sub_run_id: child_run_id.clone(),
                        result: output.clone().unwrap_or_default(),
                    },
                );
            } else {
                let _ = subagents
                    .update_status(
                        &task_id_bg,
                        SubAgentStatus::Failed(output.clone().unwrap_or_default()),
                    )
                    .await;
                events.append(
                    &parent_owned,
                    RunEventKind::SubagentFailed {
                        sub_run_id: child_run_id.clone(),
                        error: output.clone().unwrap_or_else(|| status.clone()),
                    },
                );
            }
            let _ = subagent_hooks
                .dispatch(HookRequest {
                    event: HookEvent::SubagentStop,
                    run_id: parent_owned.clone(),
                    tool_name: Some("task".into()),
                    input: serde_json::json!({
                        "sub_run_id": child_run_id.clone(),
                        "status": status.clone(),
                        "output": output.clone(),
                    }),
                })
                .await;
            task_outputs.lock().await.insert(
                task_id_bg,
                TaskRecord {
                    run_id: child_run_id,
                    status,
                    output,
                },
            );
        });

        Ok(task_id)
    }

    pub async fn task_output(&self, task_id: &str) -> Option<TaskRecord> {
        self.task_outputs.lock().await.get(task_id).cloned()
    }

    /// Snapshot of process-local background tasks (`task_id` → record).
    pub async fn list_tasks(&self) -> Vec<(String, TaskRecord)> {
        self.task_outputs
            .lock()
            .await
            .iter()
            .map(|(id, rec)| (id.clone(), rec.clone()))
            .collect()
    }

    pub async fn kill_task(&self, task_id: &str) -> bool {
        let child_run_id = self
            .task_outputs
            .lock()
            .await
            .get(task_id)
            .map(|r| r.run_id.clone());
        if let Some(rid) = child_run_id {
            // Tree cancel on the child run (and any nested grandchildren).
            self.cancel_run_tree(&rid).await;
            let _ = self
                .subagents
                .update_status(task_id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(task_id) {
                rec.status = "cancelled".into();
            }
            return true;
        }
        false
    }

    /// Poll `task_outputs` until the task reaches a terminal status or `timeout_ms` elapses.
    ///
    /// Terminal: completed / failed / cancelled / interrupted.
    /// Returns the task record on success; `"timeout"` or `"unknown task_id: …"` on error.
    pub async fn wait_task(&self, task_id: &str, timeout_ms: u64) -> Result<TaskRecord, String> {
        fn is_terminal(status: &str) -> bool {
            matches!(status, "completed" | "failed" | "cancelled" | "interrupted")
        }

        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms.max(1));
        let mut saw_task = false;
        loop {
            if let Some(rec) = self.task_output(task_id).await {
                saw_task = true;
                if is_terminal(&rec.status) {
                    return Ok(rec);
                }
            } else if saw_task {
                // Task disappeared after we had seen it — treat as cancelled.
                return Err(format!("unknown task_id: {task_id}"));
            }
            if tokio::time::Instant::now() >= deadline {
                if !saw_task {
                    return Err(format!("unknown task_id: {task_id}"));
                }
                return Err("timeout".into());
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let sleep = remaining.min(Duration::from_millis(50));
            tokio::time::sleep(sleep).await;
        }
    }
}

#[cfg(test)]
mod task_surface_tests {
    use super::*;

    #[tokio::test]
    async fn list_and_kill_task_roundtrip() {
        let rt = ProductionRuntime::new();
        rt.task_outputs.lock().await.insert(
            "task-1".into(),
            TaskRecord {
                run_id: "run-child-1".into(),
                status: "running".into(),
                output: None,
            },
        );
        rt.task_outputs.lock().await.insert(
            "task-2".into(),
            TaskRecord {
                run_id: "run-child-2".into(),
                status: "completed".into(),
                output: Some("done".into()),
            },
        );

        let listed = rt.list_tasks().await;
        assert_eq!(listed.len(), 2);
        assert!(listed
            .iter()
            .any(|(id, r)| id == "task-1" && r.status == "running"));
        assert!(listed
            .iter()
            .any(|(id, r)| id == "task-2" && r.output.as_deref() == Some("done")));

        // Unknown task → false; known task flips status even without a live engine.
        assert!(!rt.kill_task("missing").await);
        assert!(rt.kill_task("task-1").await);
        let rec = rt.task_output("task-1").await.expect("task-1 present");
        assert_eq!(rec.status, "cancelled");
    }

    #[tokio::test]
    async fn wait_task_returns_completed() {
        let rt = Arc::new(ProductionRuntime::new());
        rt.task_outputs.lock().await.insert(
            "wait-done".into(),
            TaskRecord {
                run_id: "run-w1".into(),
                status: "running".into(),
                output: None,
            },
        );
        let rt_bg = rt.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if let Some(rec) = rt_bg.task_outputs.lock().await.get_mut("wait-done") {
                rec.status = "completed".into();
                rec.output = Some("ok".into());
            }
        });
        let rec = rt
            .wait_task("wait-done", 2_000)
            .await
            .expect("should complete");
        assert_eq!(rec.status, "completed");
        assert_eq!(rec.output.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn wait_task_times_out_while_running() {
        let rt = ProductionRuntime::new();
        rt.task_outputs.lock().await.insert(
            "wait-slow".into(),
            TaskRecord {
                run_id: "run-w2".into(),
                status: "running".into(),
                output: None,
            },
        );
        let err = rt.wait_task("wait-slow", 80).await.unwrap_err();
        assert_eq!(err, "timeout");
    }

    #[tokio::test]
    async fn wait_task_unknown_id() {
        let rt = ProductionRuntime::new();
        let err = rt.wait_task("missing-task", 50).await.unwrap_err();
        assert!(err.contains("unknown task_id"), "{err}");
    }
}

#[cfg(test)]
mod tool_grant_tests {
    use super::*;

    #[tokio::test]
    async fn project_grant_skips_second_ask() {
        let rt = Arc::new(ProductionRuntime::new());
        rt.remember_tool_grant("c1", "r1", "write_file", "", "project")
            .await;
        assert!(rt.has_tool_grant("c1", "r1", "write_file", "").await);
        assert!(rt.has_tool_grant("c1", "r2", "write_file", "").await);
        assert!(!rt.has_tool_grant("c1", "r1", "run_terminal", "ls").await);
    }

    #[tokio::test]
    async fn this_run_grant_only_same_run() {
        let rt = Arc::new(ProductionRuntime::new());
        rt.remember_tool_grant("c1", "r1", "apply_patch", "", "this_run")
            .await;
        assert!(rt.has_tool_grant("c1", "r1", "apply_patch", "").await);
        assert!(!rt.has_tool_grant("c1", "r2", "apply_patch", "").await);
    }

    #[tokio::test]
    async fn once_does_not_persist() {
        let rt = Arc::new(ProductionRuntime::new());
        rt.remember_tool_grant("c1", "r1", "write_file", "", "once")
            .await;
        assert!(!rt.has_tool_grant("c1", "r1", "write_file", "").await);
    }

    #[tokio::test]
    async fn terminal_pattern_is_honored() {
        let rt = Arc::new(ProductionRuntime::new());
        rt.remember_tool_grant("c1", "r1", "run_terminal", "cargo test", "project")
            .await;
        assert!(
            rt.has_tool_grant("c1", "r1", "run_terminal", "cargo test")
                .await
        );
        assert!(
            !rt.has_tool_grant("c1", "r1", "run_terminal", "rm -rf /")
                .await
        );
    }
}

impl Default for ProductionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Real HTTP provider adapter wrapper (never returns offline mock tool-call text).
pub struct RealProvider {
    pub provider_id: String,
    pub key_id: Option<String>,
}

#[async_trait::async_trait]
impl EngineProvider for RealProvider {
    async fn stream(
        &self,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let credential = resolve_credential_for_run(
            &self.provider_id,
            self.key_id.as_deref(),
            "provider-stream",
        )
        .map_err(EngineError::Message)?;
        let protocol = credential
            .provider_type
            .clone()
            .unwrap_or_else(|| self.provider_id.clone());
        let key_id = credential.key_id.clone();
        let route_key_id = key_id.as_deref().unwrap_or("default").to_string();
        let base_url = credential.base_url.clone();
        let adapter = resolve_adapter(&protocol);

        if let Some(governor) = crate::global_governor() {
            governor
                .acquire(&self.provider_id, &route_key_id, cancel.clone())
                .await
                .map_err(|e| {
                    if e == "cancelled" {
                        EngineError::Cancelled
                    } else {
                        EngineError::Message(e)
                    }
                })?;
        }

        let provider_messages: Vec<_> = messages
            .into_iter()
            .map(engine_message_to_history)
            .map(history_message_to_provider)
            .collect();
        let provider_tools: Vec<ProviderTool> = tools
            .iter()
            .map(|t| ProviderTool {
                name: t.name.clone(),
                description: Some(t.description.clone()),
                input_schema: t.input_schema.clone(),
            })
            .collect();

        let mut request = ProviderRequest {
            model: model.to_string(),
            messages: provider_messages,
            system_prompt: system_prompt.map(str::to_string),
            tools: if provider_tools.is_empty() {
                None
            } else {
                Some(provider_tools)
            },
            max_tokens: Some(4096),
            temperature: None,
            stream: true,
            structured_output: None,
        };
        crate::request_rectifier::rectify_provider_request(
            &mut request,
            crate::routing::rectifier_enabled(),
        );

        let stream = match adapter.stream(request, credential).await {
            Ok(stream) => stream,
            Err(e) => {
                if matches!(
                    e.category,
                    provider_adapters::capabilities::ProviderErrorCategory::RateLimit
                ) {
                    if let Some(governor) = crate::global_governor() {
                        governor
                            .record_rate_limit(&self.provider_id, &route_key_id, e.retry_after_ms)
                            .await;
                    }
                }
                return Err(EngineError::Provider {
                    message: provider_error_message(
                        &e,
                        &self.provider_id,
                        &protocol,
                        model,
                        key_id.as_deref(),
                        base_url.as_deref(),
                    ),
                    code: e.code,
                    retryable: e.retryable,
                    category: format!("{:?}", e.category),
                    retry_after_ms: e.retry_after_ms,
                });
            }
        };
        let provider_id = self.provider_id.clone();
        let route_key_id = route_key_id.clone();
        let governor = crate::global_governor();
        let model = model.to_string();
        let mapped = futures_util::stream::unfold((stream, cancel), move |(mut stream, cancel)| {
            let provider_id = provider_id.clone();
            let route_key_id = route_key_id.clone();
            let governor = governor.clone();
            let protocol = protocol.clone();
            let model = model.clone();
            let key_id = key_id.clone();
            let base_url = base_url.clone();
            async move {
                if cancel.is_cancelled() {
                    return None;
                }
                tokio::select! {
                    ev = stream.next() => {
                        let ev = ev?;
                        if let ProviderEvent::Error(error) = &ev {
                            if matches!(error.category, provider_adapters::capabilities::ProviderErrorCategory::RateLimit) {
                                if let Some(governor) = &governor {
                                    governor.record_rate_limit(&provider_id, &route_key_id, error.retry_after_ms).await;
                                }
                            }
                        }
                        let event = match ev {
                                ProviderEvent::TextDelta(t) => EngineProviderEvent::TextDelta(t),
                                ProviderEvent::ReasoningDelta(t) => EngineProviderEvent::ReasoningDelta(t),
                                ProviderEvent::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                } => EngineProviderEvent::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                },
                                ProviderEvent::Usage(u) => EngineProviderEvent::Usage {
                                    input_tokens: u.input_tokens,
                                    output_tokens: u.output_tokens,
                                    reasoning_tokens: u.reasoning_tokens,
                                },
                                ProviderEvent::Completed => EngineProviderEvent::Completed,
                                ProviderEvent::Error(e) => EngineProviderEvent::Error {
                                        message: provider_error_message(&e, &provider_id, &protocol, &model, key_id.as_deref(), base_url.as_deref()),
                                        code: e.code,
                                        retryable: e.retryable,
                                        category: format!("{:?}", e.category),
                                        retry_after_ms: e.retry_after_ms,
                                },
                            };
                        Some((event, (stream, cancel)))
                    }
                    _ = cancel.cancelled() => None,
                }
            }
        });
        Ok(Box::pin(mapped))
    }
}

pub(crate) fn provider_error_message(
    error: &ProviderError,
    provider_id: &str,
    protocol: &str,
    model: &str,
    key_id: Option<&str>,
    base_url: Option<&str>,
) -> String {
    format!(
        "provider={provider_id} protocol={protocol} model={model} key_id={} base_url={} code={} category={:?} retryable={} message={}",
        key_id.unwrap_or("default"),
        base_url.map(assistant_protocol::v2::redact_secrets).unwrap_or_else(|| "default".into()),
        error.code,
        error.category,
        error.retryable,
        assistant_protocol::v2::redact_secrets(&error.message),
    )
}

/// Map engine history into provider history parts (preserves tool_calls / tool_call_id).
pub(crate) fn engine_message_to_history(m: EngineMessage) -> HistoryMessage {
    HistoryMessage {
        role: m.role,
        content: m.content,
        tool_call_id: m.tool_call_id,
        tool_name: m.tool_name,
        tool_calls: m.tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|c| HistoryToolCall {
                    id: c.id,
                    name: c.name,
                    arguments: c.arguments,
                })
                .collect()
        }),
    }
}

fn resolve_adapter(provider_id: &str) -> Box<dyn ProviderAdapter> {
    let lower = provider_id.to_ascii_lowercase();
    if lower.contains("anthropic") || lower.contains("claude") {
        Box::new(provider_adapters::providers::anthropic::AnthropicAdapter::new())
    } else if lower.contains("gemini") || lower.contains("google") {
        Box::new(provider_adapters::providers::gemini::GeminiAdapter::new())
    } else if lower.contains("deepseek") {
        Box::new(provider_adapters::providers::deepseek::DeepSeekAdapter::new())
    } else if lower.contains("ollama") {
        Box::new(provider_adapters::providers::ollama::OllamaAdapter::new())
    } else if lower.contains("compatible") || lower.contains("chat_completions") {
        Box::new(provider_adapters::providers::openai_compatible::OpenAiCompatibleAdapter::new())
    } else if lower.contains("responses") {
        // Force Responses API path via env for this adapter instance.
        std::env::set_var("NATIVES_OPENAI_API", "responses");
        Box::new(provider_adapters::providers::openai::OpenAiAdapter::new())
    } else {
        Box::new(provider_adapters::providers::openai::OpenAiAdapter::new())
    }
}

// Credential resolution moved to `production_credentials.rs` (task-01 structure).

#[cfg(test)]
mod permission_bind_tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn runtime_profile_maps_readonly_ask_and_full_access() {
        let rt = ProductionRuntime::new();
        rt.set_permission_profile("readonly").await;
        assert_eq!(
            rt.permissions.get_profile().await,
            PermissionProfile::ReadOnly
        );
        rt.set_permission_profile("ask").await;
        assert_eq!(
            rt.permissions.get_profile().await,
            PermissionProfile::ConfirmEach
        );
        rt.set_permission_profile("full_access").await;
        assert_eq!(
            rt.permissions.get_profile().await,
            PermissionProfile::Autonomous
        );
    }

    #[tokio::test]
    async fn rejects_mismatched_run_id() {
        let rt = ProductionRuntime::new();
        let (tx, _rx) = oneshot::channel();
        rt.insert_permission_waiter("p1", "run-a", "tool", tx).await;
        let err = rt
            .respond_permission("p1", true, Some("run-b"), Some("once"))
            .await
            .unwrap_err();
        assert!(err.contains("mismatch"), "{err}");
        // Still present for correct owner
        assert!(rt.interactions.has_permission("p1").await);
        let ok = rt
            .respond_permission("p1", false, Some("run-a"), Some("once"))
            .await;
        assert!(ok.is_ok());
        assert!(!rt.interactions.has_permission("p1").await);
    }

    #[test]
    fn permission_scope_preserves_session_boundary() {
        assert_eq!(normalize_permission_scope("session"), "session");
        assert_eq!(normalize_permission_scope("this_run"), "this_run");
        assert_eq!(normalize_permission_scope("project"), "project");
    }
}

#[cfg(test)]
mod provider_error_message_tests {
    use super::*;
    use provider_adapters::capabilities::ProviderErrorCategory;

    #[test]
    fn provider_error_message_includes_context_and_redacts_secrets() {
        let msg = provider_error_message(
            &ProviderError {
                code: "http_401".into(),
                message: "bad key sk-secret123".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            },
            "p1",
            "openai_chat_completions",
            "deepseek-v4-flash",
            Some("k1"),
            Some("https://token.sensenova.cn/v1"),
        );

        assert!(msg.contains("provider=p1"));
        assert!(msg.contains("protocol=openai_chat_completions"));
        assert!(msg.contains("model=deepseek-v4-flash"));
        assert!(msg.contains("retryable=false"));
        assert!(!msg.contains("sk-secret123"));
    }
}

/// Agent kind of the creative session (ADR-0014 section 8). Carried on a run as
/// `agent_profile_id`, which is the only per-run surface selector that reaches
/// `start_run`.
pub(crate) const CREATIVE_DRAFT_AGENT_KIND: &str = "creative-draft";

/// Resolve a built-in surface name to its allowlist.
///
/// Returns `None` for anything that is not a built-in surface, so the caller
/// falls back to the run-scoped or agent-profile allowlist and behaviour for
/// every existing agent kind is unchanged.
///
/// The creative surface is defined by omission as much as by inclusion: no
/// `write_file`, `edit_file`, `apply_patch` or `run_terminal`. That is what keeps
/// ADR-0014 invariant #3 ("the model cannot reach the real module directory")
/// true without a second gate — publishing stays a host command.
pub(crate) fn builtin_surface_allowlist(agent_kind: &str) -> Option<Vec<String>> {
    match agent_kind {
        CREATIVE_DRAFT_AGENT_KIND | "creative_draft" => Some(
            capability_gateway::tools::CREATIVE_DRAFT_TOOL_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        ),
        _ => None,
    }
}

/// Working instructions for the creative surface.
///
/// Tool schemas alone tell the model what it *can* call, not what the session is
/// for. Without this it treats "make me a pomodoro timer" as a chat request and
/// answers with prose instead of writing a revision — the tools are registered
/// but never used. The creative surface has no agent profile on disk, so this is
/// where its behaviour is defined.
const CREATIVE_DRAFT_SYSTEM_PROMPT: &str = r#"You are building a small, self-contained web app for the user inside the Natives creative workshop.

The user's message begins with `[draft:<draftId>]`. That id identifies the draft you are editing — pass it to every draft tool. It is not part of the user's request; do not mention it back to them.

How to work:
- Write the whole app as a single HTML document with inline CSS and JS, then save it with `write_draft_module`. The user sees a live preview of whatever you save.
- For a change request, call `read_draft_module` first and edit what is already there. Do not regenerate from scratch and do not drop features the user did not ask you to remove.
- Save your work with `write_draft_module` before you finish. A reply without a saved revision leaves the user with nothing to look at.

Hard constraints (the save is rejected if you break them):
- No remote scripts or stylesheets. No CDN links. Everything inline.
- No `eval` or `new Function`.
- Persist data with `localStorage` if the app needs to remember anything.

If a save is rejected, the error text says exactly what failed — fix it and save again. Keep replies short: the app itself is the deliverable, not a description of it."#;

/// Working instructions for a built-in surface, or `None` for agent kinds that
/// carry a profile on disk (whose prompt comes from that profile instead).
pub(crate) fn builtin_surface_system_prompt(agent_kind: &str) -> Option<&'static str> {
    match agent_kind {
        CREATIVE_DRAFT_AGENT_KIND | "creative_draft" => Some(CREATIVE_DRAFT_SYSTEM_PROMPT),
        _ => None,
    }
}

/// Register gateway tools for a run. Parent (`allowlist=None`) gets full builtins.
/// Child (`Some`) only registers the intersection so unauthorized tools are not present.
pub(crate) fn register_tools_for_surface(
    gateway: &mut CapabilityGateway,
    allowlist: Option<&[String]>,
) {
    match allowlist {
        None => gateway.register_builtins(),
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
                    gateway.register(tool);
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
    let created = rm.create_run(assistant_protocol::v2::CreateRunRequest {
        conversation_id: sess.child_conversation_id.clone(),
        provider_id: binding.provider_id.clone(),
        model_id: binding.model_id.clone(),
        key_id: Some(binding.key_id.clone()),
        agent_profile_id: None,
        permission_profile: Some("ask".into()),
        content: Some(prompt.clone()),
        attachments: None,
        max_steps: Some(15),
        parent_run_id: sess.parent_run_id.clone(),
        project_path: None,
        idempotency_key: None,
        effort: None,
        runtime_id: Some("native".into()),
    })?;
    let run = crate::run_manager::RunManager::start_detached_global(
        assistant_protocol::v2::StartRunRequest {
            run_id: Some(created.id.clone()),
            conversation_id: Some(sess.child_conversation_id.clone()),
            provider_id: Some(binding.provider_id.clone()),
            model_id: Some(binding.model_id.clone()),
            key_id: Some(binding.key_id.clone()),
            content: Some(prompt),
            attachments: None,
            trigger_message_id: None,
            permission_profile: Some("ask".into()),
            max_steps: Some(15),
            project_path: None,
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

/// Background reaper: close idle subagent sessions.
/// Safe to call without a Tokio runtime (unit tests / sync constructors): no-ops until a
/// runtime exists; production daemon always constructs under tokio::main.
fn spawn_subagent_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                if let Err(e) = reaper_tick().await {
                    eprintln!("[agent-daemon] subagent reaper: {e}");
                }
            }
        });
    });
}

async fn reaper_tick() -> Result<(), String> {
    let sessions = crate::subagent_store::list_active_for_reaper()?;
    let now = chrono::Utc::now();
    for sess in sessions {
        // Never close while a live task_output still says running.
        if let Some(rec) = crate::global_run_manager()
            .runtime
            .task_output(&sess.id)
            .await
        {
            if rec.status == "running" {
                continue;
            }
        }
        if sess.status == "running" {
            // Status open/running in DB but no live task record — fall through to idle timer.
        }

        let last = chrono::DateTime::parse_from_rfc3339(&sess.last_activity_at)
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or(now);
        let idle = now.signed_duration_since(last);

        // Parent heartbeat (frontend touch) fresh within 90s → 5min idle; else 2min.
        let parent_active =
            crate::subagent_store::parent_heartbeat_recent(&sess.parent_conversation_id, 90);
        let limit_secs = if parent_active { 300i64 } else { 120i64 };
        if idle.num_seconds() >= limit_secs {
            if let Some(rec) = crate::global_run_manager()
                .runtime
                .task_output(&sess.id)
                .await
            {
                if rec.status == "running" {
                    continue;
                }
            }
            let _ = crate::subagent_store::close_subagent_session(
                &sess.id,
                "closed",
                Some("idle timeout"),
            );
        }
    }
    Ok(())
}

/// Look up model context_window from daemon model_cache (best-effort).
fn lookup_model_context_window(provider_id: &str, model_id: &str) -> Option<u64> {
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(crate::default_assistant_db_path);
    let art = std::env::var("NATIVES_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home)
                .join(".natives")
                .join("runtime")
        })
        .join("artifacts");
    let store = crate::storage::DataStore::new(&db_path, &art).ok()?;
    let conn = store.conn().ok()?;
    conn.query_row(
        "SELECT context_window FROM model_cache
         WHERE provider_id = ?1 AND model_id = ?2
         LIMIT 1",
        rusqlite::params![provider_id, model_id],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .filter(|w| *w > 0)
    .map(|w| w as u64)
}

#[allow(dead_code)]
fn parent_conversation_recently_active(conversation_id: &str, within_secs: i64) -> bool {
    let Ok(store) = (|| -> Result<crate::storage::DataStore, String> {
        #[cfg(test)]
        if let Some((db_path, artifact_dir)) = crate::storage::test_db_override() {
            return crate::storage::DataStore::new(&db_path, &artifact_dir);
        }
        let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var("NATIVES_DB_PATH")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(crate::default_assistant_db_path);
        let artifact_dir = db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("artifacts");
        crate::storage::DataStore::new(&db_path, &artifact_dir)
    })() else {
        return false;
    };
    let Ok(conn) = store.conn() else {
        return false;
    };
    let updated: Option<String> = conn
        .query_row(
            "SELECT updated_at FROM conversation WHERE id = ?1",
            rusqlite::params![conversation_id],
            |row| row.get(0),
        )
        .ok();
    let Some(updated) = updated else {
        return false;
    };
    let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&updated) else {
        return false;
    };
    let age = chrono::Utc::now().signed_duration_since(ts.with_timezone(&chrono::Utc));
    age.num_seconds() <= within_secs
}

/// Fixture provider for offline tests — emits tool_call then text, never fake "hello from adapter" as success content for tools.
pub struct FixtureProvider {
    pub mode: FixtureMode,
}

#[derive(Clone, Copy)]
pub enum FixtureMode {
    TextOnly,
    ToolThenText,
    RequestPermissionPath,
}

#[async_trait::async_trait]
impl EngineProvider for FixtureProvider {
    async fn stream(
        &self,
        _model: &str,
        messages: Vec<EngineMessage>,
        _tools: &[ToolSchema],
        _system_prompt: Option<&str>,
        _cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        // If last message is a tool result, complete with text.
        if messages.last().map(|m| m.role == "tool").unwrap_or(false) {
            return Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::TextDelta("tool path complete".into()),
                EngineProviderEvent::Completed,
            ])));
        }
        let events = match self.mode {
            FixtureMode::TextOnly => vec![
                EngineProviderEvent::TextDelta("fixture answer".into()),
                EngineProviderEvent::Completed,
            ],
            FixtureMode::ToolThenText => vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_1".into()),
                    name: Some("read_file".into()),
                    arguments_delta: r#"{"path":"Cargo.toml"}"#.into(),
                },
                EngineProviderEvent::Completed,
            ],
            // Side-effecting tool so PermissionManager ConfirmEach emits permission_requested.
            FixtureMode::RequestPermissionPath => vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_perm".into()),
                    name: Some("write_file".into()),
                    arguments_delta: r#"{"path":"/tmp/natives-perm-test.txt","content":"x"}"#
                        .into(),
                },
                EngineProviderEvent::Completed,
            ],
        };
        Ok(Box::pin(futures_util::stream::iter(events)))
    }
}

#[cfg(test)]
mod tool_allowlist_tests {
    use super::*;
    use agent_core::{EngineToolRuntime, SubAgentConfig, SubAgentManager};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    fn gated_with_allowlist(list: Option<Vec<String>>) -> PermissionGatedTools {
        let rt = ProductionRuntime::new();
        PermissionGatedTools {
            gateway: {
                let mut g = CapabilityGateway::new();
                g.set_project_root("/tmp/natives-allowlist-test");
                g.register_builtins();
                Arc::new(g)
            },
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: Arc::new(SubAgentManager::new(SubAgentConfig::default())),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engines.clone(),
            runtime: None,
            provider_id: "openai".into(),
            key_id: None,
            parent_run_id: "allowlist-parent".into(),
            conversation_id: "c-allow".into(),
            model_id: "m".into(),
            permission_profile: "full_access".into(),
            tool_allowlist: list,
        }
    }

    #[tokio::test]
    async fn allowlist_hides_and_denies_tools_outside_list() {
        let tools = gated_with_allowlist(Some(vec![
            "read_file".into(),
            "list_dir".into(),
            "grep".into(),
        ]));
        let names: Vec<_> = tools
            .list_tool_schemas()
            .await
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert!(names.contains(&"read_file".into()));
        assert!(!names
            .iter()
            .any(|n| n == "write_file" || n == "task" || n == "run_terminal"));

        let denied = tools
            .execute_tool(
                "write_file",
                serde_json::json!({"path":"/tmp/x","content":"y"}),
                &CancellationToken::new(),
            )
            .await;
        assert!(denied.is_error);
        assert_eq!(denied.output.get("denied"), Some(&serde_json::json!(true)));
        assert_eq!(
            denied.output.get("code").and_then(|v| v.as_str()),
            Some("tool_not_allowlisted")
        );

        // task must not escalate unless allowlisted.
        let task_denied = tools
            .execute_tool(
                "task",
                serde_json::json!({"prompt":"nope","key_id":"k"}),
                &CancellationToken::new(),
            )
            .await;
        assert!(task_denied.is_error);
        assert_eq!(
            task_denied.output.get("denied"),
            Some(&serde_json::json!(true))
        );
    }

    #[tokio::test]
    async fn empty_allowlist_denies_all_tools() {
        let tools = gated_with_allowlist(Some(vec![]));
        assert!(tools.list_tool_schemas().await.is_empty());
        let denied = tools
            .execute_tool(
                "read_file",
                serde_json::json!({"path":"Cargo.toml"}),
                &CancellationToken::new(),
            )
            .await;
        assert!(denied.is_error);
        assert_eq!(denied.output.get("denied"), Some(&serde_json::json!(true)));
        let write_denied = tools
            .execute_tool(
                "write_file",
                serde_json::json!({"path":"/tmp/x","content":"y"}),
                &CancellationToken::new(),
            )
            .await;
        assert!(write_denied.is_error);
        assert_eq!(
            write_denied.output.get("denied"),
            Some(&serde_json::json!(true))
        );
    }

    #[tokio::test]
    async fn parent_none_allowlist_exposes_full_surface() {
        let tools = gated_with_allowlist(None);
        let names: Vec<_> = tools
            .list_tool_schemas()
            .await
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert!(names.len() > 5);
        assert!(names.iter().any(|n| n == "write_file"));
        assert!(names.iter().any(|n| n == "task"));
        // Draft tools are opt-in: the default surface must not carry them.
        assert!(!names.iter().any(|n| n == "write_draft_module"));
    }

    /// ADR-0014 invariant #3: the creative surface is exactly the four draft
    /// tools, and no general write tool rides along.
    #[test]
    fn creative_surface_registers_only_draft_tools() {
        let allowlist =
            builtin_surface_allowlist(CREATIVE_DRAFT_AGENT_KIND).expect("built-in surface");
        let mut gateway = CapabilityGateway::new();
        register_tools_for_surface(&mut gateway, Some(&allowlist));

        let mut names: Vec<&str> = gateway.list_tools().into_iter().map(|t| t.name).collect();
        names.sort_unstable();
        let mut expected: Vec<&str> =
            capability_gateway::tools::CREATIVE_DRAFT_TOOL_NAMES.to_vec();
        expected.sort_unstable();
        assert_eq!(names, expected);

        for banned in ["write_file", "edit_file", "apply_patch", "run_terminal"] {
            assert!(gateway.get_tool(banned).is_none(), "{banned} leaked in");
        }
    }

    #[test]
    fn unknown_agent_kind_keeps_the_existing_fallback() {
        assert!(builtin_surface_allowlist("general").is_none());
        assert!(builtin_surface_allowlist("").is_none());
    }

    #[tokio::test]
    async fn execute_task_caps_child_permission_and_sets_allowlist() {
        let _env_guard = crate::storage::DataStore::env_test_lock();
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let tools = gated_with_allowlist(None);
        // Parent is full_access in helper; request full_access is allowed.
        let ok = tools
            .execute_tool(
                "task",
                serde_json::json!({
                    "prompt": "child",
                    "provider_id": "anthropic",
                    "model_id": "claude",
                    "key_id": "child-key",
                    "permission_profile": "full_access",
                    "fixture": true
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!ok.is_error, "{:?}", ok.output);
        assert_eq!(
            ok.output.get("permission_profile").and_then(|v| v.as_str()),
            Some("full_access")
        );
        let task_id = ok
            .output
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();
        let child = tools.subagents.get(&task_id).await.expect("child record");
        assert_eq!(
            child.tool_allowlist,
            agent_core::default_subagent_tool_allowlist()
        );

        // Default request (omit profile) stays ask even under full_access parent.
        let defaulted = tools
            .execute_tool(
                "task",
                serde_json::json!({
                    "prompt": "child-default",
                    "provider_id": "anthropic",
                    "model_id": "claude",
                    "key_id": "child-key-2",
                    "fixture": true
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!defaulted.is_error, "{:?}", defaulted.output);
        assert_eq!(
            defaulted
                .output
                .get("permission_profile")
                .and_then(|v| v.as_str()),
            Some("ask")
        );

        // Parent ask: cannot upgrade to full_access (cap before spawn).
        // Use full_access permission_profile field on tools so task gate doesn't block,
        // but cap_child_permission still sees parent profile "ask" via a custom field?
        // We call the helper directly for the pure cap assertion, and use spawn path
        // with parent tools.permission_profile = ask under autonomous class by temporarily
        // using full_access for the parent tools gate while asserting cap_child_permission.
        assert_eq!(cap_child_permission("ask", "full_access"), "ask");
        assert_eq!(cap_child_permission("readonly", "full_access"), "readonly");

        // Empty allowlist on task input is fail-closed for the child record.
        let empty = tools
            .execute_tool(
                "task",
                serde_json::json!({
                    "prompt": "child-empty",
                    "provider_id": "anthropic",
                    "model_id": "claude",
                    "key_id": "child-key-3",
                    "tool_allowlist": [],
                    "fixture": true
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!empty.is_error, "{:?}", empty.output);
        let empty_id = empty
            .output
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap();
        let empty_child = tools.subagents.get(empty_id).await.unwrap();
        assert!(empty_child.tool_allowlist.is_empty());

        // readonly parent denies Process side-effect of task before spawn.
        let mut tools_ro = gated_with_allowlist(None);
        tools_ro.permission_profile = "readonly".into();
        let capped = tools_ro
            .execute_tool(
                "task",
                serde_json::json!({
                    "prompt": "child",
                    "provider_id": "anthropic",
                    "model_id": "claude",
                    "key_id": "child-key",
                    "permission_profile": "full_access",
                    "fixture": true
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(capped.is_error);
        assert_eq!(capped.output.get("denied"), Some(&serde_json::json!(true)));
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
    }

    #[tokio::test]
    async fn execute_task_ignores_model_key_id_auto_when_policy_exists() {
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir
            .path()
            .join(format!("task-auto-{}.db", uuid::Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("migrate");
        crate::conversation_store::ensure_conversation_stub(
            "c-auto",
            "openai",
            "gpt-4o",
            Some("full_access"),
            None,
        )
        .unwrap();
        crate::subagent_store::upsert_route_policy(
            "c-auto",
            "default",
            &[crate::subagent_store::RouteBinding {
                provider_id: "anthropic".into(),
                key_id: "policy-key".into(),
                model_id: "claude".into(),
            }],
        )
        .unwrap();

        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let mut tools = gated_with_allowlist(None);
        tools.conversation_id = "c-auto".into();
        tools.permission_profile = "full_access".into();
        let out = tools
            .execute_tool(
                "task",
                serde_json::json!({
                    "prompt": "ignore-auto",
                    "provider_id": "evil-provider",
                    "key_id": "auto",
                    "model_id": "evil-model",
                    "fixture": true
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error, "{:?}", out.output);
        assert_eq!(out.output["key_id"], "policy-key");
        assert_eq!(out.output["provider_id"], "anthropic");
        assert_eq!(out.output["model_id"], "claude");
        // Real conversation id (not sub-* pseudo).
        let cid = out.output["conversation_id"].as_str().unwrap_or("");
        assert!(!cid.is_empty());
        assert!(!cid.starts_with("sub-"));
        assert!(!cid.starts_with("subagent-"));
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
        crate::storage::set_test_db_override(None, None);
    }

    #[test]
    fn cap_child_permission_unit() {
        assert_eq!(cap_child_permission("ask", "full_access"), "ask");
        assert_eq!(cap_child_permission("readonly", "ask"), "readonly");
        assert_eq!(cap_child_permission("full_access", "ask"), "ask");
    }
}
