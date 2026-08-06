//! Sub-agent execution — bounded multi-agent orchestration.
//!
//! Manages independent Run identity, permission non-inheritance,
//! concurrency/depth/token limits, and parent recovery after child failure.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default readonly tool surface for sub-agents (Phase 0 security floor).
pub fn default_subagent_tool_allowlist() -> Vec<String> {
    vec!["read_file".into(), "list_dir".into(), "grep".into()]
}

/// Cap a child's permission profile so it never exceeds the parent.
///
/// Privilege order: `readonly` < `ask` < `full_access`.
/// Unknown / empty values normalize to `ask`.
///
/// Child defaults remain readonly/ask even when the parent is `full_access`
/// (callers pass the requested profile; omit or pass `ask` for the default).
pub fn cap_child_permission(parent: &str, requested: &str) -> String {
    fn rank(profile: &str) -> u8 {
        match profile.trim() {
            "readonly" | "read_only" => 0,
            "full_access" | "autonomous" | "full" => 2,
            // ask / confirm_each / empty / unknown
            _ => 1,
        }
    }
    fn label(rank: u8) -> String {
        match rank {
            0 => "readonly".into(),
            2 => "full_access".into(),
            _ => "ask".into(),
        }
    }
    label(rank(parent).min(rank(requested)))
}

/// Does a tool allowlist admit `name`?
///
/// Sole definition of allowlist matching, so the runtime that *enforces* a
/// surface and the resolver that *derives* a child surface can never disagree.
/// MCP tools (`mcp__server__tool`) are admitted by their exact name or by the
/// `mcp_call` capability entry that stands for the whole MCP surface.
pub fn tool_list_allows(list: &[String], name: &str) -> bool {
    if list.iter().any(|tool| tool == name) {
        return true;
    }
    if name.starts_with("mcp__") {
        return list.iter().any(|tool| tool == "mcp_call" || tool == name);
    }
    false
}

/// Resolve a child's permission profile from the request, the selected agent
/// profile, and the parent's own profile.
///
/// Two independent one-way valves, in this order:
///
/// 1. The agent profile can only *tighten* the request. A profile declaring
///    `permissionMode: full_access` never elevates a child whose caller did not
///    explicitly ask for it — otherwise "pick a powerful persona" would be an
///    escalation primitive for a prompt-injected parent.
/// 2. The parent caps whatever survives, via [`cap_child_permission`].
///
/// An absent request floors at `ask`, matching the sub-agent default.
pub fn resolve_child_permission(
    parent: &str,
    requested: Option<&str>,
    profile_mode: Option<&str>,
) -> String {
    let mut requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ask")
        .to_string();
    if let Some(mode) = profile_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        requested = cap_child_permission(&requested, mode);
    }
    cap_child_permission(parent, &requested)
}

/// Resolve a child's tool surface from the request, the selected agent profile,
/// and the parent's own surface.
///
/// Precedence for the starting set: an explicit request, else the profile's
/// `tools`, else the parent's surface, else the readonly default. The profile's
/// `disallowedTools` are then removed, and finally the parent's surface is a
/// hard ceiling — a child can never reach a tool the parent itself cannot call.
///
/// `parent = None` means "unrestricted surface" (a root run), so the ceiling is
/// a no-op there; the permission profile still gates every call.
pub fn resolve_child_tool_allowlist(
    parent: Option<&[String]>,
    requested: Option<&[String]>,
    profile_tools: Option<&[String]>,
    profile_disallowed: Option<&[String]>,
) -> Vec<String> {
    let mut out: Vec<String> = match (requested, profile_tools) {
        (Some(requested), _) => requested.to_vec(),
        (None, Some(tools)) => tools.to_vec(),
        (None, None) => parent
            .map(<[String]>::to_vec)
            .unwrap_or_else(default_subagent_tool_allowlist),
    };
    if let Some(denied) = profile_disallowed {
        out.retain(|tool| !denied.iter().any(|entry| entry == tool));
    }
    if let Some(parent) = parent {
        out.retain(|tool| tool_list_allows(parent, tool));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|tool| seen.insert(tool.clone()));
    out
}

/// Failure propagation for child batches (task-11, T05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailurePolicy {
    /// Parent receives child failure; siblings continue (default).
    #[default]
    Isolate,
    /// One failure cancels siblings; parent fails.
    FailFast,
    /// Wait for all; any failure makes aggregate fail.
    RequireAll,
    /// Re-queue a failed child up to `max_retries`; then isolate.
    Retry,
}

