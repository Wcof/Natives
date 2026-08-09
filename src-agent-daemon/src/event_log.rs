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
    /// Optional bounded storage actor (TASK-006 / B04). When present, event
    /// appends execute on the actor's single writer thread instead of locking
    /// the DataStore Mutex from the calling (async) thread.
    actor: Option<std::sync::Arc<crate::storage::actor::StorageActor>>,
}

impl EventLog {
    /// Create a new event log backed by the given data store.
    pub fn new(data_store: std::sync::Arc<DataStore>) -> Self {
        EventLog {
            data_store,
            actor: None,
        }
    }

    /// Create an event log that routes appends through a bounded storage actor.
    pub fn new_with_actor(
        data_store: std::sync::Arc<DataStore>,
        actor: std::sync::Arc<crate::storage::actor::StorageActor>,
    ) -> Self {
        EventLog {
            data_store,
            actor: Some(actor),
        }
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
        self.append(&event.run_id, event_type, &payload)
    }

    /// Append a Protocol v2 event. `run_sequence` must be set; DB assigns `global_sequence`.
    /// Duplicate `event_id` is idempotent (returns existing global id, no second broadcast).
    pub fn append_event_v2(&self, event: &RunEventV2) -> Result<u64, String> {
        if let Some(actor) = &self.actor {
            // TASK-006: the fact append runs on the storage actor's single
            // writer thread; the async caller parks on the bounded queue and
            // reply instead of locking the DataStore Mutex directly.
            let event = event.clone();
            let reply = actor.submit(true, move |conn| {
                Self::append_event_v2_with_conn(conn, &event)
                    .map(|global| serde_json::json!(global))
            })?;
            return reply
                .as_u64()
                .ok_or_else(|| "storage actor returned a non-sequence result".into());
        }
        let conn = self.data_store.conn()?;
        Self::append_event_v2_with_conn(&conn, event)
    }

