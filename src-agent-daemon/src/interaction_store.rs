//! Daemon-owned interaction persistence (permission / ask recovery).
//!
//! Rows live in the `interaction` table. Used by:
//! - `interaction.listPending` / `interaction.respond` RPC
//! - best-effort insert when `PermissionGatedTools` emits PermissionRequested

use crate::storage::DataStore;
use assistant_protocol::v2::methods::names;
use assistant_protocol::v2::RunEventKind;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;

fn store() -> Result<DataStore, String> {
    #[cfg(test)]
    let _env_guard = crate::storage::DataStore::env_test_lock();
    #[cfg(test)]
    if let Some((db_path, artifact_dir)) = crate::storage::test_db_override() {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return DataStore::new(&db_path, &artifact_dir);
    }
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
        });
    #[cfg(test)]
    let db_path = db_path.ok_or_else(|| {
        "test store() requires NATIVES_ASSISTANT_DB_PATH or NATIVES_DB_PATH (refusing ~/.natives default)".to_string()
    })?;
    #[cfg(not(test))]
    let db_path = db_path.unwrap_or_else(crate::default_assistant_db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let artifact_dir = std::env::var("NATIVES_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".natives").join("runtime")
        })
        .join("artifacts");
    DataStore::new(&db_path, &artifact_dir)
}

fn row_to_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let payload_raw: String = row.get(5)?;
    let response_raw: Option<String> = row.get(6)?;
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "run_id": row.get::<_, Option<String>>(1)?,
        "conversation_id": row.get::<_, Option<String>>(2)?,
        "kind": row.get::<_, String>(3)?,
        "status": row.get::<_, String>(4)?,
        "payload": serde_json::from_str::<Value>(&payload_raw).unwrap_or(Value::Null),
        "response": response_raw.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()),
        "created_at": row.get::<_, String>(7)?,
        "responded_at": row.get::<_, Option<String>>(8)?,
    }))
}

/// Best-effort insert of a pending interaction (e.g. tool permission for restart recovery).
/// Uses `id` as primary key; ON CONFLICT DO NOTHING so duplicate emits are fine.
pub fn insert_pending(
    id: &str,
    run_id: Option<&str>,
    conversation_id: Option<&str>,
    kind: &str,
    payload: Value,
) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("interaction id required".into());
    }
    let kind = if kind.trim().is_empty() {
        "unknown"
    } else {
        kind.trim()
    };
    let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
    let store = store()?;
    let conn = store.conn()?;
    conn.execute(
        "INSERT INTO interaction (id, run_id, conversation_id, kind, status, payload)
         VALUES (?1, ?2, ?3, ?4, 'pending', ?5)
         ON CONFLICT(id) DO NOTHING",
        params![id, run_id, conversation_id, kind, payload_str],
    )
    .map_err(|e| format!("insert interaction failed: {e}"))?;
    Ok(())
}

/// Mark an interaction as resolved.  An already-resolved row is idempotent;
/// storage failures and missing pending rows remain errors so permission
/// callers cannot continue without a durable response fact.
pub fn mark_resolved(id: &str, response: Value) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let response_str = serde_json::to_string(&response).unwrap_or_else(|_| "null".into());
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let changed = conn
        .execute(
            "UPDATE interaction
         SET status = 'resolved', response = ?1, responded_at = ?2
         WHERE id = ?3 AND status = 'pending'",
            params![response_str, now, id],
        )
        .map_err(|e| format!("resolve interaction failed: {e}"))?;
    if changed == 0 {
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM interaction WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("check interaction status failed: {e}"))?;
        match status.as_deref() {
            Some("resolved") => {}
            Some(other) => return Err(format!("interaction {id} is not pending ({other})")),
            None => return Err(format!("interaction not found: {id}")),
        }
    }
    Ok(())
}