impl FailurePolicy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "fail_fast" | "failfast" => Self::FailFast,
            "require_all" | "requireall" => Self::RequireAll,
            "retry" | "retry_failed" => Self::Retry,
            _ => Self::Isolate,
        }
    }

    /// Stable wire name for persistence (inverse of [`Self::parse`]).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Isolate => "isolate",
            Self::FailFast => "fail_fast",
            Self::RequireAll => "require_all",
            Self::Retry => "retry",
        }
    }

    /// What a terminal child *failure* must do to the parent run.
    ///
    /// This is the single decision the production watcher consumes (T05).
    /// `all_siblings_terminal` is true when every sibling child of the same
    /// parent has already settled — only [`FailurePolicy::RequireAll`] cares.
    /// `retries_remaining` is `max_retries.saturating_sub(retry_count)`.
    pub fn on_child_failed(
        &self,
        all_siblings_terminal: bool,
        retries_remaining: u32,
    ) -> ChildFailureEffect {
        match self {
            FailurePolicy::Isolate => ChildFailureEffect::Isolate,
            FailurePolicy::FailFast => ChildFailureEffect::FailParent,
            FailurePolicy::RequireAll if all_siblings_terminal => ChildFailureEffect::FailParent,
            FailurePolicy::RequireAll => ChildFailureEffect::Isolate,
            FailurePolicy::Retry if retries_remaining > 0 => ChildFailureEffect::Retry,
            FailurePolicy::Retry => ChildFailureEffect::Isolate,
        }
    }
}

/// Concrete parent-side action a child failure triggers under a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildFailureEffect {
    /// Parent observes `SubagentFailed` and continues; siblings unaffected.
    Isolate,
    /// Parent run fails now (and sibling children are cancelled).
    FailParent,
    /// Re-queue the child on the same hidden conversation.
    Retry,
}

/// Sub-agent configuration — every field is enforced by [`SubAgentManager`].
#[derive(Debug, Clone)]
pub struct SubAgentConfig {
    /// Backward-compatible alias for [`Self::max_concurrent_global`].
    pub max_concurrent: u32,
    pub max_concurrent_global: u32,
    pub max_concurrent_per_parent: u32,
    pub max_tasks_per_parent_total: u32,
    pub max_depth: u32,
    pub max_tokens_per_sub: u64,
    pub max_tokens_per_child: u64,
    pub max_tokens_per_tree: u64,
    pub max_tool_calls_per_child: u32,
    pub max_tool_calls_per_tree: u32,
    pub child_timeout_ms: u64,
    pub failure_policy: FailurePolicy,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        SubAgentConfig {
            max_concurrent: 3,
            max_concurrent_global: 3,
            max_concurrent_per_parent: 3,
            max_tasks_per_parent_total: 32,
            max_depth: 5,
            max_tokens_per_sub: 100_000,
            max_tokens_per_child: 100_000,
            max_tokens_per_tree: 500_000,
            max_tool_calls_per_child: 200,
            max_tool_calls_per_tree: 1_000,
            child_timeout_ms: 600_000,
            failure_policy: FailurePolicy::Isolate,
        }
    }
}

/// Atomic reservation ledger entry (Queued + Running both occupy concurrent slots).
#[derive(Debug, Clone, Default)]
struct ReservationLedger {
    /// Active concurrent reservations (Queued + Running).
    concurrent_global: u32,
    concurrent_per_parent: std::collections::HashMap<String, u32>,
    tasks_per_parent: std::collections::HashMap<String, u32>,
    tokens_used_child: std::collections::HashMap<String, u64>,
    tokens_used_tree: std::collections::HashMap<String, u64>,
    tools_used_child: std::collections::HashMap<String, u32>,
    tools_used_tree: std::collections::HashMap<String, u32>,
    /// run_id → reserved depth
    depth_of: std::collections::HashMap<String, u32>,
}

/// A sub-agent execution — real child-run identity (not metadata-only).
#[derive(Debug, Clone)]
pub struct SubAgent {
    pub id: String,
    pub run_id: String,
    pub parent_run_id: String,
    pub agent_profile_id: Option<String>,
    pub provider_id: String,
    pub key_id: String,
    pub model_id: String,
    pub permission_profile: String,
    pub tool_allowlist: Vec<String>,
    pub task: String,
    pub status: SubAgentStatus,
    pub depth: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub isolation_mode: String,
    pub working_directory: Option<String>,
    pub resume_from: Option<String>,
}

/// Sub-agent status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubAgentStatus {
    Queued,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// Manages sub-agent execution with atomic budget reservations (task-11).
pub struct SubAgentManager {
    config: SubAgentConfig,
    agents: Arc<Mutex<HashMap<String, SubAgent>>>,
    parent_children: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Single-mutex ledger: Queued+Running concurrent, tree budgets.
    ledger: Arc<Mutex<ReservationLedger>>,
}

impl SubAgentManager {
    pub fn new(config: SubAgentConfig) -> Self {
        let mut cfg = config;
        // Legacy `max_concurrent` is an alias for the global cap. Honor the
        // stricter of the two so old tests/callers that only set max_concurrent
        // still bound reservations.
        if cfg.max_concurrent == 0 {
            cfg.max_concurrent = cfg.max_concurrent_global.max(1);
        }
        if cfg.max_concurrent_global == 0 {
            cfg.max_concurrent_global = cfg.max_concurrent.max(1);
        }
        cfg.max_concurrent_global = cfg.max_concurrent_global.min(cfg.max_concurrent);
        cfg.max_concurrent = cfg.max_concurrent_global;
        SubAgentManager {
            config: cfg,
            agents: Arc::new(Mutex::new(HashMap::new())),
            parent_children: Arc::new(Mutex::new(HashMap::new())),
            ledger: Arc::new(Mutex::new(ReservationLedger::default())),
        }
    }

