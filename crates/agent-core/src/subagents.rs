//! Sub-agent execution — bounded multi-agent orchestration.
//!
//! Manages independent Run identity, permission non-inheritance,
//! concurrency/depth/token limits, and parent recovery after child failure.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default readonly tool surface for sub-agents (Phase 0 security floor).
pub fn default_subagent_tool_allowlist() -> Vec<String> {
    vec![
        "read_file".into(),
        "list_dir".into(),
        "grep".into(),
    ]
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

/// Sub-agent configuration.
#[derive(Debug, Clone)]
pub struct SubAgentConfig {
    pub max_concurrent: u32,
    pub max_depth: u32,
    pub max_tokens_per_sub: u64,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        SubAgentConfig {
            max_concurrent: 3,
            max_depth: 5,
            max_tokens_per_sub: 100_000,
        }
    }
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

/// Manages sub-agent execution.
pub struct SubAgentManager {
    config: SubAgentConfig,
    agents: Arc<Mutex<HashMap<String, SubAgent>>>,
    parent_children: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl SubAgentManager {
    pub fn new(config: SubAgentConfig) -> Self {
        SubAgentManager {
            config,
            agents: Arc::new(Mutex::new(HashMap::new())),
            parent_children: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a new sub-agent with independent provider/key/model identity.
    /// Does **not** inherit parent key or permission profile.
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
        if provider_id.trim().is_empty() || key_id.trim().is_empty() || model_id.trim().is_empty() {
            return Err(
                "Subagent requires independent provider_id + key_id + model_id".into(),
            );
        }
        // Check depth limit
        if depth > self.config.max_depth {
            return Err(format!(
                "Max sub-agent depth ({}) exceeded: {}",
                self.config.max_depth, depth
            ));
        }

        // Check concurrency limit
        let mut agents = self.agents.lock().await;
        let running_count = agents.values().filter(|a| a.status == SubAgentStatus::Running).count() as u32;
        if running_count >= self.config.max_concurrent {
            return Err(format!(
                "Max concurrent sub-agents ({}) reached",
                self.config.max_concurrent
            ));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let run_id = uuid::Uuid::new_v4().to_string();
        let sub = SubAgent {
            id: id.clone(),
            run_id,
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
    pub async fn cascade_cancel_metadata(&self, parent_run_id: &str) -> usize {
        let descendants = self.list_descendants(parent_run_id).await;
        let mut agents = self.agents.lock().await;
        let mut count = 0usize;
        for d in descendants {
            if let Some(agent) = agents.get_mut(&d.id) {
                if !matches!(
                    agent.status,
                    SubAgentStatus::Completed | SubAgentStatus::Failed(_) | SubAgentStatus::Cancelled
                ) {
                    agent.status = SubAgentStatus::Cancelled;
                    count += 1;
                }
            }
        }
        count
    }

    /// Backward-compatible alias — metadata only (engines are cancelled by cancel_run_tree).
    pub async fn cascade_cancel(&self, parent_run_id: &str) -> usize {
        self.cascade_cancel_metadata(parent_run_id).await
    }

    /// Update sub-agent status.
    pub async fn update_status(&self, id: &str, status: SubAgentStatus) -> Result<(), String> {
        let mut agents = self.agents.lock().await;
        if let Some(agent) = agents.get_mut(id) {
            agent.status = status;
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
        agents.values().filter(|a| a.status == SubAgentStatus::Running).count()
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
        let sub = spawn_default(&manager, "parent-1", "Test task", 1).await.unwrap();
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
        let mut config = SubAgentConfig::default();
        config.max_concurrent = 1;
        let manager = SubAgentManager::new(config);

        // Spawn first sub-agent
        let sub1 = spawn_default(&manager, "parent-1", "Task 1", 1).await.unwrap();
        manager.update_status(&sub1.id, SubAgentStatus::Running).await.unwrap();

        // Second spawn should fail due to concurrency limit
        let result = spawn_default(&manager, "parent-1", "Task 2", 1).await;
        assert!(result.is_err(), "Should reject concurrent sub-agent");
    }

    #[tokio::test]
    async fn test_cascade_cancel() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let a = spawn_default(&manager, "parent-1", "A", 1).await.unwrap();
        let b = spawn_default(&manager, "parent-1", "B", 1).await.unwrap();
        manager.update_status(&a.id, SubAgentStatus::Running).await.unwrap();
        manager.update_status(&b.id, SubAgentStatus::Running).await.unwrap();
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
        spawn_default(&manager, "parent-1", "Child 1", 1).await.unwrap();
        spawn_default(&manager, "parent-1", "Child 2", 1).await.unwrap();

        let children = manager.get_children("parent-1").await;
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_parent_recovery_after_child_failure() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let sub = spawn_default(&manager, "parent-1", "Failing task", 1).await.unwrap();

        // Simulate child failure
        manager.update_status(&sub.id, SubAgentStatus::Failed("Error".to_string())).await.unwrap();

        // Parent can still spawn new children
        let new_sub = spawn_default(&manager, "parent-1", "Recovery task", 1).await;
        assert!(new_sub.is_ok(), "Parent should recover after child failure");
    }

    #[tokio::test]
    async fn test_list_sub_agents() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        spawn_default(&manager, "parent-1", "Task 1", 1).await.unwrap();
        spawn_default(&manager, "parent-1", "Task 2", 1).await.unwrap();
        assert_eq!(manager.list().await.len(), 2);
    }

    #[test]
    fn test_cap_child_permission_never_upgrades() {
        // Parent ask: child cannot become full_access.
        assert_eq!(cap_child_permission("ask", "full_access"), "ask");
        assert_eq!(cap_child_permission("readonly", "ask"), "readonly");
        assert_eq!(cap_child_permission("readonly", "full_access"), "readonly");
        // Parent full_access: explicit full_access request allowed; default ask stays ask.
        assert_eq!(cap_child_permission("full_access", "full_access"), "full_access");
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
        assert!(!list.iter().any(|t| t == "write_file" || t == "task" || t == "run_terminal"));
    }
}
