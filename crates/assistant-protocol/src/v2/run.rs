//! Run types for Protocol v2.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Run lifecycle status (full remediation state machine).
///
/// ```text
/// created → queued → preparing → running
///   → waiting_permission | waiting_subagent
///   → cancelling → completed | failed | cancelled | interrupted
/// ```
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusV2 {
    /// Allocated but not yet queued (idempotent create).
    Created,
    Queued,
    Preparing,
    Running,
    WaitingPermission,
    /// Parent is blocked on one or more child runs.
    WaitingSubagent,
    Cancelling,
    Completed,
    Failed,
    /// Explicit user/system cancel completed (distinct from crash interrupt).
    Cancelled,
    Interrupted,
}

impl RunStatusV2 {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Preparing
                | Self::Running
                | Self::WaitingPermission
                | Self::WaitingSubagent
                | Self::Cancelling
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Running => "running",
            Self::WaitingPermission => "waiting_permission",
            Self::WaitingSubagent => "waiting_subagent",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    /// Wire helper only — **not** the production state graph.
    ///
    /// Production transition validation lives exclusively in
    /// `agent_core::run_state::transition`. This method is retained for
    /// lightweight UI/schema introspection and may lag the authority; do not
    /// use it to accept or reject daemon commits.
    #[deprecated(
        note = "use agent_core::run_state::transition — protocol no longer owns the state graph"
    )]
    pub fn can_transition_to(self, next: Self) -> bool {
        // Thin mirror kept for wire/UI introspection only. Source of truth:
        // agent_core::run_state. Keep edges in sync when the authority changes.
        use RunStatusV2::*;
        matches!(
            (self, next),
            (Created, Queued)
                | (Created, Cancelling)
                | (Created, Failed)
                | (Queued, Preparing)
                | (Queued, Cancelling)
                | (Queued, Failed)
                | (Preparing, Running)
                | (Preparing, WaitingPermission)
                | (Preparing, Cancelling)
                | (Preparing, Failed)
                | (Preparing, Interrupted)
                | (Running, WaitingPermission)
                | (Running, WaitingSubagent)
                | (Running, Cancelling)
                | (Running, Completed)
                | (Running, Failed)
                | (Running, Interrupted)
                | (WaitingPermission, Running)
                | (WaitingPermission, Cancelling)
                | (WaitingPermission, Completed)
                | (WaitingPermission, Failed)
                | (WaitingPermission, Interrupted)
                | (WaitingSubagent, Running)
                | (WaitingSubagent, Cancelling)
                | (WaitingSubagent, Completed)
                | (WaitingSubagent, Failed)
                | (WaitingSubagent, Interrupted)
                | (Cancelling, Cancelled)
                | (Cancelling, Interrupted)
                | (Cancelling, Failed)
        )
    }
}

/// Full run record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunV2 {
    pub id: String,
    pub conversation_id: String,
    pub status: RunStatusV2,
    pub parent_run_id: Option<String>,
    pub agent_profile_id: Option<String>,
    pub provider_id: String,
    pub key_id: Option<String>,
    pub model_id: String,
    pub permission_profile: String,
    pub trigger_message_id: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error_code: Option<String>,
    pub step_count: u32,
    pub max_steps: u32,
    /// Explicit project root for tools/hooks (never daemon process cwd).
    #[serde(default)]
    pub project_path: Option<String>,
    /// Stable ProjectIdentity UUID (task-10). Path is not identity.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Identity version observed when the run was created/bound.
    #[serde(default)]
    pub project_identity_version: Option<u32>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_event_sequence: u64,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// Optional reasoning / effort level (provider-specific; REQ-E04).
    #[serde(default)]
    pub effort: Option<String>,
    /// Runtime selector: native | claude_cli | codex_cli (REQ-T01/T02).
    #[serde(default)]
    pub runtime_id: Option<String>,
    /// Optimistic-concurrency revision for CAS commits (task-02).
    /// Incremented on every successful status transition via RunManager.
    #[serde(default)]
    pub revision: u64,
    /// Resolved capability snapshot persisted at run start (ADR-0016).
    /// Audit-facing: ids and schema names only, never secrets or config bodies.
    #[serde(default)]
    pub capability_snapshot: Option<serde_json::Value>,
    /// Lineage for retry/continue/fork/resume operations. These are additive
    /// and intentionally nullable: legacy runs remain valid and never inherit
    /// a credential lease or an unfinished future.
    #[serde(default)]
    pub retry_of_run_id: Option<String>,
    #[serde(default)]
    pub retry_of_turn_id: Option<String>,
    #[serde(default)]
    pub continued_from_run_id: Option<String>,
    #[serde(default)]
    pub branch_id: Option<String>,
    #[serde(default)]
    pub branch_parent_message_id: Option<String>,
    #[serde(default)]
    pub checkpoint_id: Option<String>,
    #[serde(default)]
    pub resume_of_run_id: Option<String>,
}