    pub fn config(&self) -> &SubAgentConfig {
        &self.config
    }

    /// Compute child depth from parent chain (caller must not hardcode 1).
    pub async fn depth_for_child(&self, parent_run_id: &str) -> u32 {
        let ledger = self.ledger.lock().await;
        let parent_depth = ledger.depth_of.get(parent_run_id).copied().unwrap_or(0);
        parent_depth + 1
    }

    /// Record root run depth 0 so children compute correctly.
    pub async fn register_root_depth(&self, run_id: &str) {
        let mut ledger = self.ledger.lock().await;
        ledger.depth_of.entry(run_id.to_string()).or_insert(0);
    }

    /// All-or-nothing concurrent reservation for a batch of N children under one parent.
    pub async fn reserve_batch(&self, parent_run_id: &str, n: u32) -> Result<(), String> {
        if n == 0 {
            return Ok(());
        }
        let mut ledger = self.ledger.lock().await;
        let global_cap = self
            .config
            .max_concurrent_global
            .max(self.config.max_concurrent);
        if ledger.concurrent_global.saturating_add(n) > global_cap {
            return Err(format!(
                "Max concurrent sub-agents ({global_cap}) would be exceeded by batch of {n}"
            ));
        }
        let per_parent = ledger
            .concurrent_per_parent
            .get(parent_run_id)
            .copied()
            .unwrap_or(0);
        if per_parent.saturating_add(n) > self.config.max_concurrent_per_parent {
            return Err(format!(
                "Max concurrent sub-agents per parent ({}) would be exceeded",
                self.config.max_concurrent_per_parent
            ));
        }
        let total = ledger
            .tasks_per_parent
            .get(parent_run_id)
            .copied()
            .unwrap_or(0);
        if total.saturating_add(n) > self.config.max_tasks_per_parent_total {
            return Err(format!(
                "Max tasks per parent ({}) would be exceeded",
                self.config.max_tasks_per_parent_total
            ));
        }
        // Commit reservation.
        ledger.concurrent_global = ledger.concurrent_global.saturating_add(n);
        *ledger
            .concurrent_per_parent
            .entry(parent_run_id.to_string())
            .or_insert(0) += n;
        *ledger
            .tasks_per_parent
            .entry(parent_run_id.to_string())
            .or_insert(0) += n;
        Ok(())
    }

    /// Undo a successful [`reserve_batch`] when the batch will not start children.
    pub async fn release_batch_reservation(&self, parent_run_id: &str, n: u32) {
        if n == 0 {
            return;
        }
        let mut ledger = self.ledger.lock().await;
        ledger.concurrent_global = ledger.concurrent_global.saturating_sub(n);
        if let Some(v) = ledger.concurrent_per_parent.get_mut(parent_run_id) {
            *v = v.saturating_sub(n);
        }
        if let Some(v) = ledger.tasks_per_parent.get_mut(parent_run_id) {
            *v = v.saturating_sub(n);
        }
    }

    /// Release concurrent reservation when a child reaches a terminal status.
    async fn release_concurrent_slot(&self, parent_run_id: &str) {
        let mut ledger = self.ledger.lock().await;
        ledger.concurrent_global = ledger.concurrent_global.saturating_sub(1);
        if let Some(v) = ledger.concurrent_per_parent.get_mut(parent_run_id) {
            *v = v.saturating_sub(1);
        }
    }

    /// Settle token usage against child + tree budgets; Err if exceeded.
    pub async fn settle_tokens(
        &self,
        child_run_id: &str,
        tree_root_run_id: &str,
        tokens: u64,
    ) -> Result<(), String> {
        let mut ledger = self.ledger.lock().await;
        let child_used = ledger
            .tokens_used_child
            .entry(child_run_id.to_string())
            .or_insert(0);
        *child_used = child_used.saturating_add(tokens);
        // Prefer the tighter of max_tokens_per_child and legacy max_tokens_per_sub.
        let child_cap = self
            .config
            .max_tokens_per_child
            .min(self.config.max_tokens_per_sub);
        if *child_used > child_cap {
            return Err(format!(
                "child token budget exceeded ({}/{})",
                *child_used, child_cap
            ));
        }
        let tree_used = ledger
            .tokens_used_tree
            .entry(tree_root_run_id.to_string())
            .or_insert(0);
        *tree_used = tree_used.saturating_add(tokens);
        if *tree_used > self.config.max_tokens_per_tree {
            return Err(format!(
                "tree token budget exceeded ({}/{})",
                *tree_used, self.config.max_tokens_per_tree
            ));
        }
        Ok(())
    }

    /// Atomically consume one tool-call unit before ToolCallRequested.
    pub async fn consume_tool_call(
        &self,
        child_run_id: &str,
        tree_root_run_id: &str,
    ) -> Result<(), String> {
        let mut ledger = self.ledger.lock().await;
        let c = ledger
            .tools_used_child
            .entry(child_run_id.to_string())
            .or_insert(0);
        *c = c.saturating_add(1);
        if *c > self.config.max_tool_calls_per_child {
            return Err(format!(
                "child tool-call budget exceeded ({}/{})",
                *c, self.config.max_tool_calls_per_child
            ));
        }
        let t = ledger
            .tools_used_tree
            .entry(tree_root_run_id.to_string())
            .or_insert(0);
        *t = t.saturating_add(1);
        if *t > self.config.max_tool_calls_per_tree {
            return Err(format!(
                "tree tool-call budget exceeded ({}/{})",
                *t, self.config.max_tool_calls_per_tree
            ));
        }
        Ok(())
    }

