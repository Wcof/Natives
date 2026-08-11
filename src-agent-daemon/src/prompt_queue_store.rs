//! Daemon-owned prompt queue + SessionCoordinator persistence.
//!
//! **Single write authority for Assistant execution data (queue / actor):**
//! this module writes `prompt_queue` + `session_actor` on the Daemon's
//! `assistant.db`. Host must not dual-write queue rows in UDS production.
//!
//! In-memory [`SessionCoordinator`] (agent-core) is the live coordination
//! actor; SQLite is the durable source of truth across restarts.

use crate::conversation_store;
use crate::run_manager::global_run_manager;
use crate::storage::DataStore;
use agent_core::{
    CoordinatorAction, DrainMode, EngineInputReceiver, EngineSafePointReceiver, HarnessAction,
    InputSafePoint, PendingInput, PendingInputKind, PromptSource, QueueItem, QueueItemStatus,
    SafePoint, SessionActorSnapshot, SessionCoordinator,
};
use assistant_protocol::v2::methods::names;
use assistant_protocol::v2::{CancelRunRequest, StartRunRequest};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

static GLOBAL_HARNESS: OnceLock<Arc<SessionCoordinator>> = OnceLock::new();

pub fn global_harness() -> Arc<SessionCoordinator> {
    GLOBAL_HARNESS
        .get_or_init(SessionCoordinator::shared)
        .clone()
}

/// Alias for clarity at call sites.
pub fn global_coordinator() -> Arc<SessionCoordinator> {
    global_harness()
}

/// SQLite-backed input lease used by the engine at safe points.
// W9 split: durable receivers -> prompt_queue_receiver, snapshot persistence ->
// prompt_queue_snapshot, promptQueue.* RPC -> prompt_queue_crud. Public paths
// are re-exported below so external `prompt_queue_store::*` keeps working.
#[path = "prompt_queue_crud.rs"]
mod prompt_queue_crud;
#[path = "prompt_queue_receiver.rs"]
mod prompt_queue_receiver;
#[path = "prompt_queue_snapshot.rs"]
mod prompt_queue_snapshot;
pub(crate) use prompt_queue_crud::{
    enqueue, interject, list, remove, reorder, request, send_now, update,
};
pub(crate) use prompt_queue_receiver::{DurableInputReceiver, DurableSafePointReceiver};
pub(crate) use prompt_queue_snapshot::{
    hydrate_conversation, load_actor_snapshot, persist_actor_snapshot,
    recover_session_actors_on_startup,
};

pub fn on_safe_point_checked(
    conversation_id: &str,
    point: SafePoint,
) -> Result<HarnessAction, String> {
    let action = global_harness().on_safe_point(conversation_id, point);
    if let CoordinatorAction::InjectInterjection { content } = &action {
        if let Err(error) = persist_actor_snapshot(conversation_id) {
            global_harness().restore_interjection(conversation_id, content.clone());
            return Err(error);
        }
    } else if !matches!(action, CoordinatorAction::None) {
        persist_actor_snapshot(conversation_id)?;
    }
    Ok(action)
}

/// Compatibility wrapper for non-engine harness callers. Production Core
/// paths use `on_safe_point_checked` so persistence errors stop the run.
pub fn on_safe_point(conversation_id: &str, point: SafePoint) -> HarnessAction {
    match on_safe_point_checked(conversation_id, point) {
        Ok(action) => action,
        Err(error) => {
            eprintln!("[prompt_queue] safe-point persistence failed: {error}");
            CoordinatorAction::None
        }
    }
}

pub fn restore_interjection_checked(conversation_id: &str, content: String) -> Result<(), String> {
    global_harness().restore_interjection(conversation_id, content);
    persist_actor_snapshot(conversation_id)
}

// W9 split note: the following helpers were split out with the receiver /
// snapshot / crud domains but are referenced by those sibling modules via
// `super::`. They live here (the aggregate module) so all split modules can
// reach them without a crate-private cycle.

pub(crate) fn store() -> Result<DataStore, String> {
    // W2: single Daemon DataStore open path (assistant.db authority + test hook).
    crate::storage::open_daemon_store()
}

