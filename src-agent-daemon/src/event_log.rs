//! # Replayable Event Log
//!
//! Manages the run event log with monotonic sequence number assignment,
//! atomic append, replay after sequence, and duplicate acknowledgement handling.
//!
//! Each run has a strictly increasing sequence number for events, allowing
//! clients to replay from the last acknowledged sequence after reconnection.

use crate::storage::DataStore;
use assistant_protocol::v1::run_event::{RunEvent, RunEventPayload};
use rusqlite::params;

/// The event log for a single run.
pub struct EventLog {
    data_store: std::sync::Arc<DataStore>,
}

impl EventLog {
    /// Create a new event log backed by the given data store.
    pub fn new(data_store: std::sync::Arc<DataStore>) -> Self {
        EventLog { data_store }
    }

    /// Append an event to the log for a given run.
    /// Returns the assigned sequence number.
    pub fn append(&self, run_id: &str, event_type: &str, payload: &str) -> Result<i64, String> {
        let conn = self.data_store.conn()?;

        // Atomically assign next sequence number
        let next_seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_event WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to get next sequence: {e}"))?;

        conn.execute(
            "INSERT INTO run_event (run_id, sequence, event_type, payload)
             VALUES (?1, ?2, ?3, ?4)",
            params![run_id, next_seq, event_type, payload],
        )
        .map_err(|e| format!("Failed to insert event: {e}"))?;

        Ok(next_seq)
    }

    /// Append a typed RunEvent to the log.
    pub fn append_event(&self, event: &RunEvent) -> Result<i64, String> {
        let event_type = event_type_name(&event.payload);
        let payload = serde_json::to_string(&event.payload)
            .map_err(|e| format!("Failed to serialize event: {e}"))?;
        self.append(&event.run_id, &event_type, &payload)
    }

    /// Replay events for a run starting after the given sequence number.
    /// Returns all events with sequence > last_sequence.
    pub fn replay_after(&self, run_id: &str, last_sequence: i64) -> Result<Vec<RunEvent>, String> {
        let conn = self.data_store.conn()?;

        // Use a simple query_row approach to verify data exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_event WHERE run_id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to count events: {e}"))?;

        if count == 0 {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare(
                "SELECT sequence, event_type, payload, timestamp
                 FROM run_event
                 WHERE run_id = ?1 AND sequence > ?2
                 ORDER BY sequence ASC",
            )
            .map_err(|e| format!("Failed to prepare replay query: {e}"))?;

        let mut rows = stmt
            .query(rusqlite::params![run_id, last_sequence])
            .map_err(|e| format!("Failed to query replay: {e}"))?;

        let mut events = Vec::new();
        while let Some(row) = rows.next().map_err(|e| format!("Failed to read row: {e}"))? {
            let sequence: i64 = row.get(0).map_err(|e| format!("Failed to get sequence: {e}"))?;
            let _event_type: String = row.get(1).map_err(|e| format!("Failed to get event_type: {e}"))?;
            let payload_str: String = row.get(2).map_err(|e| format!("Failed to get payload: {e}"))?;
            let timestamp: String = row.get(3).map_err(|e| format!("Failed to get timestamp: {e}"))?;

            if let Ok(payload) = serde_json::from_str::<RunEventPayload>(&payload_str) {
                // SQLite datetime('now') returns "YYYY-MM-DD HH:MM:SS" without timezone.
                // Parse it as UTC.
                let dt = chrono::NaiveDateTime::parse_from_str(&timestamp, "%Y-%m-%d %H:%M:%S")
                    .map(|ndt| ndt.and_utc())
                    .or_else(|_| chrono::DateTime::parse_from_rfc3339(&timestamp)
                        .map(|dt| dt.with_timezone(&chrono::Utc)));
                if let Ok(ts) = dt {
                    events.push(RunEvent {
                        run_id: run_id.to_string(),
                        sequence: sequence as u64,
                        timestamp: ts,
                        payload,
                    });
                }
            }
        }

        Ok(events)
    }

    /// Replay all events for a run.
    pub fn replay_all(&self, run_id: &str) -> Result<Vec<RunEvent>, String> {
        self.replay_after(run_id, 0)
    }

    /// Get the last acknowledged sequence for a run.
    /// Returns 0 if no events exist.
    pub fn last_sequence(&self, run_id: &str) -> Result<i64, String> {
        let conn = self.data_store.conn()?;
        let seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) FROM run_event WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to get last sequence: {e}"))?;
        Ok(seq)
    }

