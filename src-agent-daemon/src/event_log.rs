//! # Replayable Event Log
//!
//! Manages the run event log with monotonic sequence number assignment,
//! atomic append, replay after sequence, and duplicate acknowledgement handling.
//!
//! Each run has a strictly increasing sequence number for events, allowing
//! clients to replay from the last acknowledged sequence after reconnection.

use crate::storage::DataStore;
use agent_core::EventPersistence;
use assistant_protocol::v1::run_event::{RunEvent, RunEventPayload};
use assistant_protocol::v2::{RunEventKind, RunEventV2};
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

        // Normalize payload so replay can always deserialize a tagged RunEventPayload.
        let stored_payload = normalize_stored_payload(event_type, payload);

        conn.execute(
            "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                run_id,
                next_seq,
                event_type,
                stored_payload,
                chrono::Utc::now().to_rfc3339()
            ],
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

    /// Append a Protocol v2 event with the already assigned sequence.
    pub fn append_event_v2(&self, event: &RunEventV2) -> Result<(), String> {
        let conn = self.data_store.conn()?;
        let payload =
            serde_json::to_string(event).map_err(|e| format!("Failed to serialize event: {e}"))?;
        conn.execute(
            "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.run_id,
                event.sequence as i64,
                event.payload.type_name(),
                payload,
                event.timestamp.to_rfc3339()
            ],
        )
        .map_err(|e| format!("PERSISTENCE_FAILED insert run_event: {e}"))?;
        Ok(())
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
        while let Some(row) = rows
            .next()
            .map_err(|e| format!("Failed to read row: {e}"))?
        {
            let sequence: i64 = row
                .get(0)
                .map_err(|e| format!("Failed to get sequence: {e}"))?;
            let event_type: String = row
                .get(1)
                .map_err(|e| format!("Failed to get event_type: {e}"))?;
            let payload_str: String = row
                .get(2)
                .map_err(|e| format!("Failed to get payload: {e}"))?;
            let timestamp: String = row
                .get(3)
                .map_err(|e| format!("Failed to get timestamp: {e}"))?;

            let payload = match decode_payload(&event_type, &payload_str) {
                Ok(p) => p,
                Err(_) => continue,
            };
            // SQLite datetime('now') returns "YYYY-MM-DD HH:MM:SS" without timezone.
            // Prefer RFC3339 when present.
            let dt = chrono::DateTime::parse_from_rfc3339(&timestamp)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(&timestamp, "%Y-%m-%d %H:%M:%S")
                        .map(|ndt| ndt.and_utc())
                })
                .unwrap_or_else(|_| chrono::Utc::now());
            events.push(RunEvent {
                run_id: run_id.to_string(),
                sequence: sequence as u64,
                timestamp: dt,
                payload,
            });
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

    pub fn replay_after_v2(
        &self,
        run_id: &str,
        last_sequence: u64,
    ) -> Result<Vec<RunEventV2>, String> {
        let conn = self.data_store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT sequence, event_type, payload, timestamp
                 FROM run_event
                 WHERE run_id = ?1 AND sequence > ?2
                 ORDER BY sequence ASC",
            )
            .map_err(|e| format!("Failed to prepare v2 replay query: {e}"))?;

        let mut rows = stmt
            .query(params![run_id, last_sequence as i64])
            .map_err(|e| format!("Failed to query v2 replay: {e}"))?;

        let mut events = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|e| format!("Failed to read row: {e}"))?
        {
            let sequence: i64 = row
                .get(0)
                .map_err(|e| format!("Failed to get sequence: {e}"))?;
            let event_type: String = row
                .get(1)
                .map_err(|e| format!("Failed to get event_type: {e}"))?;
            let payload_str: String = row
                .get(2)
                .map_err(|e| format!("Failed to get payload: {e}"))?;
            let timestamp: String = row
                .get(3)
                .map_err(|e| format!("Failed to get timestamp: {e}"))?;
            events.push(decode_event_v2(
                run_id,
                sequence as u64,
                &event_type,
                &payload_str,
                &timestamp,
            )?);
        }
        Ok(events)
    }

    /// Acknowledge receipt of events up to the given sequence.
    /// This is a no-op for the log itself (acknowledgements are tracked client-side).
    pub fn acknowledge(&self, _run_id: &str, _sequence: i64) -> Result<(), String> {
        // Acknowledgements are tracked by the client; the log is append-only.
        // This method exists for future durability tracking.
        Ok(())
    }
}