pub(crate) fn ensure_conversation_for_queue(conversation_id: &str, params: &Value) -> Result<(), String> {
    let provider = params
        .get("provider_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let model = params
        .get("model_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let project = params.get("project_id").and_then(Value::as_str);
    conversation_store::ensure_conversation_stub(conversation_id, provider, model, None, project)
}

pub(crate) fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let attachments: Option<String> = row.get(4)?;
    // status column may be missing on pre-migration-012 DBs mid-upgrade.
    let status: String = row.get::<_, String>(9).unwrap_or_else(|_| "queued".into());
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "conversation_id": row.get::<_, String>(1)?,
        "content": row.get::<_, String>(2)?,
        "source": row.get::<_, String>(3)?,
        "attachments": attachments.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()),
        "order": row.get::<_, i64>(5)?,
        "position": row.get::<_, i64>(5)?,
        "client_temp_id": row.get::<_, Option<String>>(6)?,
        "created_at": row.get::<_, String>(7)?,
        "updated_at": row.get::<_, String>(8)?,
        "status": status,
    }))
}

pub(crate) fn value_to_queue_item(item: &Value) -> Result<QueueItem, String> {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "prompt queue row missing id".to_string())?
        .to_string();
    let conversation_id = item
        .get("conversation_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("prompt queue row {id} missing conversation_id"))?
        .to_string();
    let content = item
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("prompt queue row {id} missing content"))?
        .to_string();
    let source = item
        .get("source")
        .and_then(Value::as_str)
        .map(PromptSource::parse)
        .ok_or_else(|| format!("prompt queue row {id} missing source"))?;
    let position = item
        .get("position")
        .or_else(|| item.get("order"))
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("prompt queue row {id} missing position"))?;
    let client_temp_id = item
        .get("client_temp_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let created_at = item
        .get("created_at")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("prompt queue row {id} missing created_at"))?
        .to_string();
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .map(QueueItemStatus::parse)
        .ok_or_else(|| format!("prompt queue row {id} missing status"))?;
    Ok(QueueItem {
        id,
        conversation_id,
        content,
        source,
        position,
        client_temp_id,
        created_at,
        status,
    })
}

