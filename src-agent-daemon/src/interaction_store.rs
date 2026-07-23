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

/// Best-effort mark an interaction as resolved (e.g. from permission.respond path).
/// No-op when the row is missing or already non-pending.
pub fn mark_resolved(id: &str, response: Value) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let response_str = serde_json::to_string(&response).unwrap_or_else(|_| "null".into());
    let now = chrono::Utc::now().to_rfc3339();
    let store = match store() {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    let conn = match store.conn() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let _ = conn.execute(
        "UPDATE interaction
         SET status = 'resolved', response = ?1, responded_at = ?2
         WHERE id = ?3 AND status = 'pending'",
        params![response_str, now, id],
    );
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

/// Resolve a pending interaction. Emits `InteractionResponded` on the run event bus
/// when `run_id` is present. For `tool_permission` rows, also best-effort wakes the
/// live permission waiter via RunManager.
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
    let (run_id, conversation_id, kind) = {
        let conn = store.conn()?;

        // Load current row (for run_id / kind) before update.
        let existing: Option<(Option<String>, Option<String>, String, String)> = conn
            .query_row(
                "SELECT run_id, conversation_id, kind, status FROM interaction WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let Some((run_id, conversation_id, kind, status)) = existing else {
            // Stable orphan code shared with permission.respond (task-04).
            return Err(format!("permission_orphaned: interaction not found: {id}"));
        };
        if status == "resolved" {
            return Err(format!("already_resolved: interaction {id}"));
        }
        if status != "pending" {
            // expired / cancelled after restart — not grantable.
            return Err(format!(
                "permission_orphaned: interaction not pending: {id} (status={status})"
            ));
        }

        let changed = conn
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
        (run_id, conversation_id, kind)
    }; // drop MutexGuard before any .await

    // Emit InteractionResponded on the run bus when we know the run.
    if let Some(ref rid) = run_id {
        if !rid.is_empty() {
            crate::run_manager::global_run_manager().events().append(
                rid,
                RunEventKind::InteractionResponded {
                    interaction_id: id.clone(),
                    response: response.clone(),
                },
            );
        }
    }

    // tool_permission recovery: also wake live permission waiter if still present.
    if kind == "tool_permission" {
        let approved = response
            .get("approved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let scope = response.get("scope").and_then(Value::as_str);
        let _ = crate::run_manager::global_run_manager()
            .respond_permission_for_run(&id, approved, run_id.as_deref(), scope)
            .await;
    }

    // subagent_assignment: wake batch waiters (NOT permission.respond).
    if kind == "subagent_assignment" {
        let _ = crate::production::wake_assignment_waiter(&id, response.clone());
        // Persist route policy when pool/bindings/assignments are present.
        let cid = response
            .get("conversation_id")
            .or_else(|| response.get("parent_conversation_id"))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or(conversation_id.clone());
        if let Some(cid) = cid {
            let bindings_val = response
                .get("pool")
                .or_else(|| response.get("bindings"))
                .cloned()
                .or_else(|| {
                    // Derive pool from assignments when pool omitted.
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
                if let Ok(list) =
                    serde_json::from_value::<Vec<crate::subagent_store::RouteBinding>>(
                        bindings.clone(),
                    )
                {
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

            // Second respond fails
            let err = rt
                .block_on(respond(json!({
                    "id": id,
                    "response": { "approved": false },
                })))
                .unwrap_err();
            assert!(err.contains("not pending") || err.contains("not found"), "{err}");
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
}