impl EventPersistence for EventLog {
    fn append(&self, event: &RunEventV2) -> Result<(), String> {
        self.append_event_v2(event)
    }

    fn replay_after(&self, run_id: &str, after_sequence: u64) -> Result<Vec<RunEventV2>, String> {
        self.replay_after_v2(run_id, after_sequence)
    }

    fn last_sequence(&self, run_id: &str) -> Result<u64, String> {
        EventLog::last_sequence(self, run_id).map(|seq| seq as u64)
    }
}

/// Ensure stored payload JSON includes the serde tag for RunEventPayload.
fn normalize_stored_payload(event_type: &str, payload: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
        if value.get("type").is_some() {
            return payload.to_string();
        }
        if let serde_json::Value::Object(mut map) = value {
            map.insert(
                "type".into(),
                serde_json::Value::String(event_type.to_string()),
            );
            return serde_json::Value::Object(map).to_string();
        }
    }
    // Unit variants / empty payloads
    serde_json::json!({ "type": event_type }).to_string()
}

fn decode_payload(event_type: &str, payload_str: &str) -> Result<RunEventPayload, String> {
    if let Ok(payload) = serde_json::from_str::<RunEventPayload>(payload_str) {
        return Ok(payload);
    }
    // Recover from untagged historical rows.
    let normalized = normalize_stored_payload(event_type, payload_str);
    serde_json::from_str(&normalized).map_err(|e| format!("decode payload: {e}"))
}

fn decode_event_v2(
    run_id: &str,
    sequence: u64,
    event_type: &str,
    payload_str: &str,
    timestamp: &str,
) -> Result<RunEventV2, String> {
    if let Ok(mut event) = serde_json::from_str::<RunEventV2>(payload_str) {
        event.run_id = run_id.to_string();
        event.sequence = sequence;
        return Ok(event);
    }
    let payload = decode_payload_v2(event_type, payload_str)?;
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S")
                .map(|ndt| ndt.and_utc())
        })
        .unwrap_or_else(|_| chrono::Utc::now());
    Ok(RunEventV2 {
        run_id: run_id.to_string(),
        sequence,
        timestamp: dt,
        payload,
    })
}

fn decode_payload_v2(event_type: &str, payload_str: &str) -> Result<RunEventKind, String> {
    if let Ok(payload) = serde_json::from_str::<RunEventKind>(payload_str) {
        return Ok(payload);
    }
    let normalized = normalize_stored_payload(event_type, payload_str);
    serde_json::from_str(&normalized).map_err(|e| format!("decode v2 payload: {e}"))
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
        let seq2 = log
            .append(&run_id, "text_delta", r#"{"text":"hello"}"#)
            .unwrap();
        assert_eq!(seq2, 2, "Second event should be sequence 2");
        let seq3 = log
            .append(&run_id, "completed", r#"{"reason":"done"}"#)
            .unwrap();
        assert_eq!(seq3, 3, "Third event should be sequence 3");
    }

    #[test]
    fn test_debug_db_state() {
        let (log, run_id) = setup_event_log();
        let seq = log.append(&run_id, "started", "{}").unwrap();
        assert_eq!(seq, 1);
        // Check database directly — drop the conn guard before replay to
        // avoid re-entrant DataStore mutex deadlock.
        {
            let conn = log.data_store.conn().unwrap();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM run_event WHERE run_id = ?1",
                    params![run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "Event should be in database");
        }
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
        log.append(&run_id, "text_delta", r#"{"text":"hi"}"#)
            .unwrap();
        log.append(&run_id, "completed", r#"{"reason":"ok"}"#)
            .unwrap();

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
        log.append(&run_id, "completed", r#"{"reason":"ok"}"#)
            .unwrap();

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

        log.append(&run_id, "completed", r#"{"reason":"ok"}"#)
            .unwrap();
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

        // Create second run (drop guard before further EventLog ops)
        {
            let conn = log.data_store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'test-conv-001', 'queued', 'prov-1', 'model-1')",
                params![run_id2],
            ).unwrap();
        }

        log.append(&run_id1, "started", "{}").unwrap();
        log.append(&run_id2, "started", "{}").unwrap();
        log.append(&run_id1, "completed", r#"{"reason":"ok"}"#)
            .unwrap();

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
