use crate::storage::DataStore;
use agent_core::{AgentMessage, ContentBlock, EngineMessage, ToolResultBlock};
use assistant_protocol::v2::methods::names;
use assistant_protocol::v2::{AttachmentRef, RunEventKind, RunEventV2};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

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
) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let message_id = format!("queue:{input_id}");
    tx.execute(
        "INSERT OR IGNORE INTO message
         (id, conversation_id, role, status, run_id, turn_id, legacy_marker, created_at)
         VALUES (?1, ?2, 'user', 'complete', ?3, ?4, ?5, ?6)",
        params![
            message_id,
            conversation_id,
            run_id,
            turn_id,
            format!("prompt_queue:{input_id}"),
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT OR IGNORE INTO message_block
         (message_id, sort_order, block_type, block_json)
         VALUES (?1, 0, 'text', ?2)",
        params![
            message_id,
            serde_json::json!({ "text": content }).to_string()
        ],
    )
    .map_err(|e| e.to_string())?;
    let changed = tx
        .execute(
            "UPDATE prompt_queue SET status = 'sent', consumed_turn_id = ?1,
             lease_token = NULL, lease_run_id = NULL, leased_at = NULL, updated_at = ?2
             WHERE id = ?3 AND conversation_id = ?4 AND lease_run_id = ?5 AND status = 'leased'",
            params![
                turn_id.unwrap_or(run_id),
                now,
                input_id,
                conversation_id,
                run_id
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

fn store() -> Result<DataStore, String> {
    // Phase 0: Daemon conversation/run authority is assistant.db.
    // Prefer NATIVES_ASSISTANT_DB_PATH; fall back to NATIVES_DB_PATH for tests that
    // still use a single temp file; finally default_assistant_db_path().
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

fn fork(params: Value) -> Result<Value, String> {
    let source_id = required_str(&params, "conversation_id")?;
    let source = get(serde_json::json!({ "id": source_id }))?;
    let mut params = source;
    params["title"] = serde_json::json!(format!(
        "Fork of {}",
        params
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Conversation")
    ));
    // A fork inherits transcript metadata only; permissions/credentials are
    // re-resolved for the new branch and never copy a pending grant.
    params["permission_profile_id"] = serde_json::json!("ask");
    let forked = create(params)?;
    let fork_id = required_str(&forked, "id")?;
    let branch_id = uuid::Uuid::new_v4().to_string();
    let parent_message_id = {
        let store = store()?;
        let conn = store.conn()?;
        conn.query_row(
            "SELECT id FROM message WHERE conversation_id = ?1
             ORDER BY created_at DESC, id DESC LIMIT 1",
            rusqlite::params![source_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
    };
    let store = store()?;
    let conn = store.conn()?;
    copy_transcript(&conn, source_id, fork_id)?;
    conn.execute(
        "UPDATE conversation
         SET branch_id = ?1, parent_conversation_id = ?2,
             branch_parent_message_id = ?3
         WHERE id = ?4",
        rusqlite::params![branch_id, source_id, parent_message_id, fork_id],
    )
    .map_err(|e| e.to_string())?;
    let mut forked = forked;
    forked["branch_id"] = serde_json::json!(branch_id);
    forked["parent_conversation_id"] = serde_json::json!(source_id);
    forked["branch_parent_message_id"] = serde_json::json!(parent_message_id);
    Ok(forked)
}

/// Copy the durable typed transcript into a fork with new message identities.
/// Parent links are remapped so the fork is independent while tool-call IDs
/// inside content blocks remain stable for replay and audit correlation.
fn copy_transcript(
    conn: &rusqlite::Connection,
    source_conversation_id: &str,
    fork_conversation_id: &str,
) -> Result<(), String> {
    let mut messages = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, parent_message_id, role, status, input_tokens, output_tokens,
                        reasoning_tokens, cost_usd, created_at, turn_id, run_id, stop_reason,
                        legacy_marker, truncated
                 FROM message WHERE conversation_id = ?1 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![source_conversation_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<f64>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            messages.push(row.map_err(|e| e.to_string())?);
        }
    }
    let mut id_map = std::collections::HashMap::new();
    for (old_id, ..) in &messages {
        id_map.insert(
            old_id.clone(),
            format!("fork:{fork_conversation_id}:{old_id}"),
        );
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for (
        old_id,
        parent_id,
        role,
        status,
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cost_usd,
        created_at,
        _turn_id,
        _run_id,
        stop_reason,
        legacy_marker,
        truncated,
    ) in messages
    {
        let new_id = id_map
            .get(&old_id)
            .ok_or_else(|| "fork message identity missing".to_string())?;
        let new_parent = parent_id.as_deref().and_then(|id| id_map.get(id));
        tx.execute(
            "INSERT INTO message
             (id, conversation_id, parent_message_id, role, status, input_tokens, output_tokens,
              reasoning_tokens, cost_usd, created_at, turn_id, run_id, stop_reason, legacy_marker, truncated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                new_id,
                fork_conversation_id,
                new_parent,
                role,
                status,
                input_tokens,
                output_tokens,
                reasoning_tokens,
                cost_usd,
                created_at,
                // Runs/turns belong to the source execution lineage; the
                // fork starts a fresh run while retaining the transcript.
                Option::<String>::None,
                Option::<String>::None,
                stop_reason,
                legacy_marker,
                truncated,
            ],
        )
        .map_err(|e| e.to_string())?;
        let block_rows = {
            let mut blocks = tx
                .prepare(
                    "SELECT sort_order, block_type, block_json, artifact_id, truncated
                     FROM message_block WHERE message_id = ?1 ORDER BY sort_order ASC, id ASC",
                )
                .map_err(|e| e.to_string())?;
            let rows = blocks
                .query_map(params![old_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for (sort_order, block_type, block_json, artifact_id, block_truncated) in block_rows {
            tx.execute(
                "INSERT INTO message_block
                 (message_id, sort_order, block_type, block_json, artifact_id, truncated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    new_id,
                    sort_order,
                    block_type,
                    block_json,
                    artifact_id,
                    block_truncated,
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

fn get_messages(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let page_limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .map(|n| n.clamp(20, 200));
    let cursor = params.get("cursor");
    let cursor_created = cursor
        .and_then(|v| v.get("createdAt").or_else(|| v.get("created_at")))
        .and_then(Value::as_str);
    let cursor_id = cursor.and_then(|v| v.get("id")).and_then(Value::as_str);
    let store = store()?;
    let conn = store.conn()?;
    let mut messages: Vec<Value> = if let Some(limit) = page_limit {
        let mut stmt = conn.prepare(
            "SELECT id, role, conversation_id, parent_message_id, status, input_tokens, output_tokens, created_at,
                    turn_id, run_id, stop_reason, legacy_marker, truncated
             FROM message WHERE conversation_id = ?1
               AND (?2 IS NULL OR created_at < ?2 OR (created_at = ?2 AND id < ?3))
             ORDER BY created_at DESC, id DESC LIMIT ?4",
        ).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![conversation_id, cursor_created, cursor_id, limit + 1],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "role": row.get::<_, String>(1)?,
                        "conversation_id": row.get::<_, String>(2)?,
                        "parent_message_id": row.get::<_, Option<String>>(3)?,
                        "status": row.get::<_, String>(4)?,
                        "input_tokens": row.get::<_, Option<i64>>(5)?,
                        "output_tokens": row.get::<_, Option<i64>>(6)?,
                        "created_at": row.get::<_, String>(7)?,
                        "turn_id": row.get::<_, Option<String>>(8)?,
                        "run_id": row.get::<_, Option<String>>(9)?,
                        "stop_reason": row.get::<_, Option<String>>(10)?,
                        "legacy_marker": row.get::<_, Option<String>>(11)?,
                        "truncated": row.get::<_, i64>(12).unwrap_or(0) != 0,
                    }))
                },
            )
            .map_err(|e| e.to_string())?;
        let mut rows: Vec<Value> = rows.filter_map(Result::ok).collect();
        rows.reverse();
        rows
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, role, conversation_id, parent_message_id, status, input_tokens, output_tokens, created_at,
                    turn_id, run_id, stop_reason, legacy_marker, truncated
             FROM message WHERE conversation_id = ?1 ORDER BY created_at ASC, id ASC",
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?, "role": row.get::<_, String>(1)?,
                "conversation_id": row.get::<_, String>(2)?, "parent_message_id": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?, "input_tokens": row.get::<_, Option<i64>>(5)?,
                "output_tokens": row.get::<_, Option<i64>>(6)?, "created_at": row.get::<_, String>(7)?,
                "turn_id": row.get::<_, Option<String>>(8)?, "run_id": row.get::<_, Option<String>>(9)?,
                "stop_reason": row.get::<_, Option<String>>(10)?, "legacy_marker": row.get::<_, Option<String>>(11)?,
                "truncated": row.get::<_, i64>(12).unwrap_or(0) != 0,
            }))
        }).map_err(|e| e.to_string())?;
        rows.filter_map(Result::ok).collect()
    };

    let mut blocks = conn
        .prepare(
            "SELECT message_id, block_type, sort_order, block_json
             FROM message_block
             WHERE message_id IN (SELECT id FROM message WHERE conversation_id = ?1)
             ORDER BY sort_order ASC",
        )
        .map_err(|e| e.to_string())?;
    let block_rows = blocks
        .query_map(params![conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut by_message = std::collections::HashMap::<String, Vec<Value>>::new();
    for (message_id, block_type, index, json) in block_rows.filter_map(Result::ok) {
        by_message
            .entry(message_id)
            .or_default()
            .push(serde_json::json!({
                "type": block_type,
                "index": index,
                "content": serde_json::from_str::<Value>(&json).unwrap_or_else(|_| serde_json::json!({ "text": json })),
            }));
    }
    for message in &mut messages {
        let id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut content_blocks = by_message.remove(id).unwrap_or_default();
        let run_id = content_blocks.iter().find_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("run_reference"))
                .then(|| block.get("content")?.get("run_id")?.as_str())
                .flatten()
        });
        if let Some(run_id) = run_id {
            message["run_id"] = serde_json::json!(run_id);
            if !content_blocks
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("reasoning"))
            {
                let event_payloads = conn
                    .prepare(
                        "SELECT payload FROM run_event WHERE run_id = ?1 ORDER BY sequence ASC",
                    )
                    .and_then(|mut stmt| {
                        stmt.query_map(params![run_id], |row| row.get::<_, String>(0))
                            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
                    })
                    .unwrap_or_default();
                let events = event_payloads
                    .iter()
                    .filter_map(|payload| serde_json::from_str::<RunEventV2>(payload).ok())
                    .collect::<Vec<_>>();
                if let Some(reasoning) = reasoning_block_from_events(&events) {
                    content_blocks.insert(
                        0,
                        serde_json::json!({
                            "type": "reasoning",
                            "index": 0,
                            "content": reasoning,
                        }),
                    );
                }
            }
        }
        message["content_blocks"] = serde_json::json!(content_blocks);
    }
    Ok(Value::Array(messages))
}

fn get_messages_page(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(100)
        .clamp(20, 200);
    let cursor = params.get("cursor");
    let cursor_created = cursor
        .and_then(|v| v.get("createdAt").or_else(|| v.get("created_at")))
        .and_then(Value::as_str);
    let cursor_id = cursor.and_then(|v| v.get("id")).and_then(Value::as_str);
    let all = get_messages(
        serde_json::json!({ "conversation_id": conversation_id, "limit": limit, "cursor": { "createdAt": cursor_created, "id": cursor_id } }),
    )?;
    let mut messages: Vec<Value> = all.as_array().cloned().unwrap_or_default();
    let has_more = messages.len() > limit as usize;
    messages.truncate(limit as usize);
    let next_cursor = has_more
        .then(|| messages.first())
        .flatten()
        .and_then(|row| {
            Some(serde_json::json!({
                "createdAt": row.get("created_at")?, "id": row.get("id")?
            }))
        });
    messages.reverse();
    Ok(serde_json::json!({ "messages": messages, "nextCursor": next_cursor }))
}

pub fn engine_history(conversation_id: &str) -> Result<Vec<EngineMessage>, String> {
    let messages = get_messages(serde_json::json!({ "conversation_id": conversation_id }))?;
    let Some(rows) = messages.as_array() else {
        return Ok(Vec::new());
    };
    let raw_history: Vec<_> = rows
        .iter()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?.to_string();
            let blocks = message.get("content_blocks").and_then(Value::as_array);
            let content = blocks
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(block_text)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let images: Vec<_> = blocks
                .map(|blocks| blocks.iter().filter_map(block_image).collect())
                .unwrap_or_default();
            // An image-only turn carries no text, and dropping it here would
            // silently rewrite history — the model would see the reply to a
            // picture it was never shown.
            if content.trim().is_empty() && images.is_empty() {
                return None;
            }
            Some(EngineMessage {
                role,
                content,
                images,
                ..Default::default()
            })
        })
        .collect();
    // Legacy EngineMessage consumers expect the tool result summary adjacent
    // to the assistant call. Typed persistence keeps it as its own message;
    // this adapter only folds the display text at the compatibility edge.
    let mut history: Vec<EngineMessage> = Vec::with_capacity(raw_history.len());
    for message in raw_history {
        if message.role == "assistant" && message.content.contains("[tool result:") {
            if let Some(previous) = history.last_mut() {
                if previous.role == "assistant" {
                    if !previous.content.is_empty() {
                        previous.content.push('\n');
                    }
                    previous.content.push_str(&message.content);
                    continue;
                }
            }
        }
        history.push(message);
    }
    if let Some(summary) = latest_context_summary(conversation_id)? {
        history.insert(0, EngineMessage::text("system", summary));
    }
    Ok(history)
}