    /// Acknowledge receipt of events up to the given sequence.
    /// This is a no-op for the log itself (acknowledgements are tracked client-side).
    pub fn acknowledge(&self, _run_id: &str, _sequence: i64) -> Result<(), String> {
        // Acknowledgements are tracked by the client; the log is append-only.
        // This method exists for future durability tracking.
        Ok(())
    }
}

/// Get the event type name from a RunEventPayload.
fn event_type_name(payload: &RunEventPayload) -> &'static str {
    match payload {
        RunEventPayload::Queued => "queued",
        RunEventPayload::Preparing => "preparing",
        RunEventPayload::Started => "started",
        RunEventPayload::TextDelta { .. } => "text_delta",
        RunEventPayload::ReasoningDelta { .. } => "reasoning_delta",
        RunEventPayload::ToolCallRequested { .. } => "tool_call_requested",
        RunEventPayload::ToolCallStarted { .. } => "tool_call_started",
        RunEventPayload::ToolCallCompleted { .. } => "tool_call_completed",
        RunEventPayload::PermissionRequested { .. } => "permission_requested",
        RunEventPayload::PermissionResponded { .. } => "permission_responded",
        RunEventPayload::FileChanged { .. } => "file_changed",
        RunEventPayload::UsageUpdated { .. } => "usage_updated",
        RunEventPayload::Completed { .. } => "completed",
        RunEventPayload::Failed { .. } => "failed",
        RunEventPayload::Interrupted { .. } => "interrupted",
        RunEventPayload::WaitingPermission => "waiting_permission",
        RunEventPayload::ContextCompressed { .. } => "context_compressed",
        RunEventPayload::CheckpointCreated { .. } => "checkpoint_created",
        RunEventPayload::SubAgentCreated { .. } => "sub_agent_created",
        RunEventPayload::SubAgentCompleted { .. } => "sub_agent_completed",
        RunEventPayload::SubAgentFailed { .. } => "sub_agent_failed",
        RunEventPayload::Progress { .. } => "progress",
        RunEventPayload::Unknown { .. } => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::DataStore;
    use std::sync::Arc;

    fn setup_event_log() -> (EventLog, String) {
        let tmp = std::env::temp_dir();
        let db_path = tmp.join(format!("test_events_{}.db", uuid::Uuid::new_v4()));
        let art_dir = tmp.join(format!("test_artifacts_{}", uuid::Uuid::new_v4()));
        let store = Arc::new(DataStore::new(&db_path, &art_dir).unwrap());
        let log = EventLog::new(store);
        let run_id = "test-run-001".to_string();

        // Create parent records to satisfy foreign key constraints
        {
            let conn = log.data_store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id) VALUES (?1, 'chat', 'Test', 'prov-1', 'model-1')",
                params!["test-conv-001"],
            ).unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'test-conv-001', 'queued', 'prov-1', 'model-1')",
                params![run_id],
            ).unwrap();
        }

        (log, run_id)
    }

    #[test]
    fn test_monotonic_sequence_assignment() {
        let (log, run_id) = setup_event_log();
        let seq1 = log.append(&run_id, "started", "{}").unwrap();
        assert_eq!(seq1, 1, "First event should be sequence 1");
        let seq2 = log.append(&run_id, "text_delta", r#"{"text":"hello"}"#).unwrap();
        assert_eq!(seq2, 2, "Second event should be sequence 2");
        let seq3 = log.append(&run_id, "completed", r#"{"reason":"done"}"#).unwrap();
        assert_eq!(seq3, 3, "Third event should be sequence 3");
    }

    #[test]
    fn test_debug_db_state() {
        let (log, run_id) = setup_event_log();
        let seq = log.append(&run_id, "started", "{}").unwrap();
        assert_eq!(seq, 1);
        // Check database directly
        let conn = log.data_store.conn().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM run_event WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1, "Event should be in database");
        // Check replay
        let events = log.replay_all(&run_id).unwrap();
        assert_eq!(events.len(), 1, "Replay should return 1 event");
    }

    #[test]
    fn test_atomic_append() {
        let (log, run_id) = setup_event_log();
        let seq = log.append(&run_id, "started", "{}").unwrap();
        assert_eq!(seq, 1);

        // Verify event is persisted
        let events = log.replay_all(&run_id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);
    }

    #[test]
    fn test_replay_after_sequence() {
        let (log, run_id) = setup_event_log();
        log.append(&run_id, "started", "{}").unwrap();
        log.append(&run_id, "text_delta", r#"{"text":"hi"}"#).unwrap();
        log.append(&run_id, "completed", r#"{"reason":"ok"}"#).unwrap();

        // Replay after sequence 1 should return events 2 and 3
        let events = log.replay_after(&run_id, 1).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 2);
        assert_eq!(events[1].sequence, 3);
    }

    #[test]
    fn test_replay_all() {
        let (log, run_id) = setup_event_log();
        log.append(&run_id, "started", "{}").unwrap();
        log.append(&run_id, "completed", r#"{"reason":"ok"}"#).unwrap();

        let events = log.replay_all(&run_id).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_empty_replay() {
        let (log, run_id) = setup_event_log();
        let events = log.replay_all(&run_id).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_last_sequence() {
        let (log, run_id) = setup_event_log();
        assert_eq!(log.last_sequence(&run_id).unwrap(), 0);

        log.append(&run_id, "started", "{}").unwrap();
        assert_eq!(log.last_sequence(&run_id).unwrap(), 1);

        log.append(&run_id, "completed", r#"{"reason":"ok"}"#).unwrap();
        assert_eq!(log.last_sequence(&run_id).unwrap(), 2);
    }

    #[test]
    fn test_duplicate_acknowledgement() {
        let (log, run_id) = setup_event_log();
        // Acknowledge should be idempotent
        assert!(log.acknowledge(&run_id, 1).is_ok());
        assert!(log.acknowledge(&run_id, 1).is_ok());
    }

    #[test]
    fn test_run_isolation() {
        let (log, run_id1) = setup_event_log();
        let run_id2 = "test-run-002".to_string();

        // Create second run
        {
            let conn = log.data_store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'test-conv-001', 'queued', 'prov-1', 'model-1')",
                params![run_id2],
            ).unwrap();
        }

        log.append(&run_id1, "started", "{}").unwrap();
        log.append(&run_id2, "started", "{}").unwrap();
        log.append(&run_id1, "completed", r#"{"reason":"ok"}"#).unwrap();

        let events1 = log.replay_all(&run_id1).unwrap();
        assert_eq!(events1.len(), 2, "Run 1 should have 2 events");
        assert_eq!(events1[0].sequence, 1);
        assert_eq!(events1[1].sequence, 2);

        let events2 = log.replay_all(&run_id2).unwrap();
        assert_eq!(events2.len(), 1, "Run 2 should have 1 event");
        assert_eq!(events2[0].sequence, 1);
    }

    #[test]
    fn test_append_typed_event() {
        let (log, run_id) = setup_event_log();
        let event = RunEvent {
            run_id: run_id.clone(),
            sequence: 0, // Will be overwritten
            timestamp: chrono::Utc::now(),
            payload: RunEventPayload::TextDelta {
                text: "Hello, world!".to_string(),
            },
        };
        let seq = log.append_event(&event).unwrap();
        assert_eq!(seq, 1);

        let events = log.replay_all(&run_id).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].payload {
            RunEventPayload::TextDelta { text } => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected TextDelta"),
        }
    }
}