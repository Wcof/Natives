//! Production execution seams for the Agent Daemon Run Authority.
//!
//! Real providers (no Echo mock on production path), capability-gateway tools,
//! permission Ask/Allow/Deny, hooks, and subagent child runs.

use agent_core::metrics::MetricsSink;
use agent_core::{
    AgentEngine, EngineRunConfig, EventSequencer, LiveEventBus, PermissionManager,
    PermissionProfile, SubAgentConfig, SubAgentManager, ToolSchema,
};
use assistant_protocol::v2::RunEventKind;
use capability_gateway::CapabilityGateway;
use provider_adapters::controls::RequestControls;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

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
    /// Shared ephemeral live event bus (STREAM-CONTRACT-V2 live lane). One bus
    /// is shared across every engine/progress sink in the daemon so a Renderer
    /// can subscribe to a run's deltas through a single handle.
    pub live: LiveEventBus,
    /// Warm prepare cache (P1-01): deterministic LRU over compiled static
    /// prompts / frozen tool schemas. Never holds credentials, permission
    /// decisions, run ids, or transcripts.
    pub prepared: crate::prepared_session::PreparedAgentSessionCache,
    /// Checkpoint authority paired with this runtime's Run/Event store.
    pub(crate) checkpoints: Arc<crate::checkpoint::CheckpointManager>,
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
    /// Per-run disabled tool deny list (P0-11, subtract-only). Registered by
    /// the Host via the typed `run.create` request (`CreateRunRequest.disabled_tools`,
    /// single source in assistant-protocol — no raw params shadow field) and
    /// consumed once by the RunManager tool-surface build as the final
    /// `− disabledTools` step. Settings can only subtract, never expand capability.
    pub run_disabled_tools: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Per-run agent directive: a system prompt the *parent* agent authored for
    /// one specific child run, registered before RunManager starts it and
    /// consumed once by [`Self::start_run`].
    ///
    /// Same lifecycle as `run_tool_allowlists`, and deliberately the same shape:
    /// the child run identity is allocated by RunManager, so the only thing that
    /// needs to travel is keyed by `run_id`. Nothing here can widen permissions
    /// or the tool surface — it is prompt text only.
    pub run_agent_directives: Arc<Mutex<HashMap<String, String>>>,
    /// Bounded runtime metrics sink (non-authoritative, no secrets).
    pub metrics_sink: MetricsSink,
}

#[cfg(test)]
pub use crate::production_credentials::clear_credential_broker_for_tests;
pub use crate::production_credentials::{
    install_credential_broker, resolve_credential, resolve_credential_for_run, CredentialBrokerFn,
};
pub use crate::production_hooks::{build_production_hooks, build_production_hooks_for_project};
pub use crate::production_tools::{DaemonToolProgressSink, PermissionGatedTools};

pub use crate::provider::RealProvider;
pub(crate) use crate::provider::{
    agent_message_to_history, engine_message_to_history, provider_error_message,
};

pub use crate::runtime::TaskRecord;

// Split implementation lives in same-directory modules by responsibility
// (task-01 structure); public paths below are preserved via re-exports.
#[path = "production_builtins.rs"]
mod production_builtins;
#[path = "production_execution.rs"]
mod production_execution;
#[path = "production_fixture.rs"]
mod production_fixture;
#[path = "production_reaper.rs"]
mod production_reaper;
#[path = "production_routing.rs"]
mod production_routing;

#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use self::production_builtins::{
    builtin_prompt_surface_default, builtin_surface_allowlist, builtin_surface_system_prompt,
    compile_effective_prompt, merge_agent_directive, CREATIVE_DRAFT_AGENT_KIND,
    CREATIVE_DRAFT_PROMPT_SURFACE_ID, TASK_DIRECTIVE_PROFILE_ID,
};
pub use self::production_fixture::{FixtureMode, FixtureProvider};
use self::production_reaper::lookup_model_context_window;
#[cfg(not(test))]
use self::production_reaper::spawn_subagent_reaper;
pub(crate) use self::production_routing::{
    normalize_permission_scope, register_tools_for_surface, resolve_effective_tools,
    restart_subagent_with_binding, validate_route_binding, wake_assignment_waiter,
};