/// Load the durable typed transcript used by Core replay.  The older
/// `engine_history` helper intentionally remains for RPC/fixture compatibility;
/// production callers should prefer this lossless representation.
pub fn load_agent_messages(conversation_id: &str) -> Result<Vec<AgentMessage>, String> {
    let raw = get_messages(serde_json::json!({ "conversation_id": conversation_id }))?;
    let Some(rows) = raw.as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "message id missing".to_string())?;
        let role = row
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("assistant");
        let blocks = row
            .get("content_blocks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let stop_reason =
            row.get("stop_reason")
                .and_then(Value::as_str)
                .map(|reason| match reason {
                    "stop" => agent_core::StopReason::Stop,
                    "tool_use" => agent_core::StopReason::ToolUse,
                    "length" => agent_core::StopReason::Length,
                    "cancelled" => agent_core::StopReason::Cancelled,
                    "error" => agent_core::StopReason::Error,
                    other => agent_core::StopReason::Provider(other.to_string()),
                });
        if let Some(tool_block) = blocks
            .iter()
            .find(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        {
            let payload = tool_block.get("content").unwrap_or(tool_block);
            let content = payload
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| parse_tool_result_blocks(blocks))
                .unwrap_or_default();
            out.push(AgentMessage::ToolResult(agent_core::ToolResultMessage {
                message_id: agent_core::MessageId::from(id),
                tool_call_id: agent_core::ToolCallId::from(
                    payload
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                ),
                tool_name: tool_block
                    .get("content")
                    .and_then(|v| v.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                content,
                is_error: payload
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                code: payload
                    .get("error_code")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }));
            continue;
        }
        let content = blocks
            .iter()
            .filter_map(parse_content_block)
            .collect::<Vec<_>>();
        match role {
            "user" => out.push(AgentMessage::User(agent_core::UserMessage {
                message_id: agent_core::MessageId::from(id),
                content,
            })),
            "system" => out.push(AgentMessage::System(agent_core::SystemMessage {
                message_id: agent_core::MessageId::from(id),
                text: content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            })),
            _ => out.push(AgentMessage::Assistant(agent_core::AssistantMessage {
                message_id: agent_core::MessageId::from(id),
                content,
                stop_reason,
            })),
        }
    }
    Ok(out)
}

