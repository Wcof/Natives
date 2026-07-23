//! Sub-agent / task output supervision state (task-01 / task-11).

use agent_core::{SubAgentConfig, SubAgentManager};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskRecord {
    pub run_id: String,
    pub status: String,
    pub output: Option<String>,
}

pub struct TaskSupervisor {
    pub subagents: Arc<SubAgentManager>,
    pub task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
}

impl TaskSupervisor {
    pub fn new() -> Self {
        Self {
            subagents: Arc::new(SubAgentManager::new(SubAgentConfig::default())),
            task_outputs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_config(config: SubAgentConfig) -> Self {
        Self {
            subagents: Arc::new(SubAgentManager::new(config)),
            task_outputs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for TaskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
