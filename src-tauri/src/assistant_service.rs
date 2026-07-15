use crate::daemon::data::DataStore;
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// Shared assistant data store managed by Tauri state
pub struct AssistantStore {
    pub store: Arc<DataStore>,
}

impl AssistantStore {
    pub fn new(store: Arc<DataStore>) -> Self {
        Self { store }
    }
}

/// RPC request from frontend
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// RPC response to frontend
#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

/// Tauri command: assistant_rpc_request
/// Dispatches RPC method calls to the assistant service.
/// Frontend calls via: window.nativesAPI.assistantV2.request(method, params)
#[tauri::command]
pub async fn assistant_rpc_request(
    store: State<'_, Mutex<AssistantStore>>,
    method: String,
    params: Option<Value>,
) -> Result<RpcResponse> {
    let store_ref = {
        let guard = store.lock().await;
        Arc::clone(&guard.store)
    };
    let params = params.unwrap_or(Value::Null);
    let response = dispatch_rpc(&store_ref, &method, &params).await;
    Ok(response)
}

/// Tauri command: assistant_status
/// Lightweight health check — returns connected: true if the store is ready.
#[tauri::command]
pub async fn assistant_status(store: State<'_, Mutex<AssistantStore>>) -> Result<Value> {
    let store_ref = {
        let guard = store.lock().await;
        Arc::clone(&guard.store)
    };
    let conn = store_ref.conn();
    // Try a lightweight query to confirm DB is operational
    match conn.execute_batch("SELECT 1") {
        Ok(_) => Ok(serde_json::json!({
            "connected": true,
            "error": null
        })),
        Err(e) => Ok(serde_json::json!({
            "connected": false,
            "error": format!("Database error: {}", e)
        })),
    }
}

/// Dispatch RPC method to the appropriate handler
async fn dispatch_rpc(data_store: &Arc<DataStore>, method: &str, params: &Value) -> RpcResponse {
    match method {
        "conversation.list" => handle_conversation_list(data_store, params).await,
        "conversation.create" => handle_conversation_create(data_store, params).await,
        "conversation.getMessages" => handle_conversation_get_messages(data_store, params).await,
        "conversation.appendMessage" => {
            handle_conversation_append_message(data_store, params).await
        }
        "conversation.rename" => handle_conversation_rename(data_store, params).await,
        "conversation.update_model" => handle_conversation_update_model(data_store, params).await,
        "conversation.update_permission" => {
            handle_conversation_update_permission(data_store, params).await
        }
        "conversation.archive" => handle_conversation_archive(data_store, params).await,
        "conversation.delete" => handle_conversation_delete(data_store, params).await,
        "run.start" => handle_run_start(data_store, params).await,
        "run.cancel" => handle_run_cancel(data_store, params).await,
        "run.finish" => handle_run_finish(data_store, params).await,
        "run.retry" => handle_run_retry(data_store, params).await,
        "run.list" => handle_run_list(data_store, params).await,
        "run.getEvents" => handle_run_get_events(data_store, params).await,
        "permission.respond" => handle_permission_respond(data_store, params).await,
        "artifact.list" => handle_artifact_list(data_store, params).await,
        "artifact.open" => handle_artifact_open(data_store, params).await,
        _ => RpcResponse {
            success: false,
            data: None,
            error: Some(RpcError {
                code: "METHOD_NOT_FOUND".to_string(),
                message: format!("Unknown RPC method: {}", method),
            }),
        },
    }
}

fn error_response(code: &str, message: &str) -> RpcResponse {
    RpcResponse {
        success: false,
        data: None,
        error: Some(RpcError {
            code: code.to_string(),
            message: message.to_string(),
        }),
    }
}

fn success_response(data: Value) -> RpcResponse {
    RpcResponse {
        success: true,
        data: Some(data),
        error: None,
    }
}

// ─── Conversation handlers ───

async fn handle_conversation_list(data_store: &Arc<DataStore>, _params: &Value) -> RpcResponse {
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at, archived_at
         FROM assistant_conversations ORDER BY updated_at DESC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    let rows = match stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "mode": row.get::<_, String>(1)?,
            "project_id": row.get::<_, Option<String>>(2)?,
            "title": row.get::<_, String>(3)?,
            "provider_id": row.get::<_, String>(4)?,
            "model_id": row.get::<_, String>(5)?,
            "permission_profile_id": row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "ask".to_string()),
            "created_at": row.get::<_, String>(7)?,
            "updated_at": row.get::<_, String>(8)?,
            "archived_at": row.get::<_, Option<String>>(9)?
        }))
    }) {
        Ok(r) => r,
        Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
    };
    let conversations: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
    success_response(serde_json::json!(conversations))
}