/// Bundled inputs for a production engine turn (replaces the former 10
/// positional parameters). `capability` carries the resolved ADR-0016
/// snapshot; None = legacy behaviour (global skills, file profiles).
pub struct RunStartContext {
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub conversation_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub key_id: Option<String>,
    pub permission_profile: String,
    pub agent_profile_id: Option<String>,
    pub user_content: String,
    pub max_steps: u32,
    pub project_path: Option<std::path::PathBuf>,
    pub capability: Option<crate::capability_resolution::ResolvedCapabilitySnapshot>,
    pub hooks: Option<agent_core::HookRegistry>,
    pub effective_prompt: harness_core::CompiledPromptPlan,
    pub frozen_tool_schemas: Vec<ToolSchema>,
}

impl ProductionRuntime {
    pub fn new() -> Self {
        Self::new_with_events_and_checkpoint(
            EventSequencer::new(),
            Arc::new(crate::checkpoint::CheckpointManager::new()),
        )
    }

    pub fn new_with_event_store(data_store: Arc<crate::storage::DataStore>) -> Self {
        // TASK-006 / B04: the bounded storage actor is the single writer for
        // event facts; async engine paths submit appends instead of locking
        // the DataStore Mutex directly. The EventLog holds an Arc, so the
        // actor drains and exits when the runtime drops.
        let actor = crate::storage::actor::StorageActor::new(
            crate::storage::actor::DEFAULT_CAPACITY,
            data_store.clone(),
        );
        Self::new_with_events_and_checkpoint(
            EventSequencer::with_persistence(Arc::new(crate::event_log::EventLog::new_with_actor(
                data_store.clone(),
                actor.clone(),
            ))),
            Arc::new(crate::checkpoint::CheckpointManager::with_store_and_actor(
                data_store, actor,
            )),
        )
    }

    fn new_with_events_and_checkpoint(
        events: EventSequencer,
        checkpoints: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        let rt = Self {
            events,
            live: LiveEventBus::new(),
            prepared: crate::prepared_session::PreparedAgentSessionCache::new(),
            checkpoints,
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
            run_disabled_tools: Arc::new(Mutex::new(HashMap::new())),
            run_agent_directives: Arc::new(Mutex::new(HashMap::new())),
            metrics_sink: crate::metrics::daemon_metrics_sink(),
        };
        // Assignment bridges remain until the subagent interaction protocol is moved.
        let mut rt = rt;
        rt.assignment_waiters = rt.interactions.assignment_waiters_arc();
        rt.assignment_inflight = rt.interactions.assignment_inflight_arc();
        // Wire process supervisor force-kill into cancel tree (task-03).
        rt.execution
            .set_process_cancel_hook(Arc::new(GlobalProcessCancelHook));
        // Background reaper: idle subagent sessions. Unit tests construct
        // runtimes while holding the process-wide environment/store lock; a
        // reaper spawned there would wait on that lock while the test waits
        // for runtime shutdown. Integration/production builds keep the real
        // reaper enabled.
        #[cfg(not(test))]
        spawn_subagent_reaper();
        rt
    }

    pub(crate) fn checkpoint_manager(&self) -> &crate::checkpoint::CheckpointManager {
        &self.checkpoints
    }

