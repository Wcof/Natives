//! A8 — watermark-driven incremental projection tests (standalone module so it
//! does not depend on the private `mod tests` helpers).

use super::*;
use crate::conversation_store;
use agent_core::ContentBlock;

fn setup() -> ((String, String), tempfile::TempDir) {
    let _guard = crate::storage::DataStore::env_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("proj-incremental.db");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
    let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
    let conv = format!("proj-incr-conv-{}", uuid::Uuid::new_v4());
    let run = format!("proj-incr-run-{}", uuid::Uuid::new_v4());
    {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id)
             VALUES (?1, 'chat', 'Projector Incremental', 'prov-1', 'model-1')",
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

fn typed_turn_events(run_id: &str, turn: &str, base: u64) -> Vec<RunEventV2> {
    let mut events = Vec::new();
    let mut push = |kind: RunEventKind, offset: u64| {
        let seq = base + offset;
        events.push(event(run_id, seq, kind));
    };
    push(
        RunEventKind::TurnStarted {
            turn_id: turn.into(),
        },
        0,
    );
    push(
        RunEventKind::MessageStarted {
            turn_id: turn.into(),
            message_id: format!("msg-{turn}"),
            role: "assistant".into(),
        },
        1,
    );
    push(
        RunEventKind::TextDelta {
            text: "hello".into(),
        },
        2,
    );
    push(
        RunEventKind::ToolCallRequested {
            id: format!("call-{turn}-1"),
            name: "read_file".into(),
            input: serde_json::json!({"path": "/tmp/a.txt"}),
        },
        3,
    );
    push(
        RunEventKind::ToolCallCompleted {
            id: format!("call-{turn}-1"),
            name: "read_file".into(),
            output: serde_json::json!({"content": "file body"}),
            is_error: false,
            duration_ms: 2,
            result_message_id: Some(format!("rm-{turn}-1")),
        },
        4,
    );
    push(
        RunEventKind::MessageCompleted {
            turn_id: turn.into(),
            message_id: format!("msg-{turn}"),
            role: "assistant".into(),
            content: Some(serde_json::json!({
                "message_id": format!("msg-{turn}"),
                "role": "assistant",
                "content": serde_json::to_value(vec![
                    agent_core::ContentBlock::Text { text: "hello".into() },
                ]).unwrap(),
            })),
        },
        5,
    );
    push(
        RunEventKind::TurnCompleted {
            turn_id: turn.into(),
            stop_reason: "stop".into(),
            input_tokens: 10,
            output_tokens: 5,
        },
        6,
    );
    events
}

fn persist_events(run_id: &str, events: &[RunEventV2]) {
    let store = conversation_store::store().unwrap();
    let conn = store.conn().unwrap();
    for event in events {
        conn.execute(
            "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run_id.to_string(),
                event.run_sequence as i64,
                event.payload.type_name().to_string(),
                serde_json::to_string(event).unwrap(),
                event.timestamp.to_rfc3339(),
                event.event_id.clone(),
            ],
        )
        .unwrap();
    }
}

/// A8: incremental projection replays only events AFTER the watermark and
/// skips the already-projected prefix; second pass is a no-op.
#[test]
fn projector_incremental_uses_watermark_prefix() {
    let ((conv, run), _dir) = setup();
    let t1 = typed_turn_events(&run, "t1", 0);
    let t2 = typed_turn_events(&run, "t2", 7);
    persist_events(&run, &t1);
    persist_events(&run, &t2);

    // Project turn 1 only (watermark now at sequence 7).
    let first = project_run_from_events(&conv, &run, &t1).unwrap();
    assert_eq!(first.projected, 1);

    // Incremental pass picks up ONLY turn 2.
    let incremental = project_run_incremental(&conv, &run).unwrap();
    assert_eq!(incremental.projected, 1, "only the new turn projects");
    assert_eq!(incremental.already_projected, 0);
    let messages = conversation_store::load_agent_messages(&conv).unwrap();
    assert_eq!(messages.len(), 4, "both turns' assistant + tool results");
    // MessageCompleted.content remains the committed-content authority.
    let assistant_texts: Vec<String> = messages
        .iter()
        .filter_map(|m| match m {
            agent_core::AgentMessage::Assistant(a) => Some(
                a.content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(
        assistant_texts,
        vec!["hello".to_string(), "hello".to_string()]
    );
    // Second incremental pass is a no-op.
    let again = project_run_incremental(&conv, &run).unwrap();
    assert_eq!(again.projected, 0);
    assert_eq!(
        conversation_store::load_agent_messages(&conv)
            .unwrap()
            .len(),
        4
    );
}

/// §5 exact-name regression: projector uses watermark prefix (A8).
#[test]
fn projector_uses_watermark_prefix() {
    projector_incremental_uses_watermark_prefix();
}
