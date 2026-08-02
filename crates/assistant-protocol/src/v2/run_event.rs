//! Unified RunEvent model — single event authority for UI + persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single event within a run's execution timeline.
///
/// Rules:
/// - `event_id` is a stable UUID/ULID idempotency key (`UNIQUE`).
/// - `global_sequence` is SQLite AUTOINCREMENT (`run_event.id`) — authority-wide.
/// - `run_sequence` (wire: also accept legacy `sequence`) is per-run, starting at 1.
/// - Events are persisted before being pushed to subscribers.
/// - Payloads must never contain API keys, Authorization headers, or full raw
///   provider response bodies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunEventV2 {
    /// Stable idempotency key. Empty only for pre-migration synthetic rows.
    #[serde(default)]
    pub event_id: String,
    /// Authority-wide strictly increasing sequence (SQLite `run_event.id`).
    #[serde(default)]
    pub global_sequence: u64,
    pub run_id: String,
    /// Per-run strictly increasing sequence (preferred wire name).
    #[serde(default)]
    pub run_sequence: u64,
    /// Deprecated wire alias of [`Self::run_sequence`]. Still serialized so
    /// older clients can read `sequence`; new producers should set both equal.
    #[serde(default)]
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub payload: RunEventKind,
}

impl RunEventV2 {
    /// Effective per-run sequence (prefers `run_sequence`, falls back to legacy `sequence`).
    pub fn effective_run_sequence(&self) -> u64 {
        if self.run_sequence > 0 {
            self.run_sequence
        } else {
            self.sequence
        }
    }

    /// Keep dual-cursor fields aligned after assignment.
    pub fn set_run_sequence(&mut self, seq: u64) {
        self.run_sequence = seq;
        self.sequence = seq;
    }
}

