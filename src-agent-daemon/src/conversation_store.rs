use crate::storage::DataStore;
use assistant_protocol::v2::methods::names;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

mod conversation_context;
mod conversation_fork;
mod conversation_messages;

pub use conversation_context::{
    backfill_context_snapshots, load_active_context_messages, load_active_context_snapshot,
    load_active_context_snapshot_for_checkpoint, ActiveContextSnapshot,
};
pub(crate) use conversation_context::{
    block_image, block_text, latest_context_summary, parse_content_block, parse_tool_result_blocks,
    reasoning_block_from_events,
};
pub(crate) use conversation_fork::fork;
pub use conversation_messages::{
    append_agent_message, append_trigger_message, delete_message, engine_history,
    load_agent_messages,
};
pub(crate) use conversation_messages::{append_message, get_messages, get_messages_page};

/// Max bytes of attachment content inlined into the model context (T209).
/// Oversized attachments degrade to an explicit marker instead of being read.
const MAX_ATTACHMENT_BYTES: usize = 256 * 1024;

/// Standard base64 engine for attachment data URLs.
fn base64_engine() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        names::CONVERSATION_CREATE => create(params),
        names::CONVERSATION_LIST => list(params),
        "conversation.listPage" => list_page(params),
        names::CONVERSATION_GET => get(params),
        names::CONVERSATION_FORK => fork(params),
        names::CONVERSATION_GET_MESSAGES => get_messages(params),
        "conversation.getMessagesPage" => get_messages_page(params),
        names::CONVERSATION_APPEND_MESSAGE => append_message(params),
        names::CONVERSATION_RENAME => rename(params),
        names::CONVERSATION_UPDATE_MODEL => update_model(params),
        names::CONVERSATION_UPDATE_PERMISSION => update_permission(params),
        names::CONVERSATION_ARCHIVE => archive(params),
        names::CONVERSATION_DELETE => delete(params).await,
        _ => Err(format!("unsupported conversation method: {method}")),
    }
}

/// Public wrapper for internal message append (used by subagent_store).
pub fn append_message_public(params: Value) -> Result<Value, String> {
    append_message(params)
}