async fn handle_conversation_create(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let project_id = params.get("project_id").and_then(|v| v.as_str());
    let mode = params
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("agent");
    let title = match params.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return error_response("MISSING_PARAM", "title is required"),
    };
    let provider_id = match params.get("provider_id").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return error_response("MISSING_PARAM", "provider_id is required"),
    };
    let model_id = match params.get("model_id").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return error_response("MISSING_PARAM", "model_id is required"),
    };
    let permission_profile_id = params
        .get("permission_profile_id")
        .and_then(Value::as_str)
        .unwrap_or("ask");
    if !matches!(permission_profile_id, "readonly" | "ask" | "full_access") {
        return error_response(
            "INVALID_PARAM",
            "permission_profile_id must be readonly, ask, or full_access",
        );
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "INSERT INTO assistant_conversations (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![id, mode, project_id, title, provider_id, model_id, permission_profile_id, now, now],
    ) {
        return error_response("DB_INSERT_ERROR", &e.to_string());
    }

    success_response(serde_json::json!({
        "id": id,
        "mode": mode,
        "project_id": project_id,
        "title": title,
        "provider_id": provider_id,
        "model_id": model_id,
        "permission_profile_id": permission_profile_id,
        "created_at": now,
        "updated_at": now,
        "archived_at": null
    }))
}

async fn handle_conversation_get_messages(
    data_store: &Arc<DataStore>,
    params: &Value,
) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };

    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, role, conversation_id, parent_message_id, status, input_tokens, output_tokens, created_at
         FROM assistant_messages
         WHERE conversation_id = ?1
         ORDER BY created_at ASC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };

    let rows = match stmt.query_map(rusqlite::params![conversation_id], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "role": row.get::<_, String>(1)?,
            "conversation_id": row.get::<_, String>(2)?,
            "parent_message_id": row.get::<_, Option<String>>(3)?,
            "status": row.get::<_, String>(4)?,
            "input_tokens": row.get::<_, Option<i64>>(5)?,
            "output_tokens": row.get::<_, Option<i64>>(6)?,
            "created_at": row.get::<_, String>(7)?
        }))
    }) {
        Ok(r) => r,
        Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
    };

    let mut messages: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
    let mut blocks = match conn.prepare(
        "SELECT message_id, block_type, block_index, content FROM assistant_message_blocks WHERE message_id IN (SELECT id FROM assistant_messages WHERE conversation_id = ?1) ORDER BY block_index ASC"
    ) {
        Ok(statement) => statement,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    let block_rows = match blocks.query_map(rusqlite::params![conversation_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
    };
    let mut blocks_by_message = std::collections::HashMap::<String, Vec<Value>>::new();
    for (message_id, block_type, block_index, content) in block_rows.filter_map(|row| row.ok()) {
        let content = if block_type == "text" {
            serde_json::json!({ "text": content })
        } else {
            serde_json::from_str(&content).unwrap_or(serde_json::json!({ "content": content }))
        };
        blocks_by_message
            .entry(message_id)
            .or_default()
            .push(serde_json::json!({
                "type": block_type,
                "index": block_index,
                "content": content,
            }));
    }
    for message in &mut messages {
        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        message["content_blocks"] =
            serde_json::json!(blocks_by_message.remove(message_id).unwrap_or_default());
    }
    success_response(serde_json::json!(messages))
}

async fn handle_conversation_append_message(
    data_store: &Arc<DataStore>,
    params: &Value,
) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };
    let role = match params.get("role").and_then(Value::as_str) {
        Some("user" | "assistant" | "system") => {
            params.get("role").and_then(Value::as_str).unwrap()
        }
        _ => return error_response("INVALID_PARAM", "role must be user, assistant, or system"),
    };
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty());
    let structured_blocks = params
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if content.is_none() && structured_blocks.is_empty() {
        return error_response("MISSING_PARAM", "content or blocks is required");
    }
    let status = params
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("complete");
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    let transaction = match conn.unchecked_transaction() {
        Ok(transaction) => transaction,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    if let Err(e) = transaction.execute(
        "INSERT INTO assistant_messages (id, conversation_id, role, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, conversation_id, role, status, now],
    ) {
        return error_response("DB_INSERT_ERROR", &e.to_string());
    }
    let mut blocks = structured_blocks;
    if blocks.is_empty() {
        blocks.push(serde_json::json!({ "type": "text", "text": content.unwrap_or_default() }));
    }
    for (index, block) in blocks.iter().enumerate() {
        let block_type = match block.get("type").and_then(Value::as_str) {
            Some(block_type) => block_type,
            None => return error_response("INVALID_PARAM", "block type is required"),
        };
        let block_content = if block_type == "text" {
            block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        } else {
            block.to_string()
        };
        if let Err(e) = transaction.execute(
            "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), id, block_type, index as i64, block_content],
        ) {
            return error_response("DB_INSERT_ERROR", &e.to_string());
        }
    }
    if let Err(e) = transaction
        .execute(
            "UPDATE assistant_conversations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id],
        )
        .and_then(|_| transaction.commit())
    {
        return error_response("DB_INSERT_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": id, "created_at": now }))
}

