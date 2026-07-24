use crate::storage::DataStore;
use agent_core::EngineMessage;
use assistant_protocol::v2::methods::names;
use assistant_protocol::v2::{AttachmentRef, RunEventKind, RunEventV2};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::path::PathBuf;

pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        names::CONVERSATION_CREATE => create(params),
        names::CONVERSATION_LIST => list(params),
        names::CONVERSATION_GET => get(params),
        names::CONVERSATION_FORK => fork(params),
        names::CONVERSATION_GET_MESSAGES => get_messages(params),
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
    let project_id = params.get("project_id").and_then(Value::as_str);
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
    create(params)
}

fn get_messages(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let store = store()?;
    let conn = store.conn()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, role, conversation_id, parent_message_id, status, input_tokens, output_tokens, created_at
             FROM message WHERE conversation_id = ?1 ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "role": row.get::<_, String>(1)?,
                "conversation_id": row.get::<_, String>(2)?,
                "parent_message_id": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
                "input_tokens": row.get::<_, Option<i64>>(5)?,
                "output_tokens": row.get::<_, Option<i64>>(6)?,
                "created_at": row.get::<_, String>(7)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let mut messages: Vec<Value> = rows.filter_map(Result::ok).collect();

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
        message["content_blocks"] = serde_json::json!(by_message.remove(id).unwrap_or_default());
    }
    Ok(Value::Array(messages))
}

pub fn engine_history(conversation_id: &str) -> Result<Vec<EngineMessage>, String> {
    let messages = get_messages(serde_json::json!({ "conversation_id": conversation_id }))?;
    let Some(rows) = messages.as_array() else {
        return Ok(Vec::new());
    };
    let mut history: Vec<_> = rows
        .iter()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?.to_string();
            let content = message
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
            if content.trim().is_empty() {
                return None;
            }
            Some(EngineMessage {
                role,
                content,
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            })
        })
        .collect();
    if let Some(summary) = latest_context_summary(conversation_id)? {
        history.insert(
            0,
            EngineMessage {
                role: "system".into(),
                content: summary,
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
        );
    }
    Ok(history)
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
            let content = block.get("content").unwrap_or(block);
            let name = content
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let output = content.get("output").cloned().unwrap_or(Value::Null);
            Some(format!("[tool result: {name} => {output}]"))
        }
        _ => None,
    }
}

pub fn append_assistant_turn_from_events(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<Option<String>, String> {
    persist_context_snapshots_from_events(run_id, events)?;
    let mut text = String::new();
    let mut blocks = vec![serde_json::json!({ "type": "run_reference", "run_id": run_id })];
    for event in events {
        match &event.payload {
            RunEventKind::TextDelta { text: delta } => text.push_str(delta),
            RunEventKind::ToolCallCompleted {
                id,
                name,
                output,
                is_error,
                duration_ms,
            } => blocks.push(serde_json::json!({
                "type": "tool_result",
                "id": id,
                "name": name,
                "output": output,
                "is_error": is_error,
                "duration_ms": duration_ms,
            })),
            _ => {}
        }
    }
    if !text.trim().is_empty() {
        blocks.insert(0, serde_json::json!({ "type": "text", "text": text }));
    }
    if blocks.len() == 1 {
        return Ok(None);
    }
    let appended = append_message(serde_json::json!({
        "conversation_id": conversation_id,
        "role": "assistant",
        "blocks": blocks,
    }))?;
    Ok(appended
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn persist_context_snapshots_from_events(
    run_id: &str,
    events: &[RunEventV2],
) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    for event in events {
        let RunEventKind::ContextCompressed {
            before_tokens,
            after_tokens,
            summary,
        } = &event.payload
        else {
            continue;
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
        conn.execute(
            "INSERT INTO context_snapshot (
                id, run_id, sequence, snapshot_type, token_count, summary, snapshot_json
             )
             VALUES (?1, ?2, ?3, 'compaction', ?4, ?5, ?6)",
            params![
                uuid::Uuid::new_v4().to_string(),
                run_id,
                event.effective_run_sequence() as i64,
                *after_tokens as i64,
                summary,
                serde_json::json!({
                    "before_tokens": before_tokens,
                    "after_tokens": after_tokens,
                    "event_sequence": event.effective_run_sequence()
                })
                .to_string()
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
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO message (id, conversation_id, role, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, conversation_id, role, status, now],
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
            "INSERT INTO message_block (message_id, sort_order, block_type, block_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, index as i64, block_type, content.to_string()],
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
        if let Ok(messages) = get_messages(serde_json::json!({ "conversation_id": conversation_id }))
        {
            if let Some(rows) = messages.as_array() {
                if let Some(last) = rows.iter().rev().find(|m| {
                    m.get("role").and_then(Value::as_str) == Some("user")
                }) {
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
        let db = dir.path().join(format!("natives-{}.db", uuid::Uuid::new_v4()));
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
                    timestamp: chrono::Utc::now(),
                    payload: RunEventKind::TextDelta {
                        text: "done".into(),
                    },
                },
                RunEventV2 {
            event_id: uuid::Uuid::new_v4().to_string(),
            global_sequence: 0,
            run_sequence: 0,
                    run_id: "run-1".into(),
sequence: 2,
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
        let db = dir.path().join(format!("natives-{}.db", uuid::Uuid::new_v4()));
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
        let db = dir.path().join(format!("natives-{}.db", uuid::Uuid::new_v4()));
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
        let db = dir.path().join(format!("natives-del-{}.db", uuid::Uuid::new_v4()));
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
        assert!(usage_in >= 100, "usage_stats should retain folded tokens, got {usage_in}");
    }
}
