//! Event replay integration tests for the Agent Daemon.
//!
//! These tests verify that the event log correctly:
//! - Assigns monotonic sequences per run
//! - Persists events atomically
//! - Replays events after restart (simulated by creating a new DataStore on the same DB)
//! - Preserves exact event order

use assistant_protocol::v1::run_event::{RunEvent, RunEventPayload};
use natives_agent_daemon::event_log::EventLog;
use natives_agent_daemon::storage::DataStore;
use std::path::PathBuf;
use std::sync::Arc;

/// Helper to create a temporary event log for testing.
/// Automatically creates the test conversation and run records.
fn setup_log(db_path: &PathBuf, art_dir: &PathBuf, run_id: &str) -> EventLog {
    let store = Arc::new(DataStore::new(db_path, art_dir).unwrap());
    let log = EventLog::new(store.clone());

    // Create parent records to satisfy foreign key constraints
    let conn = store.conn().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id) VALUES ('test-conv', 'chat', 'Test', 'prov-1', 'model-1')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'test-conv', 'queued', 'prov-1', 'model-1')",
        rusqlite::params![run_id],
    ).unwrap();

    log
}

/// Helper to create a typed event for testing.
fn make_event(run_id: &str, payload: RunEventPayload) -> RunEvent {
    RunEvent {
        run_id: run_id.to_string(),
        sequence: 0, // Will be auto-assigned
        timestamp: chrono::Utc::now(),
        payload,
    }
}

// ---------------------------------------------------------------------------
// Restart and replay tests
// ---------------------------------------------------------------------------

#[test]
fn test_debug_db_state() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_debug_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_debug_art_{}", uuid::Uuid::new_v4()));
    let run_id = "debug-run".to_string();

    let log = setup_log(&db_path, &art_dir, &run_id);
    let seq = log
        .append_event(&make_event(&run_id, RunEventPayload::Started))
        .unwrap();
    assert_eq!(seq, 1, "First event should be sequence 1");

    // Check database directly with raw query
    let store = Arc::new(DataStore::new(&db_path, &art_dir).unwrap());
    let conn = store.conn().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM run_event WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "Event should be in database");

    // Check replay
    let events = log.replay_all(&run_id).unwrap();
    assert_eq!(
        events.len(),
        1,
        "Replay should return 1 event, got {}",
        events.len()
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&art_dir);
}

#[test]
fn test_restart_preserves_events() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_restart_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_restart_art_{}", uuid::Uuid::new_v4()));
    let run_id = "restart-run-001".to_string();

    // First session: write events
    let log = setup_log(&db_path, &art_dir, &run_id);
    log.append_event(&make_event(&run_id, RunEventPayload::Started))
        .unwrap();
    log.append_event(&make_event(
        &run_id,
        RunEventPayload::TextDelta {
            text: "persistent".to_string(),
        },
    ))
    .unwrap();
    log.append_event(&make_event(
        &run_id,
        RunEventPayload::Completed {
            reason: "done".to_string(),
        },
    ))
    .unwrap();
    let seq_before = log.last_sequence(&run_id).unwrap();
    assert_eq!(seq_before, 3, "Should have 3 events after first session");

    // Drop log and store (simulate restart)
    drop(log);

    // Second session: reopen the same database
    let log = setup_log(&db_path, &art_dir, &run_id);
    let events = log.replay_all(&run_id).unwrap();
    assert_eq!(events.len(), 3, "Should have 3 events after restart");
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
    assert_eq!(events[2].sequence, 3);

    // Verify exact order is preserved
    assert_eq!(events[0].run_id, run_id);
    assert_eq!(events[2].sequence, 3);
}

#[test]
fn test_append_after_restart() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_append_restart_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_append_restart_art_{}", uuid::Uuid::new_v4()));
    let run_id = "append-run-001".to_string();

    // First session
    let log = setup_log(&db_path, &art_dir, &run_id);
    log.append_event(&make_event(&run_id, RunEventPayload::Started))
        .unwrap();
    drop(log);

    // Second session: append more events
    let log = setup_log(&db_path, &art_dir, &run_id);
    log.append_event(&make_event(
        &run_id,
        RunEventPayload::TextDelta {
            text: "after restart".to_string(),
        },
    ))
    .unwrap();
    let last_seq = log.last_sequence(&run_id).unwrap();
    assert_eq!(last_seq, 2, "Sequence should continue after restart");

    let events = log.replay_all(&run_id).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
}

// ---------------------------------------------------------------------------
// Order preservation tests
// ---------------------------------------------------------------------------