/// Lossless active-context snapshot plus the durable message ids it replaced.
/// The daemon uses the id set to append messages written after compaction
/// without deleting or mutating the full transcript.
#[derive(Debug, Clone)]
pub struct ActiveContextSnapshot {
    pub messages: Vec<AgentMessage>,
    pub input_message_ids: HashSet<String>,
}

/// Load the newest lossless active-context snapshot when one exists. A missing
/// or malformed snapshot is a normal cache miss: callers fall back to the
/// durable full message history.
pub fn load_active_context_snapshot(
    conversation_id: &str,
) -> Result<Option<ActiveContextSnapshot>, String> {
    let store = store()?;
    let conn = store.conn()?;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT snapshot_json, input_message_ids FROM context_snapshot
             WHERE conversation_id = ?1 AND snapshot_type = 'compaction'
             ORDER BY sequence DESC LIMIT 1",
            params![conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((raw, input_ids)) = row else {
        return Ok(None);
    };
    let value = serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string())?;
    let ids = serde_json::from_str::<Vec<String>>(&input_ids)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    Ok(Some(ActiveContextSnapshot {
        messages: agent_core::agent_messages_from_json(&value),
        input_message_ids: ids,
    }))
}

/// Load the snapshot explicitly attached to a checkpoint. Continue/Resume
/// must not silently jump to a newer conversation snapshot.
pub fn load_active_context_snapshot_for_checkpoint(
    conversation_id: &str,
    checkpoint_id: &str,
) -> Result<Option<ActiveContextSnapshot>, String> {
    let store = store()?;
    let conn = store.conn()?;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT cs.snapshot_json, cs.input_message_ids
             FROM checkpoint cp
             JOIN context_snapshot cs ON cs.id = cp.active_context_snapshot_id
             WHERE cp.id = ?1 AND cp.conversation_id = ?2
             LIMIT 1",
            params![checkpoint_id, conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((raw, input_ids)) = row else {
        return Ok(None);
    };
    let value = serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string())?;
    let ids = serde_json::from_str::<Vec<String>>(&input_ids)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    Ok(Some(ActiveContextSnapshot {
        messages: agent_core::agent_messages_from_json(&value),
        input_message_ids: ids,
    }))
}

