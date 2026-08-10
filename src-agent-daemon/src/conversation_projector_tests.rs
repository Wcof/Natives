use super::*;
use crate::conversation_store;
use agent_core::ContentBlock;

/// Shared test store (migrations run once per test on a fresh temp DB);
/// each test uses unique run/conv ids. Returns the tempdir so it stays
/// alive for the whole test.
///
/// Note: like the rest of the daemon test suite, the NATIVES_* env
/// mutations here are intentionally NOT restored — several production
/// fixtures (e.g. `subagent_persona_tests`) rely on `NATIVES_ASSISTANT_DB_PATH`
/// being present and recreate the path on demand. Restoring it here breaks
/// those tests (pre-existing coupling, T01 hermeticity debt).
fn setup() -> ((String, String), tempfile::TempDir) {
    let _guard = crate::storage::DataStore::env_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("projector.db");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
    let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
    let conv = format!("proj-conv-{}", uuid::Uuid::new_v4());
    let run = format!("proj-run-{}", uuid::Uuid::new_v4());
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id)
             VALUES (?1, 'chat', 'Projector Test', 'prov-1', 'model-1')",
            params![conv],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id)
             VALUES (?1, ?2, 'completed', 'prov-1', 'model-1')",
            params![run, conv],
        )
        .unwrap();
    }
    ((conv, run), dir)
}

fn event(run_id: &str, sequence: u64, payload: RunEventKind) -> RunEventV2 {
    RunEventV2 {
        event_id: format!("evt-{run_id}-{sequence}"),
        global_sequence: 0,
        run_sequence: sequence,
        run_id: run_id.into(),
        timestamp: chrono::Utc::now(),
        payload,
    }
}

fn typed_turn_events(run_id: &str, turn: &str) -> Vec<RunEventV2> {
    vec![
        event(
            run_id,
            1,
            RunEventKind::TurnStarted {
                turn_id: turn.into(),
            },
        ),
        event(
            run_id,
            2,
            RunEventKind::MessageStarted {
                turn_id: turn.into(),
                message_id: format!("msg-{turn}"),
                role: "assistant".into(),
            },
        ),
        event(
            run_id,
            3,
            RunEventKind::TextDelta {
                text: "hello".into(),
            },
        ),
        event(
            run_id,
            4,
            RunEventKind::ToolCallRequested {
                id: format!("call-{turn}-1"),
                name: "read_file".into(),
                input: serde_json::json!({"path": "/tmp/a.txt"}),
            },
        ),
        event(
            run_id,
            5,
            RunEventKind::ToolCallCompleted {
                id: format!("call-{turn}-1"),
                name: "read_file".into(),
                output: serde_json::json!({"content": "file body"}),
                is_error: false,
                duration_ms: 2,
                result_message_id: Some(format!("rm-{turn}-1")),
            },
        ),
        event(
            run_id,
            6,
            RunEventKind::MessageCompleted {
                turn_id: turn.into(),
                message_id: format!("msg-{turn}"),
                role: "assistant".into(),
                // The engine stores the typed content as an externally-tagged
                // `Vec<ContentBlock>`; mirror that exact serialization.
                content: Some(serde_json::json!({
                    "message_id": format!("msg-{turn}"),
                    "role": "assistant",
                    "content": serde_json::to_value(vec![
                        agent_core::ContentBlock::Text { text: "hello".into() },
                        agent_core::ContentBlock::ToolCall(agent_core::ToolCall {
                            tool_call_id: format!("call-{turn}-1").into(),
                            name: "read_file".into(),
                            arguments_json: "{\"path\":\"/tmp/a.txt\"}".into(),
                        }),
                    ]).unwrap(),
                })),
            },
        ),
        event(
            run_id,
            7,
            RunEventKind::TurnCompleted {
                turn_id: turn.into(),
                stop_reason: "tool_use".into(),
                input_tokens: 0,
                output_tokens: 0,
            },
        ),
    ]
}