/// Atomically persist a leased steering/follow-up input and acknowledge the
/// queue row. A crash before commit leaves the lease recoverable; a crash after
/// commit cannot duplicate the user message because its id is queue-derived.
pub fn persist_queued_input_and_ack(
    conversation_id: &str,
    run_id: &str,
    input_id: &str,
    content: &str,
    turn_id: Option<&str>,
    lease_token: Option<&str>,
) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let message_id = format!("queue:{input_id}");
    let expected_marker = format!("prompt_queue:{input_id}");
    #[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
    let existing: Option<(String, String, String, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT conversation_id, role, status, run_id, turn_id
             FROM message WHERE id = ?1",
            params![message_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((existing_conversation, role, status, existing_run, existing_turn)) = existing {
        if existing_conversation != conversation_id
            || role != "user"
            || status != "complete"
            || existing_run.as_deref() != Some(run_id)
            || existing_turn.as_deref() != turn_id
        {
            return Err(format!("queue message identity collision: {message_id}"));
        }
        let stored: String = tx
            .query_row(
                "SELECT block_json FROM message_block
                 WHERE message_id = ?1 AND sort_order = 0 AND block_type = 'text'",
                params![message_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("load queued message block: {e}"))?;
        let expected = serde_json::json!({ "text": content }).to_string();
        if stored != expected {
            return Err(format!("queue message content collision: {message_id}"));
        }
    } else {
        tx.execute(
            "INSERT INTO message
             (id, conversation_id, role, status, run_id, turn_id, legacy_marker, created_at)
             VALUES (?1, ?2, 'user', 'complete', ?3, ?4, ?5, ?6)",
            params![
                message_id,
                conversation_id,
                run_id,
                turn_id,
                expected_marker,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO message_block
             (message_id, sort_order, block_type, block_json)
             VALUES (?1, 0, 'text', ?2)",
            params![
                message_id,
                serde_json::json!({ "text": content }).to_string()
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    let changed = tx
        .execute(
            "UPDATE prompt_queue SET status = 'sent', consumed_turn_id = ?1,
             lease_token = NULL, lease_run_id = NULL, leased_at = NULL, updated_at = ?2
             WHERE id = ?3 AND conversation_id = ?4 AND lease_run_id = ?5
               AND lease_token = ?6 AND status = 'leased'",
            params![
                turn_id.unwrap_or(run_id),
                now,
                input_id,
                conversation_id,
                run_id,
                lease_token
            ],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("queue lease was lost before message commit".into());
    }
    tx.execute(
        "UPDATE conversation SET updated_at = ?1 WHERE id = ?2",
        params![now, conversation_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn store() -> Result<DataStore, String> {
    // W2: single Daemon DataStore open path (assistant.db authority + test hook).
    crate::storage::open_daemon_store()
}

fn row_to_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "mode": row.get::<_, String>(1)?,
        "project_id": row.get::<_, Option<String>>(2)?,
        "title": row.get::<_, String>(3)?,
        "provider_id": row.get::<_, String>(4)?,
        "model_id": row.get::<_, String>(5)?,
        "permission_profile_id": row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "ask".into()),
        "created_at": row.get::<_, String>(7)?,
        "updated_at": row.get::<_, String>(8)?,
        "archived_at": row.get::<_, Option<String>>(9)?,
        "parent_conversation_id": row.get::<_, Option<String>>(10)?,
    }))
}

/// List conversations. Default hides child (subagent) conversations
/// (`parent_conversation_id IS NULL`). Pass `include_children: true` to include them.
fn list(params: Value) -> Result<Value, String> {
    let include_children = params
        .get("include_children")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let store = store()?;
    let conn = store.conn()?;
    let sql = if include_children {
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id,
                created_at, updated_at, archived_at, parent_conversation_id
         FROM conversation ORDER BY updated_at DESC"
    } else {
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id,
                created_at, updated_at, archived_at, parent_conversation_id
         FROM conversation
         WHERE parent_conversation_id IS NULL
         ORDER BY updated_at DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_conversation)
        .map_err(|e| e.to_string())?;
    Ok(Value::Array(rows.filter_map(Result::ok).collect()))
}

fn list_page(params: Value) -> Result<Value, String> {
    let limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(100)
        .clamp(20, 200);
    let cursor = params.get("cursor");
    let cursor_updated = cursor
        .and_then(|v| v.get("updatedAt").or_else(|| v.get("updated_at")))
        .and_then(Value::as_str);
    let cursor_id = cursor.and_then(|v| v.get("id")).and_then(Value::as_str);
    let store = store()?;
    let conn = store.conn()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id,
                created_at, updated_at, archived_at, parent_conversation_id
         FROM conversation
         WHERE parent_conversation_id IS NULL
           AND (?1 IS NULL OR updated_at < ?1 OR (updated_at = ?1 AND id < ?2))
         ORDER BY updated_at DESC, id DESC LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![cursor_updated, cursor_id, limit + 1],
            row_to_conversation,
        )
        .map_err(|e| e.to_string())?;
    let mut conversations: Vec<Value> = rows.filter_map(Result::ok).collect();
    let has_more = conversations.len() > limit as usize;
    conversations.truncate(limit as usize);
    let next_cursor = has_more
        .then(|| conversations.last())
        .flatten()
        .and_then(|row| {
            Some(serde_json::json!({
                "updatedAt": row.get("updated_at")?, "id": row.get("id")?
            }))
        });
    Ok(serde_json::json!({ "conversations": conversations, "nextCursor": next_cursor }))
}

fn get(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let store = store()?;
    let conn = store.conn()?;
    conn.query_row(
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id,
                created_at, updated_at, archived_at, parent_conversation_id
         FROM conversation WHERE id = ?1",
        params![id],
        row_to_conversation,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "conversation not found".into())
}

pub fn permission_profile(conversation_id: &str) -> Result<String, String> {
    let store = store()?;
    let conn = store.conn()?;
    conn.query_row(
        "SELECT COALESCE(permission_profile_id, 'ask') FROM conversation WHERE id = ?1",
        params![conversation_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "conversation not found".into())
}

/// Ensure a conversation row exists in the daemon DB for FK integrity.
/// Host owns conversation CRUD (assistant.db); daemon only needs a stub so
/// `run` / `message` foreign keys succeed when host-mediated runs land here.
pub fn ensure_conversation_stub(
    conversation_id: &str,
    provider_id: &str,
    model_id: &str,
    permission_profile: Option<&str>,
    project_id: Option<&str>,
) -> Result<(), String> {
    let id = conversation_id.trim();
    if id.is_empty() {
        return Err("conversation_id is required".into());
    }
    let store = store()?;
    let conn = store.conn()?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let permission = permission_profile
        .filter(|p| matches!(*p, "readonly" | "ask" | "full_access"))
        .unwrap_or("ask");
    let provider = if provider_id.trim().is_empty() {
        "unknown"
    } else {
        provider_id.trim()
    };
    let model = if model_id.trim().is_empty() {
        "unknown"
    } else {
        model_id.trim()
    };
    conn.execute(
        "INSERT INTO conversation (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
         VALUES (?1, 'agent', ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(id) DO NOTHING",
        params![
            id,
            project_id,
            "Host-mediated conversation",
            provider,
            model,
            permission,
            now,
        ],
    )
    .map_err(|e| {
        let path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
            .or_else(|_| std::env::var("NATIVES_DB_PATH"))
            .unwrap_or_else(|_| "<unset>".into());
        format!("ensure_conversation_stub failed: {e} (db={path})")
    })?;
    Ok(())
}

fn create(params: Value) -> Result<Value, String> {
    let mode = params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    if !matches!(mode, "chat" | "agent" | "goal") {
        return Err("mode must be chat, agent, or goal".into());
    }
    let title = required_str(&params, "title")?;
    let provider_id = required_str(&params, "provider_id")?;
    let model_id = required_str(&params, "model_id")?;
    let permission = params
        .get("permission_profile_id")
        .and_then(Value::as_str)
        .unwrap_or("ask");
    if !matches!(permission, "readonly" | "ask" | "full_access") {
        return Err("permission_profile_id must be readonly, ask, or full_access".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let project_id = required_str(&params, "project_id")?;
    let store = store()?;
    let conn = store.conn()?;
    conn.execute(
        "INSERT INTO conversation (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![id, mode, project_id, title, provider_id, model_id, permission, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "id": id,
        "mode": mode,
        "project_id": project_id,
        "title": title,
        "provider_id": provider_id,
        "model_id": model_id,
        "permission_profile_id": permission,
        "created_at": now,
        "updated_at": now,
        "archived_at": null,
    }))
}

fn rename(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let title = required_str(&params, "title")?;
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "title": title }))
}

fn update_model(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let provider_id = required_str(&params, "provider_id")?;
    let model_id = required_str(&params, "model_id")?;
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET provider_id = ?1, model_id = ?2, updated_at = ?3 WHERE id = ?4",
        params![provider_id, model_id, now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "updated_at": now }))
}

