use super::*;
use assistant_protocol::v2::{RunEventKind, RunEventV2};
use test_support::*;

#[tokio::test]
async fn conversation_create_requires_project_id() {
    let error = request(
        names::CONVERSATION_CREATE,
        serde_json::json!({
            "mode": "agent",
            "title": "No project",
            "provider_id": "p",
            "model_id": "m"
        }),
    )
    .await
    .expect_err("conversation without a project must be rejected");
    assert_eq!(error, "project_id is required");
}
#[test]
fn list_hides_child_conversations_by_default() {
    let _guard = env_lock();
    let _restore = EnvRestore {
        db: std::env::var("NATIVES_DB_PATH").ok(),
        asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
        rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
    };
    let _clear_db = ClearTestDb;
    let dir = tempfile::tempdir().unwrap();
    let db = dir
        .path()
        .join(format!("natives-{}.db", uuid::Uuid::new_v4()));
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    let art = dir.path().join("artifacts");
    crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
    let _warm = crate::storage::DataStore::new(&db, &art).expect("migrate");

    ensure_conversation_stub("parent-list", "openai", "gpt-4o", None, None).unwrap();
    let binding = crate::subagent_store::RouteBinding {
        provider_id: "openai".into(),
        key_id: "k1".into(),
        model_id: "gpt-4o".into(),
    };
    let (_sid, child) = crate::subagent_store::create_hidden_child_session(
        "parent-list",
        None,
        None,
        "w",
        "task",
        &binding,
        None,
        None,
    )
    .unwrap();

    let listed = list(serde_json::json!({})).unwrap();
    let arr = listed.as_array().unwrap();
    assert!(arr.iter().any(|c| c["id"] == "parent-list"));
    assert!(!arr.iter().any(|c| c["id"] == child));

    let with_children = list(serde_json::json!({ "include_children": true })).unwrap();
    let arr2 = with_children.as_array().unwrap();
    assert!(arr2.iter().any(|c| c["id"] == child));
}
#[tokio::test]
async fn delete_hard_removes_conversation_but_keeps_usage_stats() {
    let _guard = env_lock();
    let _restore = EnvRestore {
        db: std::env::var("NATIVES_DB_PATH").ok(),
        asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
        rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
    };
    let _clear_db = ClearTestDb;
    let dir = tempfile::tempdir().unwrap();
    let db = dir
        .path()
        .join(format!("natives-del-{}.db", uuid::Uuid::new_v4()));
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    let art = dir.path().join("artifacts");
    crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
    let store = crate::storage::DataStore::new(&db, &art).expect("migrate");

    ensure_conversation_stub("conv-del", "openai", "gpt-4o", None, None).unwrap();
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO run (
                id, conversation_id, status, provider_id, model_id,
                total_input_tokens, total_output_tokens, created_at, started_at
             ) VALUES (?1, ?2, 'completed', 'openai', 'gpt-4o', 100, 50, ?3, ?3)",
            rusqlite::params!["run-del", "conv-del", "2026-07-23T12:00:00Z"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, conversation_id, role, status, input_tokens, output_tokens, created_at)
             VALUES (?1, ?2, 'user', 'complete', 0, 0, ?3)",
            rusqlite::params!["msg-del", "conv-del", "2026-07-23T12:00:00Z"],
        )
        .unwrap();
    }

    let out = request(
        names::CONVERSATION_DELETE,
        serde_json::json!({ "id": "conv-del" }),
    )
    .await
    .unwrap();
    assert_eq!(out["deleted"], true);
    assert_eq!(out["hard_deleted"], true);
    assert_eq!(out["usage_stats_preserved"], true);

    let conn = store.conn().unwrap();
    let conv_left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM conversation WHERE id = ?1",
            rusqlite::params!["conv-del"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(conv_left, 0);
    let msg_left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM message WHERE conversation_id = ?1",
            rusqlite::params!["conv-del"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(msg_left, 0);
    let run_left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM run WHERE conversation_id = ?1",
            rusqlite::params!["conv-del"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(run_left, 0);
    let usage_in: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(input_tokens),0) FROM usage_stats WHERE source='natives' AND model='gpt-4o'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        usage_in >= 100,
        "usage_stats should retain folded tokens, got {usage_in}"
    );
}
#[test]
fn backfill_context_snapshots_materializes_missing_row_from_committed_event() {
    let _guard = env_lock();
    let _restore = EnvRestore {
        db: std::env::var("NATIVES_DB_PATH").ok(),
        asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
        rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
    };
    let _clear_db = ClearTestDb;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("backfill-snapshot.db");
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
    let store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
    ensure_conversation_stub("backfill-conv", "openai", "gpt-4o", None, None).unwrap();
    store
        .conn()
        .unwrap()
        .execute(
            "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
             VALUES ('backfill-run', 'backfill-conv', 'running', 'openai', 'gpt-4o')",
            [],
        )
        .unwrap();
    // Simulate the crash gap: the ContextSnapshotCommitted event is durable
    // but the run-end projection never materialized the context_snapshot row.
    let event = RunEventV2 {
        event_id: uuid::Uuid::new_v4().to_string(),
        global_sequence: 0,
        run_sequence: 1,
        run_id: "backfill-run".into(),
        timestamp: chrono::Utc::now(),
        payload: RunEventKind::ContextSnapshotCommitted {
            snapshot_id: "snapshot-backfill".into(),
            turn_id: Some("turn-backfill".into()),
            source_revision: 1,
            input_message_ids: vec![],
            summary_message_id: None,
            replaced_range: None,
            algorithm_version: "compaction-v1".into(),
            provider_context_window: None,
            artifact_reference: None,
            snapshot_json: serde_json::json!([{
                "role": "system",
                "message_id": "summary-backfill",
                "content": "compacted"
            }]),
        },
    };
    store
        .conn()
        .unwrap()
        .execute(
            "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp)
             VALUES ('backfill-run', 1, 'context_snapshot_committed', ?1, datetime('now'))",
            rusqlite::params![serde_json::to_string(&event).unwrap()],
        )
        .unwrap();
    let before: i64 = store
        .conn()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM context_snapshot WHERE run_id = 'backfill-run'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        before, 0,
        "crash gap: the event is durable but no row exists"
    );

    let touched = backfill_context_snapshots().unwrap();
    assert_eq!(
        touched, 1,
        "one run with a snapshot event must be backfilled"
    );
    let after: i64 = store
        .conn()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM context_snapshot WHERE run_id = 'backfill-run'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        after, 1,
        "startup backfill must materialize the missing context_snapshot row"
    );
    // Idempotent: a second backfill does not duplicate the row.
    let touched_again = backfill_context_snapshots().unwrap();
    assert_eq!(touched_again, 1);
    let final_count: i64 = store
        .conn()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM context_snapshot WHERE run_id = 'backfill-run'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(final_count, 1, "backfill must be idempotent");
}
