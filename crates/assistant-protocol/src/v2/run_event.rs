//! Unified RunEvent model — single event authority for UI + persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single event within a run's execution timeline.
///
/// Rules:
/// - Each run has a strictly increasing `sequence` starting at 1.
/// - Events are persisted before being pushed to subscribers.
/// - Payloads must never contain API keys, Authorization headers, or full raw
///   provider response bodies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunEventV2 {
    pub run_id: String,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub payload: RunEventKind,
}

/// Discriminated event payload for Protocol v2.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEventKind {
    Queued,
    Preparing,
    Started,
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
    FileChanged {
        path: String,
        change_type: String,
    },
    UsageUpdated {
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: Option<u64>,
    },
    ContextCompressed {
        before_tokens: u64,
        after_tokens: u64,
        summary: String,
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
    },
    GenerationAttemptDiscarded {
        attempt: u32,
        reason: String,
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
            Self::TextDelta { .. } => "text_delta",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::ToolCallRequested { .. } => "tool_call_requested",
            Self::ToolCallStarted { .. } => "tool_call_started",
            Self::ToolCallDelta { .. } => "tool_call_delta",
            Self::ToolCallCompleted { .. } => "tool_call_completed",
            Self::PermissionRequested { .. } => "permission_requested",
            Self::PermissionResponded { .. } => "permission_responded",
            Self::FileChanged { .. } => "file_changed",
            Self::UsageUpdated { .. } => "usage_updated",
            Self::ContextCompressed { .. } => "context_compressed",
            Self::SubagentCreated { .. } => "subagent_created",
            Self::SubagentCompleted { .. } => "subagent_completed",
            Self::SubagentFailed { .. } => "subagent_failed",
            Self::Progress { .. } => "progress",
            Self::GenerationAttemptStarted { .. } => "generation_attempt_started",
            Self::GenerationAttemptFailed { .. } => "generation_attempt_failed",
            Self::GenerationAttemptDiscarded { .. } => "generation_attempt_discarded",
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
    pub fn new(run_id: impl Into<String>, sequence: u64, payload: RunEventKind) -> Self {
        Self {
            run_id: run_id.into(),
            sequence,
            timestamp: Utc::now(),
            payload,
        }
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
            RunEventKind::TextDelta {
                text: "hi".into(),
            },
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
            },
            RunEventKind::GenerationAttemptDiscarded {
                attempt: 1,
                reason: "partial_stream_failed".into(),
            },
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
    fn redacts_api_keys_from_error_text() {
        let raw = "upstream failed Authorization: Bearer sk-abc1234567890 secret";
        let cleaned = redact_secrets(raw);
        assert!(!cleaned.contains("sk-abc"));
        assert!(!cleaned.contains("Bearer sk-"));
        assert!(cleaned.contains("REDACTED"));
    }
}