#[test]
fn test_event_order_preserved() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_order_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_order_art_{}", uuid::Uuid::new_v4()));
    let run_id = "order-run-001".to_string();

    let log = setup_log(&db_path, &art_dir, &run_id);

    // Append events in a specific order
    log.append_event(&make_event(&run_id, RunEventPayload::Queued))
        .unwrap();
    log.append_event(&make_event(&run_id, RunEventPayload::Preparing))
        .unwrap();
    log.append_event(&make_event(&run_id, RunEventPayload::Started))
        .unwrap();
    log.append_event(&make_event(
        &run_id,
        RunEventPayload::TextDelta {
            text: "step 1".to_string(),
        },
    ))
    .unwrap();
    log.append_event(&make_event(
        &run_id,
        RunEventPayload::ToolCallRequested {
            id: "tc1".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({}),
        },
    ))
    .unwrap();
    log.append_event(&make_event(
        &run_id,
        RunEventPayload::Completed {
            reason: "success".to_string(),
        },
    ))
    .unwrap();

    let events = log.replay_all(&run_id).unwrap();
    assert_eq!(events.len(), 6);

    // Verify types in order
    let type_names: Vec<&str> = events
        .iter()
        .map(|e| match &e.payload {
            RunEventPayload::Queued => "queued",
            RunEventPayload::Preparing => "preparing",
            RunEventPayload::Started => "started",
            RunEventPayload::TextDelta { .. } => "text_delta",
            RunEventPayload::ToolCallRequested { .. } => "tool_call_requested",
            RunEventPayload::Completed { .. } => "completed",
            _ => "other",
        })
        .collect();

    assert_eq!(
        type_names,
        vec![
            "queued",
            "preparing",
            "started",
            "text_delta",
            "tool_call_requested",
            "completed"
        ]
    );
}

// ---------------------------------------------------------------------------
// Multiple runs isolation
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_runs_preserve_order() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_multi_run_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_multi_run_art_{}", uuid::Uuid::new_v4()));

    let run_a = "run-a".to_string();
    let run_b = "run-b".to_string();

    let log = setup_log(&db_path, &art_dir, &run_a);

    // Create run_b in the database
    {
        // We need to access the store to insert run_b. But since setup_log hides it,
        // we create a temporary store and use the same DB path.
        let store = Arc::new(DataStore::new(&db_path, &art_dir).unwrap());
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'test-conv', 'queued', 'prov-1', 'model-1')",
            rusqlite::params![run_b],
        ).unwrap();
    }

    // Interleave events from two runs
    log.append_event(&make_event(&run_a, RunEventPayload::Started))
        .unwrap();
    log.append_event(&make_event(&run_b, RunEventPayload::Started))
        .unwrap();
    log.append_event(&make_event(
        &run_a,
        RunEventPayload::TextDelta {
            text: "A1".to_string(),
        },
    ))
    .unwrap();
    log.append_event(&make_event(
        &run_b,
        RunEventPayload::TextDelta {
            text: "B1".to_string(),
        },
    ))
    .unwrap();
    log.append_event(&make_event(
        &run_a,
        RunEventPayload::Completed {
            reason: "done".to_string(),
        },
    ))
    .unwrap();
    log.append_event(&make_event(
        &run_b,
        RunEventPayload::Completed {
            reason: "done".to_string(),
        },
    ))
    .unwrap();

    // Verify run A events (per-run sequences)
    let events_a = log.replay_all(&run_a).unwrap();
    assert_eq!(events_a.len(), 3);
    assert_eq!(events_a[0].sequence, 1);
    assert_eq!(events_a[1].sequence, 2);
    assert_eq!(events_a[2].sequence, 3);

    // Verify run B events (per-run sequences)
    let events_b = log.replay_all(&run_b).unwrap();
    assert_eq!(events_b.len(), 3);
    assert_eq!(events_b[0].sequence, 1);
    assert_eq!(events_b[1].sequence, 2);
    assert_eq!(events_b[2].sequence, 3);
}

// ---------------------------------------------------------------------------
// Fixture checksum test
// ---------------------------------------------------------------------------

#[test]
fn test_checksum_preserved_across_replay() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_checksum_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_checksum_art_{}", uuid::Uuid::new_v4()));
    let run_id = "checksum-run".to_string();

    let log = setup_log(&db_path, &art_dir, &run_id);

    // Write events
    let payloads = vec![
        RunEventPayload::TextDelta {
            text: "hello".to_string(),
        },
        RunEventPayload::TextDelta {
            text: "world".to_string(),
        },
        RunEventPayload::Completed {
            reason: "ok".to_string(),
        },
    ];

    for payload in &payloads {
        log.append_event(&make_event(&run_id, payload.clone()))
            .unwrap();
    }

    // Read back and verify content
    let events = log.replay_all(&run_id).unwrap();
    assert_eq!(events.len(), 3);

    // Verify text content
    match &events[0].payload {
        RunEventPayload::TextDelta { text } => assert_eq!(text, "hello"),
        _ => panic!("Expected TextDelta, got {:?}", events[0].payload),
    }
    match &events[1].payload {
        RunEventPayload::TextDelta { text } => assert_eq!(text, "world"),
        _ => panic!("Expected TextDelta, got {:?}", events[1].payload),
    }
    match &events[2].payload {
        RunEventPayload::Completed { reason } => assert_eq!(reason, "ok"),
        _ => panic!("Expected Completed, got {:?}", events[2].payload),
    }
}