/// TASK-005 acceptance #1/#2: the projector writes the full typed turn
/// (assistant content + complete tool pair) so a reloaded AgentMessage
/// transcript matches the event-derived content and the tool result follows
/// its tool call in block order.
#[test]
fn projector_preserves_full_tool_pair_and_block_order() {
    let ((conv, run), _dir) = setup();
    let events = typed_turn_events(&run, "t1");
    let projected = project_run_from_events(&conv, &run, &events)
        .unwrap()
        .projected;
    assert_eq!(projected, 1, "one committed turn projects");
    let messages = conversation_store::load_agent_messages(&conv).unwrap();
    assert_eq!(messages.len(), 2, "assistant + tool result");
    let assistant = messages
        .iter()
        .find_map(|m| match m {
            agent_core::AgentMessage::Assistant(a) => Some(a),
            _ => None,
        })
        .unwrap();
    let tool_call = assistant
        .content
        .iter()
        .find_map(|b| match b {
            ContentBlock::ToolCall(call) => Some(call),
            _ => None,
        })
        .unwrap();
    assert_eq!(tool_call.tool_call_id.to_string(), format!("call-t1-1"));
    let result = messages
        .iter()
        .find_map(|m| match m {
            agent_core::AgentMessage::ToolResult(r) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(result.tool_call_id.to_string(), format!("call-t1-1"));
    assert_eq!(result.tool_name, "read_file");
    assert!(!result.is_error);
}

/// TASK-005: re-projecting the same events is a zero-side-effect no-op.
#[test]
fn projector_is_idempotent() {
    let ((conv, run), _dir) = setup();
    let events = typed_turn_events(&run, "t1");
    project_run_from_events(&conv, &run, &events).unwrap();
    let before = conversation_store::load_agent_messages(&conv).unwrap();
    project_run_from_events(&conv, &run, &events).unwrap();
    let after = conversation_store::load_agent_messages(&conv).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(
        before, after,
        "re-projection must not change the transcript"
    );
    let store = conversation_store::store().unwrap();
    let conn = store.conn().unwrap();
    let turn_count: i64 = conn
        .query_row(
            "SELECT turn_count FROM projection_watermark
             WHERE projector='conversation' AND run_id=?1",
            params![run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(turn_count, 1, "a re-projected turn must not double-count");
}

/// TASK-005: a crashed/partial typed turn (no TurnCompleted) is never
/// materialized as a complete message.
#[test]
fn partial_turn_is_not_projected() {
    let ((conv, run), _dir) = setup();
    let events = typed_turn_events(&run, "t1");
    let mut partial = events;
    partial.retain(|e| !matches!(e.payload, RunEventKind::TurnCompleted { .. }));
    let projected = project_run_from_events(&conv, &run, &partial)
        .unwrap()
        .projected;
    assert_eq!(projected, 0);
    assert!(conversation_store::load_agent_messages(&conv)
        .unwrap()
        .is_empty());
}

/// TASK-005: a corrupt MessageCompleted payload is quarantined explicitly,
/// never silently skipped, and the turn is not projected.
#[test]
fn corrupt_event_is_quarantined_not_skipped() {
    let ((conv, run), _dir) = setup();
    let mut events = typed_turn_events(&run, "t1");
    if let RunEventV2 {
        payload: RunEventKind::MessageCompleted { content, .. },
        ..
    } = &mut events[5]
    {
        *content = Some(
            serde_json::json!({ "message_id": "msg-t1", "role": "assistant", "content": "not-an-array" }),
        );
    }
    let report = project_run_from_events(&conv, &run, &events).unwrap();
    assert_eq!(report.projected, 0, "corrupt turn must not project");
    assert_eq!(report.quarantined, 1, "corrupt turn must be quarantined");
    let store = conversation_store::store().unwrap();
    let conn = store.conn().unwrap();
    let quarantined: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM projection_quarantine WHERE run_id=?1 AND reason='corrupt_message_completed'",
            params![run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        quarantined, 1,
        "corrupt event must be explicitly quarantined"
    );
}

/// TASK-005: projecting a turn whose stored content disagrees with the
/// events quarantines the conflict instead of silently overwriting.
#[test]
fn content_conflict_is_quarantined() {
    let ((conv, run), _dir) = setup();
    let events = typed_turn_events(&run, "t1");
    project_run_from_events(&conv, &run, &events).unwrap();
    // Tamper with the stored assistant block, then re-project the same
    // events: the projector must detect the disagreement.
    let store = conversation_store::store().unwrap();
    let conn = store.conn().unwrap();
    conn.execute(
        "UPDATE message_block SET block_json = '{\"type\":\"text\",\"text\":\"tampered\"}'
         WHERE message_id = 'msg-t1' AND block_type='text'",
        [],
    )
    .unwrap();
    drop(conn);
    let result = project_run_from_events(&conv, &run, &events);
    assert!(
        result.is_ok(),
        "conflict isolates the turn, it does not fail the run"
    );
    let store = conversation_store::store().unwrap();
    let conn = store.conn().unwrap();
    let quarantined: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM projection_quarantine
             WHERE run_id=?1 AND reason='content_conflict'",
            params![run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        quarantined, 1,
        "content conflict must be quarantined explicitly"
    );
    // The stored content is untouched — no silent overwrite.
    let stored: String = conn
        .query_row(
            "SELECT block_json FROM message_block
             WHERE message_id = 'msg-t1' AND block_type='text'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored.contains("tampered"));
}

/// MIG-004: projecting a run without an FK row (the retired legacy/symbolic
/// path that used to route to the compat reader) now fails closed with an
/// explicit error instead of materializing delta-only turns.
#[test]
fn missing_run_is_rejected_not_routed_to_compat() {
    let ((conv, _), _dir) = setup();
    let run = "run-1".to_string();
    // Pre-typed delta-only events (no typed turn, no run row) are what a
    // legacy/symbolic run carried.
    let events = vec![event(
        &run,
        1,
        RunEventKind::TextDelta {
            text: "hello".into(),
        },
    )];
    let error = project_run_from_events(&conv, &run, &events)
        .expect_err("a run without an FK row must be rejected, not routed to compat");
    assert!(
        error.message.contains("requires an existing run row"),
        "error must name the retired run-row requirement: {error:?}"
    );
    assert!(
        !error.retryable,
        "a missing run row is a data anomaly, not a retryable failure"
    );
    let messages = conversation_store::load_agent_messages(&conv).unwrap();
    assert!(
        messages.is_empty(),
        "rejected projection must not materialize messages"
    );
}

/// TASK-005 (B03): startup recovery backfills committed turns whose events
/// are in the log but whose projection never ran (crash before projection),
/// and is idempotent.
#[test]
fn recover_projections_backfills_unprojected_committed_turns() {
    let ((conv, run), _dir) = setup();
    let events = typed_turn_events(&run, "t1");
    // Simulate a crash before projection: events durable in the log, no
    // watermark, no projected messages.
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        for event in &events {
            conn.execute(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run,
                    event.run_sequence as i64,
                    event.payload.type_name(),
                    serde_json::to_string(event).unwrap(),
                    event.timestamp.to_rfc3339(),
                    event.event_id,
                ],
            )
            .unwrap();
        }
    }
    assert!(
        conversation_store::load_agent_messages(&conv)
            .unwrap()
            .is_empty(),
        "nothing projected before recovery"
    );
    let recovered = recover_projections().unwrap();
    assert_eq!(
        recovered.total_projected, 1,
        "the committed turn is backfilled at startup"
    );
    let messages = conversation_store::load_agent_messages(&conv).unwrap();
    assert_eq!(messages.len(), 2, "assistant + tool result after recovery");
    // Re-running recovery is a no-op.
    assert_eq!(
        recover_projections().unwrap().total_projected,
        0,
        "recovery is idempotent"
    );
}

// ---- T03: fail loud on DB/FK/commit errors, quarantine only corruption --

/// A plain DB error (constraint/fk failure injected via a trigger) must
/// fail the run and NOT be treated as a quarantine; the watermark stays
/// untouched and the next recovery succeeds after the fault clears.
#[test]
fn recovery_fails_on_db_failure_and_retries_after_fault_clears() {
    let ((conv, run), _dir) = setup();
    let events = typed_turn_events(&run, "t1");
    // Seed the events into the log so recovery sees a target run.
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        for event in &events {
            conn.execute(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run,
                    event.run_sequence as i64,
                    event.payload.type_name(),
                    serde_json::to_string(event).unwrap(),
                    event.timestamp.to_rfc3339(),
                    event.event_id,
                ],
            )
            .unwrap();
        }
    }
    // Inject a mid-transaction FK-style failure: every message_block insert
    // aborts, which rolls the whole per-turn transaction back.
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute_batch(
            "CREATE TRIGGER proj_test_fk_fail BEFORE INSERT ON message_block
             BEGIN
                 SELECT RAISE(ABORT, 'FOREIGN KEY constraint failed');
             END;",
        )
        .unwrap();
    }
    let failure = recover_projections().unwrap_err();
    assert!(
        failure.contains("projection recovery failed"),
        "recovery must fail loudly on a DB error, got: {failure}"
    );
    assert!(
        failure.contains("FOREIGN KEY constraint failed") || failure.contains("constraint"),
        "the injected FK failure must be surfaced, got: {failure}"
    );
    // Atomic rollback: nothing may become provider history.
    assert!(
        conversation_store::load_agent_messages(&conv)
            .unwrap()
            .is_empty(),
        "a failed per-turn transaction must leave no partial rows"
    );
    // The retry watermark was NOT advanced.
    let store = conversation_store::store().unwrap();
    let conn = store.conn().unwrap();
    let watermark: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM projection_watermark WHERE projector='conversation' AND run_id=?1",
            params![run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(watermark, 0, "retry watermark must be preserved");
    // Fault clears -> recovery succeeds and materializes the full turn.
    conn.execute_batch("DROP TRIGGER proj_test_fk_fail;")
        .unwrap();
    drop(conn);
    let report = recover_projections().unwrap();
    assert_eq!(
        report.total_projected, 1,
        "the next recovery continues after the fault clears"
    );
    assert_eq!(
        conversation_store::load_agent_messages(&conv)
            .unwrap()
            .len(),
        2,
        "assistant + tool result after the successful retry"
    );
}