    /// Detect cycle if `child_run_id` would wait on ancestor (current task is non-blocking;
    /// still reject constructing parent cycles).
    pub async fn assert_no_parent_cycle(
        &self,
        parent_run_id: &str,
        child_run_id: &str,
    ) -> Result<(), String> {
        if parent_run_id == child_run_id {
            return Err("SUBAGENT_DEADLOCK: child cannot parent itself".into());
        }
        let agents = self.agents.lock().await;
        // Walk ancestors of parent; if we hit child_run_id, cycle.
        let mut cursor = Some(parent_run_id.to_string());
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = cursor {
            if !seen.insert(id.clone()) {
                return Err("SUBAGENT_DEADLOCK: parent cycle detected".into());
            }
            if id == child_run_id {
                return Err("SUBAGENT_DEADLOCK: child would block on ancestor".into());
            }
            cursor = agents
                .values()
                .find(|a| a.run_id == id)
                .map(|a| a.parent_run_id.clone());
        }
        Ok(())
    }

    pub async fn active_reservation_count(&self) -> u32 {
        self.ledger.lock().await.concurrent_global
    }

    /// Spawn a new sub-agent with independent provider/key/model identity.
    /// Does **not** inherit parent key or permission profile.
    ///
    /// Generates new `id` / `run_id`. Prefer [`register`] when the child run is
    /// created by RunManager so metadata shares the real run id.
    #[allow(clippy::too_many_arguments)] // public API: child scope fields are fixed
    pub async fn spawn(
        &self,
        parent_run_id: &str,
        task: String,
        depth: u32,
        provider_id: String,
        key_id: String,
        model_id: String,
        permission_profile: String,
        tool_allowlist: Vec<String>,
        agent_profile_id: Option<String>,
        isolation_mode: Option<String>,
        working_directory: Option<String>,
    ) -> Result<SubAgent, String> {
        self.register(
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
            parent_run_id,
            task,
            depth,
            provider_id,
            key_id,
            model_id,
            permission_profile,
            tool_allowlist,
            agent_profile_id,
            isolation_mode,
            working_directory,
        )
        .await
    }

    /// Register a sub-agent whose `run_id` was already allocated by RunManager.
    ///
    /// `id` is typically the persistent `subagent_session` id (also used as task_id).
    #[allow(clippy::too_many_arguments)] // public API: child scope fields are fixed
    pub async fn register(
        &self,
        id: String,
        run_id: String,
        parent_run_id: &str,
        task: String,
        depth: u32,
        provider_id: String,
        key_id: String,
        model_id: String,
        permission_profile: String,
        tool_allowlist: Vec<String>,
        agent_profile_id: Option<String>,
        isolation_mode: Option<String>,
        working_directory: Option<String>,
    ) -> Result<SubAgent, String> {
        if provider_id.trim().is_empty() || key_id.trim().is_empty() || model_id.trim().is_empty() {
            return Err("Subagent requires independent provider_id + key_id + model_id".into());
        }
        if id.trim().is_empty() || run_id.trim().is_empty() {
            return Err("Subagent register requires non-empty id and run_id".into());
        }
        self.assert_no_parent_cycle(parent_run_id, &run_id).await?;

        // Depth from parent chain when caller passes 0 / underestimates.
        let parent_depth = {
            let ledger = self.ledger.lock().await;
            ledger.depth_of.get(parent_run_id).copied().unwrap_or(0)
        };
        let depth = depth.max(parent_depth.saturating_add(1));
        if depth > self.config.max_depth {
            return Err(format!(
                "Max sub-agent depth ({}) exceeded: {}",
                self.config.max_depth, depth
            ));
        }

        // Atomic reservation: Queued occupies concurrent slot (task-11).
        // Single-item reserve via batch(1).
        self.reserve_batch(parent_run_id, 1).await?;

        let mut agents = self.agents.lock().await;
        let sub = SubAgent {
            id: id.clone(),
            run_id: run_id.clone(),
            parent_run_id: parent_run_id.to_string(),
            agent_profile_id,
            provider_id,
            key_id,
            model_id,
            permission_profile,
            tool_allowlist,
            task,
            status: SubAgentStatus::Queued,
            depth,
            created_at: chrono::Utc::now(),
            isolation_mode: isolation_mode.unwrap_or_else(|| "none".into()),
            working_directory,
            resume_from: None,
        };

        agents.insert(id.clone(), sub.clone());
        {
            let mut ledger = self.ledger.lock().await;
            ledger.depth_of.insert(run_id, depth);
        }

        // Track parent-child relationship
        let mut parent_children = self.parent_children.lock().await;
        parent_children
            .entry(parent_run_id.to_string())
            .or_default()
            .push(id.clone());

        Ok(sub)
    }