async fn handle_conversation_rename(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => return error_response("MISSING_PARAM", "id is required"),
    };
    let title = match params.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return error_response("MISSING_PARAM", "title is required"),
    };

    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![title, now, id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": id, "title": title }))
}

async fn handle_conversation_update_model(
    data_store: &Arc<DataStore>,
    params: &Value,
) -> RpcResponse {
    let id = match params.get("id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "id is required"),
    };
    let provider_id = match params.get("provider_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "provider_id is required"),
    };
    let model_id = match params.get("model_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "model_id is required"),
    };
    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_conversations SET provider_id = ?1, model_id = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![provider_id, model_id, now, id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": id, "updated_at": now }))
}

async fn handle_conversation_update_permission(
    data_store: &Arc<DataStore>,
    params: &Value,
) -> RpcResponse {
    let id = match params.get("id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "id is required"),
    };
    let profile = match params.get("permission_profile_id").and_then(Value::as_str) {
        Some(profile @ ("readonly" | "ask" | "full_access")) => profile,
        _ => {
            return error_response(
                "INVALID_PARAM",
                "permission_profile_id must be readonly, ask, or full_access",
            )
        }
    };
    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    match conn.execute(
        "UPDATE assistant_conversations SET permission_profile_id = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![profile, now, id],
    ) {
        Ok(0) => error_response("NOT_FOUND", "conversation not found"),
        Ok(_) => success_response(serde_json::json!({ "id": id, "permission_profile_id": profile, "updated_at": now })),
        Err(e) => error_response("DB_UPDATE_ERROR", &e.to_string()),
    }
}

async fn handle_conversation_archive(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => return error_response("MISSING_PARAM", "id is required"),
    };

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_conversations SET archived_at = ?1 WHERE id = ?2",
        rusqlite::params![chrono::Utc::now().to_rfc3339(), id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": id, "archived_at": true }))
}

async fn handle_conversation_delete(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => return error_response("MISSING_PARAM", "id is required"),
    };

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "DELETE FROM assistant_messages WHERE conversation_id = ?1",
        rusqlite::params![id],
    ) {
        return error_response("DB_DELETE_ERROR", &e.to_string());
    }
    if let Err(e) = conn.execute(
        "DELETE FROM assistant_conversations WHERE id = ?1",
        rusqlite::params![id],
    ) {
        return error_response("DB_DELETE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "deleted": true }))
}

// ─── Run handlers ───

async fn handle_run_start(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };

    let provider_id = match params.get("provider_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "provider_id is required"),
    };
    let model_id = match params.get("model_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "model_id is required"),
    };
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty());
    let attachments = params
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = data_store.conn();
    let transaction = match conn.unchecked_transaction() {
        Ok(transaction) => transaction,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    let trigger_message_id = if content.is_some() || !attachments.is_empty() {
        let message_id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = transaction.execute(
            "INSERT INTO assistant_messages (id, conversation_id, role, status, created_at) VALUES (?1, ?2, 'user', 'complete', ?3)",
            rusqlite::params![message_id, conversation_id, now],
        ) {
            return error_response("DB_INSERT_ERROR", &e.to_string());
        }
        let mut block_index = 0_i64;
        if let Some(content) = content {
            if let Err(e) = transaction.execute(
                "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'text', ?3, ?4)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), message_id, block_index, content],
            ) {
                return error_response("DB_INSERT_ERROR", &e.to_string());
            }
            block_index += 1;
        }
        for attachment in &attachments {
            let path = match attachment.get("path").and_then(Value::as_str) {
                Some(path) if !path.trim().is_empty() => path,
                _ => return error_response("INVALID_PARAM", "attachment path is required"),
            };
            let payload = serde_json::json!({
                "path": path,
                "name": attachment.get("name").and_then(Value::as_str).unwrap_or(path),
                "mime_type": attachment.get("mime_type").and_then(Value::as_str).unwrap_or("application/octet-stream"),
                "size": attachment.get("size").and_then(Value::as_i64).unwrap_or(0),
            });
            if let Err(e) = transaction.execute(
                "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'file_reference', ?3, ?4)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), message_id, block_index, payload.to_string()],
            ) {
                return error_response("DB_INSERT_ERROR", &e.to_string());
            }
            block_index += 1;
        }
        Some(message_id)
    } else {
        params
            .get("trigger_message_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let permission_profile = transaction.query_row(
        "SELECT COALESCE(permission_profile_id, 'ask') FROM assistant_conversations WHERE id = ?1",
        rusqlite::params![conversation_id],
        |row| row.get::<_, String>(0),
    ).unwrap_or_else(|_| "ask".to_string());
    if let Err(e) = transaction.execute(
        "INSERT INTO assistant_runs (id, conversation_id, status, trigger_message_id, provider_id, model_id, permission_profile, started_at)
         VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![run_id, conversation_id, trigger_message_id, provider_id, model_id, permission_profile, now],
    ).and_then(|_| transaction.execute(
        "UPDATE assistant_conversations SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, conversation_id],
    )).and_then(|_| transaction.commit()) {
        return error_response("DB_INSERT_ERROR", &e.to_string());
    }

    success_response(serde_json::json!({
        "id": run_id,
        "conversation_id": conversation_id,
        "status": "running",
        "provider_id": provider_id,
        "model_id": model_id,
        "permission_profile": permission_profile,
        "started_at": now
    }))
}

async fn handle_run_cancel(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };

    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_runs SET status = 'interrupted', finished_at = ?1 WHERE id = ?2 AND status IN ('queued', 'preparing', 'running', 'cancelling')",
        rusqlite::params![now, run_id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": run_id, "status": "interrupted" }))
}

async fn handle_run_finish(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };
    let status = match params.get("status").and_then(Value::as_str) {
        Some("completed" | "failed" | "interrupted") => {
            params.get("status").and_then(Value::as_str).unwrap()
        }
        _ => {
            return error_response(
                "INVALID_PARAM",
                "status must be completed, failed, or interrupted",
            )
        }
    };
    let error_code = params.get("error_code").and_then(Value::as_str);
    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_runs SET status = ?1, error_code = ?2, finished_at = ?3 WHERE id = ?4",
        rusqlite::params![status, error_code, now, run_id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": run_id, "status": status, "finished_at": now }))
}

async fn handle_run_retry(_data_store: &Arc<DataStore>, _params: &Value) -> RpcResponse {
    // For now, return not-implemented — will be wired to assistant_executor
    error_response("NOT_IMPLEMENTED", "run.retry is not yet implemented")
}

async fn handle_run_list(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = params.get("conversation_id").and_then(|v| v.as_str());

    if let Some(cid) = conversation_id {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, status, provider_id, model_id, permission_profile, started_at, finished_at, error_code, step_count FROM assistant_runs WHERE conversation_id = ?1 ORDER BY started_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map(rusqlite::params![cid], row_to_run) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        let collected: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
        drop(stmt);
        drop(conn);
        success_response(serde_json::json!(collected))
    } else {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, status, provider_id, model_id, permission_profile, started_at, finished_at, error_code, step_count FROM assistant_runs ORDER BY started_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map([], row_to_run) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        let collected: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
        drop(stmt);
        drop(conn);
        success_response(serde_json::json!(collected))
    }
}

fn row_to_run(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "conversation_id": row.get::<_, String>(1)?,
        "status": row.get::<_, String>(2)?,
        "provider_id": row.get::<_, String>(3)?,
        "model_id": row.get::<_, String>(4)?,
        "permission_profile": row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "ask".to_string()),
        "started_at": row.get::<_, Option<String>>(6)?,
        "finished_at": row.get::<_, Option<String>>(7)?,
        "error_code": row.get::<_, Option<String>>(8)?,
        "step_count": row.get::<_, Option<i64>>(9)?
    }))
}

