//! SubAgentManager — atomic budget reservations and sub-agent lifecycle.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::types::{SubAgent, SubAgentConfig, SubAgentStatus};

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