    /// Collect all descendant SubAgents (nested) for a parent run, breadth-first.
    /// Returns child task records whose `run_id` keys live engines / events.
    pub async fn list_descendants(&self, parent_run_id: &str) -> Vec<SubAgent> {
        let agents = self.agents.lock().await;
        let parent_children = self.parent_children.lock().await;
        let mut out = Vec::new();
        let mut queue = vec![parent_run_id.to_string()];
        let mut seen_parents = std::collections::HashSet::new();
        while let Some(pid) = queue.pop() {
            if !seen_parents.insert(pid.clone()) {
                continue;
            }
            if let Some(child_ids) = parent_children.get(&pid) {
                for id in child_ids {
                    if let Some(agent) = agents.get(id) {
                        out.push(agent.clone());
                        // Nested subagents are parented by the child's run_id.
                        queue.push(agent.run_id.clone());
                    }
                }
            }
        }
        out
    }

    /// Metadata-only: mark descendants Cancelled. Prefer ProductionRuntime::cancel_run_tree
    /// which also request_cancel()s live engines — do not call this alone in production.
    /// Releases concurrent reservations for each newly-cancelled child.
    pub async fn cascade_cancel_metadata(&self, parent_run_id: &str) -> usize {
        let descendants = self.list_descendants(parent_run_id).await;
        let mut count = 0usize;
        for d in descendants {
            let need = {
                let agents = self.agents.lock().await;
                agents
                    .get(&d.id)
                    .map(|a| {
                        !matches!(
                            a.status,
                            SubAgentStatus::Completed
                                | SubAgentStatus::Failed(_)
                                | SubAgentStatus::Cancelled
                        )
                    })
                    .unwrap_or(false)
            };
            if need
                && self
                    .update_status(&d.id, SubAgentStatus::Cancelled)
                    .await
                    .is_ok()
            {
                count += 1;
            }
        }
        count
    }

    /// Backward-compatible alias — metadata only (engines are cancelled by cancel_run_tree).
    pub async fn cascade_cancel(&self, parent_run_id: &str) -> usize {
        self.cascade_cancel_metadata(parent_run_id).await
    }

    /// Re-point a sub-agent's run id (Retry re-queue) without touching status
    /// or the concurrent slot — the slot stays reserved across the retry.
    pub async fn update_run_id(&self, id: &str, new_run_id: &str) -> Result<(), String> {
        let mut agents = self.agents.lock().await;
        let Some(agent) = agents.get_mut(id) else {
            return Err(format!("Sub-agent '{}' not found", id));
        };
        let old_run = agent.run_id.clone();
        if old_run == new_run_id {
            return Ok(());
        }
        agent.run_id = new_run_id.to_string();
        drop(agents);
        let mut ledger = self.ledger.lock().await;
        if let Some(depth) = ledger.depth_of.remove(&old_run) {
            ledger.depth_of.insert(new_run_id.to_string(), depth);
        }
        Ok(())
    }

    /// Update sub-agent status.
    pub async fn update_status(&self, id: &str, status: SubAgentStatus) -> Result<(), String> {
        let mut agents = self.agents.lock().await;
        if let Some(agent) = agents.get_mut(id) {
            let prev = agent.status.clone();
            let parent = agent.parent_run_id.clone();
            let was_active = matches!(prev, SubAgentStatus::Queued | SubAgentStatus::Running);
            let now_terminal = matches!(
                status,
                SubAgentStatus::Completed | SubAgentStatus::Failed(_) | SubAgentStatus::Cancelled
            );
            agent.status = status;
            drop(agents);
            if was_active && now_terminal {
                self.release_concurrent_slot(&parent).await;
            }
            Ok(())
        } else {
            Err(format!("Sub-agent '{}' not found", id))
        }
    }