/// Capability selection for a run: which library-managed skills, MCP servers
/// and expert (or expert team) the engine must load (ADR-0016).
///
/// Wire semantics: every field absent (`None`) = legacy behaviour (global skill
/// injection, no MCP visibility change, no profile). `Some([])` = explicitly none.
/// `expert_id` and `team_id` are mutually exclusive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expert_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
}

impl CapabilitySelection {
    /// True when no field carries an explicit selection (legacy behaviour).
    pub fn is_empty(&self) -> bool {
        self.skills.is_none()
            && self.mcp_servers.is_none()
            && self.expert_id.is_none()
            && self.team_id.is_none()
    }

    /// expert_id and team_id are mutually exclusive on the wire.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.expert_id.is_some() && self.team_id.is_some() {
            return Err("capability_selection: expert_id and team_id are mutually exclusive");
        }
        Ok(())
    }
}

/// Create a run without starting it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunRequest {
    pub conversation_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub key_id: Option<String>,
    pub agent_profile_id: Option<String>,
    pub permission_profile: Option<String>,
    pub content: Option<String>,
    pub attachments: Option<Vec<AttachmentRef>>,
    pub max_steps: Option<u32>,
    pub parent_run_id: Option<String>,
    /// Workspace / project root for hooks, context, and tool sandbox (explicit, not process cwd).
    #[serde(default)]
    pub project_path: Option<String>,
    /// Client-supplied idempotency key for retries of the same create.
    pub idempotency_key: Option<String>,
    /// Optional reasoning / effort level.
    #[serde(default)]
    pub effort: Option<String>,
    /// Runtime selector: native | claude_cli | codex_cli.
    #[serde(default)]
    pub runtime_id: Option<String>,
    /// Capability library selection (ADR-0016). None = legacy behaviour.
    #[serde(default)]
    pub capability_selection: Option<CapabilitySelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub path: String,
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
}

/// Start a previously created (or create+start) run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRunRequest {
    pub run_id: Option<String>,
    pub conversation_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub key_id: Option<String>,
    pub content: Option<String>,
    pub attachments: Option<Vec<AttachmentRef>>,
    pub trigger_message_id: Option<String>,
    pub permission_profile: Option<String>,
    pub max_steps: Option<u32>,
    /// Workspace / project root for hooks, context, and tool sandbox.
    #[serde(default)]
    pub project_path: Option<String>,
    pub idempotency_key: Option<String>,
    /// Optional reasoning / effort level for this run.
    #[serde(default)]
    pub effort: Option<String>,
    /// Runtime selector: native | claude_cli | codex_cli.
    #[serde(default)]
    pub runtime_id: Option<String>,
    /// Agent profile / expert to run as (ADR-0016 seam A: previously only on
    /// CreateRunRequest; StartRunRequest now carries it end-to-end).
    #[serde(default)]
    pub agent_profile_id: Option<String>,
    /// Capability library selection override for this run (ADR-0016).
    /// None = fall back to the conversation-level default selection.
    #[serde(default)]
    pub capability_selection: Option<CapabilitySelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRunRequest {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryRunRequest {
    pub run_id: String,
}

/// Continue a run from a durable checkpoint/context snapshot. The new run is
/// always independent; no provider/tool future from the source is revived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueRunRequest {
    pub run_id: String,
    #[serde(default)]
    pub checkpoint_id: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

/// Resume decision for `run.resume`. The builder reads the source run's
/// checkpoint, active context snapshot, and side-effect ledger and classifies
/// the restore before any run is created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResumeDecision {
    /// The restore is safe to proceed; a new independent run is created.
    SafeToContinue,
    /// Uncertain side effects exist and the caller has not confirmed them.
    /// No run is created and no provider/tool invocation happens until the
    /// caller re-invokes `run.resume` with `confirmed: true`.
    ConfirmationRequired,
    /// A hard blocker: the ledger contains an uncertain side effect that is not
    /// replay-safe, so resuming would re-run an effect whose outcome is unknown.
    /// No run is created; the blocker cannot be waived by confirmation.
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRunRequest {
    pub run_id: String,
    #[serde(default)]
    pub checkpoint_id: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    /// When `true` the caller has explicitly accepted any uncertain side
    /// effects that are replay-safe; never set this automatically.
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRunResponse {
    pub decision: ResumeDecision,
    pub reason: String,
    #[serde(default)]
    pub unresolved_effects: Vec<serde_json::Value>,
    /// Present only when a new independent run was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayRunRequest {
    pub run_id: String,
    pub after_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRunRequest {
    pub run_id: String,
    pub after_sequence: u64,
}