/// List pending interactions with optional conversation_id / run_id filters.
pub fn list_pending(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str);
    let run_id = params
        .get("run_id")
        .or_else(|| params.get("runId"))
        .and_then(Value::as_str);

    let store = store()?;
    let conn = store.conn()?;
    let sql = match (conversation_id, run_id) {
        (Some(_), Some(_)) => {
            "SELECT id, run_id, conversation_id, kind, status, payload, response, created_at, responded_at
             FROM interaction
             WHERE status = 'pending' AND conversation_id = ?1 AND run_id = ?2
             ORDER BY created_at ASC"
        }
        (Some(_), None) => {
            "SELECT id, run_id, conversation_id, kind, status, payload, response, created_at, responded_at
             FROM interaction
             WHERE status = 'pending' AND conversation_id = ?1
             ORDER BY created_at ASC"
        }
        (None, Some(_)) => {
            "SELECT id, run_id, conversation_id, kind, status, payload, response, created_at, responded_at
             FROM interaction
             WHERE status = 'pending' AND run_id = ?1
             ORDER BY created_at ASC"
        }
        (None, None) => {
            "SELECT id, run_id, conversation_id, kind, status, payload, response, created_at, responded_at
             FROM interaction
             WHERE status = 'pending'
             ORDER BY created_at ASC"
        }
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = match (conversation_id, run_id) {
        (Some(cid), Some(rid)) => stmt.query_map(params![cid, rid], row_to_json),
        (Some(cid), None) => stmt.query_map(params![cid], row_to_json),
        (None, Some(rid)) => stmt.query_map(params![rid], row_to_json),
        (None, None) => stmt.query_map([], row_to_json),
    }
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect::<Vec<_>>();

    Ok(json!({ "interactions": rows }))
}

