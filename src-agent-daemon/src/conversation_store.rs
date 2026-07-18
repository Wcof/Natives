use crate::storage::DataStore;
use assistant_protocol::v2::methods::names;
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

fn create(params: Value) -> Result<Value, String> {
    let mode = params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    if !matches!(mode, "chat" | "agent") {
        return Err("mode must be chat or agent".into());
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
