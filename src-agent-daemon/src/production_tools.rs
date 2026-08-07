//! Permission-gated tool runtime (extracted from `production.rs`, task-01 structure).
//!
//! `PermissionGatedTools` is the `EngineToolRuntime` the AgentEngine drives: it
//! enforces the allowlist, ProjectIdentity verification, permission Ask/Allow/Deny,
//! subagent budgets, checkpoints, and the task/mcp orchestration tools. Behavior is
//! unchanged from the pre-split single-file version.

use agent_core::{
    default_subagent_tool_allowlist, AgentEngine, ChildFailureEffect, EngineToolRuntime,
    EventSequencer, FailurePolicy, HookEvent, HookRegistry, HookRequest, NoopToolProgressSink,
    PermissionAggregate, PermissionManager, PermissionProfile, SubAgentManager, SubAgentStatus,
    ToolExecutionResult, ToolProgressSink, ToolProgressUpdate, ToolSchema,
};
use assistant_protocol::v2::RunEventKind;
use capability_gateway::plan_mode::{self, PlanDecision};
use capability_gateway::{CapabilityGateway, SideEffect};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::production::{normalize_permission_scope, ProductionRuntime, TaskRecord};

/// Step budget for a subagent turn loop when neither the caller nor the
/// selected agent profile asks for one.
pub const DEFAULT_CHILD_MAX_STEPS: u32 = 15;

/// Ceiling on a subagent step budget. A parent agent (or a prompt-injected one)
/// must not be able to buy an unbounded child loop by asking for a huge number;
/// the tool-call and token ledgers in `SubAgentManager` bound cost too, this
/// bounds wall-clock turns.
pub const MAX_CHILD_MAX_STEPS: u32 = 100;

/// Ceiling on `max_retries` for a Retry failure policy. Retries are bounded so
/// a failing child cannot re-queue itself (or be re-queued by a parent) forever.
pub const MAX_SUBAGENT_RETRIES: u32 = 5;

/// Ceiling on the parent-authored child system prompt, in UTF-8 bytes. Long
/// enough for a real persona brief, short enough that it cannot crowd out the
/// child's own context budget. Over the limit is an error, never a silent
/// truncation — a truncated persona is worse than a rejected one.
pub const MAX_CHILD_SYSTEM_PROMPT_BYTES: usize = 16_000;

/// Scope recorded on the `PermissionResponded` event when a hook, rather than a
/// human, answered the prompt. Deliberately not a grant scope: auto-approval is
/// per invocation and is never remembered.
pub const HOOK_AUTO_APPROVE_SCOPE: &str = "hook_auto_approve";

/// How long a submitted plan waits for a human, in seconds.
///
/// Longer than the 120s tool-confirmation window because reading a plan is a
/// different act from clicking "yes" on one command, and shorter than forever
/// because a run that nobody is watching must eventually stop occupying a slot.
/// A timeout is a rejection: the latch stays closed.
pub const PLAN_APPROVAL_TIMEOUT_SECS: u64 = 300;

/// Scope recorded when a plan approval resolves. Plan approval is a one-shot
/// gear change for a single run and is never a reusable grant.
pub const PLAN_APPROVAL_SCOPE: &str = "once";

/// Maximum allowed model-visible tools for a single Run.
/// If count exceeds 200, the Run fails closed before Provider invocation with tool_plan_too_large.
pub const MAX_MODEL_VISIBLE_TOOLS: usize = 200;

pub fn validate_tool_limit(count: usize) -> Result<(), String> {
    if count > MAX_MODEL_VISIBLE_TOOLS {
        Err(format!(
            "tool_plan_too_large: Model-visible tools count ({count}) exceeds maximum limit of {MAX_MODEL_VISIBLE_TOOLS}"
        ))
    } else {
        Ok(())
    }
}

pub fn model_visible_tool_schemas(
    gateway: &CapabilityGateway,
    tool_allowlist: Option<&[String]>,
    mcp_tool_schemas: &[ToolSchema],
    selected_mcp_servers: Option<&std::collections::HashSet<String>>,
    planning: bool,
) -> Vec<ToolSchema> {
    let allowed = |name: &str| {
        if let (Some(selected), Some(server)) = (selected_mcp_servers, mcp_server_of_tool(name)) {
            if !selected.contains(server) {
                return false;
            }
        }
        tool_allowlist.is_none_or(|list| agent_core::tool_list_allows(list, name))
    };
    let visible = |tool: &capability_gateway::Tool| match plan_mode::decision(
        tool.name,
        tool.side_effect,
        tool.permission_class,
    ) {
        PlanDecision::Control if tool.name == plan_mode::EXIT_PLAN_MODE_TOOL => planning,
        PlanDecision::Control => !planning,
        PlanDecision::Deny => !planning,
        PlanDecision::Allow => true,
    };
    let mut schemas: Vec<ToolSchema> = gateway
        .list_tools()
        .into_iter()
        .filter(|tool| allowed(tool.name) && visible(tool))
        .map(|tool| ToolSchema {
            name: tool.name.to_string(),
            description: tool.description.to_string(),
            input_schema: tool.schema.clone(),
        })
        .collect();
    for schema in mcp_tool_schemas {
        if planning
            || !allowed(&schema.name)
            || schemas.iter().any(|existing| existing.name == schema.name)
        {
            continue;
        }
        schemas.push(schema.clone());
    }
    schemas
}

/// What the permission gate should do after consulting the hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookPermissionGate {
    Deny(String),
    Prompt,
    AutoApprove(String),
}

/// Apply the permission-profile ceiling to a hook aggregate.
///
/// This is the whole of the "a hook cannot escalate" rule, in one pure function
/// so it can be tested without a daemon and so there is exactly one place to
/// audit. Two independent gates must both open before a prompt is skipped:
///
/// - `auto_approve_allowed`, the caller's policy check — the tool must be one
///   the profile would have *asked* about, not one it refuses outright;
/// - the profile must not be `ReadOnly`, which grants nothing that needs asking.
///
/// `Autonomous` is listed for completeness: it already auto-approves downstream,
/// so a hook allow there changes nothing but the audit line.
///
/// Deny is never filtered — a hook may always tighten, never loosen.
pub fn hook_permission_gate(
    aggregate: PermissionAggregate,
    profile: PermissionProfile,
    auto_approve_allowed: bool,
) -> HookPermissionGate {
    match aggregate {
        PermissionAggregate::Deny(reason) => HookPermissionGate::Deny(reason),
        PermissionAggregate::Prompt => HookPermissionGate::Prompt,
        PermissionAggregate::AutoApprove(reason) => {
            if !auto_approve_allowed || profile == PermissionProfile::ReadOnly {
                HookPermissionGate::Prompt
            } else {
                HookPermissionGate::AutoApprove(reason)
            }
        }
    }
}

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
    /// Resolved expert team for this run (ADR-0016): the task tool's `agent`
    /// parameter must name a member; runs without a team reject it.
    pub team: Option<crate::capability_resolution::ResolvedTeam>,
    /// Model-visible MCP tool schemas for the selected servers (ADR-0016).
    /// Empty = no MCP schemas surfaced (legacy runs expose none either).
    pub mcp_tool_schemas: Vec<ToolSchema>,
    /// `Some(set)` = server whitelist: `mcp__{server}__*` and `mcp_call` may
    /// only target these servers. `None` = legacy behaviour.
    pub selected_mcp_servers: Option<std::collections::HashSet<String>>,
}

pub struct DaemonToolProgressSink {
    pub events: EventSequencer,
    settled: Arc<Mutex<HashSet<String>>>,
    pending: Arc<Mutex<HashMap<String, (Instant, ToolProgressUpdate)>>>,
    scheduled_flushes: Arc<Mutex<HashSet<String>>>,
    sequence: Arc<AtomicU64>,
}

/// Process-wide registry of in-flight MCP tool calls (J03). A call enters when
/// its invocation starts and leaves when it settles, so after a cancel the
/// registry is quiet — there is no lingering request whose late response could
/// be mistaken for a completed effect.
static PENDING_MCP_CALLS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

fn pending_mcp_calls() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    PENDING_MCP_CALLS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Number of in-flight MCP calls; must be zero after every settle.
pub fn pending_mcp_call_count() -> usize {
    pending_mcp_calls().lock().unwrap().len()
}

/// Ledger settlement status for an MCP call outcome.
///
/// A transport timeout / failure while the run is *not* cancelled is `failed`;
/// a user cancel is `uncertain` because the external outcome is unknowable
/// while we tear the call down. The two states are deliberately distinct and
/// auditable (T04).
fn mcp_ledger_status(outcome_ok: bool, cancelled: bool) -> &'static str {
    if cancelled {
        "uncertain"
    } else if outcome_ok {
        "completed"
    } else {
        "failed"
    }
}

impl DaemonToolProgressSink {
    pub fn new(events: EventSequencer) -> Self {
        Self {
            events,
            settled: Arc::new(Mutex::new(HashSet::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            scheduled_flushes: Arc::new(Mutex::new(HashSet::new())),
            sequence: Arc::new(AtomicU64::new(1)),
        }
    }

    fn schedule_flush(&self, call_id: String) {
        let pending = self.pending.clone();
        let settled = self.settled.clone();
        let scheduled_flushes = self.scheduled_flushes.clone();
        let events = self.events.clone();
        let sequence = self.sequence.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let update = {
                let mut pending = pending.lock().await;
                let settled = settled.lock().await;
                let update = if settled.contains(&call_id) {
                    pending.remove(&call_id).map(|(_, value)| value)
                } else {
                    pending.remove(&call_id).map(|(_, mut value)| {
                        value.final_update = false;
                        value
                    })
                };
                // Keep the same pending → settled → scheduled lock order as
                // publish/mark_tool_call_settled. This closes the race where a
                // new update arrives while the timer is flushing the old batch.
                let mut scheduled = scheduled_flushes.lock().await;
                scheduled.remove(&call_id);
                update
            };
            if let Some(update) = update {
                let progress_sequence = sequence.fetch_add(1, AtomicOrdering::Relaxed);
                events.append(
                    &update.run_id,
                    RunEventKind::ToolOutputDelta {
                        tool_call_id: update.tool_call_id,
                        tool_name: Some(update.tool_name),
                        stream: update.stream,
                        text: update.text,
                        truncated: false,
                        turn_id: update.turn_id,
                        message_id: update.message_id,
                        progress_sequence: Some(progress_sequence),
                    },
                );
            }
        });
    }
}

#[async_trait::async_trait]
impl ToolProgressSink for DaemonToolProgressSink {
    async fn publish(&self, update: ToolProgressUpdate) {
        const MAX_BATCH_BYTES: usize = 8 * 1024;
        const MAX_BATCH_AGE: Duration = Duration::from_millis(250);
        let now = Instant::now();
        let call_id = update.tool_call_id.clone();
        let (emit, schedule) = {
            let mut pending = self.pending.lock().await;
            let settled = self.settled.lock().await;
            if settled.contains(&call_id) {
                return;
            }
            if update.final_update {
                let mut final_update = pending.remove(&call_id).map(|(_, mut value)| {
                    value.text.push_str(&update.text);
                    value.final_update = true;
                    value
                });
                if final_update.is_none() {
                    final_update = Some(update);
                }
                (final_update, false)
            } else {
                let flush = pending
                    .get_mut(&call_id)
                    .map(|(started, buffered)| {
                        buffered.text.push_str(&update.text);
                        buffered.text.len() >= MAX_BATCH_BYTES
                            || now.duration_since(*started) >= MAX_BATCH_AGE
                    })
                    .unwrap_or(false);
                if flush {
                    (pending.remove(&call_id).map(|(_, value)| value), false)
                } else if pending.contains_key(&call_id) {
                    (None, false)
                } else {
                    pending.insert(call_id.clone(), (now, update));
                    let mut scheduled = self.scheduled_flushes.lock().await;
                    (None, scheduled.insert(call_id.clone()))
                }
            }
        };
        if schedule {
            self.schedule_flush(call_id);
        }
        if let Some(update) = emit {
            let progress_sequence = self.sequence.fetch_add(1, AtomicOrdering::Relaxed);
            self.events.append(
                &update.run_id,
                RunEventKind::ToolOutputDelta {
                    tool_call_id: update.tool_call_id,
                    tool_name: Some(update.tool_name),
                    stream: update.stream,
                    text: update.text,
                    truncated: false,
                    turn_id: update.turn_id,
                    message_id: update.message_id,
                    progress_sequence: Some(progress_sequence),
                },
            );
        }
    }

    async fn mark_tool_call_settled(&self, tool_call_id: &str) {
        // Keep the same lock order as `publish`: either the event is appended
        // before settlement, or settlement wins and the late update is dropped.
        let mut pending = self.pending.lock().await;
        let mut settled = self.settled.lock().await;
        let mut scheduled = self.scheduled_flushes.lock().await;
        settled.insert(tool_call_id.to_string());
        pending.remove(tool_call_id);
        // Drain any scheduled flush task for this call so progress work is
        // immediately zero after settle (T04); a flush already running is a
        // no-op because `settled` is set and `pending` is empty.
        scheduled.remove(tool_call_id);
    }
}
impl PermissionGatedTools {
    fn checkpoint_manager(&self) -> &crate::checkpoint::CheckpointManager {
        match self.runtime.as_deref() {
            Some(runtime) => runtime.checkpoint_manager(),
            None => crate::checkpoint::global_checkpoint_manager(),
        }
    }

    fn tool_allowed(&self, name: &str) -> bool {
        // Server whitelist gate first (ADR-0016): with an active MCP selection
        // a namespaced tool outside the selected servers is invisible/denied,
        // regardless of allowlist wildcards.
        if let (Some(selected), Some(server)) =
            (&self.selected_mcp_servers, mcp_server_of_tool(name))
        {
            if !selected.contains(server) {
                return false;
            }
        }
        match &self.tool_allowlist {
            None => true,
            // Matching semantics (including the MCP surface) live in agent-core so
            // enforcement here and child-surface derivation cannot drift apart.
            Some(list) => agent_core::tool_list_allows(list, name),
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

    /// Close the Plan Mode latch for a run the host started in Plan Mode.
    ///
    /// The permission profile on a run is immutable by design, so a host that
    /// wants Plan Mode says so once by passing the `plan` profile. This turns
    /// that declaration into a real session record the first time the run
    /// touches the tool surface, which is what gives approval something to
    /// release and gives the run a fallback profile to land on.
    fn ensure_plan_latch(&self) {
        if self
            .permission_profile
            .trim()
            .eq_ignore_ascii_case(plan_mode::PLAN_PROFILE)
            && plan_mode::snapshot(&self.parent_run_id).is_none()
        {
            plan_mode::enter(&self.parent_run_id, plan_mode::PLAN_PROFILE);
            self.emit_plan_transition(plan_mode::PlanTransition::Entered, None);
        }
    }

    /// Put a Plan Mode gear change on the run's event stream.
    ///
    /// Always call this **after** the mutation: the payload is built from a
    /// fresh snapshot, so an `Approved` emitted too early would report the plan
    /// ceiling as the profile in force and a `Rejected` would undercount the
    /// rejections. A run whose session has already been torn down emits
    /// nothing rather than a synthesised one — an invented gear change is worse
    /// than a missing one.
    fn emit_plan_transition(&self, transition: plan_mode::PlanTransition, reason: Option<&str>) {
        let Some(session) = plan_mode::snapshot(&self.parent_run_id) else {
            return;
        };
        self.events.append(
            &self.parent_run_id,
            plan_mode::changed_event(&session, transition, reason),
        );
    }

    /// The gear this run actually runs under: the plan ceiling while planning,
    /// the profile captured at entry once a plan is approved, otherwise the
    /// declared profile untouched.
    fn effective_permission_profile(&self) -> String {
        plan_mode::effective_profile(&self.parent_run_id, &self.permission_profile)
    }

    /// Plan Mode verdict for a tool call, fail-closed for anything the gateway
    /// does not know about (raw `mcp__*` names, orchestration stubs).
    fn plan_decision_for(&self, name: &str) -> PlanDecision {
        match self.gateway.get_tool(name) {
            Some(tool) => plan_mode::decision(name, tool.side_effect, tool.permission_class),
            None => plan_mode::decision(
                name,
                SideEffect::Destructive,
                capability_gateway::PermissionClass::Elevation,
            ),
        }
    }
}
/// Extract the server id from a namespaced `mcp__{server}__{tool}` name.
fn mcp_server_of_tool(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("mcp__")?;
    let end = rest.find("__")?;
    Some(&rest[..end])
}

#[async_trait::async_trait]
impl EngineToolRuntime for PermissionGatedTools {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        self.ensure_plan_latch();
        let planning = plan_mode::is_active(&self.parent_run_id);
        model_visible_tool_schemas(
            &self.gateway,
            self.tool_allowlist.as_deref(),
            &self.mcp_tool_schemas,
            self.selected_mcp_servers.as_ref(),
            planning,
        )
    }

    async fn list_tool_capabilities(&self) -> Vec<agent_core::ToolCapability> {
        self.ensure_plan_latch();
        let gateway_capabilities = self.gateway.list_capabilities();
        model_visible_tool_schemas(
            &self.gateway,
            self.tool_allowlist.as_deref(),
            &self.mcp_tool_schemas,
            self.selected_mcp_servers.as_ref(),
            plan_mode::is_active(&self.parent_run_id),
        )
        .into_iter()
        .map(|schema| {
            let gateway_capability = gateway_capabilities
                .iter()
                .find(|capability| capability.name == schema.name);
            let mode = match gateway_capability.map(|capability| capability.execution_mode) {
                Some(capability_gateway::ExecutionMode::ParallelSafe) => {
                    agent_core::ToolExecutionMode::ParallelSafe
                }
                Some(capability_gateway::ExecutionMode::Exclusive) => {
                    agent_core::ToolExecutionMode::Exclusive
                }
                Some(capability_gateway::ExecutionMode::Sequential) | None => {
                    agent_core::ToolExecutionMode::Sequential
                }
            };
            let side_effect = match gateway_capability.map(|capability| capability.side_effect) {
                Some(capability_gateway::SideEffect::ReadOnly) => {
                    agent_core::ToolSideEffect::ReadOnly
                }
                Some(capability_gateway::SideEffect::Write) => agent_core::ToolSideEffect::Write,
                Some(capability_gateway::SideEffect::Destructive) => {
                    agent_core::ToolSideEffect::Destructive
                }
                Some(capability_gateway::SideEffect::Network) => {
                    agent_core::ToolSideEffect::Network
                }
                Some(capability_gateway::SideEffect::Process) => {
                    agent_core::ToolSideEffect::Process
                }
                None => agent_core::ToolSideEffect::Destructive,
            };
            agent_core::ToolCapability {
                name: schema.name,
                schema: schema.input_schema,
                execution_mode: mode,
                side_effect,
                conflict_key: gateway_capability
                    .and_then(|capability| capability.conflict_key.clone()),
            }
        })
        .collect()
    }

    async fn mark_tool_call_uncertain(
        &self,
        call_id: &str,
        name: &str,
        turn_id: Option<&str>,
        input: &Value,
    ) -> Result<(), String> {
        crate::side_effect_ledger::record_tool_effect_state(
            &self.parent_run_id,
            call_id,
            name,
            crate::side_effect_ledger::category_for_tool(name),
            "uncertain",
            false,
            turn_id,
            input,
        )
    }

    async fn execute_tool_with_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        progress: &dyn ToolProgressSink,
    ) -> ToolExecutionResult {
        progress
            .publish(ToolProgressUpdate {
                run_id: self.parent_run_id.clone(),
                tool_call_id: name.to_string(),
                tool_name: name.to_string(),
                stream: "status".into(),
                text: "started".into(),
                final_update: false,
                turn_id: None,
                message_id: None,
                progress_sequence: 0,
            })
            .await;
        let result = self
            .execute_tool_with_call_id(name, input, cancel, None)
            .await;
        progress
            .publish(ToolProgressUpdate {
                run_id: self.parent_run_id.clone(),
                tool_call_id: name.to_string(),
                tool_name: name.to_string(),
                stream: "status".into(),
                text: if result.is_error {
                    "failed"
                } else {
                    "completed"
                }
                .into(),
                final_update: true,
                turn_id: None,
                message_id: None,
                progress_sequence: 0,
            })
            .await;
        result
    }