/// Resolve a pending interaction (TASK-008 / B05).
///
/// The decision UPDATE and its delivery intent (`interaction_outbox`) commit in
/// one transaction; after commit, `deliver_outbox` appends the authoritative
/// `InteractionResponded` event and wakes the waiter, then marks the row
/// delivered. A duplicate response with the SAME decision is idempotent
/// (returns the original result); a conflicting decision is rejected. An
/// undelivered outbox row is replayed at daemon start, so a crash between the
/// decision and the event loses nothing and the handler never runs ahead of
/// its durable event.
pub async fn respond(params: Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .or_else(|| params.get("interaction_id"))
        .or_else(|| params.get("request_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if id.is_empty() {
        return Err("id required for interaction.respond".into());
    }

    let response = params
        .get("response")
        .cloned()
        .or_else(|| {
            // Convenience: flat approved/scope → response object (permission UI shape).
            let approved = params.get("approved").and_then(Value::as_bool);
            approved.map(|a| {
                json!({
                    "approved": a,
                    "scope": params.get("scope").and_then(Value::as_str).unwrap_or("once"),
                })
            })
        })
        .unwrap_or(Value::Null);
    let response_str = serde_json::to_string(&response).unwrap_or_else(|_| "null".into());
    let now = chrono::Utc::now().to_rfc3339();

    let store = store()?;
    let (run_id, conversation_id, kind, outbox_id) = {
        let conn = store.conn()?;
        #[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
        let existing: Option<(
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
        )> = conn
            .query_row(
                "SELECT run_id, conversation_id, kind, status, response, responded_at
                 FROM interaction WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let Some((run_id, conversation_id, kind, status, stored_response, stored_at)) = existing
        else {
            // Stable orphan code shared with permission.respond (task-04).
            return Err(format!("permission_orphaned: interaction not found: {id}"));
        };
        if status == "resolved" {
            // Idempotent duplicate: the same decision returns the original
            // result. A conflicting decision is rejected (A02/J04).
            if stored_response.as_deref() == Some(response_str.as_str()) {
                return Ok(json!({
                    "ok": true,
                    "id": id,
                    "status": "resolved",
                    "already_resolved": true,
                    "run_id": run_id,
                    "kind": kind,
                    "response": response,
                    "responded_at": stored_at.unwrap_or(now),
                }));
            }
            return Err(format!(
                "conflict: interaction {id} already resolved with a different decision"
            ));
        }
        if status != "pending" {
            // expired / cancelled after restart — not grantable.
            return Err(format!(
                "permission_orphaned: interaction not pending: {id} (status={status})"
            ));
        }

        // Decision + outbox delivery intent in ONE transaction (A02/J04).
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let changed = tx
            .execute(
                "UPDATE interaction
                 SET status = 'resolved', response = ?1, responded_at = ?2
                 WHERE id = ?3 AND status = 'pending'",
                params![response_str, now, id],
            )
            .map_err(|e| format!("update interaction failed: {e}"))?;
        if changed == 0 {
            return Err(format!("interaction not pending: {id}"));
        }
        let outbox_id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO interaction_outbox (id, interaction_id, run_id, kind, response)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![outbox_id, id, run_id, kind, response_str],
        )
        .map_err(|e| format!("insert interaction outbox: {e}"))?;
        tx.commit()
            .map_err(|e| format!("commit interaction outbox: {e}"))?;
        (run_id, conversation_id, kind, Some(outbox_id))
    }; // drop MutexGuard before any .await

    if let Some(outbox_id) = outbox_id {
        deliver_outbox(
            &outbox_id,
            &id,
            run_id.as_deref(),
            conversation_id.as_deref(),
            &kind,
            &response,
        )
        .await?;
    }

    Ok(json!({
        "ok": true,
        "id": id,
        "status": "resolved",
        "run_id": run_id,
        "kind": kind,
        "response": response,
        "responded_at": now,
    }))
}

/// Deliver a committed outbox decision. The authoritative event is appended
/// FIRST; if it fails, the outbox row stays undelivered (a restart replays it)
/// and the waiter is never woken ahead of its durable event. Only after the
/// event and the waiter wake succeed is the row marked delivered.
async fn deliver_outbox(
    outbox_id: &str,
    interaction_id: &str,
    run_id: Option<&str>,
    conversation_id: Option<&str>,
    kind: &str,
    response: &Value,
) -> Result<(), String> {
    if let Some(rid) = run_id.filter(|r| !r.is_empty()) {
        let event = crate::run_manager::global_run_manager().events().append(
            rid,
            RunEventKind::InteractionResponded {
                interaction_id: interaction_id.to_string(),
                response: response.clone(),
            },
        );
        if matches!(&event.payload, RunEventKind::Failed { code, .. } if code == "PERSISTENCE_FAILED")
        {
            return Err("PERSISTENCE_FAILED append InteractionResponded".into());
        }
    }

    if kind == "tool_permission" {
        let approved = response
            .get("approved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let scope = response.get("scope").and_then(Value::as_str);
        let _ = crate::run_manager::global_run_manager()
            .respond_permission_for_run(interaction_id, approved, run_id, scope)
            .await;
    }

    // subagent_assignment: wake batch waiters + persist route policy.
    if kind == "subagent_assignment" {
        let _ = crate::production::wake_assignment_waiter(interaction_id, response.clone());
        let cid = response
            .get("conversation_id")
            .or_else(|| response.get("parent_conversation_id"))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or_else(|| conversation_id.map(str::to_string));
        if let Some(cid) = cid {
            let bindings_val = response
                .get("pool")
                .or_else(|| response.get("bindings"))
                .cloned()
                .or_else(|| {
                    response.get("assignments").and_then(|arr| {
                        let list: Vec<crate::subagent_store::RouteBinding> = arr
                            .as_array()?
                            .iter()
                            .filter_map(|a| {
                                Some(crate::subagent_store::RouteBinding {
                                    provider_id: a.get("provider_id")?.as_str()?.to_string(),
                                    key_id: a.get("key_id")?.as_str()?.to_string(),
                                    model_id: a.get("model_id")?.as_str()?.to_string(),
                                })
                            })
                            .collect();
                        if list.is_empty() {
                            None
                        } else {
                            serde_json::to_value(list).ok()
                        }
                    })
                });
            if let Some(bindings) = bindings_val {
                if let Ok(list) = serde_json::from_value::<Vec<crate::subagent_store::RouteBinding>>(
                    bindings.clone(),
                ) {
                    if !list.is_empty() {
                        let mode = response
                            .get("mode")
                            .and_then(Value::as_str)
                            .unwrap_or("default");
                        let _ = crate::subagent_store::upsert_route_policy(&cid, mode, &list);
                    }
                }
            }
        }
    }

    let _ = store()?.conn()?.execute(
        "UPDATE interaction_outbox SET delivered = 1 WHERE id = ?1",
        params![outbox_id],
    );
    Ok(())
}

/// Replay undelivered decisions from the outbox at daemon start (B05). Each
/// undelivered row is delivered exactly once; a delivery failure leaves the row
/// for the next recovery pass. Returns the number of rows delivered.
pub async fn recover_interaction_outbox() -> Result<usize, String> {
    let store = store()?;
    #[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    )> = {
        let conn = store.conn()?;
        undelivered_outbox_rows(&conn)?
    };
    let mut delivered = 0;
    for (outbox_id, interaction_id, run_id, conversation_id, kind, response) in rows {
        let response: Value = serde_json::from_str(&response).unwrap_or(Value::Null);
        if deliver_outbox(
            &outbox_id,
            &interaction_id,
            run_id.as_deref(),
            conversation_id.as_deref(),
            &kind,
            &response,
        )
        .await
        .is_ok()
        {
            delivered += 1;
        }
    }
    Ok(delivered)
}

#[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
fn undelivered_outbox_rows(
    conn: &rusqlite::Connection,
) -> Result<
    Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    )>,
    String,