/// Discriminated event payload for Protocol v2.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEventKind {
    Queued,
    Preparing,
    Started,
    HookInvocationStarted {
        invocation_id: String,
        hook_id: String,
        hook_event: String,
        source: String,
        ordinal: u32,
        input_summary: String,
        input_truncated: bool,
    },
    HookInvocationCompleted {
        invocation_id: String,
        hook_id: String,
        hook_event: String,
        source: String,
        ordinal: u32,
        status: String,
        effective_decision: Option<String>,
        error_category: Option<String>,
        duration_ms: u64,
        output_summary: String,
        output_truncated: bool,
    },
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCallRequested {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Prepared after JSON/schema/hook checks and before permission/handler.
    ToolCallPrepared {
        id: String,
        name: String,
        input: serde_json::Value,
        execution_mode: String,
        side_effect: String,
    },
    ToolCallStarted {
        id: String,
        name: String,
    },
    /// Streaming tool-call argument fragment (OpenAI-style index/id/name/delta).
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCallCompleted {
        id: String,
        name: String,
        output: serde_json::Value,
        is_error: bool,
        duration_ms: u64,
    },
    /// Incremental tool/process output (terminal stdout/stderr). Batched ~250ms / 8KB.
    ToolOutputDelta {
        tool_call_id: String,
        stream: String,
        text: String,
        truncated: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress_sequence: Option<u64>,
    },
    PermissionRequested {
        tool_call_id: String,
        tool_name: String,
        reason: String,
        permission_id: String,
        input: serde_json::Value,
    },
    PermissionResponded {
        permission_id: String,
        approved: bool,
        scope: String,
    },
    /// The run's Plan Mode latch moved.
    ///
    /// Without this the gear shift is only inferable from the `enter_plan_mode` /
    /// `exit_plan_mode` tool calls around it, which puts the single most
    /// consequential fact about a run — whether it is currently allowed to touch
    /// anything — behind a tool-call reading exercise.
    ///
    /// Every field but `transition` and `effective_profile` is optional so a
    /// producer can emit the cheap transitions without assembling a payload, and
    /// so this variant can grow without invalidating stored events.
    PlanModeChanged {
        /// `entered` | `submitted` | `approved` | `rejected` | `cleared`.
        ///
        /// A transition, not a state: `submitted` and `rejected` both leave the
        /// latch closed, and the difference between them is the whole story.
        transition: String,
        /// Permission profile in force *after* this transition — `plan` while the
        /// latch is closed, otherwise the profile the run reverts to.
        effective_profile: String,
        /// The submitted plan, carried on `submitted` / `approved` / `rejected`
        /// so the timeline can show what was actually agreed to without holding
        /// a reference to the approval card that has since been dismissed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan: Option<serde_json::Value>,
        /// Running count of rejected plans. A climbing number is the visible
        /// shape of a model stuck re-proposing the same thing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rejections: Option<u32>,
        /// Why the transition happened — the model's stated reason on `entered`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    FileChanged {
        path: String,
        change_type: String,
        /// Optional pre-image for DiffViewer / rewind (empty string for creates).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
        /// Optional post-image after the tool write.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff_artifact_id: Option<String>,
    },
    TaskStarted {
        task_id: String,
        run_id: String,
        label: String,
    },
    TaskUpdated {
        task_id: String,
        status: String,
        detail: Option<String>,
    },
    TaskCompleted {
        task_id: String,
        exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact_id: Option<String>,
    },
    InteractionRequested {
        interaction_id: String,
        kind: String,
        payload: serde_json::Value,
    },
    InteractionResponded {
        interaction_id: String,
        response: serde_json::Value,
    },
    CheckpointCreated {
        checkpoint_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    CheckpointRewound {
        checkpoint_id: String,
        paths: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conflict_policy: Option<String>,
    },
    ContextUsageUpdated {
        used_tokens: u64,
        max_tokens: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        breakdown: Option<serde_json::Value>,
    },
    UsageUpdated {
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: Option<u64>,
        /// Tokens written into the provider's prompt cache this turn.
        ///
        /// `None` means the provider did not report the figure, which is not
        /// the same as a reported zero — replayed events persisted before
        /// cache reporting existed decode as `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_creation_tokens: Option<u64>,
        /// Tokens served from the provider's prompt cache this turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_read_tokens: Option<u64>,
    },
    ContextCompressed {
        before_tokens: u64,
        after_tokens: u64,
        summary: String,
    },
    /// Durable active-context snapshot committed after compaction.
    ContextSnapshotCommitted {
        snapshot_id: String,
        turn_id: Option<String>,
        source_revision: u64,
        input_message_ids: Vec<String>,
        summary_message_id: Option<String>,
        replaced_range: Option<String>,
        algorithm_version: String,
        provider_context_window: Option<u64>,
        artifact_reference: Option<String>,
        #[serde(default)]
        snapshot_json: serde_json::Value,
    },
    SubagentCreated {
        sub_run_id: String,
        agent_profile_id: Option<String>,
        task: String,
    },
    SubagentCompleted {
        sub_run_id: String,
        result: String,
    },
    SubagentFailed {
        sub_run_id: String,
        error: String,
    },
    Progress {
        message: String,
        percentage: Option<f64>,
    },
    GenerationAttemptStarted {
        attempt: u32,
        max_attempts: u32,
    },
    GenerationAttemptFailed {
        attempt: u32,
        code: String,
        retryable: bool,
        retrying: bool,
        /// Backoff the engine will actually wait before the next attempt, in
        /// milliseconds — already merged with any provider `Retry-After` and
        /// clamped. `None` when there is no next attempt, and for producers
        /// that predate the field.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_in_ms: Option<u64>,
    },
    GenerationAttemptDiscarded {
        attempt: u32,
        reason: String,
    },
    GenerationAttemptCommitted {
        attempt: u32,
    },
    /// Additive execution lifecycle facts. These are deliberately distinct
    /// from Run terminal events so a replay can reconstruct provider turns.
    TurnStarted {
        turn_id: String,
    },
    TurnCompleted {
        turn_id: String,
        stop_reason: String,
        input_tokens: u64,
        output_tokens: u64,
    },
    MessageStarted {
        turn_id: String,
        message_id: String,
        role: String,
    },
    MessageDelta {
        turn_id: String,
        message_id: String,
        text: String,
    },
    MessageCompleted {
        turn_id: String,
        message_id: String,
        role: String,
    },
    Completed {
        reason: String,
    },
    Failed {
        error: String,
        code: String,
    },
    Cancelled {
        reason: String,
    },
    Interrupted {
        reason: String,
    },
    Unknown {
        raw: serde_json::Value,
    },
}

impl RunEventKind {
    /// Stable wire name for storage columns.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Started => "started",
            Self::HookInvocationStarted { .. } => "hook_invocation_started",
            Self::HookInvocationCompleted { .. } => "hook_invocation_completed",
            Self::TextDelta { .. } => "text_delta",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::ToolCallRequested { .. } => "tool_call_requested",
            Self::ToolCallPrepared { .. } => "tool_call_prepared",
            Self::ToolCallStarted { .. } => "tool_call_started",
            Self::ToolCallDelta { .. } => "tool_call_delta",
            Self::ToolCallCompleted { .. } => "tool_call_completed",
            Self::ToolOutputDelta { .. } => "tool_output_delta",
            Self::PermissionRequested { .. } => "permission_requested",
            Self::PermissionResponded { .. } => "permission_responded",
            Self::PlanModeChanged { .. } => "plan_mode_changed",
            Self::FileChanged { .. } => "file_changed",
            Self::TaskStarted { .. } => "task_started",
            Self::TaskUpdated { .. } => "task_updated",
            Self::TaskCompleted { .. } => "task_completed",
            Self::InteractionRequested { .. } => "interaction_requested",
            Self::InteractionResponded { .. } => "interaction_responded",
            Self::CheckpointCreated { .. } => "checkpoint_created",
            Self::CheckpointRewound { .. } => "checkpoint_rewound",
            Self::ContextUsageUpdated { .. } => "context_usage_updated",
            Self::UsageUpdated { .. } => "usage_updated",
            Self::ContextCompressed { .. } => "context_compressed",
            Self::ContextSnapshotCommitted { .. } => "context_snapshot_committed",
            Self::SubagentCreated { .. } => "subagent_created",
            Self::SubagentCompleted { .. } => "subagent_completed",
            Self::SubagentFailed { .. } => "subagent_failed",
            Self::Progress { .. } => "progress",
            Self::GenerationAttemptStarted { .. } => "generation_attempt_started",
            Self::GenerationAttemptFailed { .. } => "generation_attempt_failed",
            Self::GenerationAttemptDiscarded { .. } => "generation_attempt_discarded",
            Self::GenerationAttemptCommitted { .. } => "generation_attempt_committed",
            Self::TurnStarted { .. } => "turn_started",
            Self::TurnCompleted { .. } => "turn_completed",
            Self::MessageStarted { .. } => "message_started",
            Self::MessageDelta { .. } => "message_delta",
            Self::MessageCompleted { .. } => "message_completed",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
            Self::Cancelled { .. } => "cancelled",
            Self::Interrupted { .. } => "interrupted",
            Self::Unknown { .. } => "unknown",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. }
                | Self::Failed { .. }
                | Self::Cancelled { .. }
                | Self::Interrupted { .. }
        )
    }
}