    /// Full v2 append against an already-acquired connection. Executes on the
    /// storage actor worker thread (TASK-006) or the calling thread for a
    /// direct-path log; never locks the DataStore Mutex from an async thread.
    fn append_event_v2_with_conn(
        conn: &rusqlite::Connection,
        event: &RunEventV2,
    ) -> Result<u64, String> {
        let event_id = if event.event_id.trim().is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            event.event_id.clone()
        };
        if let Ok(existing) = conn.query_row(
            "SELECT id FROM run_event WHERE event_id = ?1 LIMIT 1",
            params![&event_id],
            |row| row.get::<_, i64>(0),
        ) {
            return Ok(existing as u64);
        }
        let mut stored = event.clone();
        stored.event_id = event_id.clone();
        let run_seq = stored.run_sequence;
        let payload = serde_json::to_string(&stored)
            .map_err(|e| format!("Failed to serialize event: {e}"))?;
        let (turn_id, message_id) = match &stored.payload {
            RunEventKind::TurnStarted { turn_id } | RunEventKind::TurnCompleted { turn_id, .. } => {
                (Some(turn_id.clone()), None)
            }
            RunEventKind::MessageStarted {
                turn_id,
                message_id,
                ..
            }
            | RunEventKind::MessageDelta {
                turn_id,
                message_id,
                ..
            }
            | RunEventKind::MessageCompleted {
                turn_id,
                message_id,
                ..
            } => (Some(turn_id.clone()), Some(message_id.clone())),
            _ => (None, None),
        };
        let global = if let RunEventKind::ToolCallCompleted {
            id, name, is_error, ..
        } = &stored.payload
        {
            // D04: the ToolCallCompleted fact and the side-effect ledger
            // settlement commit in one transaction. A crash or failed write
            // rolls the pair back together, leaving the ledger intent
            // `started` so resume blocks instead of re-running an effect whose
            // fact never landed. The settlement is guarded to only move a
            // `started` row, so an already-`uncertain` effect (e.g. a failed
            // checkpoint after-image) is never silently promoted to settled.
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("PERSISTENCE_FAILED begin tool effect commit: {e}"))?;
            let row_id = Self::insert_run_event_row(
                &tx,
                &stored,
                &event_id,
                run_seq,
                &payload,
                turn_id.as_deref(),
                message_id.as_deref(),
            )?;
            let category = crate::side_effect_ledger::category_for_tool(name);
            crate::side_effect_ledger::settle_tool_effect(
                &tx,
                &stored.run_id,
                id,
                if *is_error { "failed" } else { "completed" },
                category == "workspace_file",
            )
            .map_err(|e| format!("PERSISTENCE_FAILED settle tool effect: {e}"))?;
            tx.commit()
                .map_err(|e| format!("PERSISTENCE_FAILED commit tool effect: {e}"))?;
            row_id as u64
        } else {
            Self::insert_run_event_row(
                conn,
                &stored,
                &event_id,
                run_seq,
                &payload,
                turn_id.as_deref(),
                message_id.as_deref(),
            )? as u64
        };

        if matches!(
            &stored.payload,
            RunEventKind::HookInvocationStarted { .. }
                | RunEventKind::HookInvocationCompleted { .. }
        ) {
            // Migration 026 persists the trace notice in the same INSERT
            // transaction as the run event; the bus only wakes subscribers.
            crate::rpc::harness::publish_notice(0);
        }

        // Project usage totals so the dashboard can read Natives usage without
        // an external CLI (native-first design).
        if let RunEventKind::UsageUpdated {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            ..
        } = &event.payload
        {
            let input = *input_tokens as i64;
            let output = *output_tokens as i64;
            // A provider that does not report cache usage contributes 0 to the
            // rollup rather than poisoning it — the per-event `None` is still
            // preserved verbatim in the serialized payload.
            let cache_creation = cache_creation_tokens.unwrap_or(0) as i64;
            let cache_read = cache_read_tokens.unwrap_or(0) as i64;
            let _ = conn.execute(
                "UPDATE run
                 SET total_input_tokens = COALESCE(total_input_tokens, 0) + ?1,
                     total_output_tokens = COALESCE(total_output_tokens, 0) + ?2
                 WHERE id = ?3",
                params![input, output, event.run_id],
            );
            // Best-effort dual-write to message token columns when a trigger
            // message exists (keeps conversation-level history useful).
            let _ = conn.execute(
                "UPDATE message
                 SET input_tokens = COALESCE(input_tokens, 0) + ?1,
                     output_tokens = COALESCE(output_tokens, 0) + ?2
                 WHERE id = (
                    SELECT trigger_message_id FROM run WHERE id = ?3 AND trigger_message_id IS NOT NULL
                 )",
                params![input, output, event.run_id],
            );

            // Aggregate into usage_stats when the table exists (same natives.db).
            // date uses UTC YYYY-MM-DD; dashboard localizes via range filters.
            let model = conn
                .query_row(
                    "SELECT model_id FROM run WHERE id = ?1",
                    params![event.run_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap_or_else(|_| "unknown".into());
            let date = event.timestamp.format("%Y-%m-%d").to_string();
            // Ensure table exists (older DBs may not have been migrated by Host).
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS usage_stats (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    date TEXT NOT NULL,
                    source TEXT NOT NULL,
                    source_path TEXT,
                    model TEXT NOT NULL,
                    input_tokens INTEGER NOT NULL DEFAULT 0,
                    output_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                    request_count INTEGER NOT NULL DEFAULT 0,
                    cost_usd REAL NOT NULL DEFAULT 0.0,
                    UNIQUE(date, source, model)
                );",
            );
            let _ = conn.execute(
                "INSERT INTO usage_stats
                    (date, source, source_path, model, input_tokens, output_tokens,
                     cache_creation_tokens, cache_read_tokens, request_count, cost_usd)
                 VALUES (?1, 'natives', 'daemon:run_event', ?2, ?3, ?4, ?5, ?6, 1, 0.0)
                 ON CONFLICT(date, source, model) DO UPDATE SET
                    input_tokens = input_tokens + excluded.input_tokens,
                    output_tokens = output_tokens + excluded.output_tokens,
                    cache_creation_tokens =
                        cache_creation_tokens + excluded.cache_creation_tokens,
                    cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
                    request_count = request_count + 1",
                params![date, model, input, output, cache_creation, cache_read],
            );
        }
        Ok(global)
    }

    /// Insert one `run_event` row on the given connection. Called either
    /// directly (non-tool events) or inside a transaction that also settles
    /// the side-effect ledger (ToolCallCompleted, D04).
    fn insert_run_event_row(
        conn: &rusqlite::Connection,
        stored: &RunEventV2,
        event_id: &str,
        run_seq: u64,
        payload: &str,
        turn_id: Option<&str>,
        message_id: Option<&str>,
    ) -> Result<i64, String> {
        conn.execute(
            "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id, turn_id, message_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stored.run_id,
                run_seq as i64,
                stored.payload.type_name(),
                payload,
                stored.timestamp.to_rfc3339(),
                event_id,
                turn_id,
                message_id,
            ],
        )
        .map_err(|e| format!("PERSISTENCE_FAILED insert run_event: {e}"))?;
        Ok(conn.last_insert_rowid())
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

            let payload = decode_payload(&event_type, &payload_str)
                .map_err(|error| format!("Failed to decode replay event {sequence}: {error}"))?;
            // SQLite datetime('now') returns "YYYY-MM-DD HH:MM:SS" without timezone.
            // Prefer RFC3339 when present.
            let dt = chrono::DateTime::parse_from_rfc3339(&timestamp)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(&timestamp, "%Y-%m-%d %H:%M:%S")
                        .map(|ndt| ndt.and_utc())
                })
                .map_err(|error| {
                    format!("Failed to decode replay timestamp {sequence}: {error}")
                })?;
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
                "SELECT id, sequence, event_type, payload, timestamp, COALESCE(event_id, '')
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
            let global_sequence: i64 = row
                .get(0)
                .map_err(|e| format!("Failed to get global_sequence: {e}"))?;
            let sequence: i64 = row
                .get(1)
                .map_err(|e| format!("Failed to get sequence: {e}"))?;
            let event_type: String = row
                .get(2)
                .map_err(|e| format!("Failed to get event_type: {e}"))?;
            let payload_str: String = row
                .get(3)
                .map_err(|e| format!("Failed to get payload: {e}"))?;
            let timestamp: String = row
                .get(4)
                .map_err(|e| format!("Failed to get timestamp: {e}"))?;
            let event_id: String = row
                .get(5)
                .map_err(|e| format!("Failed to get event_id: {e}"))?;
            let mut event = decode_event_v2(
                run_id,
                sequence as u64,
                &event_type,
                &payload_str,
                &timestamp,
            )?;
            event.global_sequence = global_sequence as u64;
            if !event_id.is_empty() {
                event.event_id = event_id;
            } else if event.event_id.is_empty() {
                event.event_id = format!("legacy:{run_id}:{sequence}");
            }
            events.push(event);
        }
        Ok(events)
    }

    /// Global audit replay: events with global_sequence > after, paginated.
    pub fn replay_global_after(
        &self,
        after_global: u64,
        limit: usize,
    ) -> Result<Vec<RunEventV2>, String> {
        let limit = limit.clamp(1, 500);
        let conn = self.data_store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, run_id, sequence, event_type, payload, timestamp, COALESCE(event_id, '')
                 FROM run_event
                 WHERE id > ?1
                 ORDER BY id ASC
                 LIMIT ?2",
            )
            .map_err(|e| format!("Failed to prepare global replay: {e}"))?;
        let mut rows = stmt
            .query(params![after_global as i64, limit as i64])
            .map_err(|e| format!("Failed to query global replay: {e}"))?;
        let mut events = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|e| format!("Failed to read row: {e}"))?
        {
            let global_sequence: i64 = row.get(0).map_err(|e| e.to_string())?;
            let run_id: String = row.get(1).map_err(|e| e.to_string())?;
            let sequence: i64 = row.get(2).map_err(|e| e.to_string())?;
            let event_type: String = row.get(3).map_err(|e| e.to_string())?;
            let payload_str: String = row.get(4).map_err(|e| e.to_string())?;
            let timestamp: String = row.get(5).map_err(|e| e.to_string())?;
            let event_id: String = row.get(6).map_err(|e| e.to_string())?;
            let mut event = decode_event_v2(
                &run_id,
                sequence as u64,
                &event_type,
                &payload_str,
                &timestamp,
            )?;
            event.global_sequence = global_sequence as u64;
            if !event_id.is_empty() {
                event.event_id = event_id;
            }
            events.push(event);
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
        self.append_event_v2(event).map(|_| ())
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
    serde_json::from_str(&normalized).or_else(|_| {
        // Unknown/unregistered event type (e.g. a row written by a newer or
        // foreign schema). Surface it as the forward-compatibility Unknown
        // variant instead of failing the whole replay for one undecodable row.
        serde_json::from_str::<RunEventPayload>(
            &serde_json::json!({"type": "unknown", "raw": {"event_type": event_type, "payload": payload_str}}).to_string(),
        )
        .map_err(|e| format!("decode payload: {e}"))
    })
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
        event.run_sequence = sequence;
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
        event_id: format!("legacy:{run_id}:{sequence}"),
        global_sequence: 0,
        run_id: run_id.to_string(),
        run_sequence: sequence,
        timestamp: dt,
        payload,
    })
}