    /// Shared live (ephemeral) event bus for the daemon.
    ///
    /// Cheap clone shares one bounded broadcast/ring across all engines and
    /// progress sinks. Subscribers attach per-run via `subscribe_after`.
    pub fn live_events(&self) -> LiveEventBus {
        self.live.clone()
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

    pub async fn peek_run_tool_allowlist(&self, run_id: &str) -> Option<Vec<String>> {
        self.run_tool_allowlists.lock().await.get(run_id).cloned()
    }

    /// Register the subtract-only disabled tool list for a run (P0-11).
    /// Consumed once by [`crate::run_manager::RunManager`] tool-surface build.
    pub async fn set_run_disabled_tools(&self, run_id: &str, disabled: Vec<String>) {
        self.run_disabled_tools
            .lock()
            .await
            .insert(run_id.to_string(), disabled);
    }

    /// Take the disabled tool list for a run (consumed once at start).
    pub async fn take_run_disabled_tools(&self, run_id: &str) -> Option<Vec<String>> {
        self.run_disabled_tools.lock().await.remove(run_id)
    }

    /// Register the parent-authored system prompt for a child run that will be
    /// started via RunManager. Consumed once by [`Self::start_run`].
    ///
    /// Empty / whitespace-only directives are dropped rather than stored so a
    /// blank prompt never shadows the profile prompt.
    pub async fn set_run_agent_directive(&self, run_id: &str, system_prompt: String) {
        if system_prompt.trim().is_empty() {
            return;
        }
        self.run_agent_directives
            .lock()
            .await
            .insert(run_id.to_string(), system_prompt);
    }

    pub async fn take_run_agent_directive(&self, run_id: &str) -> Option<String> {
        self.run_agent_directives.lock().await.remove(run_id)
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

    /// Legacy fixture compatibility. Production runs use the immutable profile
    /// captured in `RunStartContext`; this method intentionally does not mutate
    /// the shared PermissionManager profile.
    #[deprecated(note = "run profiles are bound in RunStartContext")]
    pub async fn set_permission_profile(&self, profile: &str) {
        let _ = profile;
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
        // Persist the response before touching the in-memory waiter. If the
        // durable interaction is unavailable, leave the waiter untouched so
        // the Engine remains fail-closed and the RPC reports the failure.
        crate::interaction_store::mark_resolved(
            request_id,
            serde_json::json!({ "approved": approved, "scope": scope }),
        )?;
        let (_bound_run, _tool_name, tx) = self
            .interactions
            .resolve_permission_for_run(request_id, run_id)
            .await?;
        tx.send((approved, scope))
            .map_err(|_| "permission waiter was closed before response delivery".to_string())?;
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
    /// `ctx.capability` is the resolved ADR-0016 snapshot produced by
    /// `capability_resolution::resolve` at the RunManager insertion point.
    pub async fn start_run(
        &self,
        ctx: RunStartContext,
    ) -> Result<agent_core::EngineOutcome, String> {
        let RunStartContext {
            run_id,
            parent_run_id,
            conversation_id,
            provider_id,
            model_id,
            key_id,
            permission_profile,
            agent_profile_id,
            user_content,
            max_steps,
            project_path,
            capability,
            hooks,
            effective_prompt,
            frozen_tool_schemas,
        } = ctx;
        let project_root = project_path.ok_or_else(|| {
            "project_path is required for daemon runs; process cwd fallback is disabled".to_string()
        })?;
        // RunManager resolved and persisted the Harness before entering this
        // method. Keep a compatibility fallback only for direct test callers.
        let hooks =
            hooks.unwrap_or_else(|| build_production_hooks_for_project(Some(&project_root)));
        // Context budget: min(Profile tokenBudget, model context_window); default 128K.
        // chars/4 is only used when Provider usage is unavailable (engine estimate path).
        // Profile priority: resolved capability snapshot (DB authority) →
        // DB/file lookup by id (legacy callers without a snapshot).
        let profile = capability
            .as_ref()
            .and_then(|c| c.profile.clone())
            .or_else(|| {
                agent_profile_id.as_deref().and_then(|id| {
                    crate::capability_resolution::load_profile(id, Some(&project_root))
                })
            });
        let model_window = lookup_model_context_window(&provider_id, &model_id);
        let budget = agent_core::ContextBudget::resolve(
            profile.as_ref().and_then(|profile| profile.token_budget),
            model_window,
        );
        let cancel = self
            .ensure_execution_token(&run_id, parent_run_id.as_deref())
            .await?;
        let engine = Arc::new(
            AgentEngine::with_live(self.events.clone(), self.live.clone())
                .with_cancel_token(cancel.clone())
                .with_hooks(hooks)
                .with_session_harness(crate::prompt_queue_store::global_harness())
                .with_safe_point_receiver(Arc::new(
                    crate::prompt_queue_store::DurableSafePointReceiver::new(
                        conversation_id.clone(),
                    ),
                ))
                .with_input_receiver(Arc::new(
                    crate::prompt_queue_store::DurableInputReceiver::new(
                        conversation_id.clone(),
                        run_id.clone(),
                    ),
                ))
                .with_progress_sink(Arc::new(DaemonToolProgressSink::new(self.live.clone())))
                .with_provider_context_window(model_window)
                .with_context_budget(budget.history_compact_chars, budget.tool_output_max_chars),
        );
        // Phase 3: logical checkpoint at run start (lazy before-images on writes).
        let checkpoint_id = self
            .checkpoint_manager()
            .begin_run(&run_id, &conversation_id, &project_root)
            .map_err(|error| format!("checkpoint begin failed: {error}"))?;
        crate::global_run_manager()
            .set_run_checkpoint_id(&run_id, &checkpoint_id)
            .map_err(|error| format!("checkpoint lineage bind failed: {error}"))?;
        self.events
            .append_checked(
                &run_id,
                RunEventKind::CheckpointCreated {
                    checkpoint_id,
                    label: Some("run_start".into()),
                },
            )
            .map_err(|error| format!("checkpoint event persistence failed: {error}"))?;
        // Mark coordinator running so terminal drain / cancel-and-send are scoped.
        crate::prompt_queue_store::global_harness().mark_running(
            &conversation_id,
            &run_id,
            &user_content,
        );
        crate::prompt_queue_store::persist_actor_snapshot(&conversation_id)
            .map_err(|error| format!("persist actor snapshot on run start failed: {error}"))?;

        let run_effort = crate::global_run_manager()
            .get_run(&run_id)
            .and_then(|run| run.effort);
        let controls = run_request_controls(run_effort.as_deref());
        let provider = crate::routing::RoutedProvider::new(crate::routing::load_plan(
            provider_id.clone(),
            key_id.clone(),
            model_id.clone(),
        ))
        .with_controls(controls);
        // Child subagent runs may have pre-registered a readonly (or custom) surface.
        // A built-in surface name (e.g. the creative session) resolves next; it has
        // no profile on disk, so this is the only place its allowlist can come from.
        let mut tool_allowlist = self
            .take_run_tool_allowlist(&run_id)
            .await
            .or_else(|| {
                agent_profile_id
                    .as_deref()
                    .and_then(builtin_surface_allowlist)
            })
            .or_else(|| profile.as_ref().and_then(|profile| profile.tools.clone()));
        // 问题10：唯一有效工具解析（denylist 永远减法，deny-only 也生效）。
        let disallowed = profile
            .as_ref()
            .and_then(|profile| profile.disallowed_tools.as_ref());
        let effective = crate::production::resolve_effective_tools(
            tool_allowlist.as_deref(),
            disallowed.map(Vec::as_slice),
        );
        if let Some(effective) = effective {
            tool_allowlist = Some(effective);
        }
        let tools = PermissionGatedTools {
            gateway: {
                let mut g = CapabilityGateway::new();
                g.set_project_root(project_root.to_string_lossy().to_string());
                register_tools_for_surface(&mut g, tool_allowlist.as_deref());
                g.validate_registered_schemas()
                    .map_err(|error| format!("tool schema validation failed: {}", error.message))?;
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
            team: capability.as_ref().and_then(|c| c.team.clone()),
            mcp_tool_schemas: capability
                .as_ref()
                .map(|c| c.mcp_tool_schemas.clone())
                .unwrap_or_default(),
            selected_mcp_servers: capability
                .as_ref()
                .filter(|c| c.selection_active)
                .map(|c| c.mcp_servers.iter().cloned().collect()),
        };

        // Compact history against resolved token budget (chars/4 fallback estimate).
        let typed_history = crate::conversation_store::load_agent_messages(&conversation_id)?;
        let bound_run = crate::global_run_manager().get_run(&run_id);
        // A Continue/Resume run is bound to the checkpoint snapshot its plan was
        // approved against. Loading a newer conversation snapshot would change
        // the resumed context, so those runs restore exactly and fail closed
        // when their checkpoint has no committed snapshot.
        let exact_checkpoint_restore = bound_run.as_ref().is_some_and(|run| {
            run.resume_of_run_id.is_some() || run.continued_from_run_id.is_some()
        });
        let checkpoint_snapshot = bound_run
            .and_then(|run| run.checkpoint_id)
            .and_then(|checkpoint_id| {
                crate::conversation_store::load_active_context_snapshot_for_checkpoint(
                    &conversation_id,
                    &checkpoint_id,
                )
                .transpose()
            })
            .transpose()?;
        let active_snapshot = resolve_active_snapshot_for_start(
            exact_checkpoint_restore,
            checkpoint_snapshot,
            || crate::conversation_store::load_active_context_snapshot(&conversation_id),
        )?;
        let typed_history = match active_snapshot {
            Some(snapshot) => {
                let mut active = snapshot.messages;
                let snapshot_message_ids: std::collections::HashSet<String> = active
                    .iter()
                    .map(|message| match message {
                        agent_core::AgentMessage::User(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::Assistant(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::ToolResult(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::System(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::Custom(value) => value.message_id.to_string(),
                    })
                    .collect();
                active.extend(typed_history.into_iter().filter(|message| {
                    let id = match message {
                        agent_core::AgentMessage::User(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::Assistant(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::ToolResult(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::System(value) => value.message_id.to_string(),
                        agent_core::AgentMessage::Custom(value) => value.message_id.to_string(),
                    };
                    !snapshot.input_message_ids.contains(&id) && !snapshot_message_ids.contains(&id)
                }));
                active
            }
            None => typed_history,
        };
        // The Core owns active-context compaction. Keep the daemon boundary
        // lossless: typed history goes through the typed entry point without
        // flattening to EngineMessage. Legacy rows are converted once below
        // when a database predates typed message rows.
        let legacy_history = crate::conversation_store::engine_history(&conversation_id)?;
        let typed_history = if typed_history.is_empty() && !legacy_history.is_empty() {
            agent_core::engine_messages_to_agent_messages(&legacy_history)
        } else {
            typed_history
        };
        let config = EngineRunConfig {
            run_id: run_id.clone(),
            conversation_id: conversation_id.clone(),
            model: model_id,
            system_prompt: if effective_prompt.effective_full_text.is_empty() {
                None
            } else {
                Some(effective_prompt.effective_full_text)
            },
            messages: legacy_history,
            user_content,
            max_steps,
        };

        crate::production_tools::validate_tool_limit(frozen_tool_schemas.len())?;
        // Register only once all preflight persistence and context loads have
        // succeeded; an early error must not leave a cancellable stale handle.
        self.engines
            .lock()
            .await
            .insert(run_id.clone(), engine.clone());
        let outcome = match engine
            .run_with_typed_messages(
                config,
                &provider,
                &tools,
                frozen_tool_schemas,
                typed_history,
            )
            .await
        {
            Ok(o) => o,
            Err(e) => agent_core::EngineOutcome::failed(e.code(), e.to_string(), e.retryable()),
        };
        // The provider/tool future is finished before durable post-processing;
        // do not retain a stale engine handle if history/checkpoint persistence
        // below fails.
        self.engines.lock().await.remove(&run_id);
        // Terminal cleanup (STREAM-CONTRACT-V2 Terminal): drop the run's live
        // ring/broadcast state so its deltas stop being retained once the run
        // is over. Renderer clears transient live state on the durable
        // terminal event.
        self.live.remove_run(&run_id);
        // P1-07: replay from the run-start durable watermark, not from sequence
        // 0. Events before the run's `CheckpointCreated{label:run_start}` (run
        // creation / prepare bookkeeping) are never needed for the terminal
        // projection or checkpoint metadata; replaying them on every run start
        // was pure waste on the real production tail.
        let run_events = self
            .events
            .replay_after_checked(&run_id, 0)
            .map_err(|error| format!("run event replay failed: {error}"))?;
        let run_start_watermark = run_events
            .iter()
            .find_map(|event| match &event.payload {
                assistant_protocol::v2::RunEventKind::CheckpointCreated {
                    label: Some(label),
                    ..
                } if label == "run_start" => Some(event.run_sequence),
                _ => None,
            })
            .unwrap_or(0);
        let run_events: Vec<_> = run_events
            .into_iter()
            .filter(|event| event.run_sequence >= run_start_watermark)
            .collect();
        if let Some(turn_id) = run_events
            .iter()
            .rev()
            .find_map(|event| match &event.payload {
                assistant_protocol::v2::RunEventKind::TurnCompleted { turn_id, .. } => {
                    Some(turn_id.as_str())
                }
                _ => None,
            })
        {
            let snapshot_id = run_events
                .iter()
                .rev()
                .find_map(|event| match &event.payload {
                    assistant_protocol::v2::RunEventKind::ContextSnapshotCommitted {
                        snapshot_id,
                        ..
                    } => Some(snapshot_id.as_str()),
                    _ => None,
                });
            let ledger_cursor = crate::side_effect_ledger::ledger_watermark(&run_id)?;
            self.checkpoint_manager()
                .set_run_metadata(
                    &run_id,
                    Some(turn_id),
                    snapshot_id,
                    ledger_cursor.as_deref(),
                )
                .map_err(|error| format!("checkpoint metadata persistence failed: {error}"))?;
        }
        let success = matches!(outcome, agent_core::EngineOutcome::Completed { .. });
        // Persist every complete typed turn, including cancelled/provider-error
        // outcomes.  The store itself rejects only genuinely partial typed
        // turns, so a failed Run cannot silently lose an already committed
        // assistant message while still keeping incomplete streams out.
        crate::conversation_projector::project_run_from_events(
            &conversation_id,
            &run_id,
            &run_events,
        )?;
        // Finalize checkpoint — failure closes related side effects (no silent half-state).
        let checkpoint = self
            .checkpoint_manager()
            .finalize_run(&run_id)
            .map_err(|error| format!("checkpoint finalize_run failed: {error}"))?;
        self.events
            .append_checked(
                &run_id,
                RunEventKind::CheckpointCommitted {
                    checkpoint_id: checkpoint.id,
                },
            )
            .map_err(|error| format!("checkpoint commit event persistence failed: {error}"))?;
        // Do NOT commit_outcome or append terminal lifecycle events here.
        // RunManager is the sole lifecycle committer after this returns.
        // SessionCoordinator: drain next prompt / cancel-and-send after real terminal.
        // Never re-executes the just-finished run — only starts a *new* queued item.
        crate::prompt_queue_store::on_run_terminal(&conversation_id, &run_id, success)
            .await
            .map_err(|error| format!("prompt queue terminal settlement failed: {error}"))?;
        Ok(outcome)
    }
}

impl Default for ProductionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Choose the active-context source for a run's Provider request.
///
/// A Continue/Resume run is bound to the snapshot its checkpoint committed and
/// must never fall back to a newer conversation snapshot: doing so would change
/// the context the continuation was approved against. Fresh and Retry runs may
/// use the newest committed snapshot as a cache, so that fallback stays lazy
/// and is only consulted when no checkpoint snapshot is bound.
fn resolve_active_snapshot_for_start(
    exact_checkpoint_restore: bool,
    checkpoint_snapshot: Option<crate::conversation_store::ActiveContextSnapshot>,
    latest_snapshot: impl FnOnce() -> Result<
        Option<crate::conversation_store::ActiveContextSnapshot>,
        String,
    >,
) -> Result<Option<crate::conversation_store::ActiveContextSnapshot>, String> {
    match checkpoint_snapshot {
        Some(snapshot) => Ok(Some(snapshot)),
        None if exact_checkpoint_restore => Err(
            "continue run is bound to a checkpoint without a committed active context snapshot; exact restore is impossible"
                .to_string(),
        ),
        None => latest_snapshot(),
    }
}

/// Request-side controls for one run.
///
/// `run.start`'s `effort` is the only control with a producer today: the client
/// sends it, RunManager stores it on the run record, and this is where it stops
/// being an inert string and becomes a provider parameter (`reasoning_effort`,
/// `thinking`, or `thinkingBudget` — whichever the routed model speaks).
///
/// An absent or unrecognised level yields default controls, which produce a
/// request byte-identical to one that never mentioned reasoning at all; see
/// `provider_adapters::capabilities::ReasoningEffort::parse` for the vocabulary.
///
/// The other controls (`tool_choice`, `parallel_tool_calls`, `prompt_cache`)
/// have no run-level producer yet and are deliberately left at their defaults
/// rather than wired to an input nobody sets. The prompt cache has its own
/// operator kill switch (`NATIVES_PROMPT_CACHE`) that does not go through here.
pub fn run_request_controls(effort: Option<&str>) -> RequestControls {
    RequestControls::default().with_effort_str(effort)
}

#[cfg(test)]
#[path = "production_active_snapshot_resolution_tests.rs"]
mod active_snapshot_resolution_tests;
#[cfg(test)]
#[path = "production_permission_bind_tests.rs"]
mod permission_bind_tests;
#[cfg(test)]
#[path = "production_prompt_cache_integrity_tests.rs"]
mod prompt_cache_integrity_tests;
#[cfg(test)]
#[path = "production_run_controls_tests.rs"]
mod run_controls_tests;
#[cfg(test)]
#[path = "production_subagent_persona_tests.rs"]
mod subagent_persona_tests;
#[cfg(test)]
#[path = "production_task_surface_tests.rs"]
mod task_surface_tests;
#[cfg(test)]
#[path = "production_tool_allowlist_tests.rs"]
mod tool_allowlist_tests;
#[cfg(test)]
#[path = "production_tool_grant_tests.rs"]
mod tool_grant_tests;
