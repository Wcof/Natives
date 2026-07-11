use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Run status — the state machine of a single agent execution.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Preparing,
    Running,
    WaitingPermission,
    Cancelling,
    Completed,
    Failed,
    Interrupted,
}

/// A single execution run (agent loop iteration) within a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub conversation_id: String,
    pub status: RunStatus,
    pub trigger_message_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub runtime_id: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error_code: Option<String>,
    pub step_count: Option<u32>,
    pub max_steps: Option<u32>,
    pub token_budget: Option<u64>,
    pub total_input_tokens: Option<u64>,
    pub total_output_tokens: Option<u64>,
}

/// Input for starting a new run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRunInput {
    pub conversation_id: String,
    pub trigger_message_id: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub max_steps: Option<u32>,
    pub max_duration_secs: Option<u64>,
    pub token_budget: Option<u64>,
    pub permission_profile: Option<PermissionProfile>,
}

/// Permission profile for a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionProfile {
    /// Every side-effect tool call requires approval.
    ConfirmEach,
    /// Auto-execute within project sandbox and policy.
    Autonomous,
}

impl RunStatus {
    /// Returns true if the status is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, RunStatus::Completed | RunStatus::Failed | RunStatus::Interrupted)
    }

    /// Returns true if the status is an active (non-terminal, non-queued) state.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            RunStatus::Preparing | RunStatus::Running | RunStatus::WaitingPermission | RunStatus::Cancelling
        )
    }

    /// Validates whether a transition from `self` to `next` is allowed.
    pub fn can_transition_to(&self, next: &RunStatus) -> bool {
        use RunStatus::*;
        match (self, next) {
            // Initial
            (Queued, Preparing) => true,
            (Queued, Cancelling) => true,
            (Queued, Failed) => true,
            // Preparing
            (Preparing, Running) => true,
            (Preparing, WaitingPermission) => true,
            (Preparing, Cancelling) => true,
            (Preparing, Failed) => true,
            (Preparing, Interrupted) => true,
            // Running
            (Running, WaitingPermission) => true,
            (Running, Cancelling) => true,
            (Running, Completed) => true,
            (Running, Failed) => true,
            (Running, Interrupted) => true,
            // WaitingPermission
            (WaitingPermission, Running) => true,
            (WaitingPermission, Cancelling) => true,
            (WaitingPermission, Completed) => true,
            (WaitingPermission, Failed) => true,
            (WaitingPermission, Interrupted) => true,
            // Cancelling
            (Cancelling, Interrupted) => true,
            (Cancelling, Failed) => true,
            // All other transitions are invalid
            _ => false,
        }
    }
}