/// A busy database aborts recovery with a retryable failure; after the
/// lock is released the same recovery succeeds.
#[test]
fn recovery_fails_on_busy_db_and_retries_after_lock_release() {
    let _guard = crate::storage::DataStore::env_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("projector-busy.db");
    // Short busy timeout so the held write lock surfaces SQLITE_BUSY
    // immediately instead of parking the test for 30s.
    std::env::set_var("NATIVES_TEST_BUSY_TIMEOUT_MS", "0");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
    let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
    let conv = format!("proj-busy-conv-{}", uuid::Uuid::new_v4());
    let run = format!("proj-busy-run-{}", uuid::Uuid::new_v4());
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id)
             VALUES (?1, 'chat', 'Busy Test', 'prov-1', 'model-1')",
            params![conv],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id)
             VALUES (?1, ?2, 'completed', 'prov-1', 'model-1')",
            params![run, conv],
        )
        .unwrap();
        for event in typed_turn_events(&run, "t1") {
            conn.execute(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run,
                    event.run_sequence as i64,
                    event.payload.type_name(),
                    serde_json::to_string(&event).unwrap(),
                    event.timestamp.to_rfc3339(),
                    event.event_id,
                ],
            )
            .unwrap();
        }
    }
    // Hold the SQLite write lock from a second connection. The short busy
    // timeout (still set) makes the projector's next write surface
    // SQLITE_BUSY immediately instead of parking for the default 30s.
    let lock_conn = rusqlite::Connection::open(&db).unwrap();
    lock_conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let failure = recover_projections().unwrap_err();
    assert!(
        failure.contains("projection recovery failed") || failure.contains("busy"),
        "recovery must fail on a busy database, got: {failure}"
    );
    assert!(
        conversation_store::load_agent_messages(&conv)
            .unwrap()
            .is_empty(),
        "busy failure must not leave partial rows"
    );
    // Release the lock; the retry watermark was preserved, so the same
    // recovery re-attempts and succeeds.
    drop(lock_conn);
    let report = recover_projections().unwrap();
    assert_eq!(
        report.total_projected, 1,
        "retry after lock release succeeds"
    );
    // Remove the test-only knob so it never leaks into a later test.
    std::env::remove_var("NATIVES_TEST_BUSY_TIMEOUT_MS");
}