async fn handle_run_get_events(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };

    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT run_id, sequence, timestamp, event_type, payload
         FROM assistant_run_events
         WHERE run_id = ?1
         AND sequence > COALESCE(?2, 0)
         ORDER BY sequence ASC",
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };

    let after_sequence = params
        .get("after_sequence")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let events: Vec<Value> =
        match stmt.query_map(rusqlite::params![run_id, after_sequence], |row| {
            let payload: Value =
                serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or(Value::Null);
            Ok(serde_json::json!({
                "run_id": row.get::<_, String>(0)?,
                "sequence": row.get::<_, i64>(1)?,
                "timestamp": row.get::<_, String>(2)?,
                "type": row.get::<_, String>(3)?,
                "payload": payload
            }))
        }) {
            Ok(r) => r.filter_map(|r| r.ok()).collect(),
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };

    success_response(serde_json::json!(events))
}

// ─── Permission handler ───

async fn handle_permission_respond(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let request_id = match params.get("request_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "request_id is required"),
    };
    let approved = params
        .get("approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let scope = params
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("once");

    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_permission_requests SET status = ?1, scope = ?2, responded_at = ?3 WHERE id = ?4",
        rusqlite::params![if approved { "approved" } else { "rejected" }, scope, now, request_id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "request_id": request_id, "approved": approved }))
}