    /// Get children of a parent run.
    pub async fn get_children(&self, parent_run_id: &str) -> Vec<SubAgent> {
        let agents = self.agents.lock().await;
        let parent_children = self.parent_children.lock().await;
        if let Some(children) = parent_children.get(parent_run_id) {
            children
                .iter()
                .filter_map(|id| agents.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get a sub-agent by ID.
    pub async fn get(&self, id: &str) -> Option<SubAgent> {
        let agents = self.agents.lock().await;
        agents.get(id).cloned()
    }

    /// Get all sub-agents.
    pub async fn list(&self) -> Vec<SubAgent> {
        let agents = self.agents.lock().await;
        agents.values().cloned().collect()
    }

    /// Get the number of running sub-agents.
    pub async fn running_count(&self) -> usize {
        let agents = self.agents.lock().await;
        agents
            .values()
            .filter(|a| a.status == SubAgentStatus::Running)
            .count()
    }

    /// Queued + Running count (matches reservation ledger when consistent).
    pub async fn active_count(&self) -> usize {
        let agents = self.agents.lock().await;
        agents
            .values()
            .filter(|a| matches!(a.status, SubAgentStatus::Queued | SubAgentStatus::Running))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn spawn_default(
        manager: &SubAgentManager,
        parent: &str,
        task: &str,
        depth: u32,
    ) -> Result<SubAgent, String> {
        manager
            .spawn(
                parent,
                task.to_string(),
                depth,
                "openai".into(),
                format!("key-{}", uuid::Uuid::new_v4()),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
    }

    #[tokio::test]
    async fn test_spawn_sub_agent() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let sub = spawn_default(&manager, "parent-1", "Test task", 1)
            .await
            .unwrap();
        assert_eq!(sub.parent_run_id, "parent-1");
        assert_eq!(sub.status, SubAgentStatus::Queued);
        assert!(!sub.run_id.is_empty());
        assert_eq!(sub.provider_id, "openai");
        assert!(!sub.key_id.is_empty());
        // Independent identity: not empty key/model
        assert_eq!(sub.model_id, "gpt-4o");
        assert_eq!(sub.permission_profile, "ask");
    }

    #[tokio::test]
    async fn test_spawn_requires_key_identity() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let err = manager
            .spawn(
                "parent-1",
                "x".into(),
                1,
                "openai".into(),
                "".into(),
                "gpt-4o".into(),
                "ask".into(),
                vec![],
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("key_id"));
    }

    #[tokio::test]
    async fn test_depth_limit() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let result = spawn_default(&manager, "parent-1", "Deep task", 100).await;
        assert!(result.is_err(), "Should reject deep sub-agent");
    }

    #[tokio::test]
    async fn test_concurrency_limit() {
        let config = SubAgentConfig {
            max_concurrent: 1,
            ..Default::default()
        };
        let manager = SubAgentManager::new(config);

        // Spawn first sub-agent
        let sub1 = spawn_default(&manager, "parent-1", "Task 1", 1)
            .await
            .unwrap();
        manager
            .update_status(&sub1.id, SubAgentStatus::Running)
            .await
            .unwrap();

        // Second spawn should fail due to concurrency limit
        let result = spawn_default(&manager, "parent-1", "Task 2", 1).await;
        assert!(result.is_err(), "Should reject concurrent sub-agent");
    }

    #[tokio::test]
    async fn test_cascade_cancel() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let a = spawn_default(&manager, "parent-1", "A", 1).await.unwrap();
        let b = spawn_default(&manager, "parent-1", "B", 1).await.unwrap();
        manager
            .update_status(&a.id, SubAgentStatus::Running)
            .await
            .unwrap();
        manager
            .update_status(&b.id, SubAgentStatus::Running)
            .await
            .unwrap();
        let n = manager.cascade_cancel("parent-1").await;
        assert_eq!(n, 2);
        assert_eq!(
            manager.get(&a.id).await.unwrap().status,
            SubAgentStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn test_get_children() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        spawn_default(&manager, "parent-1", "Child 1", 1)
            .await
            .unwrap();
        spawn_default(&manager, "parent-1", "Child 2", 1)
            .await
            .unwrap();

        let children = manager.get_children("parent-1").await;
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_parent_recovery_after_child_failure() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let sub = spawn_default(&manager, "parent-1", "Failing task", 1)
            .await
            .unwrap();

        // Simulate child failure
        manager
            .update_status(&sub.id, SubAgentStatus::Failed("Error".to_string()))
            .await
            .unwrap();

        // Parent can still spawn new children
        let new_sub = spawn_default(&manager, "parent-1", "Recovery task", 1).await;
        assert!(new_sub.is_ok(), "Parent should recover after child failure");
    }

    #[tokio::test]
    async fn test_list_sub_agents() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        spawn_default(&manager, "parent-1", "Task 1", 1)
            .await
            .unwrap();
        spawn_default(&manager, "parent-1", "Task 2", 1)
            .await
            .unwrap();
        assert_eq!(manager.list().await.len(), 2);
    }

    #[test]
    fn test_cap_child_permission_never_upgrades() {
        // Parent ask: child cannot become full_access.
        assert_eq!(cap_child_permission("ask", "full_access"), "ask");
        assert_eq!(cap_child_permission("readonly", "ask"), "readonly");
        assert_eq!(cap_child_permission("readonly", "full_access"), "readonly");
        // Parent full_access: explicit full_access request allowed; default ask stays ask.
        assert_eq!(
            cap_child_permission("full_access", "full_access"),
            "full_access"
        );
        assert_eq!(cap_child_permission("full_access", "ask"), "ask");
        assert_eq!(cap_child_permission("full_access", "readonly"), "readonly");
        assert_eq!(cap_child_permission("autonomous", "full"), "full_access");
        // Unknown → ask floor.
        assert_eq!(cap_child_permission("ask", ""), "ask");
        assert_eq!(cap_child_permission("", "full_access"), "ask");
    }

    #[test]
    fn test_default_subagent_tool_allowlist_is_readonly() {
        let list = default_subagent_tool_allowlist();
        assert!(list.contains(&"read_file".into()));
        assert!(list.contains(&"list_dir".into()));
        assert!(list.contains(&"grep".into()));
        assert!(!list
            .iter()
            .any(|t| t == "write_file" || t == "task" || t == "run_terminal"));
    }

    #[tokio::test]
    async fn queued_occupies_concurrent_reservation() {
        let config = SubAgentConfig {
            max_concurrent: 1,
            max_concurrent_global: 1,
            ..Default::default()
        };
        let manager = SubAgentManager::new(config);
        let _a = spawn_default(&manager, "p", "A", 1).await.unwrap();
        // Still Queued — must block second spawn.
        assert_eq!(manager.active_reservation_count().await, 1);
        let err = spawn_default(&manager, "p", "B", 1).await.unwrap_err();
        assert!(err.contains("concurrent") || err.contains("Max"), "{err}");
    }

    #[tokio::test]
    async fn batch_preflight_all_or_nothing() {
        let config = SubAgentConfig {
            max_concurrent_global: 2,
            max_concurrent: 2,
            ..Default::default()
        };
        let manager = SubAgentManager::new(config);
        // Preflight of 3 must fail without leaving reservations.
        let err = manager.reserve_batch("p", 3).await.unwrap_err();
        assert!(err.contains("concurrent") || err.contains("Max"), "{err}");
        assert_eq!(manager.active_reservation_count().await, 0);
        manager.reserve_batch("p", 2).await.unwrap();
        assert_eq!(manager.active_reservation_count().await, 2);
        manager.release_batch_reservation("p", 2).await;
        assert_eq!(manager.active_reservation_count().await, 0);
    }

    #[tokio::test]
    async fn depth_increments_from_parent_chain() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        manager.register_root_depth("root").await;
        let c1 = spawn_default(&manager, "root", "L1", 0).await.unwrap();
        assert_eq!(c1.depth, 1);
        let c2 = manager
            .spawn(
                &c1.run_id,
                "L2".into(),
                0,
                "openai".into(),
                format!("key-{}", uuid::Uuid::new_v4()),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(c2.depth, 2);
    }

    #[tokio::test]
    async fn tool_and_token_budgets_enforce() {
        let config = SubAgentConfig {
            max_tool_calls_per_child: 2,
            max_tokens_per_child: 10,
            max_tokens_per_tree: 15,
            ..Default::default()
        };
        let manager = SubAgentManager::new(config);
        manager.consume_tool_call("c1", "root").await.unwrap();
        manager.consume_tool_call("c1", "root").await.unwrap();
        let err = manager.consume_tool_call("c1", "root").await.unwrap_err();
        assert!(err.contains("tool-call"), "{err}");
        manager.settle_tokens("c1", "root", 10).await.unwrap();
        let err = manager.settle_tokens("c1", "root", 1).await.unwrap_err();
        assert!(err.contains("token"), "{err}");
    }

    #[tokio::test]
    async fn parent_cycle_rejected() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let err = manager
            .assert_no_parent_cycle("same", "same")
            .await
            .unwrap_err();
        assert!(err.contains("DEADLOCK"), "{err}");
    }

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tool_list_allows_exact_and_mcp_surface() {
        let list = owned(&["read_file", "mcp_call"]);
        assert!(tool_list_allows(&list, "read_file"));
        assert!(!tool_list_allows(&list, "write_file"));
        // `mcp_call` stands for the whole MCP surface.
        assert!(tool_list_allows(&list, "mcp__github__create_issue"));
        // Without it, only the exact MCP tool name is admitted.
        let exact = owned(&["mcp__github__create_issue"]);
        assert!(tool_list_allows(&exact, "mcp__github__create_issue"));
        assert!(!tool_list_allows(&exact, "mcp__github__delete_repo"));
        assert!(!tool_list_allows(&[], "read_file"));
    }