pub fn load_active_context_messages(conversation_id: &str) -> Result<Vec<AgentMessage>, String> {
    Ok(load_active_context_snapshot(conversation_id)?
        .map(|snapshot| snapshot.messages)
        .unwrap_or_default())
}

fn parse_content_block(block: &Value) -> Option<ContentBlock> {
    let kind = block.get("type").and_then(Value::as_str)?;
    let content = block.get("content").unwrap_or(block);
    match kind {
        "text" => Some(ContentBlock::Text {
            text: content
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        "thinking" | "reasoning" => Some(ContentBlock::Thinking {
            text: content
                .get("text")
                .or_else(|| content.get("reasoning"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            signature: content
                .get("signature")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        "tool_call" => Some(ContentBlock::ToolCall(agent_core::ToolCall {
            tool_call_id: agent_core::ToolCallId::from(
                content
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            name: content
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            arguments_json: content
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })),
        "image" => serde_json::from_value::<agent_core::ImageSource>(
            content
                .get("source")
                .cloned()
                .unwrap_or_else(|| content.clone()),
        )
        .ok()
        .map(|source| ContentBlock::Image { source }),
        _ => None,
    }
}

fn parse_tool_result_blocks(blocks: &[Value]) -> Vec<ToolResultBlock> {
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => Some(ToolResultBlock::Text {
                text: block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }),
            Some("json") => Some(ToolResultBlock::Json {
                value: block.get("value").cloned().unwrap_or(Value::Null),
            }),
            Some("artifact") => Some(ToolResultBlock::Artifact {
                artifact_id: block
                    .get("artifact_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                preview: block
                    .get("preview")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }),
            _ => None,
        })
        .collect()
}

fn latest_context_summary(conversation_id: &str) -> Result<Option<String>, String> {
    let store = store()?;
    let conn = store.conn()?;
    conn.query_row(
        "SELECT cs.summary
         FROM context_snapshot cs
         JOIN run r ON r.id = cs.run_id
         WHERE r.conversation_id = ?1
           AND cs.snapshot_type = 'compaction'
           AND COALESCE(cs.summary, '') <> ''
         ORDER BY cs.created_at DESC, cs.sequence DESC
         LIMIT 1",
        params![conversation_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Recover a stored `image` content block for replay into a provider request.
///
/// Returns `None` for every other block type, and for an image block whose URL
/// is absent — a placeholder [`EngineImage`] would reach the adapters and be
/// announced to the model as a degraded image, claiming a picture existed where
/// the record has none.
fn block_image(block: &Value) -> Option<agent_core::EngineImage> {
    if block.get("type").and_then(Value::as_str)? != "image" {
        return None;
    }
    let content = block.get("content").unwrap_or(block);
    let url = content
        .get("image_url")
        .or_else(|| content.get("imageUrl"))
        .or_else(|| content.get("url"))
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())?;
    Some(agent_core::EngineImage {
        url: url.to_string(),
        media_type: content
            .get("mime_type")
            .or_else(|| content.get("mimeType"))
            .and_then(Value::as_str)
            .map(str::to_string),
        detail: None,
    })
}

fn block_text(block: &Value) -> Option<String> {
    match block.get("type").and_then(Value::as_str)? {
        "text" => block
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string),
        "file_reference" => {
            let content = block.get("content").unwrap_or(block);
            let path = content.get("path").and_then(Value::as_str).unwrap_or("");
            let name = content.get("name").and_then(Value::as_str).unwrap_or(path);
            Some(format!("[attachment: {name} at {path}]"))
        }
        "tool_result" => {
            let metadata = block
                .get("content")
                .filter(|value| value.is_object())
                .unwrap_or(block);
            let name = metadata
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let output = metadata
                .get("output")
                .cloned()
                .or_else(|| metadata.get("content").cloned())
                .unwrap_or(Value::Null);
            Some(format!("[tool result: {name} => {output}]"))
        }
        _ => None,
    }
}

fn reasoning_block_from_events(events: &[RunEventV2]) -> Option<Value> {
    let mut reasoning = String::new();
    let mut started_at = None;
    let mut finished_at = None;
    for event in events {
        if let RunEventKind::ReasoningDelta { text } = &event.payload {
            started_at.get_or_insert(event.timestamp);
            reasoning.push_str(text);
        }
        if started_at.is_some()
            && finished_at.is_none()
            && matches!(
                &event.payload,
                RunEventKind::TextDelta { .. }
                    | RunEventKind::ToolCallRequested { .. }
                    | RunEventKind::ToolCallStarted { .. }
                    | RunEventKind::ToolCallDelta { .. }
                    | RunEventKind::ToolCallCompleted { .. }
                    | RunEventKind::Completed { .. }
                    | RunEventKind::Failed { .. }
                    | RunEventKind::Cancelled { .. }
                    | RunEventKind::Interrupted { .. }
            )
        {
            finished_at = Some(event.timestamp);
        }
    }
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return None;
    }
    let duration_ms = started_at
        .zip(finished_at.or_else(|| events.last().map(|e| e.timestamp)))
        .map(|(start, end)| (end - start).num_milliseconds().max(0) as u64)
        .unwrap_or(0);
    Some(serde_json::json!({
        "reasoning": reasoning,
        "duration_ms": duration_ms,
    }))
}

pub fn append_assistant_turn_from_events(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<Option<String>, String> {
    let mut groups: Vec<Vec<RunEventV2>> = Vec::new();
    let mut current = Vec::new();
    for event in events {
        if matches!(&event.payload, RunEventKind::TurnStarted { .. }) && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(event.clone());
        if matches!(&event.payload, RunEventKind::TurnCompleted { .. }) {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    let mut last_id = None;
    for group in groups {
        if let Some(id) = append_single_assistant_turn(conversation_id, run_id, &group)? {
            last_id = Some(id);
        }
    }
    Ok(last_id)
}

fn append_single_assistant_turn(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<Option<String>, String> {
    persist_context_snapshots_from_events(run_id, events)?;
    let mut text = String::new();
    let mut thinking = String::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    let mut committed_content: Option<Vec<agent_core::ContentBlock>> = None;
    let turn_id = events.iter().find_map(|event| match &event.payload {
        RunEventKind::TurnStarted { turn_id } => Some(turn_id.clone()),
        _ => None,
    });
    let assistant_message_id = events.iter().find_map(|event| match &event.payload {
        RunEventKind::MessageStarted {
            message_id, role, ..
        } if role == "assistant" => Some(message_id.clone()),
        _ => None,
    });
    let stop_reason = events.iter().find_map(|event| match &event.payload {
        RunEventKind::TurnCompleted { stop_reason, .. } => Some(stop_reason.clone()),
        _ => None,
    });
    for event in events {
        match &event.payload {
            RunEventKind::TextDelta { text: delta } => text.push_str(delta),
            RunEventKind::ReasoningDelta { text: delta } => thinking.push_str(delta),
            RunEventKind::ToolCallRequested { id, name, input } => {
                tool_calls.push(agent_core::ToolCall {
                    tool_call_id: id.clone().into(),
                    name: name.clone(),
                    arguments_json: input.to_string(),
                });
            }
            RunEventKind::ToolCallCompleted {
                id,
                name,
                output,
                is_error,
                duration_ms,
            } => tool_results.push((
                id.clone(),
                name.clone(),
                output.clone(),
                *is_error,
                *duration_ms,
            )),
            RunEventKind::MessageCompleted { content, .. } => {
                committed_content = content
                    .as_ref()
                    .and_then(|value| value.get("content"))
                    .and_then(|value| serde_json::from_value(value.clone()).ok());
            }
            _ => {}
        }
    }
    let content = committed_content.unwrap_or_else(|| {
        let mut content = Vec::new();
        if !thinking.trim().is_empty() {
            content.push(agent_core::ContentBlock::Thinking {
                text: thinking,
                signature: None,
            });
        }
        if !text.trim().is_empty() {
            content.push(agent_core::ContentBlock::Text { text });
        }
        content.extend(
            tool_calls
                .into_iter()
                .map(agent_core::ContentBlock::ToolCall),
        );
        content
    });
    if content.is_empty() && tool_results.is_empty() {
        return Ok(None);
    }
    let typed_turn_id = turn_id.unwrap_or_else(|| format!("legacy-turn:{run_id}"));
    let assistant_id = assistant_message_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    persist_turn_record(run_id, &typed_turn_id, stop_reason.as_deref(), events)?;
    let has_typed_thinking = content
        .iter()
        .any(|block| matches!(block, agent_core::ContentBlock::Thinking { .. }));
    let assistant = agent_core::AssistantMessage {
        message_id: assistant_id.into(),
        content,
        stop_reason: stop_reason.map(agent_core::StopReason::Provider),
    };
    let appended_id = append_agent_message(
        conversation_id,
        Some(run_id),
        Some(&typed_turn_id),
        &AgentMessage::Assistant(assistant),
    )?;
    if !has_typed_thinking {
        if let Some(reasoning) = reasoning_block_from_events(events) {
            let db = store()?;
            let conn = db.conn()?;
            conn.execute(
            "INSERT OR IGNORE INTO message_block (message_id, sort_order, block_type, block_json)
             VALUES (?1, -1, 'reasoning', ?2)",
            params![appended_id, reasoning.to_string()],
        )
        .map_err(|e| e.to_string())?;
        }
    }
    for (id, name, output, is_error, duration_ms) in tool_results {
        let code = output
            .get("error_code")
            .or_else(|| output.get("code"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let content = output
            .get("artifact_id")
            .and_then(Value::as_str)
            .map(|artifact_id| {
                vec![ToolResultBlock::Artifact {
                    artifact_id: artifact_id.to_string(),
                    preview: output
                        .get("preview")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }]
            })
            .unwrap_or_else(|| vec![ToolResultBlock::Json { value: output }]);
        let result = agent_core::ToolResultMessage {
            message_id: agent_core::MessageId::new(),
            tool_call_id: id.into(),
            tool_name: name,
            content,
            is_error,
            code,
        };
        let _ = duration_ms;
        append_agent_message(
            conversation_id,
            Some(run_id),
            Some(&typed_turn_id),
            &AgentMessage::ToolResult(result),
        )?;
    }
    Ok(Some(appended_id))
}

fn persist_turn_record(
    run_id: &str,
    turn_id: &str,
    stop_reason: Option<&str>,
    events: &[RunEventV2],
) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    let run_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE id = ?1)",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("check turn run: {e}"))?;
    if !run_exists {
        return Ok(());
    }
    let sequence = events
        .iter()
        .filter_map(|event| match &event.payload {
            RunEventKind::TurnStarted { .. } => Some(event.effective_run_sequence()),
            _ => None,
        })
        .next()
        .unwrap_or(0);
    conn.execute(
        "INSERT OR IGNORE INTO turn (id, run_id, sequence, status, stop_reason, completed_at)
         VALUES (?1, ?2, ?3, 'committed', ?4, ?5)",
        params![
            turn_id,
            run_id,
            sequence as i64,
            stop_reason,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map_err(|e| format!("persist turn: {e}"))?;
    Ok(())
}

fn persist_context_snapshots_from_events(
    run_id: &str,
    events: &[RunEventV2],
) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    let conversation_id: Option<String> = conn
        .query_row(
            "SELECT conversation_id FROM run WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let branch_id: Option<String> = conversation_id.as_deref().and_then(|conversation_id| {
        conn.query_row(
            "SELECT branch_id FROM conversation WHERE id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
    });
    for event in events {
        let (snapshot_id, before_tokens, after_tokens, summary, committed) = match &event.payload {
            RunEventKind::ContextCompressed {
                before_tokens,
                after_tokens,
                summary,
            } => (None, *before_tokens, *after_tokens, summary.as_str(), None),
            RunEventKind::ContextSnapshotCommitted {
                snapshot_id,
                input_message_ids,
                summary_message_id,
                replaced_range,
                algorithm_version,
                provider_context_window,
                artifact_reference,
                turn_id,
                source_revision,
                snapshot_json,
            } => (
                Some(snapshot_id.as_str()),
                0,
                0,
                "",
                Some((
                    input_message_ids,
                    summary_message_id.as_deref(),
                    replaced_range.as_deref(),
                    algorithm_version.as_str(),
                    *provider_context_window,
                    artifact_reference.as_deref(),
                    turn_id.as_deref(),
                    *source_revision,
                    snapshot_json,
                )),
            ),
            _ => continue,
        };
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM context_snapshot
                 WHERE run_id = ?1 AND sequence = ?2 AND snapshot_type = 'compaction'",
                params![run_id, event.effective_run_sequence() as i64],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists > 0 {
            continue;
        }
        let event_turn_id = events[..events
            .iter()
            .position(|candidate| {
                candidate.effective_run_sequence() == event.effective_run_sequence()
            })
            .unwrap_or(0)]
            .iter()
            .rev()
            .find_map(|candidate| match &candidate.payload {
                RunEventKind::TurnCompleted { turn_id, .. }
                | RunEventKind::TurnStarted { turn_id } => Some(turn_id.clone()),
                _ => None,
            });
        let turn_id = committed
            .and_then(|value| value.6.map(str::to_string))
            .or(event_turn_id);
        let snapshot_id = snapshot_id
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let (
            input_message_ids,
            summary_message_id,
            replaced_range,
            algorithm_version,
            provider_context_window,
            artifact_reference,
            source_revision,
            snapshot_json,
        ) = committed
            .map(|value| {
                (
                    serde_json::to_string(value.0).unwrap_or_else(|_| "[]".into()),
                    value.1.map(str::to_string),
                    value.2.map(str::to_string),
                    value.3.to_string(),
                    value.4,
                    value.5.map(str::to_string),
                    value.7,
                    value.8.clone(),
                )
            })
            .unwrap_or_else(|| {
                (
                    "[]".into(),
                    None,
                    None,
                    "mechanical-v1".into(),
                    None,
                    None,
                    event.effective_run_sequence(),
                    serde_json::json!({
                        "before_tokens": before_tokens,
                        "after_tokens": after_tokens,
                        "event_sequence": event.effective_run_sequence(),
                    }),
                )
            });
        let estimated_tokens = if after_tokens > 0 {
            after_tokens as i64
        } else {
            (snapshot_json.to_string().len() as i64 / 4).max(1)
        };
        conn.execute(
            "INSERT INTO context_snapshot (
                id, run_id, conversation_id, branch_id, turn_id, sequence, snapshot_type, token_count, summary,
                source_revision, input_message_ids, summary_message_id, replaced_range,
                algorithm_version, provider_context_window, artifact_reference, snapshot_json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'compaction', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                snapshot_id,
                run_id,
                conversation_id,
                branch_id,
                turn_id,
                event.effective_run_sequence() as i64,
                estimated_tokens,
                summary,
                source_revision as i64,
                input_message_ids,
                summary_message_id,
                replaced_range,
                algorithm_version,
                provider_context_window.map(|value| value as i64),
                artifact_reference,
                snapshot_json.to_string()
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn append_message(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let role = required_str(&params, "role")?;
    if !matches!(role, "system" | "user" | "assistant") {
        return Err("role must be user, assistant, or system".into());
    }
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let mut blocks = params
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if blocks.is_empty() {
        let content = content.ok_or_else(|| "content or blocks is required".to_string())?;
        blocks.push(serde_json::json!({ "type": "text", "text": content }));
    }
    let status = params
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("complete");
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let turn_id = params.get("turn_id").and_then(Value::as_str);
    let run_id = params.get("run_id").and_then(Value::as_str);
    let legacy_marker = params.get("legacy_marker").and_then(Value::as_str);
    let stop_reason = params.get("stop_reason").and_then(Value::as_str);
    let truncated = params
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO message (id, conversation_id, role, status, turn_id, run_id, legacy_marker, truncated, stop_reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, conversation_id, role, status, turn_id, run_id, legacy_marker, if truncated { 1 } else { 0 }, stop_reason, now],
    )
    .map_err(|e| e.to_string())?;
    for (index, block) in blocks.iter().enumerate() {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "block type is required".to_string())?;
        let content = if block_type == "text" {
            serde_json::json!({ "text": block.get("text").and_then(Value::as_str).unwrap_or_default() })
        } else {
            block.clone()
        };
        tx.execute(
            "INSERT INTO message_block (message_id, sort_order, block_type, block_json, artifact_id, truncated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, index as i64, block_type, content.to_string(), block.get("artifact_id").and_then(Value::as_str), if block.get("truncated").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 }],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "UPDATE conversation SET updated_at = ?1 WHERE id = ?2",
        params![now, conversation_id],
    )
    .and_then(|_| tx.commit())
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "id": id, "created_at": now }))
}

/// Persist the typed Core message without passing through an EngineMessage
/// transcript. The JSON block representation is the durable compatibility
/// boundary for old renderers; identity and ordering stay typed and stable.
pub fn append_agent_message(
    conversation_id: &str,
    run_id: Option<&str>,
    turn_id: Option<&str>,
    message: &AgentMessage,
) -> Result<String, String> {
    // Legacy fixtures use symbolic ids without corresponding FK rows. Real
    // daemon runs are UUIDs and keep the binding; the compatibility marker is
    // still written below for symbolic test/replay events.
    let durable_run_id = run_id.filter(|value| !value.starts_with("run-"));
    let durable_turn_id =
        turn_id.filter(|value| !value.starts_with("turn-") && !value.starts_with("legacy-turn:"));
    let (id, role, blocks, stop_reason) = agent_message_parts(message);
    let mut payload = serde_json::json!({
        "id": id,
        "conversation_id": conversation_id,
        "role": role,
        "run_id": durable_run_id,
        "turn_id": durable_turn_id,
        "blocks": blocks,
    });
    if role == "assistant" {
        if let Some(run_id) = run_id {
            payload["blocks"]
                .as_array_mut()
                .expect("typed message blocks are an array")
                .push(serde_json::json!({
                    "type": "run_reference",
                    "run_id": run_id,
                }));
        }
    }
    if let Some(reason) = stop_reason {
        payload["stop_reason"] = serde_json::Value::String(reason);
    }
    if turn_id.is_some_and(|value| value.starts_with("legacy-turn:")) {
        payload["legacy_marker"] = serde_json::Value::String("legacy_turn_unknown".into());
    }
    let row = append_message(payload)?;
    row.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "typed message append returned no id".into())
}

fn agent_message_parts(
    message: &AgentMessage,
) -> (String, &'static str, Vec<Value>, Option<String>) {
    match message {
        AgentMessage::User(user) => (
            user.message_id.0.clone(),
            "user",
            content_blocks_to_json(&user.content),
            None,
        ),
        AgentMessage::Assistant(assistant) => (
            assistant.message_id.0.clone(),
            "assistant",
            content_blocks_to_json(&assistant.content),
            assistant.stop_reason.as_ref().map(ToString::to_string),
        ),
        AgentMessage::ToolResult(result) => (
            result.message_id.0.clone(),
            "assistant",
            vec![serde_json::json!({
                "type": "tool_result",
                "tool_call_id": result.tool_call_id,
                "name": result.tool_name,
                "is_error": result.is_error,
                "error_code": result.code,
                "content": result_blocks_to_json(&result.content),
            })],
            None,
        ),
        AgentMessage::System(system) => (
            system.message_id.0.clone(),
            "system",
            vec![serde_json::json!({ "type": "text", "text": system.text })],
            None,
        ),
        AgentMessage::Custom(custom) => (
            custom.message_id.0.clone(),
            "assistant",
            vec![serde_json::json!({ "type": custom.kind, "payload": custom.payload })],
            None,
        ),
    }
}

fn content_blocks_to_json(blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ContentBlock::Thinking { text, signature } => serde_json::json!({ "type": "thinking", "text": text, "signature": signature }),
            ContentBlock::Image { source } => serde_json::json!({ "type": "image", "source": source }),
            ContentBlock::ToolCall(call) => serde_json::json!({ "type": "tool_call", "tool_call_id": call.tool_call_id, "name": call.name, "arguments": call.arguments_json }),
        })
        .collect()
}

fn result_blocks_to_json(blocks: &[ToolResultBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|block| match block {
            ToolResultBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ToolResultBlock::Json { value } => serde_json::json!({ "type": "json", "value": value }),
            ToolResultBlock::Artifact { artifact_id, preview } => serde_json::json!({ "type": "artifact", "artifact_id": artifact_id, "preview": preview }),
        })
        .collect()
}

pub fn append_trigger_message(
    conversation_id: &str,
    content: Option<&str>,
    attachments: Option<&[AttachmentRef]>,
) -> Result<Option<String>, String> {
    let mut blocks = Vec::new();
    if let Some(content) = content.filter(|s| !s.trim().is_empty()) {
        blocks.push(serde_json::json!({ "type": "text", "text": content }));
    }
    let attachments = attachments.unwrap_or(&[]);
    if attachments.len() > 10 {
        return Err("At most 10 attachments are allowed".into());
    }
    for attachment in attachments {
        if attachment.path.trim().is_empty() {
            return Err("attachment path is required".into());
        }
        blocks.push(serde_json::json!({
            "type": "file_reference",
            "path": attachment.path,
            "name": attachment.name.clone(),
            "mime_type": attachment.mime_type.clone(),
            "size": attachment.size,
        }));
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    let appended = append_message(serde_json::json!({
        "conversation_id": conversation_id,
        "role": "user",
        "blocks": blocks,
    }))?;
    Ok(appended
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string))
}

/// Append the current user turn once per run. If `run_id` already has a
/// `trigger_message_id` on the run row, returns that id without inserting.
/// If the latest user message text matches `content`, reuses it (idempotent
/// retry). Otherwise inserts a new daemon-owned message id.
pub fn append_trigger_message_idempotent(
    conversation_id: &str,
    content: Option<&str>,
    attachments: Option<&[AttachmentRef]>,
    run_id: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(run_id) = run_id.filter(|s| !s.trim().is_empty()) {
        if let Ok(store) = store() {
            if let Ok(conn) = store.conn() {
                if let Ok(Some(existing)) = conn
                    .query_row(
                        "SELECT trigger_message_id FROM run WHERE id = ?1 AND trigger_message_id IS NOT NULL",
                        params![run_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                {
                    return Ok(Some(existing));
                }
            }
        }
    }

    // If the latest user message already has the same text, reuse it (duplicate start).
    if let Some(text) = content.filter(|s| !s.trim().is_empty()) {
        if let Ok(messages) =
            get_messages(serde_json::json!({ "conversation_id": conversation_id }))
        {
            if let Some(rows) = messages.as_array() {
                if let Some(last) = rows
                    .iter()
                    .rev()
                    .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
                {
                    let last_text = last
                        .get("content_blocks")
                        .and_then(Value::as_array)
                        .map(|blocks| {
                            blocks
                                .iter()
                                .filter_map(block_text)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    if last_text.trim() == text.trim() {
                        if let Some(id) = last.get("id").and_then(Value::as_str) {
                            return Ok(Some(id.to_string()));
                        }
                    }
                }
            }
        }
    }

    append_trigger_message(conversation_id, content, attachments)
}

pub fn delete_message(message_id: &str) -> Result<(), String> {
    if message_id.trim().is_empty() {
        return Ok(());
    }
    let store = store()?;
    let conn = store.conn()?;
    conn.execute("DELETE FROM message WHERE id = ?1", params![message_id])
        .map_err(|e| e.to_string())?;
    Ok(())
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
mod tests {
    use super::*;

    fn env_lock() -> crate::storage::EnvTestGuard {
        crate::storage::DataStore::env_test_lock()
    }

    /// Stored image blocks used to vanish on the way back out of SQLite, so a
    /// follow-up turn re-sent the conversation without the picture the model
    /// had already been shown.
    #[test]
    fn stored_image_blocks_are_recovered_for_replay() {
        let block = serde_json::json!({
            "type": "image",
            "content": { "imageUrl": "data:image/webp;base64,UklGRg==", "mimeType": "image/webp" }
        });
        let image = block_image(&block).expect("image block should be recovered");
        assert_eq!(image.url, "data:image/webp;base64,UklGRg==");
        assert_eq!(image.media_type.as_deref(), Some("image/webp"));

        // Flat spelling (no nested `content`) is what some writers persist.
        let flat = serde_json::json!({ "type": "image", "image_url": "https://a.test/b.png" });
        assert_eq!(
            block_image(&flat).expect("flat image block").url,
            "https://a.test/b.png"
        );
    }

    #[test]
    fn non_image_blocks_and_urlless_images_yield_nothing() {
        assert!(block_image(&serde_json::json!({"type": "text"})).is_none());
        assert!(block_image(&serde_json::json!({"type": "image"})).is_none());
        assert!(block_image(&serde_json::json!({
            "type": "image",
            "content": { "imageUrl": "  " }
        }))
        .is_none());
    }

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

    struct ClearTestDb;
    impl Drop for ClearTestDb {
        fn drop(&mut self) {
            crate::storage::set_test_db_override(None, None);
        }
    }

    struct EnvRestore {
        db: Option<String>,
        asst: Option<String>,
        rt: Option<String>,
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

    #[tokio::test]
    async fn conversation_round_trip_uses_daemon_tables() {
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

        let created = request(
            names::CONVERSATION_CREATE,
            serde_json::json!({
                "mode": "agent",
                "title": "Test",
                "project_id": "/project/test",
                "provider_id": "p",
                "model_id": "m",
                "permission_profile_id": "readonly"
            }),
        )
        .await
        .unwrap();
        let id = created["id"].as_str().unwrap();
        request(
            names::CONVERSATION_APPEND_MESSAGE,
            serde_json::json!({
                "conversation_id": id,
                "role": "user",
                "blocks": [{ "type": "text", "text": "hello" }]
            }),
        )
        .await
        .unwrap();

        let list = request(names::CONVERSATION_LIST, serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(list[0]["permission_profile_id"], "readonly");
        let messages = request(
            names::CONVERSATION_GET_MESSAGES,
            serde_json::json!({ "conversation_id": id }),
        )
        .await
        .unwrap();
        assert_eq!(messages[0]["content_blocks"][0]["content"]["text"], "hello");
        let history = engine_history(id).unwrap();
        assert_eq!(history[0].content, "hello");
        let reasoning_started = chrono::Utc::now();
        let reasoning_finished = reasoning_started + chrono::Duration::milliseconds(1500);
        append_assistant_turn_from_events(
            id,
            "run-1",
            &[
                RunEventV2 {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    global_sequence: 0,
                    run_sequence: 0,
                    run_id: "run-1".into(),
                    sequence: 1,
                    timestamp: reasoning_started,
                    payload: RunEventKind::ReasoningDelta {
                        text: "inspect persisted path".into(),
                    },
                },
                RunEventV2 {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    global_sequence: 0,
                    run_sequence: 0,
                    run_id: "run-1".into(),
                    sequence: 2,
                    timestamp: reasoning_finished,
                    payload: RunEventKind::TextDelta {
                        text: "done".into(),
                    },
                },
                RunEventV2 {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    global_sequence: 0,
                    run_sequence: 0,
                    run_id: "run-1".into(),
                    sequence: 3,
                    timestamp: chrono::Utc::now(),
                    payload: RunEventKind::ToolCallCompleted {
                        id: "tool-1".into(),
                        name: "read_file".into(),
                        output: serde_json::json!({"ok": true}),
                        is_error: false,
                        duration_ms: 1,
                    },
                },
            ],
        )
        .unwrap();
        let history = engine_history(id).unwrap();
        assert_eq!(history[1].role, "assistant");
        assert!(history[1].content.contains("done"));
        assert!(history[1].content.contains("tool result: read_file"));
        let messages = request(
            names::CONVERSATION_GET_MESSAGES,
            serde_json::json!({ "conversation_id": id }),
        )
        .await
        .unwrap();
        let assistant = messages
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["run_id"], "run-1");
        let reasoning = assistant["content_blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|block| block["type"] == "reasoning")
            .unwrap();
        assert_eq!(reasoning["content"]["reasoning"], "inspect persisted path");
        assert_eq!(reasoning["content"]["duration_ms"], 1500);
    }

    #[test]
    fn context_compression_events_persist_snapshot_and_reenter_history() {
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

        let store = store().unwrap();
        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id)
             VALUES ('compact-conv', 'agent', 'Compact', 'openai', 'gpt-4o')",
                [],
            )
            .unwrap();
        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
             VALUES ('compact-run', 'compact-conv', 'completed', 'openai', 'gpt-4o')",
                [],
            )
            .unwrap();
        append_assistant_turn_from_events(
            "compact-conv",
            "compact-run",
            &[
                RunEventV2 {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    global_sequence: 0,
                    run_sequence: 0,
                    run_id: "compact-run".into(),
                    sequence: 7,
                    timestamp: chrono::Utc::now(),
                    payload: RunEventKind::ContextCompressed {
                        before_tokens: 100,
                        after_tokens: 20,
                        summary: "Previous compacted facts: alpha survives.".into(),
                    },
                },
                RunEventV2 {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    global_sequence: 0,
                    run_sequence: 0,
                    run_id: "compact-run".into(),
                    sequence: 8,
                    timestamp: chrono::Utc::now(),
                    payload: RunEventKind::TextDelta {
                        text: "current answer".into(),
                    },
                },
            ],
        )
        .unwrap();

        let count: i64 = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM context_snapshot WHERE run_id = 'compact-run'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let history = engine_history("compact-conv").unwrap();
        assert_eq!(history[0].role, "system");
        assert!(history[0].content.contains("alpha survives"));
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
    fn typed_message_round_trip_preserves_tool_call_identity() {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let _clear_db = ClearTestDb;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("typed-roundtrip.db");
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        ensure_conversation_stub("typed-conv", "openai", "gpt-4o", None, None).unwrap();
        let message = AgentMessage::ToolResult(agent_core::ToolResultMessage {
            message_id: agent_core::MessageId::from("message-1"),
            tool_call_id: agent_core::ToolCallId::from("call-1"),
            tool_name: "read_file".into(),
            content: vec![ToolResultBlock::Json {
                value: serde_json::json!({"ok": true}),
            }],
            is_error: false,
            code: None,
        });
        append_agent_message("typed-conv", None, None, &message).unwrap();
        let loaded = load_agent_messages("typed-conv").unwrap();
        assert_eq!(loaded, vec![message]);
    }
}