/// The classifier must map busy/locked/disk-full to retryable and
/// constraint (FK) to fatal — the load-bearing distinction for the retry
/// watermark.
#[test]
fn db_error_classifier_maps_busy_full_to_retryable_and_fk_to_fatal() {
    let busy = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DatabaseBusy,
            extended_code: 5,
        },
        None,
    );
    let locked = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DatabaseLocked,
            extended_code: 6,
        },
        None,
    );
    let full = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DiskFull,
            extended_code: 13,
        },
        None,
    );
    let fk = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::ConstraintViolation,
            extended_code: 787, // SQLITE_CONSTRAINT_FOREIGNKEY
        },
        None,
    );
    for transient in [&busy, &locked, &full] {
        assert!(
            classify_db_error(transient),
            "{transient:?} must be retryable"
        );
    }
    assert!(
        !classify_db_error(&fk),
        "an FK violation is permanent, never retryable"
    );
}

/// Kill/restart atomicity (T03): when the per-turn transaction fails
/// mid-write, no partial rows become provider history, and a later restart
/// re-projects cleanly.
#[test]
fn failed_turn_never_leaves_partial_provider_history() {
    fn seed(run: &str, events: &[RunEventV2]) {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        for event in events {
            conn.execute(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run,
                    event.run_sequence as i64,
                    event.payload.type_name(),
                    serde_json::to_string(event).unwrap(),
                    event.timestamp.to_rfc3339(),
                    event.event_id,
                ],
            )
            .unwrap();
        }
    }
    let ((conv, run), _dir) = setup();
    let events_t1 = typed_turn_events(&run, "t1");
    // Second turn carries the next sequence range so the recovery query
    // (event_sequence > watermark) still sees it as unprojected.
    let events_t2: Vec<RunEventV2> = typed_turn_events(&run, "t2")
        .into_iter()
        .enumerate()
        .map(|(i, mut e)| {
            e.run_sequence += 7;
            e.global_sequence += 7;
            e.event_id = format!("evt-{run}-{}", 8 + i as u64);
            e
        })
        .collect();
    // First projection succeeds fully and is durable in the log.
    seed(&run, &events_t1);
    assert_eq!(
        project_run_from_events(&conv, &run, &events_t1)
            .unwrap()
            .projected,
        1
    );
    let baseline = conversation_store::load_agent_messages(&conv).unwrap();
    // Add the second turn, then inject a fault at its message insert.
    seed(&run, &events_t2);
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute_batch(
            "CREATE TRIGGER proj_test_fk_fail2 BEFORE INSERT ON message_block
             BEGIN
                 SELECT RAISE(ABORT, 'FOREIGN KEY constraint failed');
             END;",
        )
        .unwrap();
    }
    let mut events2 = events_t1.clone();
    events2.extend(events_t2);
    let failure = project_run_from_events(&conv, &run, &events2).unwrap_err();
    assert!(
        !failure.retryable || failure.message.contains("constraint"),
        "an FK-style abort is a fatal failure, got: {failure}"
    );
    let after_failure = conversation_store::load_agent_messages(&conv).unwrap();
    assert_eq!(
        baseline, after_failure,
        "the failed second turn must not alter the transcript"
    );
    // Restart (fault cleared): recovery completes the second turn.
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute_batch("DROP TRIGGER proj_test_fk_fail2;")
            .unwrap();
    }
    assert_eq!(
        recover_projections().unwrap().total_projected,
        1,
        "restart re-projects the failed turn"
    );
    assert_eq!(
        conversation_store::load_agent_messages(&conv)
            .unwrap()
            .len(),
        baseline.len() + 2,
        "second turn materialized after restart"
    );
}