/// Called when a run reaches a real terminal state.
/// Atomically claims finish via `finish_run(expected_run_id)` so concurrent
/// send_now / duplicate terminals never double-advance the queue.
pub async fn on_run_terminal(
    conversation_id: &str,
    run_id: &str,
    success: bool,
) -> Result<Option<String>, String> {
    let harness = global_harness();
    let Some(action) = harness.finish_run(conversation_id, run_id, success) else {
        // Stale terminal (wrong run id, already finished, or empty active).
        return Ok(None);
    };
    persist_actor_snapshot(conversation_id)?;

    match action {
        CoordinatorAction::StartPrompt { item } => {
            let store = store()?;
            let (provider_id, model_id, project_path) = {
                let conn = store.conn()?;
                conn.query_row(
                    "SELECT provider_id, model_id, project_id FROM conversation WHERE id = ?1",
                    params![conversation_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("conversation not found: {conversation_id}"))?
            };

            let start_req = StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: None,
                conversation_id: Some(conversation_id.to_string()),
                provider_id: Some(provider_id),
                model_id: Some(model_id),
                key_id: None,
                content: Some(item.content.clone()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: None,
                max_steps: None,
                project_path,
                // Unified with send_now: prompt-queue:{item_id}
                idempotency_key: Some(format!("prompt-queue:{}", item.id)),
                effort: None,
                runtime_id: None,
            };
            let run = match crate::run_manager::RunManager::start_detached_global(start_req) {
                Ok(run) => run,
                Err(error) => {
                    harness.requeue(conversation_id, item.clone());
                    persist_actor_snapshot(conversation_id)?;
                    return Err(error);
                }
            };
            let conn = store.conn()?;
            let now = chrono::Utc::now().to_rfc3339();
            harness.mark_running_item(conversation_id, &run.id, Some(&item.id), &item.content);
            let changed = conn
                .execute(
                    "UPDATE prompt_queue SET status = 'sent', updated_at = ?1 WHERE id = ?2",
                    params![now, item.id],
                )
                .map_err(|e| format!("mark queued prompt sent: {e}"))?;
            if changed != 1 {
                return Err("queued prompt was not transitioned to sent".into());
            }
            conn.execute("DELETE FROM prompt_queue WHERE id = ?1", params![item.id])
                .map_err(|e| format!("remove sent prompt: {e}"))?;
            persist_actor_snapshot(conversation_id)?;
            Ok(Some(run.id))
        }
        _ => Ok(None),
    }
}

/// Convert harness QueueItem to JSON (tests / diagnostics).
pub fn queue_item_json(item: &QueueItem) -> Value {
    json!({
        "id": item.id,
        "conversation_id": item.conversation_id,
        "content": item.content,
        "source": item.source.as_str(),
        "order": item.position,
        "client_temp_id": item.client_temp_id,
        "created_at": item.created_at,
        "status": item.status.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_lock() -> crate::storage::EnvTestGuard {
        crate::storage::DataStore::env_test_lock()
    }

    struct EnvRestore {
        db: Option<String>,
        asst: Option<String>,
        rt: Option<String>,
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(v) = self.db.take() {
                std::env::set_var("NATIVES_DB_PATH", v);
            } else {
                std::env::remove_var("NATIVES_DB_PATH");
            }
            if let Some(v) = self.asst.take() {
                std::env::set_var("NATIVES_ASSISTANT_DB_PATH", v);
            } else {
                std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
            }
            if let Some(v) = self.rt.take() {
                std::env::set_var("NATIVES_RUNTIME_DIR", v);
            } else {
                std::env::remove_var("NATIVES_RUNTIME_DIR");
            }
        }
    }

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("assistant-{}.db", Uuid::new_v4()));
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let warm = crate::storage::DataStore::new(&db, &art).expect("prompt_queue temp db migrate");
        assert!(
            warm.has_table("conversation") && warm.has_table("prompt_queue"),
            "temp db missing tables: {}",
            db.display()
        );
        f();
        drop(warm);
        crate::storage::set_test_db_override(None, None);
        drop(dir);
        // _restore drops here even on panic
    }

    #[test]
    fn db_enqueue_list_order() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid, "openai", "gpt-4o", None, None)
                .unwrap();
            let a = enqueue(json!({
                "conversation_id": cid,
                "content": "one",
            }))
            .unwrap();
            let b = enqueue(json!({
                "conversation_id": cid,
                "content": "two",
            }))
            .unwrap();
            assert_eq!(a["order"], 0);
            assert_eq!(b["order"], 1);
            let list = list(json!({ "conversation_id": cid })).unwrap();
            let arr = list.as_array().unwrap();
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0]["content"], "one");
            assert_eq!(arr[1]["content"], "two");
        });
    }

    #[test]
    fn malformed_queue_snapshot_is_rejected() {
        assert!(value_to_queue_item(&json!({
            "id": "q1",
            "conversation_id": "c1",
            "content": "prompt",
            "source": "user",
            "position": 0,
            "created_at": "2026-01-01T00:00:00Z"
        }))
        .is_err());
    }

    #[test]
    fn db_update_remove_reorder() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid, "openai", "gpt-4o", None, None)
                .unwrap();
            let a = enqueue(json!({"conversation_id": cid, "content": "a"})).unwrap();
            let b = enqueue(json!({"conversation_id": cid, "content": "b"})).unwrap();
            let id_a = a["id"].as_str().unwrap().to_string();
            let id_b = b["id"].as_str().unwrap().to_string();
            update(json!({"id": id_a, "content": "a2"})).unwrap();
            reorder(json!({
                "conversation_id": cid,
                "ids": [id_b, id_a],
            }))
            .unwrap();
            let listed = list(json!({"conversation_id": cid})).unwrap();
            let arr = listed.as_array().unwrap();
            assert_eq!(arr[0]["id"], id_b);
            assert_eq!(arr[1]["content"], "a2");
            remove(json!({"id": id_b})).unwrap();
            let listed = list(json!({"conversation_id": cid})).unwrap();
            assert_eq!(listed.as_array().unwrap().len(), 1);
        });
    }

    #[test]
    fn interject_is_a_durable_steering_queue_item() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            let result = interject(json!({
                "conversation_id": cid,
                "content": "inject me",
            }))
            .unwrap();
            assert_eq!(result["status"], "queued");
            assert!(global_harness().pending_interjection(&cid).is_none());
            let queued = global_harness().list(&cid);
            assert_eq!(queued.len(), 1);
            assert_eq!(queued[0].source, PromptSource::Interjection);
        });
    }

    #[test]
    fn steering_queue_survives_coordinator_rehydrate() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            interject(json!({
                "conversation_id": cid,
                "content": "consume durably",
            }))
            .unwrap();
            global_harness().clear_conversation(&cid);
            hydrate_conversation(&cid).unwrap();
            assert_eq!(global_harness().list(&cid).len(), 1);
        });
    }

    #[test]
    fn interject_does_not_use_legacy_pending_slot() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            interject(json!({
                "conversation_id": cid,
                "content": "durable inject",
            }))
            .unwrap();
            assert!(global_harness().pending_interjection(&cid).is_none());
        });
    }

    #[test]
    fn durable_steering_ack_removes_live_queue_item() {
        with_temp_db(|| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                let cid = format!("pq-{}", Uuid::new_v4());
                let run_id = format!("run-{}", Uuid::new_v4());
                interject(json!({
                    "conversation_id": cid,
                    "content": "ack me",
                }))
                .unwrap();
                let receiver = DurableInputReceiver::new(&cid, &run_id);
                store()
                    .unwrap()
                    .conn()
                    .unwrap()
                    .execute(
                        "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                         VALUES (?1, ?2, 'running', 'test', 'test')",
                        params![run_id, cid],
                    )
                    .unwrap();
                let mut inputs = receiver
                    .drain(
                        PendingInputKind::Steering,
                        DrainMode::All,
                        InputSafePoint::AfterToolBatch,
                    )
                    .await
                    .unwrap();
                assert_eq!(inputs.len(), 1);
                let input = inputs.pop().unwrap();
                receiver.ack(&input, None).await.unwrap();
                assert!(global_harness().list(&cid).is_empty());
            });
        });
    }

    #[test]
    fn steering_lease_excludes_a_second_run() {
        with_temp_db(|| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                let cid = format!("pq-{}", Uuid::new_v4());
                let run1 = format!("run-{}", Uuid::new_v4());
                let run2 = format!("run-{}", Uuid::new_v4());
                interject(json!({
                    "conversation_id": cid,
                    "content": "single lease",
                }))
                .unwrap();
                let s = store().unwrap();
                {
                    let conn = s.conn().unwrap();
                    for run_id in [&run1, &run2] {
                        conn.execute(
                            "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                             VALUES (?1, ?2, 'running', 'test', 'test')",
                            params![run_id, cid],
                        )
                        .unwrap();
                    }
                }
                let first = DurableInputReceiver::new(&cid, &run1);
                let second = DurableInputReceiver::new(&cid, &run2);
                let leased = first
                    .drain(
                        PendingInputKind::Steering,
                        DrainMode::All,
                        InputSafePoint::AfterToolBatch,
                    )
                    .await
                    .unwrap();
                assert_eq!(leased.len(), 1, "first run leases the steering input");
                let second_attempt = second
                    .drain(
                        PendingInputKind::Steering,
                        DrainMode::All,
                        InputSafePoint::AfterToolBatch,
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    second_attempt.len(),
                    0,
                    "a second run must not lease an already-leased steering input"
                );
            });
        });
    }

    #[test]
    fn migration_012_session_actor_table_exists() {
        with_temp_db(|| {
            let s = store().unwrap();
            assert!(
                s.has_table("session_actor"),
                "migration 012 must create session_actor"
            );
            assert!(s.has_table("prompt_queue"));
            let conn = s.conn().unwrap();
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('prompt_queue') WHERE name = 'status'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            assert_eq!(n, 1, "prompt_queue.status column required");
        });
    }

    #[test]
    fn send_now_cancels_active_and_starts_new_run() {
        with_temp_db(|| {
            // Fixture provider + same global RunManager that send_now cancels/starts.
            std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let cid = format!("pq-sendnow-{}", Uuid::new_v4());
                conversation_store::ensure_conversation_stub(
                    &cid,
                    "openai",
                    "gpt-4o",
                    None,
                    Some("/tmp"),
                )
                .unwrap();
                // Use process-global manager: send_now cancels via global_run_manager().
                // start_detached requires &Arc<Self>; use start_detached_global.
                let rm = crate::run_manager::global_run_manager();
                let active = rm
                    .create_run(assistant_protocol::v2::CreateRunRequest {
                        capability_selection: None,
                        disabled_tools: None,
                        conversation_id: cid.clone(),
                        provider_id: "openai".into(),
                        model_id: "gpt-4o".into(),
                        key_id: Some("k".into()),
                        agent_profile_id: None,
                        permission_profile: Some("ask".into()),
                        content: Some("running".into()),
                        attachments: None,
                        max_steps: Some(3),
                        parent_run_id: None,
                        project_path: Some("/tmp".into()),
                        idempotency_key: Some(format!("active-{}", Uuid::new_v4())),
                        effort: None,
                        runtime_id: Some("native".into()),
                    })
                    .unwrap();
                let _ = crate::run_manager::RunManager::start_detached_global(
                    assistant_protocol::v2::StartRunRequest {
                        agent_profile_id: None,
                        capability_selection: None,
                        run_id: Some(active.id.clone()),
                        conversation_id: Some(cid.clone()),
                        provider_id: Some("openai".into()),
                        model_id: Some("gpt-4o".into()),
                        key_id: Some("k".into()),
                        content: Some("running".into()),
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: Some("ask".into()),
                        max_steps: Some(3),
                        project_path: Some("/tmp".into()),
                        idempotency_key: None,
                        effort: None,
                        runtime_id: Some("native".into()),
                    },
                )
                .unwrap();
                global_harness().mark_running(&cid, &active.id, "running");

                let queued = enqueue(json!({
                    "conversation_id": cid,
                    "content": "send me now",
                }))
                .unwrap();
                let qid = queued["id"].as_str().unwrap().to_string();

                let started = send_now(json!({ "id": qid })).await.unwrap();
                assert!(
                    started.get("id").and_then(|v| v.as_str()).is_some(),
                    "send_now should return a run: {started}"
                );
                let new_id = started["id"].as_str().unwrap().to_string();
                assert_ne!(new_id, active.id, "should start a new run id");

                // Poll until original is terminal (cancel is async via global manager).
                let mut orig_terminal = false;
                for _ in 0..80 {
                    if let Some(orig) = rm.get_run(&active.id) {
                        if orig.status.is_terminal() {
                            orig_terminal = true;
                            break;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                assert!(
                    orig_terminal,
                    "active run should become terminal after send_now"
                );

                let listed = list(json!({ "conversation_id": cid })).unwrap();
                let arr = listed.as_array().cloned().unwrap_or_default();
                assert!(
                    arr.iter()
                        .all(|i| i.get("id").and_then(|v| v.as_str()) != Some(qid.as_str())),
                    "queue should not contain send_now item: {listed}"
                );
            });
        });
    }

    /// TASK-010 (C02): a crash AFTER lease but BEFORE ack is at-least-once —
    /// the row is reclaimed to queued on restart and a fresh run can drain it
    /// (nothing lost, no double-commit of the same lease).
    #[test]
    fn prompt_queue_crash_reclaims_unacked_lease_on_restart() {
        with_temp_db(|| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                let cid = format!("pq-crash-{}", Uuid::new_v4());
                let run1 = format!("run-crash-{}", Uuid::new_v4());
                let run2 = format!("run-crash-{}", Uuid::new_v4());
                interject(json!({
                    "conversation_id": cid,
                    "content": "survive the crash",
                }))
                .unwrap();
                let s = store().unwrap();
                {
                    let conn = s.conn().unwrap();
                    for run_id in [&run1, &run2] {
                        conn.execute(
                            "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                             VALUES (?1, ?2, 'running', 'test', 'test')",
                            params![run_id, cid],
                        )
                        .unwrap();
                    }
                }

                // Pre-crash run leases the input, then "crashes" before ack.
                let first = DurableInputReceiver::new(&cid, &run1);
                let leased = first
                    .drain(
                        PendingInputKind::Steering,
                        DrainMode::All,
                        InputSafePoint::AfterToolBatch,
                    )
                    .await
                    .unwrap();
                assert_eq!(leased.len(), 1, "the input is leased");
                let status_after_lease: String = store()
                    .unwrap()
                    .conn()
                    .unwrap()
                    .query_row(
                        "SELECT status FROM prompt_queue WHERE conversation_id = ?1",
                        params![cid],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(status_after_lease, "leased");

                // Restart: the stale lease is reclaimed to queued.
                let reclaimed = recover_session_actors_on_startup().unwrap();
                assert!(reclaimed >= 1, "recovery ran");
                let status_after_recovery: String = store()
                    .unwrap()
                    .conn()
                    .unwrap()
                    .query_row(
                        "SELECT status FROM prompt_queue WHERE conversation_id = ?1",
                        params![cid],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(
                    status_after_recovery, "queued",
                    "a crashed lease is reclaimed, never lost"
                );

                // A fresh run (restart) can drain it again — at-least-once.
                let second = DurableInputReceiver::new(&cid, &run2);
                let re_leased = second
                    .drain(
                        PendingInputKind::Steering,
                        DrainMode::All,
                        InputSafePoint::AfterToolBatch,
                    )
                    .await
                    .unwrap();
                assert_eq!(re_leased.len(), 1, "the reclaimed input is redelivered");
            });
        });
    }
}