    async fn execute_tool_with_progress_for_call(
        &self,
        call_id: &str,
        turn_id: Option<&str>,
        message_id: Option<&str>,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        progress
            .publish(ToolProgressUpdate {
                run_id: self.parent_run_id.clone(),
                tool_call_id: call_id.to_string(),
                tool_name: name.to_string(),
                stream: "status".into(),
                text: "started".into(),
                final_update: false,
                turn_id: turn_id.map(str::to_string),
                message_id: message_id.map(str::to_string),
                progress_sequence: 0,
            })
            .await;
        let result = self
            .execute_tool_with_call_id_and_progress(
                name,
                input,
                cancel,
                Some(call_id),
                turn_id,
                message_id,
                progress.clone(),
            )
            .await;
        let child_run_id = result
            .output
            .get("run_id")
            .and_then(Value::as_str)
            .filter(|_| result.output.get("status").and_then(Value::as_str) == Some("running"))
            .map(str::to_string);
        if let Some(child_run_id) = child_run_id {
            spawn_subagent_progress(
                self.events.clone(),
                self.parent_run_id.clone(),
                call_id.to_string(),
                child_run_id,
                progress.clone(),
                turn_id.map(str::to_string),
                message_id.map(str::to_string),
            );
        } else {
            progress
                .publish(ToolProgressUpdate {
                    run_id: self.parent_run_id.clone(),
                    tool_call_id: call_id.to_string(),
                    tool_name: name.to_string(),
                    stream: "status".into(),
                    text: if result.is_error {
                        "failed"
                    } else {
                        "completed"
                    }
                    .into(),
                    final_update: true,
                    turn_id: turn_id.map(str::to_string),
                    message_id: message_id.map(str::to_string),
                    progress_sequence: 0,
                })
                .await;
        }
        result
    }

    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
    ) -> ToolExecutionResult {
        self.execute_tool_with_call_id(name, input, cancel, None)
            .await
    }

    async fn execute_tool_with_call_id(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        call_id: Option<&str>,
    ) -> ToolExecutionResult {
        self.execute_tool_with_call_id_and_progress(
            name,
            input,
            cancel,
            call_id,
            None,
            None,
            Arc::new(NoopToolProgressSink),
        )
        .await
    }

    async fn execute_tool_with_call_id_and_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        call_id: Option<&str>,
        turn_id: Option<&str>,
        message_id: Option<&str>,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        if cancel.is_cancelled() {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "cancelled"}),
                is_error: true,
                duration_ms: 0,
            };
        }
        let stream_tool_call_id = call_id
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Hard allowlist gate before permission / orchestration (Phase 0).
        if !self.tool_allowed(name) {
            return Self::deny_not_allowlisted(name);
        }
        // Generic mcp_call carries the server in its input — the ADR-0016
        // server whitelist must gate it too, not just namespaced names.
        if name == "mcp_call" {
            if let Some(selected) = &self.selected_mcp_servers {
                let server = input
                    .get("server")
                    .or_else(|| input.get("server_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !selected.contains(server) {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!(
                                "mcp server '{server}' is not part of this run's capability selection"
                            ),
                            "denied": true,
                            "code": "MCP_SERVER_NOT_SELECTED",
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            }
        }

        // Plan Mode latch, ahead of everything else.
        //
        // Ahead of the identity check because the exit path must stay reachable
        // even when the project binding is unhappy — a run that cannot submit a
        // plan and cannot write is a run with no way out. Ahead of the subagent
        // budget because a refused call is not work and must not cost a slot.
        self.ensure_plan_latch();
        let planning = plan_mode::is_active(&self.parent_run_id);
        if name == plan_mode::EXIT_PLAN_MODE_TOOL {
            return self.handle_exit_plan_mode(input, planning).await;
        }
        if name == plan_mode::ENTER_PLAN_MODE_TOOL {
            return self.handle_enter_plan_mode(input).await;
        }
        if planning && self.plan_decision_for(name) == PlanDecision::Deny {
            // Deliberately not a permission prompt. Plan Mode exists so the user
            // reviews one plan instead of a stream of approval cards; turning a
            // blocked write into a card would rebuild the thing it replaces.
            let err = plan_mode::blocked_error(name);
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": err.message,
                    "code": err.code,
                    "denied": true,
                    "plan_mode": true,
                }),
                is_error: true,
                duration_ms: 0,
            };
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
        if name.starts_with("mcp__")
            && !self
                .mcp_tool_schemas
                .iter()
                .any(|schema| schema.name == name)
        {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "UNKNOWN_TOOL",
                    "error": format!("unknown advertised MCP tool: {name}"),
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        if let Some(tool) = tool {
            if let Err(error) = CapabilityGateway::validate_external_input(&tool.schema, &input) {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "SCHEMA_INVALID",
                        "error": error.message,
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        // Dynamic MCP tools are not local Gateway handlers, but their
        // advertised schemas are still an execution trust boundary. Validate
        // the actual tool arguments before permission or transport dispatch;
        // generic `mcp_call` unwraps its nested `arguments` object first.
        if is_mcp {
            let schema_name = if name == "mcp_call" {
                let server = input
                    .get("server")
                    .or_else(|| input.get("server_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let tool_name = input
                    .get("tool")
                    .or_else(|| input.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                (!server.is_empty() && !tool_name.is_empty())
                    .then(|| format!("mcp__{server}__{tool_name}"))
            } else {
                Some(name.to_string())
            };
            if let Some(schema_name) = schema_name {
                if let Some(schema) = self
                    .mcp_tool_schemas
                    .iter()
                    .find(|schema| schema.name == schema_name)
                {
                    let schema_input = if name == "mcp_call" {
                        input
                            .get("arguments")
                            .or_else(|| input.get("input"))
                            .unwrap_or(&Value::Null)
                    } else {
                        &input
                    };
                    if let Err(error) = CapabilityGateway::validate_external_input(
                        &schema.input_schema,
                        schema_input,
                    ) {
                        return ToolExecutionResult {
                            output: serde_json::json!({
                                "error_code": "SCHEMA_INVALID",
                                "error": error.message,
                            }),
                            is_error: true,
                            duration_ms: 0,
                        };
                    }
                }
            }
        }

        // Permission gate: PermissionClass + SideEffect (G5).
        // Orchestration tools always go through this gate (never early-return around it).
        // Read the gear through the Plan Mode latch, never the raw declared
        // profile: while planning it reads `plan`, and after approval it reads
        // the profile captured at entry (which can only be what the run already
        // had).
        let effective_profile = self.effective_permission_profile();
        let profile_str = match effective_profile.as_str() {
            "full_access" | "autonomous" | "full" => "autonomous",
            "readonly" => "readonly",
            p if p == plan_mode::PLAN_PROFILE => plan_mode::PLAN_PROFILE,
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
            // The ceiling a PermissionRequest hook may not raise. A hook's
            // `permissionDecision: "allow"` skips a *confirmation*; it can never
            // buy a capability the profile itself withholds. So auto-approval is
            // offered only where the policy genuinely says "ask a human":
            //   - `Denied(_)` is a policy refusal, not a question;
            //   - `readonly` reaches here only for read-only side effects, and
            //     `request_permission_for_profile` refuses that profile anyway.
            let auto_approve_allowed = matches!(
                class_result,
                capability_gateway::policy::PolicyResult::NeedsApproval(_)
            ) && profile_str != "readonly";
            if let Some(denied) = self
                .await_tool_permission(&stream_tool_call_id, name, &input, auto_approve_allowed)
                .await
            {
                return denied;
            }
        }

        // This is the first event that authorizes handler execution. It is
        // emitted only after allowlist, project identity and permission gates;
        // rejected calls never receive a started fact.
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::ToolCallStarted {
                    id: stream_tool_call_id.clone(),
                    name: name.to_string(),
                },
            )
            .is_err()
        {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": "tool start event could not be persisted"
                }),
                is_error: true,
                duration_ms: 0,
            };
        }
        if name == "skill" {
            let skill_name = input.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(project_root) = self.gateway.project_root.as_deref() else {
                return ToolExecutionResult {
                    output: serde_json::json!({"error": "project root required to load skill"}),
                    is_error: true,
                    duration_ms: 0,
                };
            };
            return match crate::skill_store::load_skill_for_project(
                std::path::Path::new(project_root),
                skill_name,
            ) {
                Ok(output) => ToolExecutionResult {
                    output,
                    is_error: false,
                    duration_ms: 0,
                },
                Err(error) => ToolExecutionResult {
                    output: serde_json::json!({"error": error}),
                    is_error: true,
                    duration_ms: 0,
                },
            };
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
            return self
                .execute_mcp_call(
                    name,
                    input,
                    Some(&stream_tool_call_id),
                    turn_id,
                    message_id,
                    cancel,
                    progress.clone(),
                )
                .await;
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
        if name == "run_terminal" && input.get("cwd").and_then(|v| v.as_str()).is_none() {
            if let Some(root) = &self.gateway.project_root {
                if let Some(obj) = input.as_object_mut() {
                    obj.insert("cwd".into(), Value::String(root.clone()));
                }
            }
        }

        let started = Instant::now();
        // Create tool call context FIRST so Gateway path preflight can
        // authorize write paths before any checkpoint I/O (N01).
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(|| cancel.clone())
        } else {
            cancel.clone()
        };
        let live_settled = Arc::new(AtomicBool::new(false));
        let live_dropped_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let live_forwarder = if name == "run_terminal" {
            // H03: bounded live-output channel. Overflow drops at the producer
            // and is counted in `live_dropped_bytes`; a slow consumer can never
            // grow memory unboundedly.
            let (tx, mut rx) = tokio::sync::mpsc::channel::<capability_gateway::ToolProgressChunk>(
                capability_gateway::TERMINAL_PROGRESS_CAPACITY,
            );
            let run_id = self.parent_run_id.clone();
            let call_id = stream_tool_call_id.clone();
            let turn_id = turn_id.map(str::to_string);
            let message_id = message_id.map(str::to_string);
            let settled = live_settled.clone();
            // Terminal live output goes through the SAME batched progress sink
            // as every other tool (8KiB/250ms) — there is no second direct
            // EventLog path for run_terminal. The handler drains supervisor
            // chunks into this channel; the sink batches and late-drops after
            // the call settles, just like MCP/Subagent progress.
            let progress = progress.clone();
            let forwarder = tokio::spawn(async move {
                while let Some(chunk) = rx.recv().await {
                    if settled.load(AtomicOrdering::Acquire) {
                        break;
                    }
                    progress
                        .publish(agent_core::ToolProgressUpdate {
                            run_id: run_id.clone(),
                            tool_call_id: call_id.clone(),
                            tool_name: "run_terminal".into(),
                            stream: chunk.stream,
                            text: chunk.text,
                            final_update: false,
                            turn_id: turn_id.clone(),
                            message_id: message_id.clone(),
                            progress_sequence: 0,
                        })
                        .await;
                }
            });
            Some((tx, forwarder))
        } else {
            None
        };
        let progress_tx = live_forwarder.as_ref().map(|(tx, _)| tx.clone());
        let tool_context = self
            .build_tool_call_context(
                stream_tool_call_id.clone(),
                cancel.clone(),
                progress_tx,
                live_dropped_bytes.clone(),
                turn_id.map(str::to_string),
                message_id.map(str::to_string),
            )
            .await;

        // A4 — ReadOnly Fast Path.
        //
        // Classification source is the Gateway's SideEffect, never the legacy
        // name-default `external`. Genuinely read-only tools (read_file /
        // list_dir / grep / search / memory reads) skip checkpoint before/after,
        // the side-effect ledger, and the conflict lease entirely — they have no
        // durable side effect to record or resume. ToolCallStarted (above) and
        // ToolCallCompleted (emitted by the engine loop after this returns)
        // remain durable facts, so the read-only invocation is still
        // observable/replayable. Mutating/process/network/MCP tools keep the
        // full strict path below.
        if matches!(side_effect, SideEffect::ReadOnly) {
            let started = Instant::now();
            let result = match self
                .gateway
                .execute(name, input.clone(), &tool_context)
                .await
            {
                Ok(out) => {
                    let mut output = out.result;
                    attach_tool_output_artifact(
                        &self.parent_run_id,
                        &stream_tool_call_id,
                        &mut output,
                    );
                    let is_error = output.get("error_code").and_then(Value::as_str).is_some()
                        || output.get("error").is_some();
                    ToolExecutionResult {
                        output,
                        is_error,
                        duration_ms: out.duration_ms.max(started.elapsed().as_millis() as u64),
                    }
                }
                Err(error) => ToolExecutionResult {
                    output: serde_json::json!({ "error": error, "code": "readonly_tool_failed" }),
                    is_error: true,
                    duration_ms: started.elapsed().as_millis() as u64,
                },
            };
            return result;
        }

        // Phase 3: Gateway path preflight MUST precede any checkpoint I/O (N01).
        // A rejected path returns before the ledger and the handler, so it
        // causes zero I/O. Checkpoint only ever receives Gateway-authorized
        // canonical paths (`TrustedPath`); it no longer accepts raw strings.
        let trusted_paths = match self
            .gateway
            .preflight_write_paths(name, &input, &tool_context)
        {
            Ok(paths) => paths,
            Err(e) => {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": e.code,
                        "error": e.message,
                    }),
                    is_error: true,
                    duration_ms: 0,
                }
            }
        };
        for trusted in &trusted_paths {
            if let Err(error) = self
                .checkpoint_manager()
                .capture_before_async(&self.parent_run_id, trusted)
                .await
            {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "PERSISTENCE_FAILED",
                        "error": format!("checkpoint before-image could not be persisted: {error}"),
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        if let Err(error) = crate::side_effect_ledger::record_tool_effect_state(
            &self.parent_run_id,
            &stream_tool_call_id,
            name,
            crate::side_effect_ledger::category_for_tool(name),
            "started",
            false,
            turn_id,
            &input,
        ) {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": format!("tool side-effect ledger could not be started: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        // D02: hold the cross-run conflict lease for the duration of the
        // handler. The RAII guard releases it on every exit path, so a cancel
        // or failure never leaves the conflict key leased.
        let conflict_key = self.gateway.conflict_key_for(name);
        let _conflict_lease = match conflict_key {
            Some(key) => match crate::runtime::conflict_lease::global_conflict_leases()
                .acquire_guard(&key, &self.parent_run_id)
            {
                Ok(guard) => Some(guard),
                Err(error) => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "CONFLICT_LEASE_DENIED",
                            "error": error,
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            },
            None => None,
        };

        let result = match self
            .gateway
            .execute(name, input.clone(), &tool_context)
            .await
        {
            Ok(out) => {
                let mut output = out.result;
                attach_tool_output_artifact(&self.parent_run_id, &stream_tool_call_id, &mut output);
                let mut checkpoint_error = None;
                for trusted in &trusted_paths {
                    if let Err(error) = self
                        .checkpoint_manager()
                        .capture_after_async(&self.parent_run_id, trusted)
                        .await
                    {
                        checkpoint_error = Some(error);
                        break;
                    }
                }
                if let Some(error) = checkpoint_error.as_deref() {
                    // The handler already ran but the after-image could not be
                    // persisted, so the effect outcome is not fully known:
                    // surface a durable `uncertain` mark. If even that write
                    // fails, the intent stays `started`, which the resume gate
                    // treats as unresolved — never replay-safe (D04).
                    let _ = crate::side_effect_ledger::record_tool_effect_state(
                        &self.parent_run_id,
                        &stream_tool_call_id,
                        name,
                        crate::side_effect_ledger::category_for_tool(name),
                        "uncertain",
                        false,
                        turn_id,
                        &input,
                    );
                    output = serde_json::json!({
                        "error_code": "PERSISTENCE_FAILED",
                        "error": format!("checkpoint after-image could not be persisted: {error}"),
                    });
                }
                // On success the ledger intent stays `started`: the daemon
                // event log settles it to a terminal status in the same
                // transaction that appends the ToolCallCompleted fact (D04).
                // A crash between the handler returning and that append leaves
                // the effect unresolved, so resume blocks instead of re-running
                // it.
                if name == "run_terminal" && live_forwarder.is_none() {
                    emit_terminal_output_deltas(
                        &self.events,
                        &self.parent_run_id,
                        &stream_tool_call_id,
                        &output,
                    );
                    // Background shell tasks: surface on Activity task list.
                    if output
                        .get("background")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        let task_id = output
                            .get("task_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&stream_tool_call_id)
                            .to_string();
                        let label = output
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
                                    output: output
                                        .get("output")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                },
                            );
                        }
                    }
                }
                if checkpoint_error.is_none() {
                    for trusted in &trusted_paths {
                        let rel = trusted.project_relative.to_string_lossy().into_owned();
                        // Best-effort FileChanged with before/after from checkpoint live map.
                        if let Ok(preview) = self
                            .checkpoint_manager()
                            .checkpoint_for_run_public(&self.parent_run_id)
                        {
                            if let Some(snap) = preview.files.iter().find(|f| f.path == rel) {
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
                }
                if name == "notification" {
                    let hooks = crate::production_hooks::build_production_hooks_for_project(
                        self.gateway
                            .project_root
                            .as_deref()
                            .map(std::path::Path::new),
                    )
                    .with_events(self.events.clone());
                    let _ = hooks
                        .dispatch(HookRequest {
                            event: HookEvent::Notification,
                            run_id: self.parent_run_id.clone(),
                            tool_name: Some(name.to_string()),
                            input: serde_json::json!({
                                "input": input,
                                "output": output.clone(),
                            }),
                        })
                        .await;
                }
                // T06: convert a successful `creative_proposal` tool result into
                // a durable typed proposal fact so the Host can pull it over UDS
                // and surface a pending approval inbox. The tool result itself
                // stays a normal ToolOutput — the fact is the durable side
                // record. A persistence failure must not fail the tool call
                // (the model still completed its work); it only loses the
                // bridge, which is logged for observability.
                if name == crate::proposal_fact::PROPOSAL_TOOL
                    && output.get("ok").and_then(Value::as_bool) == Some(true)
                {
                    if let Some(payload) =
                        crate::proposal_fact::proposal_from_tool_output(name, &output)
                    {
                        if let Some(store) =
                            crate::run_manager::global_run_manager().data_store_ref()
                        {
                            if let Err(e) = crate::proposal_fact::record_proposal_fact(
                                &store,
                                &self.parent_run_id,
                                turn_id,
                                &stream_tool_call_id,
                                &payload,
                            ) {
                                eprintln!(
                                    "[proposal_fact] failed to record creative proposal fact: {e}"
                                );
                            }
                        }
                    }
                }
                let is_error =
                    output.get("error_code").and_then(Value::as_str) == Some("PERSISTENCE_FAILED");
                ToolExecutionResult {
                    output,
                    is_error,
                    duration_ms: out.duration_ms.max(started.elapsed().as_millis() as u64),
                }
            }
            Err(err) => {
                let status = if err.code == "cancelled" {
                    "cancelled"
                } else if err.code == "timeout" {
                    "uncertain"
                } else {
                    "failed"
                };
                let ledger_result = crate::side_effect_ledger::record_tool_effect_state(
                    &self.parent_run_id,
                    &stream_tool_call_id,
                    name,
                    crate::side_effect_ledger::category_for_tool(name),
                    status,
                    false,
                    turn_id,
                    &input,
                );
                let output = match ledger_result {
                    Ok(()) => serde_json::json!({"error": err.message, "code": err.code}),
                    Err(ledger_error) => serde_json::json!({
                        "error_code": "PERSISTENCE_FAILED",
                        "error": format!("tool side-effect ledger could not be settled: {ledger_error}"),
                    }),
                };
                ToolExecutionResult {
                    output,
                    is_error: true,
                    duration_ms: started.elapsed().as_millis() as u64,
                }
            }
        };
        live_settled.store(true, AtomicOrdering::Release);
        drop(tool_context);
        if let Some((tx, forwarder)) = live_forwarder {
            drop(tx);
            forwarder.abort();
        }
        result
    }
}

fn spawn_subagent_progress(
    events: EventSequencer,
    parent_run_id: String,
    tool_call_id: String,
    child_run_id: String,
    progress: Arc<dyn ToolProgressSink>,
    turn_id: Option<String>,
    message_id: Option<String>,
) {
    tokio::spawn(async move {
        let mut cursor = 0u64;
        let mut pending = String::new();
        let mut last_emit = Instant::now() - Duration::from_millis(50);
        for _ in 0..3_600 {
            let child_events = match events.replay_after_checked(&child_run_id, cursor) {
                Ok(events) => events,
                Err(_) => break,
            };
            for event in child_events {
                cursor = cursor.max(event.effective_run_sequence());
                match event.payload {
                    RunEventKind::TextDelta { text } | RunEventKind::ReasoningDelta { text } => {
                        pending.push_str(&text)
                    }
                    RunEventKind::Progress { message, .. } => {
                        pending.push_str(&message);
                    }
                    _ => {}
                }
            }
            if !pending.is_empty() && last_emit.elapsed() >= Duration::from_millis(50) {
                let text = std::mem::take(&mut pending);
                progress
                    .publish(ToolProgressUpdate {
                        run_id: parent_run_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool_name: "task".into(),
                        stream: "subagent".into(),
                        text,
                        final_update: false,
                        turn_id: turn_id.clone(),
                        message_id: message_id.clone(),
                        progress_sequence: 0,
                    })
                    .await;
                last_emit = Instant::now();
            }
            let Some(run) = crate::global_run_manager().get_run(&child_run_id) else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            if run.status.is_terminal() {
                if !pending.is_empty() {
                    progress
                        .publish(ToolProgressUpdate {
                            run_id: parent_run_id.clone(),
                            tool_call_id: tool_call_id.clone(),
                            tool_name: "task".into(),
                            stream: "subagent".into(),
                            text: std::mem::take(&mut pending),
                            final_update: false,
                            turn_id: turn_id.clone(),
                            message_id: message_id.clone(),
                            progress_sequence: 0,
                        })
                        .await;
                }
                progress
                    .publish(ToolProgressUpdate {
                        run_id: parent_run_id,
                        tool_call_id: tool_call_id.clone(),
                        tool_name: "task".into(),
                        stream: "subagent".into(),
                        text: run.status.as_str().to_string(),
                        final_update: true,
                        turn_id,
                        message_id,
                        progress_sequence: 0,
                    })
                    .await;
                progress.mark_tool_call_settled(&tool_call_id).await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

fn attach_tool_output_artifact(run_id: &str, call_id: &str, output: &mut Value) {
    const ARTIFACT_THRESHOLD: usize = 16 * 1024;
    let serialized = output.to_string();
    if serialized.len() < ARTIFACT_THRESHOLD {
        return;
    }
    let preview: String = serialized.chars().take(4_000).collect();
    if let Ok(meta) = crate::artifact_store::global_artifacts().put(
        run_id,
        &format!("tool-{call_id}.json"),
        serialized.as_bytes(),
        Some("application/json".into()),
    ) {
        *output = serde_json::json!({
            "artifact_id": meta.id,
            "preview": preview,
            "truncated": true,
            "bytes": serialized.len(),
        });
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
                        tool_name: Some("run_terminal".into()),
                        stream: stream.into(),
                        text: String::new(),
                        truncated: true,
                        turn_id: None,
                        message_id: None,
                        progress_sequence: None,
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
                    tool_name: Some("run_terminal".into()),
                    stream: stream.into(),
                    text: chunk,
                    truncated: persisted >= MAX_PERSIST || end < bytes.len() && take < CHUNK,
                    turn_id: None,
                    message_id: None,
                    progress_sequence: None,
                },
            );
            offset = end;
            if take == 0 {
                break;
            }
        }
    }
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
        progress: Option<tokio::sync::mpsc::Sender<capability_gateway::ToolProgressChunk>>,
        progress_dropped_bytes: Arc<std::sync::atomic::AtomicU64>,
        turn_id: Option<String>,
        message_id: Option<String>,
    ) -> capability_gateway::ToolCallContext {
        if let Some(identity) = self.verified_project_identity().await {
            let mut context =
                capability_gateway::ToolCallContext::from_verified_identity_with_cancel(
                    identity.project_id.clone(),
                    identity.identity_version,
                    std::path::PathBuf::from(&identity.canonical_path),
                    self.parent_run_id.clone(),
                    self.conversation_id.clone(),
                    tool_call_id,
                    self.permission_profile.clone(),
                    cancel,
                );
            context.set_progress(progress, progress_dropped_bytes);
            context.turn_id = turn_id;
            context.message_id = message_id;
            return context;
        }
        let project_root = self
            .gateway
            .project_root
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let mut context = capability_gateway::ToolCallContext::with_cancel(
            project_root,
            self.parent_run_id.clone(),
            self.conversation_id.clone(),
            tool_call_id,
            self.permission_profile.clone(),
            cancel,
        );
        context.set_progress(progress, progress_dropped_bytes);
        context.turn_id = turn_id;
        context.message_id = message_id;
        context
    }

    /// Run the permission gate for one tool call.
    ///
    /// `None` means "proceed"; `Some` is the denial to return to the model.
    ///
    /// `auto_approve_allowed` is the profile ceiling computed by the caller: it
    /// is the *only* switch that lets a hook's `permissionDecision: "allow"`
    /// skip the prompt. Hooks never widen it.
    async fn await_tool_permission(
        &self,
        tool_call_id: &str,
        name: &str,
        input: &Value,
        auto_approve_allowed: bool,
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
        let project_root = self
            .gateway
            .project_root
            .as_deref()
            .map(std::path::Path::new);
        let permission_hooks =
            crate::production_hooks::build_production_hooks_for_project(project_root)
                .with_events(self.events.clone());
        let hook_outcomes = permission_hooks
            .dispatch_outcomes(HookRequest {
                event: HookEvent::PermissionRequest,
                run_id: self.parent_run_id.clone(),
                tool_name: Some(name.to_string()),
                input: input.clone(),
            })
            .await;
        // Plan Mode maps to ConfirmEach here: what survives the latch is a read
        // or a network read, and the user is still the one who says yes to it.
        let profile = match self.effective_permission_profile().as_str() {
            "readonly" | "read_only" => PermissionProfile::ReadOnly,
            "full_access" | "autonomous" | "full" => PermissionProfile::Autonomous,
            _ => PermissionProfile::ConfirmEach,
        };
        match hook_permission_gate(
            HookRegistry::aggregate_permission(&hook_outcomes),
            profile,
            auto_approve_allowed,
        ) {
            HookPermissionGate::Deny(reason) => {
                return Some(ToolExecutionResult {
                    output: serde_json::json!({
                        "error": reason,
                        "denied": true,
                        "denied_by_hook": true,
                    }),
                    is_error: true,
                    duration_ms: 0,
                });
            }
            HookPermissionGate::AutoApprove(reason) => {
                // Auto-approval must never be silent: a skipped confirmation
                // that leaves no trace is worse than a prompt. Until the
                // protocol grows a dedicated event, the request/response pair
                // carries the audit — it is the only existing shape that
                // records the tool, its input, and who answered.
                let permission_id = format!("hook-auto-{}", uuid::Uuid::new_v4());
                eprintln!(
                    "[production] hook auto-approved tool `{name}` on run {} ({reason})",
                    self.parent_run_id
                );
                if self
                    .events
                    .append_checked(
                        &self.parent_run_id,
                        RunEventKind::PermissionRequested {
                            tool_call_id: tool_call_id.to_string(),
                            tool_name: name.to_string(),
                            reason: format!("Approve tool `{name}`"),
                            permission_id: permission_id.clone(),
                            input: input.clone(),
                        },
                    )
                    .is_err()
                {
                    return Some(ToolExecutionResult {
                        output: serde_json::json!({"error": "permission event persistence failed"}),
                        is_error: true,
                        duration_ms: 0,
                    });
                }
                if self
                    .events
                    .append_checked(
                        &self.parent_run_id,
                        RunEventKind::PermissionResponded {
                            permission_id,
                            approved: true,
                            scope: HOOK_AUTO_APPROVE_SCOPE.to_string(),
                        },
                    )
                    .is_err()
                {
                    return Some(ToolExecutionResult {
                        output: serde_json::json!({"error": "permission event persistence failed"}),
                        is_error: true,
                        duration_ms: 0,
                    });
                }
                return None;
            }
            HookPermissionGate::Prompt => {}
        }

        let permission_id = match self
            .permissions
            .request_permission_for_profile(
                profile,
                &self.parent_run_id,
                tool_call_id,
                name,
                format!("Approve {name}?"),
                input.clone(),
            )
            .await
        {
            Ok(id) => id,
            Err(error) => {
                return Some(ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "PERMISSION_PERSISTENCE_FAILED",
                        "error": format!(
                            "permission request could not be created: {}",
                            error.technical_message
                        ),
                    }),
                    is_error: true,
                    duration_ms: 0,
                });
            }
        };

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
        if let Err(error) = crate::interaction_store::insert_pending(
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
        ) {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission interaction could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, Some(permission_id.clone()));
        if let Err(error) = crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id)
        {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "persistence_failed"}),
            );
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission actor snapshot could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::PermissionRequested {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: name.to_string(),
                    reason: format!("Approve tool `{name}`"),
                    permission_id: permission_id.clone(),
                    input: input.clone(),
                },
            )
            .is_err()
        {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "persistence_failed"}),
            );
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return Some(ToolExecutionResult {
                output: serde_json::json!({"error": "permission event persistence failed"}),
                is_error: true,
                duration_ms: 0,
            });
        }
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
        // A timeout/cancellation has no UI RPC response to mark the durable
        // interaction complete. Leaving it pending makes reconnect replay an
        // orphaned approval card after its oneshot waiter has gone away.
        if let Err(error) = crate::interaction_store::mark_resolved(
            &permission_id,
            serde_json::json!({ "approved": approved, "scope": scope }),
        ) {
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission response could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, None);
        if let Err(error) = crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id)
        {
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission actor snapshot could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::PermissionResponded {
                    permission_id,
                    approved,
                    scope: scope.clone(),
                },
            )
            .is_err()
        {
            return Some(ToolExecutionResult {
                output: serde_json::json!({"error": "permission event persistence failed"}),
                is_error: true,
                duration_ms: 0,
            });
        }
        if approved {
            if let Some(rt) = &self.runtime {
                // Structured grant only — empty write_file pattern no longer means any path.
                rt.remember_tool_grant_invocation(&inv, &scope).await;
                let _ = pattern; // kept for legacy audit trails if needed
            }
        }
        if !approved {
            let _ = permission_hooks
                .dispatch(HookRequest {
                    event: HookEvent::PermissionDenied,
                    run_id: self.parent_run_id.clone(),
                    tool_name: Some(name.to_string()),
                    input: input.clone(),
                })
                .await;
        }
        // Phase 2: AfterPermissionResolved is a documented safe point. Message
        // mutation lives in AgentEngine (apply_safe_point); the tool layer cannot
        // push into provider history here. Call the harness so the seam is live,
        // then re-queue any interjection so Engine's AfterTool/ProviderBatch can
        // inject it into messages (on_safe_point consumes pending).
        match crate::prompt_queue_store::on_safe_point_checked(
            &self.conversation_id,
            agent_core::SafePoint::AfterPermissionResolved,
        ) {
            Ok(agent_core::HarnessAction::InjectInterjection { content }) => {
                if let Err(error) = crate::prompt_queue_store::restore_interjection_checked(
                    &self.conversation_id,
                    content,
                ) {
                    return Some(ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "QUEUE_PERSISTENCE_FAILED",
                            "error": format!("safe-point interjection restore failed: {error}"),
                        }),
                        is_error: true,
                        duration_ms: 0,
                    });
                }
            }
            Ok(_) => {}
            Err(error) => {
                return Some(ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "QUEUE_PERSISTENCE_FAILED",
                        "error": format!("safe-point state could not be persisted: {error}"),
                    }),
                    is_error: true,
                    duration_ms: 0,
                });
            }
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

    /// `enter_plan_mode`: close the latch for this run.
    ///
    /// Handled here rather than left to the gateway loop below so it skips the
    /// ProjectIdentity gate. Entering Plan Mode only ever removes capability, so
    /// there is nothing for a project binding to protect — and refusing it on an
    /// unbound run would deny the agent the one move that makes an unbound run
    /// safer.
    async fn handle_enter_plan_mode(&self, input: Value) -> ToolExecutionResult {
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(CancellationToken::new)
        } else {
            CancellationToken::new()
        };
        let context = self
            .build_tool_call_context(
                uuid::Uuid::new_v4().to_string(),
                cancel,
                None,
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
                None,
                None,
            )
            .await;
        match self
            .gateway
            .execute(plan_mode::ENTER_PLAN_MODE_TOOL, input, &context)
            .await
        {
            Ok(out) => ToolExecutionResult {
                output: out.result,
                is_error: false,
                duration_ms: out.duration_ms,
            },
            Err(err) => ToolExecutionResult {
                output: serde_json::json!({"error": err.message, "code": err.code}),
                is_error: true,
                duration_ms: 0,
            },
        }
    }

    /// `exit_plan_mode`: submit a plan and block on a real human answer.
    ///
    /// The whole safety property of Plan Mode lives in this function: the model
    /// hands over a structured plan, the user is the one who answers, and the
    /// latch is released *only* on an approval that came back through the
    /// interaction hub. Nothing the model can say short-circuits it.
    async fn handle_exit_plan_mode(&self, input: Value, planning: bool) -> ToolExecutionResult {
        if !planning {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": "this run is not in Plan Mode",
                    "code": "not_in_plan_mode",
                    "approved": false,
                }),
                is_error: true,
                duration_ms: 0,
            };
        }
        let plan = match plan_mode::parse_plan(&input) {
            Ok(plan) => plan,
            Err(err) => {
                // A malformed plan is the model's problem to fix, not the
                // user's to squint at. Never show a card built from it.
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": err.message,
                        "code": err.code,
                        "approved": false,
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        };
        if let Err(err) = plan_mode::record_submission(&self.parent_run_id, plan.clone()) {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": err.message,
                    "code": err.code,
                    "approved": false,
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        self.emit_plan_transition(plan_mode::PlanTransition::Submitted, None);

        let plan_json = serde_json::to_value(&plan).unwrap_or_else(|_| serde_json::json!({}));
        let started = Instant::now();
        let (approved, _scope) = self.await_plan_approval(&plan, &plan_json).await;

        if !approved {
            let rejections = plan_mode::reject(&self.parent_run_id).unwrap_or(0);
            self.emit_plan_transition(plan_mode::PlanTransition::Rejected, None);
            // Not `is_error`: a rejection is a legitimate answer to a question
            // the model asked. Flagging it as a failure invites retry logic to
            // treat "the user said no" as a transient fault.
            return ToolExecutionResult {
                output: serde_json::json!({
                    "approved": false,
                    "plan_mode": true,
                    "rejections": rejections,
                    "message": "The user did not approve this plan. You are still in Plan Mode: \
                                nothing has been executed. Ask what they want changed, revise, \
                                and submit again. Do not claim the plan was approved.",
                }),
                is_error: false,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }

        match plan_mode::approve(&self.parent_run_id) {
            Ok(profile) => {
                self.emit_plan_transition(plan_mode::PlanTransition::Approved, None);
                ToolExecutionResult {
                    output: serde_json::json!({
                        "approved": true,
                        "plan_mode": false,
                        "permission_profile": profile,
                        "plan": plan_json,
                        "message": "The user approved this plan. Plan Mode is off and the run is back \
                                    on its original permission profile. Execute the approved steps and \
                                    nothing beyond them.",
                    }),
                    is_error: false,
                    duration_ms: started.elapsed().as_millis() as u64,
                }
            }
            Err(err) => ToolExecutionResult {
                output: serde_json::json!({
                    "error": err.message,
                    "code": err.code,
                    "approved": false,
                }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            },
        }
    }

    /// Put a plan in front of a human and wait.
    ///
    /// Deliberately does **not** go through `PermissionManager`: that path
    /// auto-approves on `autonomous` and refuses outright on `readonly`, and
    /// both would be wrong here. Plan approval is the one interaction that must
    /// always reach a person, whatever the profile says — an autonomous run in
    /// Plan Mode is exactly the case the gear was built for.
    ///
    /// Recorded as a `tool_permission` interaction so the existing respond RPC
    /// and the restart-recovery path resolve it unchanged; the GUI tells it
    /// apart by `tool_name` and renders the plan from the event payload.
    async fn await_plan_approval(
        &self,
        plan: &capability_gateway::Plan,
        plan_json: &Value,
    ) -> (bool, String) {
        let permission_id = uuid::Uuid::new_v4().to_string();
        let tool_call_id = uuid::Uuid::new_v4().to_string();
        let reason = format!("Approve plan: {}", plan.title);
        let card = serde_json::json!({
            "kind": "plan_approval",
            "plan": plan_json,
            "step_count": plan.steps.len(),
            "peak_risk": plan.peak_risk(),
            "has_irreversible_step": plan.has_irreversible_step(),
        });

        // Install the waiter before publishing the event, same race as the tool
        // permission path: a fast UI must not answer a question nobody is
        // listening for.
        let (tx, rx) = oneshot::channel::<(bool, String)>();
        self.interactions
            .register_permission(
                &permission_id,
                &self.parent_run_id,
                plan_mode::EXIT_PLAN_MODE_TOOL,
                tx,
            )
            .await;
        if crate::interaction_store::insert_pending(
            &permission_id,
            Some(&self.parent_run_id),
            Some(&self.conversation_id),
            "tool_permission",
            serde_json::json!({
                "tool_call_id": tool_call_id,
                "tool_name": plan_mode::EXIT_PLAN_MODE_TOOL,
                "reason": reason,
                "input": card,
            }),
        )
        .is_err()
        {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            return (false, "persistence_failed".into());
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, Some(permission_id.clone()));
        if crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id).is_err() {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "persistence_failed"}),
            );
            return (false, "persistence_failed".into());
        }
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::PermissionRequested {
                    tool_call_id,
                    tool_name: plan_mode::EXIT_PLAN_MODE_TOOL.to_string(),
                    reason: reason.clone(),
                    permission_id: permission_id.clone(),
                    input: card,
                },
            )
            .is_err()
        {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "persistence_failed"}),
            );
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return (false, "persistence_failed".into());
        }

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
                (false, "cancelled".to_string())
            }
            res = tokio::time::timeout(
                Duration::from_secs(PLAN_APPROVAL_TIMEOUT_SECS),
                rx,
            ) => {
                // Silence is not consent. Any failure to hear a real "yes"
                // resolves to rejection and leaves the latch closed.
                res.ok()
                    .and_then(|r| r.ok())
                    .map(|(ok, _)| (ok, PLAN_APPROVAL_SCOPE.to_string()))
                    .unwrap_or((false, "timeout".to_string()))
            }
        };

        if crate::interaction_store::mark_resolved(
            &permission_id,
            serde_json::json!({ "approved": approved, "scope": scope }),
        )
        .is_err()
        {
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return (false, "persistence_failed".into());
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, None);
        if crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id).is_err() {
            return (false, "persistence_failed".into());
        }
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::PermissionResponded {
                    permission_id,
                    approved,
                    scope: scope.clone(),
                },
            )
            .is_err()
        {
            return (false, "persistence_failed".into());
        }
        (approved, scope)
    }

    /// Same tree-cancel rules as ProductionRuntime::cancel_run_tree, using
    /// the shared engines/subagents/events maps held by this tool runtime.
    async fn cancel_run_tree_local(&self, run_id: &str) {
        // A cancelled run has no plan to approve. Dropping the session here is
        // safe in a way that eviction is not: the run is gone, so there is no
        // latch left to reopen.
        plan_mode::clear(run_id);
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
            plan_mode::clear(&d.run_id);
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
    #[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
    async fn execute_mcp_call(
        &self,
        name: &str,
        input: Value,
        core_call_id: Option<&str>,
        turn_id: Option<&str>,
        message_id: Option<&str>,
        parent_cancel: &CancellationToken,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
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
        let call_id = core_call_id
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("mcp-{server_id}-{tool_name}"));
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(|| parent_cancel.clone())
        } else {
            parent_cancel.clone()
        };
        let ledger_summary = serde_json::json!({
            "server": server_id.clone(),
            "tool": tool_name.clone(),
        });
        if let Err(error) = crate::side_effect_ledger::record_tool_effect_state(
            &self.parent_run_id,
            &call_id,
            "mcp_call",
            "mcp",
            "started",
            false,
            turn_id,
            &ledger_summary,
        ) {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": format!("MCP side-effect ledger could not be started: {error}"),
                }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
        let progress_callback: Arc<dyn Fn(Value) + Send + Sync> = {
            let progress = progress.clone();
            let run_id = self.parent_run_id.clone();
            let call_id = call_id.clone();
            let turn_id = turn_id.map(str::to_string);
            let message_id = message_id.map(str::to_string);
            Arc::new(move |frame: Value| {
                let text = frame
                    .get("params")
                    .and_then(|params| params.get("message").or_else(|| params.get("progress")))
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| value.to_string())
                    })
                    .unwrap_or_else(|| frame.to_string());
                let progress = progress.clone();
                let run_id = run_id.clone();
                let call_id = call_id.clone();
                let turn_id = turn_id.clone();
                let message_id = message_id.clone();
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        progress
                            .publish(ToolProgressUpdate {
                                run_id,
                                tool_call_id: call_id,
                                tool_name: "mcp_call".into(),
                                stream: "mcp".into(),
                                text,
                                final_update: false,
                                turn_id,
                                message_id,
                                progress_sequence: 0,
                            })
                            .await;
                    });
                }
            })
        };
        pending_mcp_calls().lock().unwrap().insert(call_id.clone());
        let outcome = crate::runtime::mcp_invocation::invoke_mcp_tool_with_progress(
            &server_id,
            &tool_name,
            arguments,
            &cancel,
            Some(&self.parent_run_id),
            Some(progress_callback),
        )
        .await;
        // J03: the pending registry is quiet once the call settles — no
        // lingering request can later be mistaken for a fresh effect.
        pending_mcp_calls().lock().unwrap().remove(&call_id);
        // T04: settle the progress channel so late progress frames are rejected
        // and any buffered flush task is drained — progress tasks return to 0.
        progress.mark_tool_call_settled(&call_id).await;
        match outcome {
            Ok(result) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                // J03: a late success that lands after the run was cancelled is
                // never recorded as completed — the external outcome is
                // unknowable while we are tearing the call down.
                let status = mcp_ledger_status(true, cancel.is_cancelled());
                // MCP is not auto-rollbackable — record for restore coverage honesty.
                let ledger_result = crate::side_effect_ledger::record_tool_effect_state(
                    &self.parent_run_id,
                    &call_id,
                    "mcp_call",
                    "mcp",
                    status,
                    false,
                    turn_id,
                    &ledger_summary,
                );
                if let Err(error) = ledger_result {
                    let _ = crate::side_effect_ledger::record_tool_effect_state(
                        &self.parent_run_id,
                        &call_id,
                        "mcp_call",
                        "mcp",
                        "uncertain",
                        false,
                        turn_id,
                        &ledger_summary,
                    );
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "PERSISTENCE_FAILED",
                            "error": format!("MCP side-effect ledger could not be completed: {error}"),
                        }),
                        is_error: true,
                        duration_ms,
                    };
                }
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
                let ledger_result = crate::side_effect_ledger::record_tool_effect_state(
                    &self.parent_run_id,
                    &call_id,
                    "mcp_call",
                    "mcp",
                    mcp_ledger_status(false, cancel.is_cancelled()),
                    false,
                    turn_id,
                    &ledger_summary,
                );
                if let Err(error) = ledger_result {
                    let _ = crate::side_effect_ledger::record_tool_effect_state(
                        &self.parent_run_id,
                        &call_id,
                        "mcp_call",
                        "mcp",
                        "uncertain",
                        false,
                        turn_id,
                        &ledger_summary,
                    );
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "PERSISTENCE_FAILED",
                            "error": format!("MCP side-effect ledger could not be settled: {error}"),
                        }),
                        is_error: true,
                        duration_ms,
                    };
                }
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

        // Expert team delegation (ADR-0016): `agent` must name a roster member;
        // outside the roster (or without a team) it is rejected fail-closed.
        let agent_param = input
            .get("agent")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let member_profile = match (&self.team, agent_param) {
            (Some(team), Some(agent)) => {
                if !team.members.iter().any(|m| m.expert_id == agent) {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!(
                                "agent '{agent}' is not a member of team '{}'",
                                team.team_id
                            ),
                            "code": "TEAM_MEMBER_INVALID",
                            "members": team.members.iter().map(|m| m.expert_id.clone()).collect::<Vec<_>>(),
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
                match crate::capability_resolution::load_profile(
                    agent,
                    self.gateway
                        .project_root
                        .as_deref()
                        .map(std::path::Path::new),
                ) {
                    Some(profile) => Some((agent.to_string(), profile)),
                    None => {
                        return ToolExecutionResult {
                            output: serde_json::json!({
                                "error": format!("team member profile not loadable: {agent}"),
                                "code": "TEAM_MEMBER_INVALID",
                            }),
                            is_error: true,
                            duration_ms: 0,
                        };
                    }
                }
            }
            (None, Some(agent)) => {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!(
                            "'agent' parameter ('{agent}') requires an active expert team selection"
                        ),
                        "code": "TEAM_NOT_ACTIVE",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
            (_, None) => None,
        };
        let member_profile_id = member_profile.as_ref().map(|(id, _)| id.clone());

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // ── Persona: an on-disk profile the parent picked, plus a prompt it wrote ──
        //
        // `subagent_type` is the Claude-Code-compatible name; the daemon's own
        // field name is accepted too. A profile the parent names but that does
        // not exist is an error, never a silent fallback to "no persona".
        let requested_profile_id = input
            .get("subagent_type")
            .or_else(|| input.get("agent_profile_id"))
            .or_else(|| input.get("agent_type"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let project_root_path = self
            .gateway
            .project_root
            .as_deref()
            .map(std::path::Path::new);
        let profile = match &requested_profile_id {
            Some(id) => match agent_core::load_agent_profile(id, project_root_path) {
                Some(p) => Some(p),
                None => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!("agent profile `{id}` not found in the project or user profile directories"),
                            "code": "agent_profile_not_found",
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            },
            None => None,
        };

        // Parent-authored system prompt for this child. Prompt text only — it is
        // never consulted when resolving permissions or the tool surface.
        let child_directive = match input
            .get("system_prompt")
            .or_else(|| input.get("agent_prompt"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(sp) if sp.len() > MAX_CHILD_SYSTEM_PROMPT_BYTES => {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!(
                            "system_prompt is {} bytes; the limit is {MAX_CHILD_SYSTEM_PROMPT_BYTES}. Put task detail in `prompt`, not the persona.",
                            sp.len()
                        ),
                        "code": "system_prompt_too_long",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
            Some(sp) => Some(sp.to_string()),
            None => None,
        };

        // ── Permission and tool surface ──
        //
        // Both resolutions live in `agent_core::subagents`: the profile can only
        // tighten what was requested, and the parent is a hard ceiling. Neither a
        // parent-authored prompt nor a self-selected profile can widen either one,
        // so a prompt-injected parent gains nothing by writing a hostile persona.
        // The profile mode fed to the valve is the persona the parent named, or
        // — when the child is a roster member instead — the member's own profile
        // (ADR-0016). Either way it can only tighten: `resolve_child_permission`
        // caps the request by the profile and then by the parent.
        let child_perm = agent_core::resolve_child_permission(
            &self.permission_profile,
            input.get("permission_profile").and_then(|v| v.as_str()),
            profile
                .as_ref()
                .and_then(|p| p.permission_mode.as_deref())
                .or_else(|| {
                    member_profile
                        .as_ref()
                        .and_then(|(_, m)| m.permission_mode.as_deref())
                }),
        );
        // A present-but-malformed `tool_allowlist` falls back to the readonly
        // default rather than to "inherit the parent surface".
        let requested_allowlist: Option<Vec<String>> = input.get("tool_allowlist").map(|value| {
            value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(default_subagent_tool_allowlist)
        });
        let mut child_allowlist = agent_core::resolve_child_tool_allowlist(
            self.tool_allowlist.as_deref(),
            requested_allowlist.as_deref(),
            profile.as_ref().and_then(|p| p.tools.as_deref()),
            profile.as_ref().and_then(|p| p.disallowed_tools.as_deref()),
        );
        // Member persona narrows the surface further (ADR-0016): profile.tools
        // intersects what survived the parent ceiling, minus disallowed. Both
        // operations are `retain`, so a member profile can only remove tools —
        // naming a roster member never buys reach the parent lacked.
        if let Some((_, member)) = &member_profile {
            if let Some(tools) = member.tools.as_ref().filter(|t| !t.is_empty()) {
                child_allowlist.retain(|tool| tools.iter().any(|t| t == tool));
            }
            if let Some(disallowed) = member.disallowed_tools.as_ref() {
                child_allowlist.retain(|tool| !disallowed.iter().any(|d| d == tool));
            }
        }
        let child_allowlist = child_allowlist;

        // ── Step budget: request, else the profile's, else the daemon default ──
        let child_max_steps = input
            .get("max_steps")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(u32::MAX as u64) as u32)
            .or_else(|| profile.as_ref().and_then(|p| p.max_steps))
            .or_else(|| member_profile.as_ref().and_then(|(_, m)| m.max_steps))
            .unwrap_or(DEFAULT_CHILD_MAX_STEPS)
            .clamp(1, MAX_CHILD_MAX_STEPS);

        // ── T05: failure policy + budget (persisted on the reservation) ──
        //
        // The policy decides what a terminal child failure does to the parent:
        // Isolate (default) keeps the parent running, FailFast fails it and
        // cancels siblings, RequireAll aggregates a batch failure, Retry
        // re-queues transient provider failures up to `max_retries`. Budgets
        // are capped by the daemon config so a parent can never buy a child
        // bigger than the machine-wide ceiling.
        let failure_policy = input
            .get("failure_policy")
            .and_then(|v| v.as_str())
            .map(FailurePolicy::parse)
            .unwrap_or(self.subagents.config().failure_policy);
        let max_retries = input
            .get("max_retries")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(MAX_SUBAGENT_RETRIES as u64) as u32)
            .unwrap_or(0);
        let child_max_tokens = input
            .get("max_tokens")
            .or_else(|| input.get("max_budget_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v.min(self.subagents.config().max_tokens_per_child))
            .unwrap_or(self.subagents.config().max_tokens_per_child);
        let child_max_cost = input.get("max_cost_usd").and_then(|v| v.as_f64());

        // Persisted on the child run row and reported to every observer of this
        // spawn. A roster member wins over a parent-named persona because child
        // capability resolve loads the member's skills from this id (ADR-0016);
        // without a team it is the persona `ProductionRuntime::start_run`
        // reloads for the system prompt, tools and token budget.
        let child_profile_id = member_profile_id
            .clone()
            .or_else(|| requested_profile_id.clone());

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
        // Persist the child scope so a route restart restores it exactly
        // (migration 029). N05: bind the child to the parent's REAL project
        // identity (id + version) — never write the project path into
        // project_id. A persist failure fails closed BEFORE any child run is
        // created.
        //
        // Fixture mode (offline tests) has no durable store: the session is a
        // fake id and there is nothing to persist, so the DB steps are skipped
        // to keep the fixture path hermetic.
        let identity = self.verified_project_identity().await;
        let fixture_mode = use_fixture_flag(&input);
        let project_id_for_scope = identity
            .as_ref()
            .map(|i| i.project_id.clone())
            .or_else(|| self.gateway.project_root.clone());
        if !fixture_mode {
            if let Err(error) = crate::subagent_store::persist_subagent_scope(
                &session_id,
                &crate::subagent_store::SubagentScope {
                    project_path: self.gateway.project_root.clone(),
                    project_id: project_id_for_scope.clone(),
                    project_identity_version: identity.as_ref().map(|i| i.identity_version as i64),
                    permission_profile: Some(child_perm.clone()),
                    agent_profile_id: child_profile_id.clone(),
                    max_steps: Some(child_max_steps as i64),
                    tool_allowlist: child_allowlist.clone(),
                },
            ) {
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&error),
                );
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!("persist child scope failed: {error}"),
                        "code": "PERSISTENCE_FAILED",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        // Standard RunManager path: create_run + start_detached (no embedded Engine).
        let project_path = self.gateway.project_root.clone();

        // SubagentStart runs before the child exists, so a hook can refuse the
        // spawn while refusing is still free. Denial is honoured rather than
        // logged: this event is the only place a policy can stop a run from
        // fanning out, and a hook that says no must not be overruled by the
        // model having asked nicely.
        let subagent_hooks = crate::production_hooks::build_production_hooks_for_project(
            project_path.as_deref().map(std::path::Path::new),
        )
        .with_events(self.events.clone());
        let start_responses = subagent_hooks
            .dispatch(HookRequest {
                event: HookEvent::SubagentStart,
                run_id: self.parent_run_id.clone(),
                tool_name: Some("task".into()),
                input: serde_json::json!({
                    "prompt": prompt.clone(),
                    "name": name.clone(),
                    "agent_profile_id": child_profile_id.clone(),
                    "permission_profile": child_perm.clone(),
                    "tool_allowlist": child_allowlist.clone(),
                    "model_id": child_model.clone(),
                }),
            })
            .await;
        if let Err(reason) = HookRegistry::aggregate_allow(&start_responses) {
            // E04: the hidden session was created before the hook ran — close
            // it so a denied spawn leaves no persistent session orphan.
            let _ =
                crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&reason));
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": format!("subagent hook denied: {reason}"),
                    "code": "subagent_denied_by_hook",
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        // T05 saga phase 1 — durable reservation BEFORE the child run exists.
        // If anything after this fails, the compensation releases the slot and
        // closes the session in reverse order, idempotently. Fixture mode has
        // no durable store and skips the reservation (slot lives in-memory).
        let depth = self.subagents.depth_for_child(&self.parent_run_id).await;
        if !fixture_mode {
            let scope_snapshot = serde_json::json!({
                "project_path": self.gateway.project_root.clone(),
                "project_id": project_id_for_scope.clone(),
                "project_identity_version": identity.as_ref().map(|i| i.identity_version as i64),
                "permission_profile": child_perm.clone(),
                "agent_profile_id": child_profile_id.clone(),
                "max_steps": child_max_steps,
                "tool_allowlist": child_allowlist.clone(),
                "failure_policy": failure_policy.as_str(),
                "max_tokens": child_max_tokens,
            });
            if let Err(error) = crate::subagent_store::reserve_subagent_slot(
                &crate::subagent_store::SubagentReservation {
                    session_id: session_id.clone(),
                    parent_run_id: self.parent_run_id.clone(),
                    tree_root_run_id: self.parent_run_id.clone(),
                    depth,
                    max_tokens: Some(child_max_tokens),
                    max_cost_usd: child_max_cost,
                    failure_policy: failure_policy.as_str().to_string(),
                    max_retries,
                    scope_snapshot,
                },
            ) {
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&error),
                );
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!("subagent reservation failed: {error}"),
                        "code": "SUBAGENT_RESERVE_FAILED",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        let created = match crate::global_run_manager().create_run(
            assistant_protocol::v2::CreateRunRequest {
                // Child runs never inherit the parent conversation's selection;
                // member skills come from the member profile at child resolve.
                capability_selection: None,
                conversation_id: child_conversation_id.clone(),
                provider_id: child_provider.clone(),
                model_id: child_model.clone(),
                key_id: Some(child_key.clone()),
                // Persisted on the run row; `ProductionRuntime::start_run` reloads
                // the profile from it (system prompt, tools, token budget).
                agent_profile_id: child_profile_id.clone(),
                permission_profile: Some(child_perm.clone()),
                content: Some(prompt.clone()),
                attachments: None,
                max_steps: Some(child_max_steps),
                parent_run_id: Some(self.parent_run_id.clone()),
                project_path: project_path.clone(),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            },
        ) {
            Ok(r) => r,
            Err(e) => {
                // Saga compensation: release the reserved slot, close session.
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&e));
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
                child_profile_id.clone(),
                Some("none".into()),
                project_path.clone(),
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                // Saga compensation (reverse order, idempotent): release the
                // durable slot, fail the queued child run, close the session.
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&e));
                let _ =
                    crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&e));
                // E04: the child run was already created (queued) before the
                // reservation — settle it so a failed registration leaves no
                // queued orphan run behind.
                crate::global_run_manager().fail_run_if_active(
                    &child_run_id,
                    format!("subagent reservation failed: {e}"),
                    "SUBAGENT_REGISTER_FAILED",
                );
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

        // Apply child tool surface + parent-authored system prompt before
        // RunManager starts the engine. Both are keyed by the child run id and
        // consumed once by `ProductionRuntime::start_run`.
        crate::global_run_manager()
            .runtime
            .set_run_tool_allowlist(&child_run_id, child_allowlist.clone())
            .await;
        if let Some(directive) = child_directive.clone() {
            crate::global_run_manager()
                .runtime
                .set_run_agent_directive(&child_run_id, directive)
                .await;
        }

        if let Err(error) = self.events.append_checked(
            &self.parent_run_id,
            RunEventKind::SubagentCreated {
                sub_run_id: child_run_id.clone(),
                agent_profile_id: child_profile_id.clone(),
                task: prompt.clone(),
            },
        ) {
            // Saga compensation: cancel the child, release both slot ledgers
            // (in-memory via a terminal status, durable via release), close.
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest {
                    run_id: child_run_id.clone(),
                })
                .await;
            let _ = self
                .subagents
                .update_status(&session_id, SubAgentStatus::Failed(error.clone()))
                .await;
            let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&error));
            let _ =
                crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&error));
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": format!("subagent creation event could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

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
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(child_run_id.clone()),
                conversation_id: Some(child_conversation_id.clone()),
                provider_id: Some(child_provider.clone()),
                model_id: Some(child_model.clone()),
                key_id: Some(child_key.clone()),
                content: Some(prompt.clone()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some(child_perm.clone()),
                max_steps: Some(child_max_steps),
                project_path: project_path.clone(),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            },
        );
        if let Err(e) = start_result {
            // The child never started, so nothing will consume its directive.
            let _ = crate::global_run_manager()
                .runtime
                .take_run_agent_directive(&child_run_id)
                .await;
            // Saga compensation (T05): a failed start must not leave a
            // registered child holding its in-memory slot nor a durable
            // reservation with no live run behind it.
            let _ = self
                .subagents
                .update_status(&session_id, SubAgentStatus::Failed(e.clone()))
                .await;
            crate::global_run_manager().fail_run_if_active(
                &child_run_id,
                format!("start child run failed: {e}"),
                "SUBAGENT_START_FAILED",
            );
            let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&e));
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

        // Background watcher: when RunManager marks the run terminal, update
        // session/task, settle usage against the durable budget, apply the
        // failure policy, and release the slot exactly once.
        spawn_subagent_watcher(
            self.events.clone(),
            self.subagents.clone(),
            self.task_outputs.clone(),
            self.parent_run_id.clone(),
            session_id.clone(),
            session_id.clone(),
            child_conversation_id.clone(),
            child.id.clone(),
            child_run_id.clone(),
            project_path.clone(),
            self.subagents.config().child_timeout_ms.max(1),
            self.parent_run_id.clone(),
            self.subagents.config().max_tokens_per_tree,
            child_max_tokens,
            binding,
            child_perm.clone(),
            child_allowlist.clone(),
            child_profile_id.clone(),
            child_directive.clone(),
            child_max_steps,
            failure_policy,
            max_retries,
        );

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
                "agent_profile_id": child_profile_id,
                "tool_allowlist": child_allowlist,
                "max_steps": child_max_steps,
                "system_prompt_authored": child_directive.is_some(),
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
            // If the stored policy has no bindings (e.g. because the policy was
            // saved before a provider reset or after a user cancellation), fall
            // back to the main-session credential rather than hard-failing.
            if policy.bindings.is_empty() {
                if !default_binding.key_id.trim().is_empty() {
                    // Repair the stale policy so next subagent benefits too.
                    let _ = crate::subagent_store::upsert_route_policy(
                        &self.conversation_id,
                        "default",
                        std::slice::from_ref(&default_binding),
                    );
                    let mut map = HashMap::new();
                    for (call_id, _, _) in tasks {
                        map.insert(call_id.clone(), default_binding.clone());
                    }
                    return Ok(map);
                }
                // No usable default either — drop the broken policy row so the
                // assignment interaction is shown to the user on the next call.
                let _ = crate::subagent_store::delete_route_policy(&self.conversation_id);
                // Fall through to assignment interaction below.
            } else {
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
        // A caller can explicitly confirm a usable main-session route when
        // an older parent run did not persist default_binding.key_id. Use that
        // confirmed route before falling back to the legacy default binding.
        if !assignments.is_empty() {
            for a in &assignments {
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
        } else if mode == "default" {
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
            for (pi, (call_id, _, _)) in tasks.iter().enumerate() {
                map.insert(call_id.clone(), bindings[pi % bindings.len()].clone());
            }
        }

        // Partial custom assignments may use the confirmed pool for remaining tasks.
        if !assignments.is_empty() && map.len() < tasks.len() {
            let pool: Vec<crate::subagent_store::RouteBinding> = response
                .get("pool")
                .or_else(|| response.get("bindings"))
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let mut pi = 0usize;
            #[allow(clippy::explicit_counter_loop)]
            // pi counts only processed items (continue skips)
            for (call_id, _, _) in tasks {
                if map.contains_key(call_id) || pool.is_empty() {
                    continue;
                }
                let b = pool[pi % pool.len()].clone();
                crate::production::validate_route_binding(&b)?;
                map.insert(call_id.clone(), b);
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

/// Launch the background watcher for one child run (T05).
///
/// It polls the child to a terminal status, settles provider usage against the
/// *durable* budget incrementally (cancelling the child the moment the budget
/// is exceeded), consumes the persisted failure policy, releases the slot
/// exactly once, and re-enters itself when `Retry` re-queues the child.
#[allow(clippy::too_many_arguments)] // watcher re-entry needs the full child scope
fn spawn_subagent_watcher(
    events: EventSequencer,
    subagents: Arc<SubAgentManager>,
    task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: String,
    session_id: String,
    task_id: String,
    child_conversation_id: String,
    mem_task_id: String,
    child_run_id: String,
    project_path: Option<String>,
    child_timeout_ms: u64,
    tree_root_for_budget: String,
    max_tokens_per_tree: u64,
    child_max_tokens: u64,
    binding: crate::subagent_store::RouteBinding,
    child_perm: String,
    child_allowlist: Vec<String>,
    child_profile_id: Option<String>,
    child_directive: Option<String>,
    child_max_steps: u32,
    failure_policy: FailurePolicy,
    max_retries: u32,
) {
    tokio::spawn(async move {
        watch_subagent_run(
            events,
            subagents,
            task_outputs,
            parent_run_id,
            session_id,
            task_id,
            child_conversation_id,
            mem_task_id,
            project_path,
            child_timeout_ms,
            tree_root_for_budget,
            max_tokens_per_tree,
            child_max_tokens,
            binding,
            child_perm,
            child_allowlist,
            child_profile_id,
            child_directive,
            child_max_steps,
            failure_policy,
            max_retries,
            child_run_id,
            0,
            None,
        )
        .await;
    });
}

/// Re-enter the watcher for a Retry re-queue with a fresh child run id.
/// Same `async move` pattern as [`spawn_subagent_watcher`] so the spawned
/// future stays `Send` (EventSequencer is Send but not Sync, so a direct
/// `tokio::spawn(watch_subagent_run(...))` from inside an async fn is not).
#[allow(clippy::too_many_arguments)] // watcher re-entry needs the full child scope
fn spawn_retry_watcher(
    events: EventSequencer,
    subagents: Arc<SubAgentManager>,
    task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: String,
    session_id: String,
    task_id: String,
    child_conversation_id: String,
    mem_task_id: String,
    project_path: Option<String>,
    tree_root_for_budget: String,
    child_max_tokens: u64,
    binding: crate::subagent_store::RouteBinding,
    child_perm: String,
    child_allowlist: Vec<String>,
    child_profile_id: Option<String>,
    child_directive: Option<String>,
    child_max_steps: u32,
    failure_policy: FailurePolicy,
    max_retries: u32,
    new_run_id: String,
) {
    tokio::spawn(async move {
        let timeout_ms = subagents.config().child_timeout_ms.max(1);
        let tree_cap = subagents.config().max_tokens_per_tree;
        watch_subagent_run(
            events,
            subagents,
            task_outputs,
            parent_run_id,
            session_id,
            task_id,
            child_conversation_id,
            mem_task_id,
            project_path,
            timeout_ms,
            tree_root_for_budget,
            tree_cap,
            child_max_tokens,
            binding,
            child_perm,
            child_allowlist,
            child_profile_id,
            child_directive,
            child_max_steps,
            failure_policy,
            max_retries,
            new_run_id,
            0,
            None,
        )
        .await;
    });
}

#[allow(clippy::too_many_arguments)] // watcher re-entry needs the full child scope
async fn watch_subagent_run(
    events: EventSequencer,
    subagents: Arc<SubAgentManager>,
    task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: String,
    session_id: String,
    task_id: String,
    child_conversation_id: String,
    mem_task_id: String,
    project_path: Option<String>,
    child_timeout_ms: u64,
    tree_root_for_budget: String,
    max_tokens_per_tree: u64,
    // Child max tokens is enforced by the durable `subagent_session` budget;
    // the in-memory ledger is kept for the tool-call hook only.
    _child_max_tokens: u64,
    binding: crate::subagent_store::RouteBinding,
    child_perm: String,
    child_allowlist: Vec<String>,
    child_profile_id: Option<String>,
    child_directive: Option<String>,
    child_max_steps: u32,
    failure_policy: FailurePolicy,
    max_retries: u32,
    child_run_id: String,
    mut cursor: u64,
    mut budget_exceeded: Option<String>,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(child_timeout_ms.max(1));
    loop {
        if tokio::time::Instant::now() >= deadline {
            // Timeout → unified cancel tree for the child, then fail it.
            crate::global_run_manager()
                .runtime
                .cancel_run(&child_run_id)
                .await;
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest {
                    run_id: child_run_id.clone(),
                })
                .await;
            let _ = crate::global_run_manager()
                .runtime
                .take_run_agent_directive(&child_run_id)
                .await;
            let message = format!("subagent timed out after {}ms", child_timeout_ms.max(1));
            let _ = crate::subagent_store::settle_subagent_usage(&session_id, 0, None);
            child_failed_terminal(
                events.clone(),
                &subagents,
                &task_outputs,
                &parent_run_id,
                &session_id,
                &task_id,
                &mem_task_id,
                &child_run_id,
                &project_path,
                &message,
                failure_policy,
                max_retries,
                &binding,
                &child_conversation_id,
                &child_perm,
                &child_allowlist,
                &child_profile_id,
                &child_directive,
                child_max_steps,
            )
            .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let Some(run) = crate::global_run_manager().get_run(&child_run_id) else {
            continue;
        };
        let status = run.status.as_str().to_string();

        // Incremental usage settle: replay events after the cursor, feed every
        // new UsageUpdated delta into the durable budget, and cancel the child
        // the moment the budget is exceeded — not only at terminal.
        match events.replay_after_checked(&child_run_id, cursor) {
            Ok(replayed) => {
                for e in &replayed {
                    cursor = cursor.max(e.effective_run_sequence());
                }
                let delta = usage_delta_from_events(&replayed);
                if delta > 0 && budget_exceeded.is_none() {
                    // Keep the in-memory ledger aligned for tool-call hooks.
                    let _ = subagents
                        .settle_tokens(&child_run_id, &tree_root_for_budget, delta)
                        .await;
                    match crate::subagent_store::settle_subagent_usage(&session_id, delta, None) {
                        Err(e) => {
                            budget_exceeded = Some(e);
                        }
                        Ok(()) => {
                            if let Ok(tree_used) = crate::subagent_store::subagent_tree_tokens_used(
                                &tree_root_for_budget,
                            ) {
                                if tree_used > max_tokens_per_tree {
                                    budget_exceeded = Some(format!(
                                        "subagent tree token budget exceeded ({tree_used}/{max_tokens_per_tree})"
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Err(error) => {
                let message = format!("child event replay failed: {error}");
                let _ = subagents
                    .update_status(&mem_task_id, SubAgentStatus::Failed(message.clone()))
                    .await;
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&message));
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&message),
                );
                let _ = events.append_checked(
                    &parent_run_id,
                    RunEventKind::SubagentFailed {
                        sub_run_id: child_run_id.clone(),
                        error: message.clone(),
                    },
                );
                task_outputs.lock().await.insert(
                    task_id.clone(),
                    TaskRecord {
                        run_id: child_run_id.clone(),
                        status: "failed".into(),
                        output: Some(message),
                    },
                );
                return;
            }
        }

        if budget_exceeded.is_some() {
            // Budget reached → the child cannot be Completed. Cancel any live
            // run (idempotent when already terminal) and fail it.
            crate::global_run_manager()
                .runtime
                .cancel_run(&child_run_id)
                .await;
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest {
                    run_id: child_run_id.clone(),
                })
                .await;
            let message = budget_exceeded.clone().unwrap_or_default();
            child_failed_terminal(
                events.clone(),
                &subagents,
                &task_outputs,
                &parent_run_id,
                &session_id,
                &task_id,
                &mem_task_id,
                &child_run_id,
                &project_path,
                &message,
                failure_policy,
                0,
                &binding,
                &child_conversation_id,
                &child_perm,
                &child_allowlist,
                &child_profile_id,
                &child_directive,
                child_max_steps,
            )
            .await;
            return;
        }

        if !run.status.is_terminal() {
            continue;
        }

        // ── Terminal handling ──
        let child_events = match events.replay_after_checked(&child_run_id, cursor) {
            Ok(events) => events,
            Err(error) => {
                let message = format!("child event replay failed: {error}");
                let _ = subagents
                    .update_status(&mem_task_id, SubAgentStatus::Failed(message.clone()))
                    .await;
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&message));
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&message),
                );
                let _ = events.append_checked(
                    &parent_run_id,
                    RunEventKind::SubagentFailed {
                        sub_run_id: child_run_id.clone(),
                        error: message.clone(),
                    },
                );
                task_outputs.lock().await.insert(
                    task_id.clone(),
                    TaskRecord {
                        run_id: child_run_id.clone(),
                        status: "failed".into(),
                        output: Some(message),
                    },
                );
                return;
            }
        };
        let final_delta = usage_delta_from_events(&child_events);
        if final_delta > 0 && budget_exceeded.is_none() {
            let _ = subagents
                .settle_tokens(&child_run_id, &tree_root_for_budget, final_delta)
                .await;
            if let Err(e) =
                crate::subagent_store::settle_subagent_usage(&session_id, final_delta, None)
            {
                budget_exceeded = Some(e);
            }
        }
        if let Some(message) = budget_exceeded {
            let _ = subagents
                .update_status(&mem_task_id, SubAgentStatus::Failed(message.clone()))
                .await;
            let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&message));
            let _ = crate::subagent_store::close_subagent_session(
                &session_id,
                "failed",
                Some(&message),
            );
            let _ = events.append_checked(
                &parent_run_id,
                RunEventKind::SubagentFailed {
                    sub_run_id: child_run_id.clone(),
                    error: message.clone(),
                },
            );
            task_outputs.lock().await.insert(
                task_id.clone(),
                TaskRecord {
                    run_id: child_run_id.clone(),
                    status: "failed".into(),
                    output: Some(message.clone()),
                },
            );
            fail_parent_and_cancel_siblings(
                &subagents,
                &parent_run_id,
                &format!("subagent budget exceeded: {message}"),
            )
            .await;
            return;
        }

        let text = child_events
            .iter()
            .filter_map(|e| match &e.payload {
                RunEventKind::TextDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<String>();
        if status == "completed" {
            child_completed_terminal(
                events.clone(),
                &subagents,
                &task_outputs,
                &parent_run_id,
                &session_id,
                &task_id,
                &mem_task_id,
                &child_run_id,
                &project_path,
                &text,
                failure_policy,
            )
            .await;
            return;
        }

        let err_msg = run.error_code.clone().unwrap_or_else(|| status.clone());
        let message = if text.trim().is_empty() {
            err_msg
        } else {
            format!("{err_msg}: {text}")
        };
        child_failed_terminal(
            events.clone(),
            &subagents,
            &task_outputs,
            &parent_run_id,
            &session_id,
            &task_id,
            &mem_task_id,
            &child_run_id,
            &project_path,
            &message,
            failure_policy,
            max_retries,
            &binding,
            &child_conversation_id,
            &child_perm,
            &child_allowlist,
            &child_profile_id,
            &child_directive,
            child_max_steps,
        )
        .await;
        return;
    }
}

/// Sum of input+output tokens in a batch of replayed events (provider deltas).
fn usage_delta_from_events(events: &[assistant_protocol::v2::RunEventV2]) -> u64 {
    events
        .iter()
        .filter_map(|e| match &e.payload {
            RunEventKind::UsageUpdated {
                input_tokens,
                output_tokens,
                ..
            } => Some((*input_tokens).saturating_add(*output_tokens)),
            _ => None,
        })
        .fold(0u64, u64::saturating_add)
}

/// Settle a child that reached `completed`: persist usage, release the slot
/// exactly once, emit the parent event, and let RequireAll fail the parent
/// when the batch aggregate failed.
#[allow(clippy::too_many_arguments)] // terminal settlement needs full context
async fn child_completed_terminal(
    events: EventSequencer,
    subagents: &Arc<SubAgentManager>,
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: &str,
    session_id: &str,
    task_id: &str,
    mem_task_id: &str,
    child_run_id: &str,
    project_path: &Option<String>,
    text: &str,
    failure_policy: FailurePolicy,
) {
    let _ = subagents
        .update_status(mem_task_id, SubAgentStatus::Completed)
        .await;
    let _ = crate::subagent_store::release_subagent_slot(session_id, None);
    let _ = crate::subagent_store::close_subagent_session(session_id, "completed", None);
    let mut final_status = "completed".to_string();
    let mut task_output = text.to_string();
    if let Err(error) = events.append_checked(
        parent_run_id,
        RunEventKind::SubagentCompleted {
            sub_run_id: child_run_id.to_string(),
            result: text.to_string(),
        },
    ) {
        final_status = "failed".into();
        task_output = format!("PERSISTENCE_FAILED: subagent completion event: {error}");
        let _ = subagents
            .update_status(mem_task_id, SubAgentStatus::Failed(task_output.clone()))
            .await;
        let _ =
            crate::subagent_store::close_subagent_session(session_id, "failed", Some(&task_output));
    }
    // RequireAll: the batch fails when every sibling is settled and any failed.
    if failure_policy == FailurePolicy::RequireAll {
        let (all_terminal, any_failed) = sibling_settled_state(task_outputs, task_id).await;
        if all_terminal && any_failed {
            fail_parent_and_cancel_siblings(
                subagents,
                parent_run_id,
                "subagent batch failed under require_all policy",
            )
            .await;
        }
    }
    task_outputs.lock().await.insert(
        task_id.to_string(),
        TaskRecord {
            run_id: child_run_id.to_string(),
            status: final_status,
            output: if task_output.is_empty() {
                None
            } else {
                Some(task_output)
            },
        },
    );
    fire_subagent_stop(project_path, parent_run_id, child_run_id, "completed", text).await;
}

/// Handle a child that failed (or was cancelled / interrupted / timed out /
/// budget-exceeded). Consumes the failure policy: Isolate keeps the parent
/// running, FailFast/RequireAll fail it, Retry re-queues transient failures.
/// Returns true when a retry was launched (the caller must not release).
#[allow(clippy::too_many_arguments)] // terminal settlement needs full context
async fn child_failed_terminal(
    events: EventSequencer,
    subagents: &Arc<SubAgentManager>,
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: &str,
    session_id: &str,
    task_id: &str,
    mem_task_id: &str,
    child_run_id: &str,
    project_path: &Option<String>,
    message: &str,
    failure_policy: FailurePolicy,
    max_retries: u32,
    binding: &crate::subagent_store::RouteBinding,
    child_conversation_id: &str,
    child_perm: &str,
    child_allowlist: &[String],
    child_profile_id: &Option<String>,
    child_directive: &Option<String>,
    child_max_steps: u32,
) -> bool {
    let _ = subagents
        .update_status(mem_task_id, SubAgentStatus::Failed(message.to_string()))
        .await;
    let retry_state = crate::subagent_store::get_subagent_session(session_id)
        .ok()
        .flatten();
    let retries_remaining = retry_state
        .as_ref()
        .map(|s| s.max_retries.saturating_sub(s.retry_count))
        .unwrap_or(0);
    let retryable = crate::subagent_store::is_failover_eligible_error(message);
    let effect = if retryable {
        failure_policy.on_child_failed(false, retries_remaining)
    } else {
        // Budget / permission / max-steps / deadlock errors are not retried:
        // re-queueing would just burn more budget on the same outcome.
        failure_policy.on_child_failed(false, 0)
    };

    if effect == ChildFailureEffect::Retry {
        // Keep the reservation and slot; re-queue a fresh run on the same
        // hidden conversation. `requeue_child_run` bumps the retry counter.
        let retry = retries_remaining.saturating_sub(1);
        match requeue_child_run(
            session_id,
            child_conversation_id,
            parent_run_id,
            binding,
            &retry,
            message,
            child_perm,
            child_allowlist,
            child_profile_id,
            child_directive,
            child_max_steps,
        )
        .await
        {
            Ok(new_run_id) => {
                let _ = subagents.update_run_id(mem_task_id, &new_run_id).await;
                let _ = crate::subagent_store::update_subagent_session_status(
                    session_id, "running", None,
                );
                if let Some(rec) = task_outputs.lock().await.get_mut(task_id) {
                    rec.run_id = new_run_id.clone();
                    rec.status = "running".into();
                    rec.output = None;
                }
                let _ = events.append_checked(
                    parent_run_id,
                    RunEventKind::Progress {
                        message: format!("subagent retry #{retry} after: {message}"),
                        percentage: None,
                    },
                );
                spawn_retry_watcher(
                    events.clone(),
                    subagents.clone(),
                    task_outputs.clone(),
                    parent_run_id.to_string(),
                    session_id.to_string(),
                    task_id.to_string(),
                    child_conversation_id.to_string(),
                    mem_task_id.to_string(),
                    project_path.clone(),
                    parent_run_id.to_string(),
                    subagents.config().max_tokens_per_child,
                    binding.clone(),
                    child_perm.to_string(),
                    child_allowlist.to_vec(),
                    child_profile_id.clone(),
                    child_directive.clone(),
                    child_max_steps,
                    failure_policy,
                    max_retries,
                    new_run_id,
                );
                return true;
            }
            Err(requeue_error) => {
                // Fall through to failure handling with the enriched message.
                let enriched = format!("{message}; requeue failed: {requeue_error}");
                let _ = subagents
                    .update_status(mem_task_id, SubAgentStatus::Failed(enriched.clone()))
                    .await;
                finalize_child_failure(
                    events,
                    subagents,
                    task_outputs,
                    parent_run_id,
                    session_id,
                    task_id,
                    child_run_id,
                    project_path,
                    &enriched,
                    failure_policy,
                )
                .await;
                return false;
            }
        }
    }

    finalize_child_failure(
        events,
        subagents,
        task_outputs,
        parent_run_id,
        session_id,
        task_id,
        child_run_id,
        project_path,
        message,
        failure_policy,
    )
    .await;
    false
}

/// Shared tail of child failure: release slot + close session + emit parent
/// event + apply FailFast/RequireAll parent action + update task record.
#[allow(clippy::too_many_arguments)] // terminal settlement needs full context
async fn finalize_child_failure(
    events: EventSequencer,
    subagents: &Arc<SubAgentManager>,
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: &str,
    session_id: &str,
    task_id: &str,
    child_run_id: &str,
    project_path: &Option<String>,
    message: &str,
    failure_policy: FailurePolicy,
) {
    let _ = crate::subagent_store::release_subagent_slot(session_id, Some(message));
    let _ = crate::subagent_store::close_subagent_session(session_id, "failed", Some(message));
    let task_output = message.to_string();
    let _ = events.append_checked(
        parent_run_id,
        RunEventKind::SubagentFailed {
            sub_run_id: child_run_id.to_string(),
            error: message.to_string(),
        },
    );
    // FailFast fails the parent immediately; RequireAll waits until every
    // sibling has settled, then fails the parent on the aggregate failure.
    let (all_terminal, _any_failed) = sibling_settled_state(task_outputs, task_id).await;
    if matches!(
        failure_policy.on_child_failed(all_terminal, 0),
        ChildFailureEffect::FailParent
    ) {
        fail_parent_and_cancel_siblings(
            subagents,
            parent_run_id,
            &format!("subagent failed under {:?}: {message}", failure_policy),
        )
        .await;
    }
    task_outputs.lock().await.insert(
        task_id.to_string(),
        TaskRecord {
            run_id: child_run_id.to_string(),
            status: "failed".into(),
            output: Some(task_output),
        },
    );
    fire_subagent_stop(project_path, parent_run_id, child_run_id, "failed", message).await;
}

/// Create a fresh child run for a Retry re-queue and bump the retry counter.
#[allow(clippy::too_many_arguments)] // re-queue needs the exact child scope
async fn requeue_child_run(
    session_id: &str,
    child_conversation_id: &str,
    parent_run_id: &str,
    binding: &crate::subagent_store::RouteBinding,
    retry_number: &u32,
    reason: &str,
    child_perm: &str,
    child_allowlist: &[String],
    child_profile_id: &Option<String>,
    child_directive: &Option<String>,
    child_max_steps: u32,
) -> Result<String, String> {
    let session = crate::subagent_store::get_subagent_session(session_id)?
        .ok_or_else(|| format!("subagent session not found: {session_id}"))?;
    let project_path = session.project_path.clone();
    let prompt = if session.task.trim().is_empty() {
        format!("Retry (attempt {retry_number}) after: {reason}")
    } else {
        session.task.to_string()
    };
    let created =
        crate::global_run_manager().create_run(assistant_protocol::v2::CreateRunRequest {
            capability_selection: None,
            conversation_id: child_conversation_id.to_string(),
            provider_id: binding.provider_id.clone(),
            model_id: binding.model_id.clone(),
            key_id: Some(binding.key_id.clone()),
            agent_profile_id: child_profile_id.clone(),
            permission_profile: Some(child_perm.to_string()),
            content: Some(prompt.clone()),
            attachments: None,
            max_steps: Some(child_max_steps),
            parent_run_id: Some(parent_run_id.to_string()),
            project_path,
            idempotency_key: None,
            effort: None,
            runtime_id: Some("native".into()),
        })?;
    crate::global_run_manager()
        .runtime
        .set_run_tool_allowlist(&created.id, child_allowlist.to_vec())
        .await;
    if let Some(directive) = child_directive.as_deref().filter(|s| !s.is_empty()) {
        crate::global_run_manager()
            .runtime
            .set_run_agent_directive(&created.id, directive.to_string())
            .await;
    }
    crate::run_manager::RunManager::start_detached_global(
        assistant_protocol::v2::StartRunRequest {
            agent_profile_id: None,
            capability_selection: None,
            run_id: Some(created.id.clone()),
            conversation_id: Some(child_conversation_id.to_string()),
            provider_id: Some(binding.provider_id.clone()),
            model_id: Some(binding.model_id.clone()),
            key_id: Some(binding.key_id.clone()),
            content: Some(prompt),
            attachments: None,
            trigger_message_id: None,
            permission_profile: Some(child_perm.to_string()),
            max_steps: Some(child_max_steps),
            project_path: session.project_path.clone(),
            idempotency_key: None,
            effort: None,
            runtime_id: Some("native".into()),
        },
    )?;
    let _ = crate::subagent_store::bump_subagent_retry(session_id);
    Ok(created.id)
}

/// (all_other_siblings_terminal, any_other_sibling_failed) for the parent's
/// task ledger. Used by RequireAll to detect the aggregate outcome.
async fn sibling_settled_state(
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    exclude_task_id: &str,
) -> (bool, bool) {
    let map = task_outputs.lock().await;
    let mut any_running = false;
    let mut any_failed = false;
    for (tid, rec) in map.iter() {
        if tid == exclude_task_id {
            continue;
        }
        if rec.status == "running" {
            any_running = true;
        }
        if rec.status == "failed" {
            any_failed = true;
        }
    }
    (!any_running, any_failed)
}

/// FailFast / RequireAll aggregate action: cancel every sibling child's engine
/// and metadata, then fail the parent run (idempotent).
async fn fail_parent_and_cancel_siblings(
    subagents: &Arc<SubAgentManager>,
    parent_run_id: &str,
    reason: &str,
) {
    for sibling in subagents.get_children(parent_run_id).await {
        let _ = subagents
            .update_status(&sibling.id, SubAgentStatus::Cancelled)
            .await;
        crate::global_run_manager()
            .runtime
            .cancel_run(&sibling.run_id)
            .await;
    }
    let _ = crate::global_run_manager()
        .runtime
        .cancel_run(parent_run_id)
        .await;
    crate::global_run_manager().fail_run_if_active(
        parent_run_id,
        reason.to_string(),
        "SUBAGENT_FAILFAST",
    );
}

/// SubagentStop hook fired for every terminal outcome (matches prior behavior).
async fn fire_subagent_stop(
    project_path: &Option<String>,
    parent_run_id: &str,
    child_run_id: &str,
    status: &str,
    output: &str,
) {
    let _ = crate::production_hooks::build_production_hooks_for_project(
        project_path.as_deref().map(std::path::Path::new),
    )
    .dispatch(HookRequest {
        event: HookEvent::SubagentStop,
        run_id: parent_run_id.to_string(),
        tool_name: Some("task".into()),
        input: serde_json::json!({
            "sub_run_id": child_run_id,
            "status": status,
            "output": output,
        }),
    })
    .await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::SubAgentConfig;

    #[derive(Default)]
    struct CapturedEvents(std::sync::Mutex<Vec<assistant_protocol::v2::RunEventV2>>);

    impl agent_core::EventPersistence for CapturedEvents {
        fn append(&self, event: &assistant_protocol::v2::RunEventV2) -> Result<(), String> {
            self.0.lock().unwrap().push(event.clone());
            Ok(())
        }

        fn replay_after(
            &self,
            run_id: &str,
            after_sequence: u64,
        ) -> Result<Vec<assistant_protocol::v2::RunEventV2>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|event| {
                    event.run_id == run_id && event.effective_run_sequence() > after_sequence
                })
                .cloned()
                .collect())
        }

        fn last_sequence(&self, run_id: &str) -> Result<u64, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.run_id == run_id)
                .map(|event| event.effective_run_sequence())
                .max()
                .unwrap_or(0))
        }
    }

    #[tokio::test]
    async fn settled_tool_drops_late_progress() {
        let captured = Arc::new(CapturedEvents::default());
        let sink = DaemonToolProgressSink::new(EventSequencer::with_persistence(captured.clone()));
        let update = ToolProgressUpdate {
            run_id: "progress-run".into(),
            tool_call_id: "progress-call".into(),
            tool_name: "run_terminal".into(),
            stream: "stdout".into(),
            text: "before settlement".into(),
            final_update: true,
            turn_id: Some("turn-1".into()),
            message_id: Some("message-1".into()),
            progress_sequence: 0,
        };
        sink.publish(update.clone()).await;
        sink.mark_tool_call_settled(&update.tool_call_id).await;
        sink.publish(update).await;

        let events = captured.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].payload,
            RunEventKind::ToolOutputDelta { .. }
        ));
    }

    #[tokio::test]
    async fn progress_flushes_after_batch_window_without_next_update() {
        let captured = Arc::new(CapturedEvents::default());
        let sink = DaemonToolProgressSink::new(EventSequencer::with_persistence(captured.clone()));
        sink.publish(ToolProgressUpdate {
            run_id: "progress-timer-run".into(),
            tool_call_id: "progress-timer-call".into(),
            tool_name: "run_terminal".into(),
            stream: "stdout".into(),
            text: "idle batch".into(),
            final_update: false,
            turn_id: Some("turn-1".into()),
            message_id: Some("message-1".into()),
            progress_sequence: 0,
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let events = captured.0.lock().unwrap();
        assert!(events.iter().any(|event| matches!(
            event.payload,
            RunEventKind::ToolOutputDelta { ref text, .. } if text == "idle batch"
        )));
    }

    #[test]
    fn model_visible_tool_limit_fails_closed_without_truncation() {
        assert!(validate_tool_limit(MAX_MODEL_VISIBLE_TOOLS).is_ok());
        let error = validate_tool_limit(MAX_MODEL_VISIBLE_TOOLS + 1).unwrap_err();
        assert!(error.contains("tool_plan_too_large"));
    }
    use agent_core::{HookDecision, HookHandler, HookOutcome, HookResponse, PermissionVerdict};

    /// A hook that returns one fixed outcome, so the permission gate can be
    /// exercised without spawning processes.
    struct FixedHook(fn() -> HookOutcome);

    #[async_trait::async_trait]
    impl HookHandler for FixedHook {
        async fn handle(&self, _request: HookRequest) -> HookResponse {
            (self.0)().into_response()
        }

        async fn handle_outcome(&self, _request: HookRequest) -> HookOutcome {
            (self.0)()
        }
    }

    fn allowing() -> HookOutcome {
        HookOutcome::Permission(PermissionVerdict::Allow {
            reason: "hook approved".into(),
        })
    }

    fn denying() -> HookOutcome {
        HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny {
                reason: "hook denied".into(),
            },
        })
    }

    fn failing() -> HookOutcome {
        HookOutcome::Failed {
            reason: "hook crashed".into(),
        }
    }

    fn asking() -> HookOutcome {
        HookOutcome::Permission(PermissionVerdict::Ask {
            reason: "human please".into(),
        })
    }

    async fn gate_through_registry(
        outcomes: &[fn() -> HookOutcome],
        profile: PermissionProfile,
        auto_approve_allowed: bool,
    ) -> HookPermissionGate {
        let mut registry = HookRegistry::new();
        registry.enable_security_fail_closed();
        for outcome in outcomes {
            registry.register(HookEvent::PermissionRequest, Box::new(FixedHook(*outcome)));
        }
        let dispatched = registry
            .dispatch_outcomes(HookRequest {
                event: HookEvent::PermissionRequest,
                run_id: "run-1".into(),
                tool_name: Some("write_file".into()),
                input: serde_json::json!({ "path": "a.txt" }),
            })
            .await;
        hook_permission_gate(
            HookRegistry::aggregate_permission(&dispatched),
            profile,
            auto_approve_allowed,
        )
    }

    #[tokio::test]
    async fn hook_allow_skips_the_prompt() {
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::ConfirmEach, true).await,
            HookPermissionGate::AutoApprove("hook approved".into())
        );
    }

    /// The core fail-closed property, checked in both dispatch orders so the
    /// answer cannot depend on which hook happens to run first.
    #[tokio::test]
    async fn deny_wins_over_allow() {
        for order in [
            [allowing as fn() -> HookOutcome, denying],
            [denying, allowing],
        ] {
            assert_eq!(
                gate_through_registry(&order, PermissionProfile::ConfirmEach, true).await,
                HookPermissionGate::Deny("hook denied".into()),
                "a deny must survive any ordering"
            );
        }
    }

    /// A hook that could not run is treated as a deny on this security event,
    /// even next to a hook that approved.
    #[tokio::test]
    async fn hook_failure_is_fail_closed() {
        let gate =
            gate_through_registry(&[allowing, failing], PermissionProfile::ConfirmEach, true).await;
        match gate {
            HookPermissionGate::Deny(reason) => assert!(reason.contains("hook crashed")),
            other => panic!("a failed security hook must deny, got {other:?}"),
        }
    }

    /// The ceiling: a readonly session cannot be talked into skipping its
    /// confirmation by a hook.
    #[tokio::test]
    async fn readonly_profile_ignores_hook_allow() {
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::ReadOnly, true).await,
            HookPermissionGate::Prompt
        );
    }

    /// The other half of the ceiling: when the policy refused rather than
    /// asked, the caller withholds `auto_approve_allowed` and the hook's allow
    /// buys nothing.
    #[tokio::test]
    async fn policy_refusal_ignores_hook_allow() {
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::ConfirmEach, false).await,
            HookPermissionGate::Prompt
        );
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::Autonomous, false).await,
            HookPermissionGate::Prompt
        );
    }

    /// A deny is never filtered by the ceiling — hooks may always tighten.
    #[test]
    fn deny_passes_every_ceiling() {
        for profile in [
            PermissionProfile::ReadOnly,
            PermissionProfile::ConfirmEach,
            PermissionProfile::Autonomous,
        ] {
            for allowed in [false, true] {
                assert_eq!(
                    hook_permission_gate(PermissionAggregate::Deny("no".into()), profile, allowed),
                    HookPermissionGate::Deny("no".into())
                );
            }
        }
    }

    /// Silence from the hooks leaves the pre-existing prompt behaviour intact.
    #[test]
    fn no_hook_opinion_still_prompts() {
        assert_eq!(
            hook_permission_gate(
                PermissionAggregate::Prompt,
                PermissionProfile::ConfirmEach,
                true
            ),
            HookPermissionGate::Prompt
        );
    }

    /// An explicit `ask` from any hook forces the prompt back on.
    #[tokio::test]
    async fn hook_ask_overrides_hook_allow() {
        assert_eq!(
            gate_through_registry(&[allowing, asking], PermissionProfile::ConfirmEach, true).await,
            HookPermissionGate::Prompt
        );
    }

    /// TASK-007 (H03): after a tool call settles, a flood of late progress
    /// updates is rejected — zero events appended (terminal is authoritative).
    #[tokio::test]
    async fn progress_backpressure_rejects_updates_after_terminal() {
        let captured = Arc::new(CapturedEvents::default());
        let sink = DaemonToolProgressSink::new(EventSequencer::with_persistence(captured.clone()));
        sink.mark_tool_call_settled("late-call").await;
        for i in 0..100 {
            sink.publish(ToolProgressUpdate {
                run_id: "bp-run".into(),
                tool_call_id: "late-call".into(),
                tool_name: "run_terminal".into(),
                stream: "stdout".into(),
                text: format!("late line {i}"),
                final_update: false,
                turn_id: None,
                message_id: None,
                progress_sequence: 0,
            })
            .await;
        }
        let events = captured.0.lock().unwrap();
        assert!(
            events
                .iter()
                .all(|e| !matches!(&e.payload, RunEventKind::ToolOutputDelta { .. })),
            "no progress event may be appended after the call settles"
        );
    }

    /// TASK-007 (J03): the in-flight MCP registry returns to zero once a call
    /// settles — no lingering request can be mistaken for a fresh effect.
    #[test]
    fn progress_backpressure_mcp_registry_quiet_after_settle() {
        pending_mcp_calls()
            .lock()
            .unwrap()
            .insert("mcp-call-1".to_string());
        assert_eq!(pending_mcp_call_count(), 1);
        pending_mcp_calls().lock().unwrap().remove("mcp-call-1");
        assert_eq!(
            pending_mcp_call_count(),
            0,
            "registry is quiet after settle"
        );
    }

    /// T04: the ledger must distinguish a transport timeout (not cancelled →
    /// `failed`) from a user cancel (`uncertain`) so resume/audit can tell them
    /// apart.
    #[test]
    fn mcp_ledger_status_distinguishes_timeout_from_cancel() {
        assert_eq!(mcp_ledger_status(true, false), "completed");
        assert_eq!(mcp_ledger_status(false, false), "failed"); // timeout
        assert_eq!(mcp_ledger_status(true, true), "uncertain"); // cancel
        assert_eq!(mcp_ledger_status(false, true), "uncertain"); // cancel
    }

    /// T04: settling an MCP call must drain the progress sink's pending buffer
    /// and scheduled flush tasks — after a cancel no progress work is left.
    #[tokio::test]
    async fn mcp_settle_drains_progress_tasks_to_zero() {
        let captured = Arc::new(CapturedEvents::default());
        let sink = DaemonToolProgressSink::new(EventSequencer::with_persistence(captured.clone()));
        // A pending non-final update buffers into the sink and schedules a flush.
        sink.publish(ToolProgressUpdate {
            run_id: "drain-run".into(),
            tool_call_id: "drain-call".into(),
            tool_name: "mcp_call".into(),
            stream: "mcp".into(),
            text: "pending delta".into(),
            final_update: false,
            turn_id: Some("turn-1".into()),
            message_id: Some("message-1".into()),
            progress_sequence: 0,
        })
        .await;
        assert!(
            !sink.pending.lock().await.is_empty(),
            "buffered progress is pending before settle"
        );
        sink.mark_tool_call_settled("drain-call").await;
        assert!(
            sink.pending.lock().await.is_empty(),
            "pending buffer drained after settle"
        );
        assert!(
            sink.scheduled_flushes.lock().await.is_empty(),
            "no scheduled flush task remains after settle"
        );
        // A late update after settle is rejected entirely.
        sink.publish(ToolProgressUpdate {
            run_id: "drain-run".into(),
            tool_call_id: "drain-call".into(),
            tool_name: "mcp_call".into(),
            stream: "mcp".into(),
            text: "late".into(),
            final_update: false,
            turn_id: None,
            message_id: None,
            progress_sequence: 0,
        })
        .await;
        assert!(
            sink.pending.lock().await.is_empty(),
            "late update must not repopulate the buffer"
        );
    }

    /// T05 FailFast action: one child failure cancels every sibling and fails
    /// the parent run — the parent-side outcome the watcher applies.
    #[tokio::test]
    async fn fail_fast_action_cancels_siblings_and_fails_parent() {
        let _env = crate::storage::DataStore::env_test_lock();
        // Restore env on drop so `NATIVES_RUN_MANAGER_MEMORY` cannot leak into
        // later tests in the same process.
        let _env_restore = crate::storage::EnvRestore::capture();
        // Memory-only global RunManager (hermetic; the durable budget side is
        // covered by the subagent_store restart/recovery tests).
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        let rm = crate::run_manager::RunManager::new();
        let make_run = |conversation_id: &str, parent: Option<&str>| {
            rm.create_run(assistant_protocol::v2::CreateRunRequest {
                capability_selection: None,
                conversation_id: conversation_id.to_string(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("full_access".into()),
                content: Some("x".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: parent.map(|p| p.to_string()),
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap()
        };
        let probe_parent = make_run("c-ff", None);
        let parent = make_run("c-ff", None);
        let child_a = make_run("c-ff", Some(&parent.id));
        let child_b = make_run("c-ff", Some(&parent.id));

        let subagents = Arc::new(SubAgentManager::new(SubAgentConfig::default()));
        subagents.register_root_depth(&parent.id).await;
        let a = subagents
            .register(
                "session-a".into(),
                child_a.id.clone(),
                &parent.id,
                "a".into(),
                1,
                "openai".into(),
                "k".into(),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let b = subagents
            .register(
                "session-b".into(),
                child_b.id.clone(),
                &parent.id,
                "b".into(),
                1,
                "openai".into(),
                "k".into(),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
            .unwrap();

        crate::run_manager::install_global_for_test(rm);

        // Probe: a queued run must be able to fail (FailFast depends on it).
        let commit_result = crate::global_run_manager().commit_status(
            &probe_parent.id,
            assistant_protocol::v2::RunStatusV2::Failed,
            agent_core::TransitionMetadata::empty()
                .with_error_code("PROBE")
                .with_lifecycle_hint("failed"),
        );
        match &commit_result {
            Ok(run) => assert_eq!(
                run.status.as_str(),
                "failed",
                "commit_status Ok must report the failed run: {run:?}"
            ),
            Err(e) => panic!("queued → failed must be legal for FailFast: {e}"),
        }

        fail_parent_and_cancel_siblings(&subagents, &parent.id, "injected child failure").await;

        assert_eq!(
            subagents.get(&a.id).await.unwrap().status,
            SubAgentStatus::Cancelled,
            "FailFast must cancel sibling A"
        );
        assert_eq!(
            subagents.get(&b.id).await.unwrap().status,
            SubAgentStatus::Cancelled,
            "FailFast must cancel sibling B"
        );
        let parent_run = crate::global_run_manager().get_run(&parent.id).unwrap();
        assert_eq!(
            parent_run.status.as_str(),
            "failed",
            "FailFast must fail the parent run"
        );
        crate::run_manager::install_memory_global_for_test();
    }
}

/// Plan Mode as the tool runtime actually enforces it.
///
/// The gateway unit tests pin the latch and the plan shape; these pin the part
/// that can only be observed from here — which tools the model is shown, that a
/// blocked write never becomes a permission prompt, and that the latch opens
/// only for an approval that came back through the interaction hub.
#[cfg(test)]
mod plan_mode_runtime_tests {
    use super::*;
    use crate::production::ProductionRuntime;

    fn tools_for(run_id: &str, profile: &str, root: &std::path::Path) -> PermissionGatedTools {
        let rt = ProductionRuntime::new();
        let mut gateway = CapabilityGateway::new();
        gateway.set_project_root(root.to_string_lossy().to_string());
        let _ = gateway.register_builtins();
        PermissionGatedTools {
            gateway: Arc::new(gateway),
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engines.clone(),
            runtime: None,
            provider_id: "test".into(),
            key_id: None,
            parent_run_id: run_id.to_string(),
            conversation_id: format!("conv-{run_id}"),
            model_id: "test-model".into(),
            permission_profile: profile.to_string(),
            tool_allowlist: None,
            // No capability selection: these fixtures exercise Plan Mode, not
            // the ADR-0016 team/MCP gates, and `None` is the legacy surface.
            team: None,
            mcp_tool_schemas: Vec::new(),
            selected_mcp_servers: None,
        }
    }

    fn run_id(tag: &str) -> String {
        format!("plan-{tag}-{}", uuid::Uuid::new_v4())
    }

    /// macOS hands out `/var/...` temp dirs that canonicalize to `/private/var`.
    /// The gateway compares canonical paths, so the fixture must too.
    fn project_root(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().canonicalize().unwrap()
    }

    fn sample_plan() -> Value {
        serde_json::json!({
            "plan": {
                "title": "Rename the config loader",
                "summary": "Move loader.rs into config/ and update imports.",
                "steps": [
                    {"title": "Find every import", "kind": "research"},
                    {"title": "Move the file", "kind": "edit", "targets": ["src/loader.rs"]}
                ]
            }
        })
    }

    #[tokio::test]
    async fn planning_hides_writes_and_offers_only_the_exit() {
        let id = run_id("visible");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);
        let names: Vec<String> = tools
            .list_tool_schemas()
            .await
            .into_iter()
            .map(|t| t.name)
            .collect();

        assert!(names.iter().any(|n| n == "read_file"));
        assert!(names.iter().any(|n| n == "grep"));
        assert!(names.iter().any(|n| n == plan_mode::EXIT_PLAN_MODE_TOOL));
        for hidden in [
            "write_file",
            "edit_file",
            "apply_patch",
            "run_terminal",
            "task",
            "mcp_call",
            plan_mode::ENTER_PLAN_MODE_TOOL,
        ] {
            assert!(
                !names.iter().any(|n| n == hidden),
                "`{hidden}` must not be offered while planning"
            );
        }
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_normal_run_never_sees_the_exit_tool() {
        let id = run_id("normal");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, "ask", &root);
        let names: Vec<String> = tools
            .list_tool_schemas()
            .await
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert!(names.iter().any(|n| n == "write_file"));
        assert!(names.iter().any(|n| n == plan_mode::ENTER_PLAN_MODE_TOOL));
        assert!(!names.iter().any(|n| n == plan_mode::EXIT_PLAN_MODE_TOOL));
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_blocked_write_is_refused_immediately_not_prompted() {
        let id = run_id("blocked");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);
        let cancel = CancellationToken::new();

        let out = tools
            .execute_tool(
                "write_file",
                serde_json::json!({
                    "path": root.join("x.txt").to_string_lossy(),
                    "content": "nope"
                }),
                &cancel,
            )
            .await;

        assert!(out.is_error);
        assert_eq!(out.output["code"], "plan_mode_blocked");
        assert_eq!(
            tools.interactions.permission_count().await,
            0,
            "Plan Mode must not turn a blocked write into an approval card"
        );
        assert!(!root.join("x.txt").exists());
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn reads_still_work_while_planning() {
        let id = run_id("read");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let file = root.join("notes.txt");
        std::fs::write(&file, "hello plan").unwrap();
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);

        let out = tools
            .execute_tool(
                "read_file",
                serde_json::json!({"path": file.to_string_lossy()}),
                &CancellationToken::new(),
            )
            .await;

        assert!(!out.is_error, "{:?}", out.output);
        assert_eq!(out.output["content"], "hello plan");
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn model_can_enter_plan_mode_on_its_own() {
        let id = run_id("enter");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, "full_access", &root);

        let out = tools
            .execute_tool(
                plan_mode::ENTER_PLAN_MODE_TOOL,
                serde_json::json!({"reason": "wide blast radius"}),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error, "{:?}", out.output);
        assert!(plan_mode::is_active(&id));

        // The autonomous run is now genuinely gated, not just labelled.
        let blocked = tools
            .execute_tool(
                "run_terminal",
                serde_json::json!({"command": "true", "cwd": "."}),
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(blocked.output["code"], "plan_mode_blocked");
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_malformed_plan_never_reaches_the_user() {
        let id = run_id("malformed");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);

        let out = tools
            .execute_tool(
                plan_mode::EXIT_PLAN_MODE_TOOL,
                serde_json::json!({"plan": {"title": "", "steps": []}}),
                &CancellationToken::new(),
            )
            .await;

        assert!(out.is_error);
        assert_eq!(out.output["code"], "invalid_plan");
        assert_eq!(tools.interactions.permission_count().await, 0);
        assert!(plan_mode::is_active(&id));
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn exit_outside_plan_mode_is_refused() {
        let id = run_id("outside");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, "ask", &root);

        let out = tools
            .execute_tool(
                plan_mode::EXIT_PLAN_MODE_TOOL,
                sample_plan(),
                &CancellationToken::new(),
            )
            .await;
        assert!(out.is_error);
        assert_eq!(out.output["code"], "not_in_plan_mode");
        plan_mode::clear(&id);
    }

    /// Poll the run's event log for the plan approval request and answer it the
    /// way the UI would.
    async fn answer_plan_prompt(tools: &PermissionGatedTools, approved: bool) -> String {
        for _ in 0..200 {
            let pending = tools.events.replay_after(&tools.parent_run_id, 0);
            let found = pending.iter().find_map(|event| match &event.payload {
                RunEventKind::PermissionRequested {
                    tool_name,
                    permission_id,
                    ..
                } if tool_name == plan_mode::EXIT_PLAN_MODE_TOOL => Some(permission_id.clone()),
                _ => None,
            });
            if let Some(permission_id) = found {
                if let Some((_run, _tool, tx)) =
                    tools.interactions.resolve_permission(&permission_id).await
                {
                    let _ = tx.send((approved, "once".into()));
                    return permission_id;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no plan approval prompt was raised");
    }

    #[tokio::test]
    async fn approval_releases_the_latch_and_restores_the_declared_profile() {
        // T01: hermetic. The plan-mode gate persists a pending interaction
        // (interaction_store) and the side-effect ledger, so the fixture needs
        // a temp SQLite store; the process-global RunManager is installed as a
        // MEMORY manager so the verified-project check escapes (no bound run)
        // and checkpointing is in-memory. ~/.natives is never touched.
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        let env_dir = tempfile::tempdir().unwrap();
        let db = env_dir.path().join("plan.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", env_dir.path());
        crate::storage::set_test_db_override(
            Some(db.clone()),
            Some(env_dir.path().join("artifacts")),
        );
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        crate::run_manager::install_memory_global_for_test();
        crate::checkpoint::install_checkpoint_global_for_test(
            crate::checkpoint::CheckpointManager::new(),
        );
        let id = run_id("approve");
        // interaction/session_actor rows reference conversation + run, so the
        // fixture must create the FK stubs before the prompt is raised.
        let conv_id = format!("conv-{id}");
        crate::conversation_store::ensure_conversation_stub(
            &conv_id, "openai", "gpt-4o", None, None,
        )
        .unwrap();
        {
            let store =
                crate::storage::DataStore::new(&db, &env_dir.path().join("artifacts")).unwrap();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                     VALUES (?1, ?2, 'created', 'openai', 'gpt-4o')",
                    rusqlite::params![id, conv_id],
                )
                .unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = Arc::new(tools_for(&id, "full_access", &root));
        plan_mode::enter(&id, "full_access");
        // T01: the direct tool path bypasses production begin_run; register the
        // run with the installed memory checkpoint manager so capture_before
        // (write_file after approval) finds a live checkpoint.
        crate::checkpoint::global_checkpoint_manager()
            .begin_run(&id, &conv_id, &root)
            .unwrap();

        let submitting = {
            let tools = tools.clone();
            tokio::spawn(async move {
                tools
                    .execute_tool(
                        plan_mode::EXIT_PLAN_MODE_TOOL,
                        sample_plan(),
                        &CancellationToken::new(),
                    )
                    .await
            })
        };
        answer_plan_prompt(&tools, true).await;
        let out = submitting.await.unwrap();

        assert!(!out.is_error, "{:?}", out.output);
        assert_eq!(out.output["approved"], true);
        assert_eq!(out.output["permission_profile"], "full_access");
        assert!(!plan_mode::is_active(&id));
        assert_eq!(tools.effective_permission_profile(), "full_access");

        // And the writes it was denied a moment ago now go through.
        let after = tools
            .execute_tool(
                "write_file",
                serde_json::json!({
                    "path": root.join("done.txt").to_string_lossy(),
                    "content": "ok"
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!after.is_error, "{:?}", after.output);
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn rejection_keeps_the_run_planning() {
        // T01: hermetic — same env/DB + memory-global setup as the approval
        // sibling (pending interaction + ledger need a temp store).
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        let env_dir = tempfile::tempdir().unwrap();
        let db = env_dir.path().join("plan-reject.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", env_dir.path());
        crate::storage::set_test_db_override(
            Some(db.clone()),
            Some(env_dir.path().join("artifacts")),
        );
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        crate::run_manager::install_memory_global_for_test();
        crate::checkpoint::install_checkpoint_global_for_test(
            crate::checkpoint::CheckpointManager::new(),
        );
        let id = run_id("reject");
        // interaction/session_actor rows reference conversation + run, so the
        // fixture must create the FK stubs before the prompt is raised.
        let conv_id = format!("conv-{id}");
        crate::conversation_store::ensure_conversation_stub(
            &conv_id, "openai", "gpt-4o", None, None,
        )
        .unwrap();
        {
            let store =
                crate::storage::DataStore::new(&db, &env_dir.path().join("artifacts")).unwrap();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                     VALUES (?1, ?2, 'created', 'openai', 'gpt-4o')",
                    rusqlite::params![id, conv_id],
                )
                .unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = Arc::new(tools_for(&id, "full_access", &root));
        plan_mode::enter(&id, "full_access");

        let submitting = {
            let tools = tools.clone();
            tokio::spawn(async move {
                tools
                    .execute_tool(
                        plan_mode::EXIT_PLAN_MODE_TOOL,
                        sample_plan(),
                        &CancellationToken::new(),
                    )
                    .await
            })
        };
        answer_plan_prompt(&tools, false).await;
        let out = submitting.await.unwrap();

        assert!(
            !out.is_error,
            "a rejection is an answer, not a tool failure: {:?}",
            out.output
        );
        assert_eq!(out.output["approved"], false);
        assert_eq!(out.output["rejections"], 1);
        assert!(plan_mode::is_active(&id), "the latch must stay closed");

        let blocked = tools
            .execute_tool(
                "write_file",
                serde_json::json!({
                    "path": root.join("nope.txt").to_string_lossy(),
                    "content": "x"
                }),
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(blocked.output["code"], "plan_mode_blocked");
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_cancelled_run_does_not_count_as_approval() {
        let id = run_id("cancel");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = Arc::new(tools_for(&id, "full_access", &root));
        plan_mode::enter(&id, "full_access");

        let submitting = {
            let tools = tools.clone();
            tokio::spawn(async move {
                tools
                    .execute_tool(
                        plan_mode::EXIT_PLAN_MODE_TOOL,
                        sample_plan(),
                        &CancellationToken::new(),
                    )
                    .await
            })
        };
        // Wait for the waiter to exist, then tear it down the way run cancel does.
        for _ in 0..200 {
            if tools.interactions.permission_count().await > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tools
            .interactions
            .cancel_runs(std::slice::from_ref(&id))
            .await;
        let out = submitting.await.unwrap();

        assert_eq!(out.output["approved"], false);
        assert!(plan_mode::is_active(&id));
        plan_mode::clear(&id);
    }
}

/// A4 — ReadOnly Fast Path regression tests.
#[cfg(test)]
mod readonly_fast_path_tests {
    use super::*;
    use crate::production::ProductionRuntime;
    use tokio_util::sync::CancellationToken;

    fn tools_for(run_id: &str, profile: &str, root: &std::path::Path) -> PermissionGatedTools {
        let rt = ProductionRuntime::new();
        let mut gateway = CapabilityGateway::new();
        gateway.set_project_root(root.to_string_lossy().to_string());
        let _ = gateway.register_builtins();
        PermissionGatedTools {
            gateway: Arc::new(gateway),
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engines.clone(),
            runtime: None,
            provider_id: "test".into(),
            key_id: None,
            parent_run_id: run_id.to_string(),
            conversation_id: format!("conv-{run_id}"),
            model_id: "test-model".into(),
            permission_profile: profile.to_string(),
            tool_allowlist: None,
            team: None,
            mcp_tool_schemas: Vec::new(),
            selected_mcp_servers: None,
        }
    }

    /// A4: a genuinely ReadOnly tool must take the fast path — no side-effect
    /// ledger row, no checkpoint snapshot. ToolCallStarted/ToolCallCompleted
    /// remain durable facts (engine loop), so the invocation is still
    /// observable; only checkpoint/ledger/lease overhead is skipped.
    #[tokio::test]
    async fn readonly_tool_does_not_create_side_effect_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().unwrap();
        let run_id = format!("a4-ro-{}", uuid::Uuid::new_v4());
        let tools = tools_for(&run_id, "full_access", &root);
        // read_file is classified ReadOnly by the Gateway capability registry.
        let project_file = root.join("probe.txt");
        std::fs::write(&project_file, "a4 probe").expect("write probe");
        let out = tools
            .execute_tool(
                "read_file",
                serde_json::json!({ "path": project_file.to_string_lossy() }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error, "read_file must succeed: {:?}", out.output);
        // The ledger must have NO record for this run (fast path skipped it).
        let watermark = crate::side_effect_ledger::ledger_watermark(&run_id)
            .ok()
            .flatten();
        assert_eq!(
            watermark.as_deref(),
            Some("0"),
            "readonly tool must not create side-effect ledger records"
        );
    }
}