// ─── Artifact handlers ───

async fn handle_artifact_list(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = params.get("conversation_id").and_then(|v| v.as_str());
    let run_id = params.get("run_id").and_then(|v| v.as_str());

    if let Some(run_id) = run_id {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, run_id, path, mime_type, created_at, label, kind, size FROM assistant_artifacts WHERE run_id = ?1 ORDER BY created_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map(rusqlite::params![run_id], row_to_artifact) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        success_response(serde_json::json!(rows
            .filter_map(|row| row.ok())
            .collect::<Vec<Value>>()))
    } else if let Some(cid) = conversation_id {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, run_id, path, mime_type, created_at, label, kind, size FROM assistant_artifacts WHERE conversation_id = ?1 ORDER BY created_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map(rusqlite::params![cid], row_to_artifact) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        let collected: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
        drop(stmt);
        drop(conn);
        success_response(serde_json::json!(collected))
    } else {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, run_id, path, mime_type, created_at, label, kind, size FROM assistant_artifacts ORDER BY created_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map([], row_to_artifact) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        let collected: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
        drop(stmt);
        drop(conn);
        success_response(serde_json::json!(collected))
    }
}

fn row_to_artifact(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "conversation_id": row.get::<_, String>(1)?,
        "run_id": row.get::<_, String>(2)?,
        "path": row.get::<_, String>(3)?,
        "mime_type": row.get::<_, String>(4)?,
        "created_at": row.get::<_, String>(5)?,
        "label": row.get::<_, Option<String>>(6)?,
        "kind": row.get::<_, String>(7)?,
        "size": row.get::<_, i64>(8)?
    }))
}