    #[test]
    fn resolve_child_permission_profile_only_tightens() {
        // A profile declaring full_access does not elevate an unrequested child.
        assert_eq!(
            resolve_child_permission("full_access", None, Some("full_access")),
            "ask"
        );
        // Nor does it elevate past a readonly request.
        assert_eq!(
            resolve_child_permission("full_access", Some("readonly"), Some("full_access")),
            "readonly"
        );
        // A restrictive profile tightens an explicit full_access request.
        assert_eq!(
            resolve_child_permission("full_access", Some("full_access"), Some("readonly")),
            "readonly"
        );
        // No profile: the request stands, still capped by the parent.
        assert_eq!(
            resolve_child_permission("full_access", Some("full_access"), None),
            "full_access"
        );
        assert_eq!(resolve_child_permission("full_access", None, None), "ask");
    }

    #[test]
    fn resolve_child_permission_parent_is_the_hard_ceiling() {
        // The headline escalation attempt: readonly parent, child asks for
        // full_access, and the chosen profile also declares full_access.
        assert_eq!(
            resolve_child_permission("readonly", Some("full_access"), Some("full_access")),
            "readonly"
        );
        assert_eq!(
            resolve_child_permission("ask", Some("full_access"), Some("full_access")),
            "ask"
        );
        // Unknown / blank parent floors at ask.
        assert_eq!(
            resolve_child_permission("", Some("full_access"), Some("full_access")),
            "ask"
        );
        assert_eq!(
            resolve_child_permission("   ", Some("full_access"), None),
            "ask"
        );
        // Blank request is treated as absent, not as an elevation.
        assert_eq!(
            resolve_child_permission("full_access", Some(""), None),
            "ask"
        );
    }

