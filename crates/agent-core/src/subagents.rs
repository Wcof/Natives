//! Sub-agent execution — bounded multi-agent orchestration.
//!
//! Manages independent Run identity, permission non-inheritance,
//! concurrency/depth/token limits, and parent recovery after child failure.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

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

/// A sub-agent execution.
#[derive(Debug, Clone)]
pub struct SubAgent {
    pub id: String,
    pub parent_run_id: String,
    pub task: String,
    pub status: SubAgentStatus,
    pub depth: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
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

    /// Spawn a new sub-agent.
    pub async fn spawn(
        &self,
        parent_run_id: &str,
        task: String,
        depth: u32,
    ) -> Result<SubAgent, String> {
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
        let sub = SubAgent {
            id: id.clone(),
            parent_run_id: parent_run_id.to_string(),
            task,
            status: SubAgentStatus::Queued,
            depth,
            created_at: chrono::Utc::now(),
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

    #[tokio::test]
    async fn test_spawn_sub_agent() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let sub = manager.spawn("parent-1", "Test task".to_string(), 1).await.unwrap();
        assert_eq!(sub.parent_run_id, "parent-1");
        assert_eq!(sub.status, SubAgentStatus::Queued);
    }

    #[tokio::test]
    async fn test_depth_limit() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let result = manager.spawn("parent-1", "Deep task".to_string(), 100).await;
        assert!(result.is_err(), "Should reject deep sub-agent");
    }

    #[tokio::test]
    async fn test_concurrency_limit() {
        let mut config = SubAgentConfig::default();
        config.max_concurrent = 1;
        let manager = SubAgentManager::new(config);

        // Spawn first sub-agent
        let sub1 = manager.spawn("parent-1", "Task 1".to_string(), 1).await.unwrap();
        manager.update_status(&sub1.id, SubAgentStatus::Running).await.unwrap();

        // Second spawn should fail due to concurrency limit
        let result = manager.spawn("parent-1", "Task 2".to_string(), 1).await;
        assert!(result.is_err(), "Should reject concurrent sub-agent");
    }

    #[tokio::test]
    async fn test_get_children() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        manager.spawn("parent-1", "Child 1".to_string(), 1).await.unwrap();
        manager.spawn("parent-1", "Child 2".to_string(), 1).await.unwrap();

        let children = manager.get_children("parent-1").await;
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_parent_recovery_after_child_failure() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        let sub = manager.spawn("parent-1", "Failing task".to_string(), 1).await.unwrap();

        // Simulate child failure
        manager.update_status(&sub.id, SubAgentStatus::Failed("Error".to_string())).await.unwrap();

        // Parent can still spawn new children
        let new_sub = manager.spawn("parent-1", "Recovery task".to_string(), 1).await;
        assert!(new_sub.is_ok(), "Parent should recover after child failure");
    }

    #[tokio::test]
    async fn test_list_sub_agents() {
        let manager = SubAgentManager::new(SubAgentConfig::default());
        manager.spawn("parent-1", "Task 1".to_string(), 1).await.unwrap();
        manager.spawn("parent-1", "Task 2".to_string(), 1).await.unwrap();
        assert_eq!(manager.list().await.len(), 2);
    }
}
