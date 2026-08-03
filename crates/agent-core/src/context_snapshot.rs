//! Explicit separation between durable history and the bounded provider view.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub message_id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FullHistory {
    pub entries: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActiveContext {
    pub entries: Vec<HistoryEntry>,
    pub estimated_tokens: u64,
    pub provider_context_window: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveContextSnapshot {
    pub snapshot_id: String,
    pub run_id: String,
    pub source_history_revision: u64,
    pub created_at: String,
    pub context: ActiveContext,
    pub compaction_artifact_id: Option<String>,
}

impl ActiveContextSnapshot {
    pub fn new(run_id: impl Into<String>, revision: u64, context: ActiveContext) -> Self {
        Self {
            snapshot_id: uuid::Uuid::new_v4().to_string(),
            run_id: run_id.into(),
            source_history_revision: revision,
            created_at: chrono::Utc::now().to_rfc3339(),
            context,
            compaction_artifact_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips_without_losing_history_metadata() {
        let snapshot = ActiveContextSnapshot::new(
            "run-1",
            7,
            ActiveContext {
                entries: vec![HistoryEntry {
                    message_id: "m-1".into(),
                    role: "user".into(),
                    content: "hello".into(),
                    tool_call_id: None,
                }],
                estimated_tokens: 12,
                provider_context_window: Some(128_000),
            },
        );
        let decoded: ActiveContextSnapshot =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        assert_eq!(decoded.run_id, "run-1");
        assert_eq!(decoded.source_history_revision, 7);
        assert_eq!(decoded.context.entries[0].content, "hello");
    }
}