> {
    let mut stmt = conn
        .prepare(
            "SELECT id, interaction_id, run_id, conversation_id, kind, response
             FROM interaction_outbox WHERE delivered = 0",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// RPC entry: `interaction.*` methods.
pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        names::INTERACTION_LIST_PENDING => list_pending(params),
        names::INTERACTION_RESPOND => respond(params).await,
        _ => Err(format!("unsupported interaction method: {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store;
    use uuid::Uuid;

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
        let warm = crate::storage::DataStore::new(&db, &art).expect("interaction temp db migrate");
        assert!(
            warm.has_table("interaction"),
            "temp db missing interaction table: {}",
            db.display()
        );
        f();
        drop(warm);
        crate::storage::set_test_db_override(None, None);
        drop(dir);
    }

    #[test]
    fn insert_list_respond_roundtrip() {
        with_temp_db(|| {
            let cid = format!("ix-c-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid, "openai", "gpt-4o", None, None)
                .unwrap();
            // run_id is optional FK — leave None for pure store test
            let id = format!("ix-{}", Uuid::new_v4());
            insert_pending(
                &id,
                None,
                Some(&cid),
                "tool_permission",
                json!({
                    "tool_name": "write_file",
                    "reason": "Approve write_file?",
                }),
            )
            .unwrap();

            // Duplicate insert is best-effort no-op
            insert_pending(
                &id,
                None,
                Some(&cid),
                "tool_permission",
                json!({"tool_name": "write_file"}),
            )
            .unwrap();

            let listed = list_pending(json!({ "conversation_id": cid })).unwrap();
            let arr = listed["interactions"].as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0]["id"], id);
            assert_eq!(arr[0]["status"], "pending");
            assert_eq!(arr[0]["kind"], "tool_permission");
            assert_eq!(arr[0]["payload"]["tool_name"], "write_file");

            // Filter by missing run → empty
            let empty = list_pending(json!({ "run_id": "no-such-run" })).unwrap();
            assert!(empty["interactions"].as_array().unwrap().is_empty());

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let out = rt
                .block_on(respond(json!({
                    "id": id,
                    "approved": true,
                    "scope": "this_run",
                })))
                .unwrap();
            assert_eq!(out["status"], "resolved");
            assert_eq!(out["response"]["approved"], true);

            let listed = list_pending(json!({ "conversation_id": cid })).unwrap();
            assert!(listed["interactions"].as_array().unwrap().is_empty());

            // Same decision again is idempotent — returns the original result.
            let again = rt
                .block_on(respond(json!({
                    "id": id,
                    "response": { "approved": true, "scope": "this_run" },
                })))
                .unwrap();
            assert_eq!(again["already_resolved"], true);
            assert_eq!(again["response"]["approved"], true);

            // A conflicting decision is rejected.
            let err = rt
                .block_on(respond(json!({
                    "id": id,
                    "response": { "approved": false },
                })))
                .unwrap_err();
            assert!(err.contains("conflict"), "{err}");
        });
    }

    #[test]
    fn list_filters_by_run_and_conversation() {
        with_temp_db(|| {
            let cid_a = format!("ix-a-{}", Uuid::new_v4());
            let cid_b = format!("ix-b-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid_a, "openai", "gpt-4o", None, None)
                .unwrap();
            conversation_store::ensure_conversation_stub(&cid_b, "openai", "gpt-4o", None, None)
                .unwrap();

            insert_pending(
                &format!("ix-1-{}", Uuid::new_v4()),
                None,
                Some(&cid_a),
                "ask",
                json!({"q": 1}),
            )
            .unwrap();
            insert_pending(
                &format!("ix-2-{}", Uuid::new_v4()),
                None,
                Some(&cid_b),
                "ask",
                json!({"q": 2}),
            )
            .unwrap();

            let a = list_pending(json!({ "conversation_id": cid_a })).unwrap();
            assert_eq!(a["interactions"].as_array().unwrap().len(), 1);
            let all = list_pending(json!({})).unwrap();
            assert_eq!(all["interactions"].as_array().unwrap().len(), 2);
        });
    }

    /// TASK-008 (A02/J04): a duplicate response with the SAME decision is
    /// idempotent (returns the original result); a conflicting decision is
    /// rejected.
    #[test]
    fn interaction_idempotency_duplicate_returns_original_and_conflict_rejected() {
        with_temp_db(|| {
            let cid = format!("ix-c-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid, "openai", "gpt-4o", None, None)
                .unwrap();
            let id = format!("ix-{}", Uuid::new_v4());
            insert_pending(
                &id,
                None,
                Some(&cid),
                "tool_permission",
                json!({ "tool_name": "write_file" }),
            )
            .unwrap();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let out = rt
                .block_on(respond(json!({
                    "id": id,
                    "response": { "approved": true, "scope": "once" },
                })))
                .unwrap();
            assert_eq!(out["status"], "resolved");
            // Same decision → idempotent, returns the original result.
            let again = rt
                .block_on(respond(json!({
                    "id": id,
                    "response": { "approved": true, "scope": "once" },
                })))
                .unwrap();
            assert_eq!(again["already_resolved"], true);
            assert_eq!(again["response"]["approved"], true);
            // Conflicting decision → rejected.
            let err = rt
                .block_on(respond(json!({
                    "id": id,
                    "response": { "approved": false },
                })))
                .unwrap_err();
            assert!(err.contains("conflict"), "{err}");
        });
    }

    /// TASK-008 (A02/J04): an undelivered outbox row (crash after the decision
    /// commit, before delivery) is replayed by recovery and marked delivered.
    #[test]
    fn interaction_idempotency_outbox_recovery_delivers_undelivered() {
        with_temp_db(|| {
            let cid = format!("ix-c-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid, "openai", "gpt-4o", None, None)
                .unwrap();
            let id = format!("ix-{}", Uuid::new_v4());
            insert_pending(&id, None, Some(&cid), "custom", json!({})).unwrap();
            // Simulate a crash after the decision commit and before delivery:
            // resolved interaction + an undelivered outbox row.
            {
                let db = store().unwrap();
                let conn = db.conn().unwrap();
                conn.execute(
                    "UPDATE interaction SET status='resolved', response='{\"ok\":true}' WHERE id=?1",
                    params![id],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO interaction_outbox (id, interaction_id, run_id, kind, response)
                     VALUES ('outbox-1', ?1, NULL, 'custom', '{\"ok\":true}')",
                    params![id],
                )
                .unwrap();
            }
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let delivered = rt.block_on(recover_interaction_outbox()).unwrap();
            assert_eq!(delivered, 1, "undelivered decision is replayed");
            let db = store().unwrap();
            let conn = db.conn().unwrap();
            let marked: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM interaction_outbox WHERE id='outbox-1' AND delivered=1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(marked, 1, "delivered row is marked delivered");
        });
    }

    /// TASK-008 (A02/J04): when the authoritative event append fails, delivery
    /// stops and the outbox row stays undelivered — the waiter (handler) is
    /// never woken ahead of a durable event.
    #[test]
    fn interaction_idempotency_event_failure_blocks_delivery() {
        with_temp_db(|| {
            // T01: bind the process-global RunManager to THIS test's temp store
            // so outbox delivery appends against the migrated schema (FK on
            // run_event.run_id -> run.id). Deterministic and never ~/.natives.
            let global_store = std::sync::Arc::new(store().unwrap());
            crate::run_manager::install_global_for_test(
                crate::run_manager::RunManager::new_with_store(global_store),
            );
            // The outbox has no run FK, so a row can reference a deleted run.
            // Delivering it appends InteractionResponded against a missing run,
            // which fails; the row must stay undelivered for a later replay.
            {
                let db = store().unwrap();
                let conn = db.conn().unwrap();
                conn.execute(
                    "INSERT INTO interaction_outbox (id, interaction_id, run_id, kind, response)
                     VALUES ('outbox-fail', 'ix-fail', 'deleted-run', 'tool_permission', '{\"approved\":true}')",
                    [],
                )
                .unwrap();
            }
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let delivered = rt.block_on(recover_interaction_outbox()).unwrap();
            assert_eq!(
                delivered, 0,
                "a failing delivery is not counted as delivered"
            );
            let db = store().unwrap();
            let conn = db.conn().unwrap();
            let still_undelivered: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM interaction_outbox WHERE id='outbox-fail' AND delivered=0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                still_undelivered, 1,
                "event failure leaves the row for replay"
            );
        });
    }
}