async fn handle_artifact_open(_data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return error_response("MISSING_PARAM", "path is required"),
    };

    // Open artifact with system default application
    if let Err(e) = open::that(path) {
        return error_response("OPEN_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "opened": path }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn conversation_messages_round_trip_with_current_schema() {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        let created = dispatch_rpc(
            &store,
            "conversation.create",
            &serde_json::json!({
                "mode": "agent", "title": "Test", "provider_id": "provider", "model_id": "model"
            }),
        )
        .await;
        assert!(created.success);
        let conversation_id = created.data.unwrap()["id"].as_str().unwrap().to_string();

        let appended = dispatch_rpc(
            &store,
            "conversation.appendMessage",
            &serde_json::json!({
                "conversation_id": conversation_id, "role": "user", "content": "Hello"
            }),
        )
        .await;
        assert!(appended.success);

        let messages = dispatch_rpc(
            &store,
            "conversation.getMessages",
            &serde_json::json!({
                "conversation_id": conversation_id
            }),
        )
        .await;
        assert!(messages.success);
        let messages = messages.data.unwrap();
        assert_eq!(messages[0]["content_blocks"][0]["content"]["text"], "Hello");
    }

    #[tokio::test]
    async fn conversation_permission_and_attachments_round_trip() {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        let created = dispatch_rpc(
            &store,
            "conversation.create",
            &serde_json::json!({
                "mode": "agent",
                "title": "Attachment test",
                "provider_id": "provider",
                "model_id": "model",
                "permission_profile_id": "ask"
            }),
        )
        .await;
        assert!(created.success);
        let conversation_id = created.data.unwrap()["id"].as_str().unwrap().to_string();

        let updated = dispatch_rpc(
            &store,
            "conversation.update_permission",
            &serde_json::json!({
                "id": conversation_id,
                "permission_profile_id": "readonly"
            }),
        )
        .await;
        assert!(updated.success);

        let started = dispatch_rpc(
            &store,
            "run.start",
            &serde_json::json!({
                "conversation_id": conversation_id,
                "provider_id": "provider",
                "model_id": "model",
                "content": "Inspect this file",
                "attachments": [{
                    "path": "/tmp/example.txt",
                    "name": "example.txt",
                    "mime_type": "text/plain",
                    "size": 12
                }]
            }),
        )
        .await;
        assert!(started.success);
        let run_id = started.data.as_ref().unwrap()["id"].as_str().unwrap();
        store.conn().execute(
            "INSERT INTO assistant_permission_requests (id, run_id, tool_call_id, tool_name, reason, input, created_at) VALUES ('permission', ?1, 'tool', 'Read', 'test', '{}', ?2)",
            rusqlite::params![run_id, chrono::Utc::now().to_rfc3339()],
        ).unwrap();
        let responded = dispatch_rpc(&store, "permission.respond", &serde_json::json!({
            "request_id": "permission", "approved": true, "scope": "this_run"
        })).await;
        assert!(responded.success);
        let permission: (String, String) = store.conn().query_row(
            "SELECT status, scope FROM assistant_permission_requests WHERE id = 'permission'", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(permission, ("approved".into(), "this_run".into()));

        let conversations = dispatch_rpc(&store, "conversation.list", &Value::Null)
            .await
            .data
            .unwrap();
        assert_eq!(conversations[0]["permission_profile_id"], "readonly");
        let runs = dispatch_rpc(
            &store,
            "run.list",
            &serde_json::json!({
                "conversation_id": conversation_id
            }),
        )
        .await
        .data
        .unwrap();
        assert_eq!(runs[0]["permission_profile"], "readonly");
        let messages = dispatch_rpc(
            &store,
            "conversation.getMessages",
            &serde_json::json!({
                "conversation_id": conversation_id
            }),
        )
        .await
        .data
        .unwrap();
        assert_eq!(messages[0]["content_blocks"][1]["type"], "file_reference");
        assert_eq!(
            messages[0]["content_blocks"][1]["content"]["path"],
            "/tmp/example.txt"
        );
    }

    #[tokio::test]
    async fn structured_assistant_blocks_round_trip() {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        let created = dispatch_rpc(
            &store,
            "conversation.create",
            &serde_json::json!({
                "mode": "agent", "title": "Blocks", "provider_id": "p", "model_id": "m"
            }),
        )
        .await;
        let conversation_id = created.data.unwrap()["id"].as_str().unwrap().to_string();
        let appended = dispatch_rpc(&store, "conversation.appendMessage", &serde_json::json!({
            "conversation_id": conversation_id,
            "role": "assistant",
            "blocks": [{ "type": "reasoning", "reasoning": "checked" }, { "type": "text", "text": "done" }]
        })).await;
        assert!(appended.success);
        let messages = dispatch_rpc(
            &store,
            "conversation.getMessages",
            &serde_json::json!({ "conversation_id": conversation_id }),
        )
        .await
        .data
        .unwrap();
        assert_eq!(
            messages[0]["content_blocks"][0]["content"]["reasoning"],
            "checked"
        );
        assert_eq!(messages[0]["content_blocks"][1]["content"]["text"], "done");
    }
}