fn update_permission(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let profile = required_str(&params, "permission_profile_id")?;
    if !matches!(profile, "readonly" | "ask" | "full_access") {
        return Err("permission_profile_id must be readonly, ask, or full_access".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET permission_profile_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![profile, now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "permission_profile_id": profile, "updated_at": now }))
}

fn archive(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET archived_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "archived_at": true }))
}

async fn delete(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?.to_string();
    // Collect this conversation + hidden children so we hard-delete the whole tree.
    let mut conversation_ids: Vec<String> = vec![id.clone()];
    if let Ok(sessions) = crate::subagent_store::list_subagent_sessions(Some(&id), true) {
        for sess in sessions {
            if !conversation_ids
                .iter()
                .any(|c| c == &sess.child_conversation_id)
            {
                conversation_ids.push(sess.child_conversation_id.clone());
            }
        }
    }

    // Cancel every live run under the tree before rows disappear.
    for cid in &conversation_ids {
        for run in crate::global_run_manager()
            .list_runs(Some(cid))
            .into_iter()
            .filter(|run| run.status.is_active())
        {
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest { run_id: run.id })
                .await;
        }
    }
    if let Ok(sessions) = crate::subagent_store::list_subagent_sessions(Some(&id), true) {
        for sess in sessions {
            let _ = crate::global_run_manager()
                .runtime
                .kill_task(&sess.id)
                .await;
            let _ = crate::subagent_store::close_subagent_session(
                &sess.id,
                "cancelled",
                Some("parent conversation deleted"),
            );
        }
    }

    // Fold remaining token totals into durable usage_stats *before* CASCADE removes
    // run/message rows. usage_stats is date/model aggregate billing — no conversation content.
    let mut usage_rows_folded = 0u64;
    {
        let store = store()?;
        let conn = store.conn()?;
        for cid in &conversation_ids {
            usage_rows_folded += fold_conversation_tokens_into_usage_stats(&conn, cid)?;
        }
        // Hard-delete: conversation content gone; billing stays in usage_stats.
        // CASCADE clears messages/runs/events/subagent_session/route_policy/children.
        for cid in conversation_ids.iter().rev() {
            // Children first is not required with CASCADE from parent, but deleting each
            // id is idempotent and covers orphan child rows without parent FK path.
            let _ = conn.execute("DELETE FROM conversation WHERE id = ?1", params![cid]);
        }
        // Ensure root is gone even if children-only path ran.
        let changed = conn
            .execute("DELETE FROM conversation WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            // Already deleted is success (idempotent).
            let still: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
                    params![id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if still {
                return Err("conversation not found".into());
            }
        }
    }

    // Drop in-memory run rows for deleted conversations (best-effort).
    crate::global_run_manager().forget_conversations(&conversation_ids);

    Ok(serde_json::json!({
        "deleted": true,
        "hard_deleted": true,
        "usage_stats_preserved": true,
        "usage_rows_folded": usage_rows_folded,
    }))
}