impl RunEventV2 {
    pub fn new(run_id: impl Into<String>, run_sequence: u64, payload: RunEventKind) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            global_sequence: 0,
            run_id: run_id.into(),
            run_sequence,
            sequence: run_sequence,
            timestamp: Utc::now(),
            payload,
        }
    }

    pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self {
        self.event_id = event_id.into();
        self
    }

    pub fn with_global_sequence(mut self, global_sequence: u64) -> Self {
        self.global_sequence = global_sequence;
        self
    }
}

/// Redact secrets from free-form error/debug strings before they enter events.
pub fn redact_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &input[i..];
        let lower = rest.to_ascii_lowercase();
        let redact_from = if lower.starts_with("authorization:") {
            Some(("Authorization:".len(), true))
        } else if lower.starts_with("bearer ") {
            Some(("Bearer ".len(), true))
        } else if lower.starts_with("api_key=") || lower.starts_with("api-key=") {
            Some((8, true))
        } else if rest.starts_with("sk-ant-") {
            Some(("sk-ant-".len(), false))
        } else if rest.starts_with("sk-") {
            Some(("sk-".len(), false))
        } else {
            None
        };

        if let Some((prefix_len, stop_at_ws)) = redact_from {
            out.push_str(&input[i..i + prefix_len]);
            i += prefix_len;
            if stop_at_ws {
                // skip spaces then secret token
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            while i < bytes.len() {
                let c = bytes[i] as char;
                if stop_at_ws {
                    if c.is_whitespace() || c == '"' || c == '\'' {
                        break;
                    }
                } else if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    break;
                }
                i += 1;
            }
            out.push_str("[REDACTED]");
            continue;
        }

        out.push(input[i..].chars().next().unwrap_or('?'));
        i += input[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_all_core_event_kinds() {
        let kinds = vec![
            RunEventKind::Queued,
            RunEventKind::Preparing,
            RunEventKind::Started,
            RunEventKind::TextDelta { text: "hi".into() },
            RunEventKind::ReasoningDelta {
                text: "think".into(),
            },
            RunEventKind::ToolCallDelta {
                index: 0,
                id: Some("c1".into()),
                name: Some("read_file".into()),
                arguments_delta: "{\"p".into(),
            },
            RunEventKind::UsageUpdated {
                input_tokens: 1,
                output_tokens: 2,
                reasoning_tokens: Some(3),
                cache_creation_tokens: Some(4),
                cache_read_tokens: Some(5),
            },
            RunEventKind::FileChanged {
                path: "a.rs".into(),
                change_type: "modified".into(),
                before: Some("old".into()),
                after: Some("new".into()),
                before_hash: Some("h1".into()),
                after_hash: Some("h2".into()),
                diff_artifact_id: None,
            },
            RunEventKind::ContextUsageUpdated {
                used_tokens: 100,
                max_tokens: 128000,
                breakdown: Some(serde_json::json!({"message": 80, "tool": 20})),
            },
            RunEventKind::CheckpointCreated {
                checkpoint_id: "cp-1".into(),
                label: Some("run_start".into()),
            },
            RunEventKind::CheckpointRewound {
                checkpoint_id: "cp-1".into(),
                paths: vec!["a.rs".into()],
                conflict_policy: Some("fail".into()),
            },
            RunEventKind::GenerationAttemptStarted {
                attempt: 1,
                max_attempts: 3,
            },
            RunEventKind::GenerationAttemptFailed {
                attempt: 1,
                code: "http_503".into(),
                retryable: true,
                retrying: true,
                retry_in_ms: Some(1_500),
            },
            RunEventKind::GenerationAttemptDiscarded {
                attempt: 1,
                reason: "partial_stream_failed".into(),
            },
            RunEventKind::GenerationAttemptCommitted { attempt: 2 },
            RunEventKind::Completed {
                reason: "ok".into(),
            },
            RunEventKind::Failed {
                error: "boom".into(),
                code: "E".into(),
            },
            RunEventKind::Interrupted {
                reason: "cancelled".into(),
            },
        ];
        for kind in kinds {
            let event = RunEventV2::new("run-1", 1, kind.clone());
            let json = serde_json::to_string(&event).unwrap();
            let back: RunEventV2 = serde_json::from_str(&json).unwrap();
            assert_eq!(back.payload.type_name(), kind.type_name());
            assert!(!json.to_lowercase().contains("authorization"));
        }
    }

    #[test]
    fn plan_mode_changed_round_trips_and_omits_absent_fields() {
        let full = RunEventKind::PlanModeChanged {
            transition: "submitted".into(),
            effective_profile: "plan".into(),
            plan: Some(serde_json::json!({"title": "t", "steps": []})),
            rejections: Some(2),
            reason: Some("touches the migration".into()),
        };
        let json = serde_json::to_string(&full).unwrap();
        assert!(json.contains("\"type\":\"plan_mode_changed\""));
        assert_eq!(
            serde_json::from_str::<RunEventKind>(&json).unwrap(),
            full,
            "full payload must survive a round trip"
        );

        // The cheap transitions carry nothing extra and must not serialize nulls.
        let bare = RunEventKind::PlanModeChanged {
            transition: "entered".into(),
            effective_profile: "plan".into(),
            plan: None,
            rejections: None,
            reason: None,
        };
        let bare_json = serde_json::to_string(&bare).unwrap();
        for absent in ["\"plan\":", "\"rejections\":", "\"reason\":", "null"] {
            assert!(!bare_json.contains(absent), "{absent} in {bare_json}");
        }
    }

    /// A stored event written before the optional fields existed must still
    /// decode. This is the whole reason they are `#[serde(default)]`.
    #[test]
    fn plan_mode_changed_decodes_a_minimal_stored_payload() {
        let stored =
            r#"{"type":"plan_mode_changed","transition":"approved","effective_profile":"ask"}"#;
        let decoded: RunEventKind = serde_json::from_str(stored).unwrap();
        assert_eq!(decoded.type_name(), "plan_mode_changed");
        assert!(!decoded.is_terminal());
        match decoded {
            RunEventKind::PlanModeChanged {
                transition,
                effective_profile,
                plan,
                rejections,
                reason,
            } => {
                assert_eq!(transition, "approved");
                assert_eq!(effective_profile, "ask");
                assert!(plan.is_none() && rejections.is_none() && reason.is_none());
            }
            other => panic!("expected plan_mode_changed, got {}", other.type_name()),
        }
    }

    #[test]
    fn redacts_api_keys_from_error_text() {
        let raw = "upstream failed Authorization: Bearer sk-abc1234567890 secret";
        let cleaned = redact_secrets(raw);
        assert!(!cleaned.contains("sk-abc"));
        assert!(!cleaned.contains("Bearer sk-"));
        assert!(cleaned.contains("REDACTED"));
    }
}