    #[test]
    fn resolve_child_tool_allowlist_precedence() {
        let parent = owned(&["read_file", "grep", "write_file", "task"]);
        // Explicit request wins over the profile.
        assert_eq!(
            resolve_child_tool_allowlist(
                Some(&parent),
                Some(&owned(&["grep"])),
                Some(&owned(&["write_file"])),
                None
            ),
            owned(&["grep"])
        );
        // Profile tools apply when nothing is requested.
        assert_eq!(
            resolve_child_tool_allowlist(
                Some(&parent),
                None,
                Some(&owned(&["read_file", "grep"])),
                None
            ),
            owned(&["read_file", "grep"])
        );
        // Neither: inherit the parent surface.
        assert_eq!(
            resolve_child_tool_allowlist(Some(&parent), None, None, None),
            parent
        );
        // Unrestricted parent + nothing declared: readonly default floor.
        assert_eq!(
            resolve_child_tool_allowlist(None, None, None, None),
            default_subagent_tool_allowlist()
        );
        // An explicitly empty request is fail-closed, not "fall back to default".
        assert!(resolve_child_tool_allowlist(Some(&parent), Some(&[]), None, None).is_empty());
        // Duplicates collapse, order preserved.
        assert_eq!(
            resolve_child_tool_allowlist(
                None,
                Some(&owned(&["grep", "read_file", "grep"])),
                None,
                None
            ),
            owned(&["grep", "read_file"])
        );
    }

    #[test]
    fn resolve_child_tool_allowlist_parent_is_the_hard_ceiling() {
        let parent = owned(&["read_file", "grep", "task"]);
        // Profile asking for a surface the parent lacks gets intersected down.
        assert_eq!(
            resolve_child_tool_allowlist(
                Some(&parent),
                None,
                Some(&owned(&["read_file", "write_file", "run_terminal"])),
                None
            ),
            owned(&["read_file"])
        );
        // Same for a directly requested surface.
        assert_eq!(
            resolve_child_tool_allowlist(
                Some(&parent),
                Some(&owned(&["run_terminal", "write_file", "apply_patch"])),
                None,
                None
            ),
            Vec::<String>::new()
        );
        // Profile disallowedTools subtract even when the parent would allow them.
        assert_eq!(
            resolve_child_tool_allowlist(
                Some(&parent),
                None,
                Some(&owned(&["read_file", "grep"])),
                Some(&owned(&["grep"]))
            ),
            owned(&["read_file"])
        );
        // MCP: a parent holding only `mcp_call` still admits a named MCP tool.
        let mcp_parent = owned(&["mcp_call"]);
        assert_eq!(
            resolve_child_tool_allowlist(
                Some(&mcp_parent),
                Some(&owned(&["mcp__github__create_issue", "read_file"])),
                None,
                None
            ),
            owned(&["mcp__github__create_issue"])
        );
        // Unrestricted parent: no ceiling, permission profile still gates calls.
        assert_eq!(
            resolve_child_tool_allowlist(None, Some(&owned(&["run_terminal"])), None, None),
            owned(&["run_terminal"])
        );
    }

    #[test]
    fn failure_policy_parse() {
        assert_eq!(FailurePolicy::parse("isolate"), FailurePolicy::Isolate);
        assert_eq!(FailurePolicy::parse("fail_fast"), FailurePolicy::FailFast);
        assert_eq!(
            FailurePolicy::parse("require_all"),
            FailurePolicy::RequireAll
        );
        assert_eq!(FailurePolicy::parse("retry"), FailurePolicy::Retry);
    }

    /// T05: the failure policy must produce a concrete, testable parent-side
    /// effect for every terminal child failure — the production watcher (and
    /// nothing else) consumes this decision.
    #[test]
    fn failure_policy_effects_on_child_failure() {
        use ChildFailureEffect::*;
        // Isolate: parent observes the failure and continues — regardless of
        // sibling state or retries left.
        assert_eq!(
            FailurePolicy::Isolate.on_child_failed(true, 0),
            Isolate
        );
        assert_eq!(
            FailurePolicy::Isolate.on_child_failed(false, 3),
            Isolate
        );
        // FailFast: one failure fails the parent immediately, even while
        // siblings are still running.
        assert_eq!(
            FailurePolicy::FailFast.on_child_failed(false, 0),
            FailParent
        );
        assert_eq!(
            FailurePolicy::FailFast.on_child_failed(true, 0),
            FailParent
        );
        // RequireAll waits for every sibling to settle before failing the
        // parent (aggregate outcome).
        assert_eq!(
            FailurePolicy::RequireAll.on_child_failed(false, 0),
            Isolate
        );
        assert_eq!(
            FailurePolicy::RequireAll.on_child_failed(true, 0),
            FailParent
        );
        // Retry re-queues the child while retries remain; exhausting retries
        // isolates (parent continues, child is failed).
        assert_eq!(
            FailurePolicy::Retry.on_child_failed(true, 1),
            Retry
        );
        assert_eq!(
            FailurePolicy::Retry.on_child_failed(false, 1),
            Retry
        );
        assert_eq!(
            FailurePolicy::Retry.on_child_failed(true, 0),
            Isolate
        );
    }
}