/// Snapshot token totals for a conversation into `usage_stats` (billing-only aggregate).
/// Does not store messages, titles, or prompts.
fn fold_conversation_tokens_into_usage_stats(
    conn: &rusqlite::Connection,
    conversation_id: &str,
) -> Result<u64, String> {
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

    // Prefer run-level totals (already projected from usage_updated events).
    // date: first 10 chars of RFC3339 / sqlite datetime → YYYY-MM-DD.
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(NULLIF(TRIM(model_id), ''), 'unknown'),
                    substr(COALESCE(started_at, created_at, datetime('now')), 1, 10),
                    COALESCE(SUM(COALESCE(total_input_tokens, 0)), 0),
                    COALESCE(SUM(COALESCE(total_output_tokens, 0)), 0),
                    COUNT(*)
             FROM run
             WHERE conversation_id = ?1
             GROUP BY 1, 2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut folded = 0u64;
    for row in rows.flatten() {
        let (model, date, input, output, request_count) = row;
        if input == 0 && output == 0 {
            continue;
        }
        conn.execute(
            "INSERT INTO usage_stats
                (date, source, source_path, model, input_tokens, output_tokens,
                 cache_creation_tokens, cache_read_tokens, request_count, cost_usd)
             VALUES (?1, 'natives', 'conversation.delete', ?2, ?3, ?4, 0, 0, ?5, 0.0)
             ON CONFLICT(date, source, model) DO UPDATE SET
                input_tokens = input_tokens + excluded.input_tokens,
                output_tokens = output_tokens + excluded.output_tokens,
                request_count = request_count + excluded.request_count",
            params![date, model, input, output, request_count],
        )
        .map_err(|e| e.to_string())?;
        folded += 1;
    }
    Ok(folded)
}

fn exec_update(sql: &str, params: impl rusqlite::Params) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    match conn.execute(sql, params).map_err(|e| e.to_string())? {
        0 => Err("conversation not found".into()),
        _ => Ok(()),
    }
}

fn id_param(params: &Value) -> Result<&str, String> {
    params
        .get("id")
        .or_else(|| params.get("conversation_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".into())
}

fn required_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} is required"))
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) fn env_lock() -> crate::storage::EnvTestGuard {
        crate::storage::DataStore::env_test_lock()
    }

    pub(crate) struct ClearTestDb;
    impl Drop for ClearTestDb {
        fn drop(&mut self) {
            crate::storage::set_test_db_override(None, None);
        }
    }

    pub(crate) struct EnvRestore {
        pub(crate) db: Option<String>,
        pub(crate) asst: Option<String>,
        pub(crate) rt: Option<String>,
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match self.db.take() {
                Some(v) => std::env::set_var("NATIVES_DB_PATH", v),
                None => std::env::remove_var("NATIVES_DB_PATH"),
            }
            match self.asst.take() {
                Some(v) => std::env::set_var("NATIVES_ASSISTANT_DB_PATH", v),
                None => std::env::remove_var("NATIVES_ASSISTANT_DB_PATH"),
            }
            match self.rt.take() {
                Some(v) => std::env::set_var("NATIVES_RUNTIME_DIR", v),
                None => std::env::remove_var("NATIVES_RUNTIME_DIR"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
