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
        names::CONVERSATION_LIST => list(),
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

fn store() -> Result<DataStore, String> {
    let db_path = std::env::var("NATIVES_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::default_natives_db_path());
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
    }))
}

fn list() -> Result<Value, String> {
    let store = store()?;
    let conn = store.conn()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at, archived_at
             FROM conversation ORDER BY updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
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
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at, archived_at
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
                params![run_id, event.sequence as i64],
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
                event.sequence as i64,
                *after_tokens as i64,
                summary,
                serde_json::json!({
                    "before_tokens": before_tokens,
                    "after_tokens": after_tokens,
                    "event_sequence": event.sequence
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
    let id = id_param(&params)?;
    for run in crate::global_run_manager()
        .list_runs(Some(id))
        .into_iter()
        .filter(|run| run.status.is_active())
    {
        let _ = crate::global_run_manager()
            .cancel(assistant_protocol::v2::CancelRunRequest { run_id: run.id })
            .await;
    }
    exec_update("DELETE FROM conversation WHERE id = ?1", params![id])?;
    Ok(serde_json::json!({ "deleted": true }))
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

    #[tokio::test]
    async fn conversation_round_trip_uses_daemon_tables() {
        let dir = tempfile::tempdir().unwrap();
        let previous_db = std::env::var("NATIVES_DB_PATH").ok();
        let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
        std::env::set_var("NATIVES_DB_PATH", dir.path().join("natives.db"));
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

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

        let list = request(names::CONVERSATION_LIST, Value::Null)
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
                    run_id: "run-1".into(),
                    sequence: 1,
                    timestamp: chrono::Utc::now(),
                    payload: RunEventKind::TextDelta {
                        text: "done".into(),
                    },
                },
                RunEventV2 {
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

        if let Some(value) = previous_db {
            std::env::set_var("NATIVES_DB_PATH", value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
    }

    #[test]
    fn context_compression_events_persist_snapshot_and_reenter_history() {
        let dir = tempfile::tempdir().unwrap();
        let previous_db = std::env::var("NATIVES_DB_PATH").ok();
        let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
        std::env::set_var("NATIVES_DB_PATH", dir.path().join("natives.db"));
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

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

        if let Some(value) = previous_db {
            std::env::set_var("NATIVES_DB_PATH", value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
    }
}