fn decode_payload_v2(event_type: &str, payload_str: &str) -> Result<RunEventKind, String> {
    if let Ok(payload) = serde_json::from_str::<RunEventKind>(payload_str) {
        return Ok(payload);
    }
    let normalized = normalize_stored_payload(event_type, payload_str);
    serde_json::from_str(&normalized).or_else(|_| {
        // Unknown/unregistered event type: surface as the forward-compatibility
        // Unknown variant instead of failing the whole replay for one row.
        serde_json::from_str::<RunEventKind>(
            &serde_json::json!({"type": "unknown", "raw": {"event_type": event_type, "payload": payload_str}}).to_string(),
        )
        .map_err(|e| format!("decode v2 payload: {e}"))
    })
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

    #[test]
    fn usage_updated_increments_run_totals_and_usage_stats() {
        use assistant_protocol::v2::{RunEventKind, RunEventV2};
        let (log, run_id) = setup_event_log();
        let event = RunEventV2 {
            event_id: uuid::Uuid::new_v4().to_string(),
            global_sequence: 0,
            run_sequence: 1,
            run_id: run_id.clone(),
            timestamp: chrono::Utc::now(),
            payload: RunEventKind::UsageUpdated {
                input_tokens: 10,
                output_tokens: 5,
                reasoning_tokens: None,
                cache_creation_tokens: Some(7),
                cache_read_tokens: Some(9),
            },
        };
        log.append_event_v2(&event).unwrap();
        let conn = log.data_store.conn().unwrap();
        let (inp, out): (i64, i64) = conn
            .query_row(
                "SELECT total_input_tokens, total_output_tokens FROM run WHERE id=?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((inp, out), (10, 5));
        let (count, creation, read): (i64, i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(cache_creation_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0)
                 FROM usage_stats WHERE source='natives' AND input_tokens=10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((0, 0, 0));
        assert!(count >= 1, "usage_stats row should exist");
        assert_eq!(
            (creation, read),
            (7, 9),
            "cache tokens must reach usage_stats, not be written as literal 0"
        );
    }

    /// A provider that reports no cache activity must not be recorded as if it
    /// had reported zero — the rollup takes 0, but the persisted event keeps
    /// `None` so `daemon.getCapabilities` consumers can tell the two apart.
    #[test]
    fn absent_cache_reporting_rolls_up_as_zero_without_claiming_a_measurement() {
        use assistant_protocol::v2::{RunEventKind, RunEventV2};
        let (log, run_id) = setup_event_log();
        let event = RunEventV2 {
            event_id: uuid::Uuid::new_v4().to_string(),
            global_sequence: 0,
            run_sequence: 1,
            run_id: run_id.clone(),
            timestamp: chrono::Utc::now(),
            payload: RunEventKind::UsageUpdated {
                input_tokens: 3,
                output_tokens: 4,
                reasoning_tokens: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
            },
        };
        log.append_event_v2(&event).unwrap();

        // v2 replay carries the payload verbatim; the v1 projection has no
        // cache fields at all, which is why this asserts through v2.
        let replayed = log.replay_after_v2(&run_id, 0).unwrap();
        match &replayed[0].payload {
            RunEventKind::UsageUpdated {
                cache_creation_tokens,
                cache_read_tokens,
                ..
            } => {
                assert!(cache_creation_tokens.is_none());
                assert!(cache_read_tokens.is_none());
            }
            other => panic!("expected UsageUpdated, got {other:?}"),
        }

        let conn = log.data_store.conn().unwrap();
        let (creation, read): (i64, i64) = conn
            .query_row(
                "SELECT cache_creation_tokens, cache_read_tokens
                 FROM usage_stats WHERE source='natives' AND input_tokens=3",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((creation, read), (0, 0));
    }

    /// D04: appending the ToolCallCompleted fact settles the matching ledger
    /// intent in the same transaction — the event and the terminal status are
    /// either both committed or neither.
    #[test]
    fn tool_completed_event_settles_ledger_atomically() {
        let (log, run_id) = setup_event_log();
        {
            let conn = log.data_store.conn().unwrap();
            conn.execute(
                "INSERT INTO side_effect_record
                 (id, run_id, tool_call_id, category, target_summary, side_effect_class,
                  status, replay_safe, resource, started_at, ledger_sequence)
                 VALUES (?1, ?2, 'call-1', 'workspace_file', '{}', 'workspace_file',
                         'started', 0, '/tmp/f.txt', datetime('now'), 1)",
                params!["sid-1", run_id],
            )
            .unwrap();
        }
        let event = RunEventV2 {
            event_id: "evt-1".into(),
            global_sequence: 0,
            run_sequence: 1,
            run_id: run_id.clone(),
            timestamp: chrono::Utc::now(),
            payload: RunEventKind::ToolCallCompleted {
                id: "call-1".into(),
                name: "write_file".into(),
                output: serde_json::json!({"ok": true}),
                is_error: false,
                duration_ms: 3,
                result_message_id: Some("rm-1".into()),
            },
        };
        log.append_event_v2(&event).unwrap();
        let conn = log.data_store.conn().unwrap();
        let (status, replay_safe): (String, i64) = conn
            .query_row(
                "SELECT status, replay_safe FROM side_effect_record
                 WHERE run_id = ?1 AND tool_call_id = 'call-1'",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "completed");
        assert_eq!(replay_safe, 1, "workspace effects settle as replay-safe");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_event WHERE run_id = ?1 AND event_id = 'evt-1'",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "the fact event must be committed with the settlement"
        );
    }

    /// D04: a tool call whose effect is already `uncertain` (e.g. a failed
    /// checkpoint after-image) must not be promoted to settled by the fact
    /// append — the event is still recorded, the ledger stays unresolved.
    #[test]
    fn tool_completed_does_not_overwrite_uncertain_effect() {
        let (log, run_id) = setup_event_log();
        {
            let conn = log.data_store.conn().unwrap();
            conn.execute(
                "INSERT INTO side_effect_record
                 (id, run_id, tool_call_id, category, target_summary, side_effect_class,
                  status, replay_safe, resource, started_at, ledger_sequence)
                 VALUES (?1, ?2, 'call-u', 'process', '{}', 'process',
                         'uncertain', 0, 'cmd', datetime('now'), 1)",
                params!["sid-u", run_id],
            )
            .unwrap();
        }
        let event = RunEventV2 {
            event_id: "evt-u".into(),
            global_sequence: 0,
            run_sequence: 1,
            run_id: run_id.clone(),
            timestamp: chrono::Utc::now(),
            payload: RunEventKind::ToolCallCompleted {
                id: "call-u".into(),
                name: "run_terminal".into(),
                output: serde_json::json!({"ok": true}),
                is_error: false,
                duration_ms: 3,
                result_message_id: Some("rm-u".into()),
            },
        };
        log.append_event_v2(&event).unwrap();
        let conn = log.data_store.conn().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM side_effect_record
                 WHERE run_id = ?1 AND tool_call_id = 'call-u'",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            status, "uncertain",
            "settlement must not promote an uncertain effect to settled"
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_event WHERE run_id = ?1 AND event_id = 'evt-u'",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    /// D04: a tool call with no ledger intent (read-only, preflight-denied)
    /// still appends its fact event; settlement is a no-op.
    #[test]
    fn tool_completed_without_intent_appends_event_only() {
        let (log, run_id) = setup_event_log();
        let event = RunEventV2 {
            event_id: "evt-x".into(),
            global_sequence: 0,
            run_sequence: 1,
            run_id: run_id.clone(),
            timestamp: chrono::Utc::now(),
            payload: RunEventKind::ToolCallCompleted {
                id: "call-x".into(),
                name: "web_fetch".into(),
                output: serde_json::json!({"ok": true}),
                is_error: false,
                duration_ms: 3,
                result_message_id: Some("rm-x".into()),
            },
        };
        log.append_event_v2(&event).unwrap();
        let conn = log.data_store.conn().unwrap();
        let event_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_event WHERE run_id = ?1 AND event_id = 'evt-x'",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(event_count, 1);
        let ledger_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM side_effect_record
                 WHERE run_id = ?1 AND tool_call_id = 'call-x'",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger_count, 0, "no intent means nothing to settle");
    }

    /// TASK-006: the fact append routes through the bounded storage actor and
    /// still commits the ledger settlement atomically with the event.
    #[test]
    fn append_via_storage_actor_settles_ledger_atomically() {
        let tmp = std::env::temp_dir();
        let db_path = tmp.join(format!("test_events_actor_{}.db", uuid::Uuid::new_v4()));
        let art_dir = tmp.join(format!("test_artifacts_actor_{}", uuid::Uuid::new_v4()));
        let store = Arc::new(DataStore::new(&db_path, &art_dir).unwrap());
        let actor = crate::storage::actor::StorageActor::new(8, store.clone());
        let log = EventLog::new_with_actor(store.clone(), actor);
        let run_id = "test-run-actor".to_string();
        {
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id) VALUES (?1, 'chat', 'Test', 'prov-1', 'model-1')",
                params!["test-conv-actor"],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'test-conv-actor', 'queued', 'prov-1', 'model-1')",
                params![run_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO side_effect_record
                 (id, run_id, tool_call_id, category, target_summary, side_effect_class,
                  status, replay_safe, resource, started_at, ledger_sequence)
                 VALUES (?1, ?2, 'call-a', 'workspace_file', '{}', 'workspace_file',
                         'started', 0, '/tmp/f.txt', datetime('now'), 1)",
                params!["sid-actor", run_id],
            )
            .unwrap();
        }
        let event = RunEventV2 {
            event_id: "evt-actor".into(),
            global_sequence: 0,
            run_sequence: 1,
            run_id: run_id.clone(),
            timestamp: chrono::Utc::now(),
            payload: RunEventKind::ToolCallCompleted {
                id: "call-a".into(),
                name: "write_file".into(),
                output: serde_json::json!({"ok": true}),
                is_error: false,
                duration_ms: 3,
                result_message_id: Some("rm-actor".into()),
            },
        };
        let global = log.append_event_v2(&event).unwrap();
        assert!(global > 0, "actor append returns a durable global sequence");
        let conn = store.conn().unwrap();
        let (status, replay_safe): (String, i64) = conn
            .query_row(
                "SELECT status, replay_safe FROM side_effect_record
                 WHERE run_id = ?1 AND tool_call_id = 'call-a'",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "completed");
        assert_eq!(replay_safe, 1);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_event WHERE run_id = ?1 AND event_id = 'evt-actor'",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
