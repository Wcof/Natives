use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single event within a run's execution timeline.
///
/// Events are monotonically sequenced per run and used for:
/// - Real-time streaming to the UI.
/// - Replay after reconnection (client sends last known sequence).
/// - Persistence and audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub run_id: String,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub payload: RunEventPayload,
}

/// The type of a run event, with its typed payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEventPayload {
    /// Run has been queued.
    Queued,
    /// Run is preparing (assembling context, etc.).
    Preparing,
    /// Run started.
    Started,
    /// Text delta from the model (streaming).
    TextDelta { text: String },
    /// Reasoning delta from the model (streaming).
    ReasoningDelta { text: String },
    /// Model is making a tool call.
    ToolCallRequested {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool call started executing.
    ToolCallStarted {
        id: String,
        name: String,
        started_at: String,
    },
    /// Tool call completed.
    ToolCallCompleted {
        id: String,
        name: String,
        output: serde_json::Value,
        is_error: bool,
        duration_ms: u64,
    },
    /// Permission request for a tool call.
    PermissionRequested {
        tool_call_id: String,
        tool_name: String,
        reason: String,
        permission_id: String,
    },
    /// Permission response received.
    PermissionResponded {
        permission_id: String,
        approved: bool,
        scope: String,
    },
    /// A file was changed (created, modified, deleted).
    FileChanged {
        path: String,
        change_type: String,
        diff: Option<String>,
    },
    /// Usage update.
    UsageUpdated {
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: Option<u64>,
    },
    /// Run completed successfully.
    Completed { reason: String },
    /// Run failed.
    Failed { error: String, code: String },
    /// Run was cancelled/interrupted.
    Interrupted { reason: String },
    /// Run is waiting for permission.
    WaitingPermission,
    /// Context compression occurred.
    ContextCompressed {
        before_tokens: u64,
        after_tokens: u64,
        summary: String,
    },
    /// A checkpoint was created.
    CheckpointCreated { checkpoint_id: String },
    /// A sub-agent was created.
    SubAgentCreated {
        sub_run_id: String,
        task: String,
    },
    /// A sub-agent completed.
    SubAgentCompleted {
        sub_run_id: String,
        result: String,
    },
    /// A sub-agent failed.
    SubAgentFailed {
        sub_run_id: String,
        error: String,
    },
    /// Generic progress update.
    Progress { message: String, percentage: Option<f64> },
    /// Unknown event type (for forward compatibility).
    Unknown { raw: serde_json::Value },
}