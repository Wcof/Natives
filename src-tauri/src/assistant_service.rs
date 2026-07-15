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
pub async fn assistant_status(
    store: State<'_, Mutex<AssistantStore>>,
) -> Result<Value> {
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
        "conversation.appendMessage" => handle_conversation_append_message(data_store, params).await,
        "conversation.rename" => handle_conversation_rename(data_store, params).await,
        "conversation.update_model" => handle_conversation_update_model(data_store, params).await,
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
        "SELECT id, mode, project_id, title, provider_id, model_id, created_at, updated_at, archived_at
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
            "created_at": row.get::<_, String>(6)?,
            "updated_at": row.get::<_, String>(7)?,
            "archived_at": row.get::<_, Option<String>>(8)?
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
    let mode = params.get("mode").and_then(|v| v.as_str()).unwrap_or("agent");
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

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "INSERT INTO assistant_conversations (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![id, mode, project_id, title, provider_id, model_id, now, now],
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
        "created_at": now,
        "updated_at": now,
        "archived_at": null
    }))
}

async fn handle_conversation_get_messages(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
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
        blocks_by_message.entry(message_id).or_default().push(serde_json::json!({
            "type": block_type,
            "index": block_index,
            "content": content,
        }));
    }
    for message in &mut messages {
        let message_id = message.get("id").and_then(Value::as_str).unwrap_or_default();
        message["content_blocks"] = serde_json::json!(blocks_by_message.remove(message_id).unwrap_or_default());
    }
    success_response(serde_json::json!(messages))
}

async fn handle_conversation_append_message(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };
    let role = match params.get("role").and_then(Value::as_str) {
        Some("user" | "assistant" | "system") => params.get("role").and_then(Value::as_str).unwrap(),
        _ => return error_response("INVALID_PARAM", "role must be user, assistant, or system"),
    };
    let content = match params.get("content").and_then(Value::as_str) {
        Some(content) if !content.trim().is_empty() => content,
        _ => return error_response("MISSING_PARAM", "content is required"),
    };
    let status = params.get("status").and_then(Value::as_str).unwrap_or("complete");
    let id = uuid::Uuid::new_v4().to_string();
    let block_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    let transaction = match conn.unchecked_transaction() {
        Ok(transaction) => transaction,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    if let Err(e) = transaction.execute(
        "INSERT INTO assistant_messages (id, conversation_id, role, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, conversation_id, role, status, now],
    ).and_then(|_| transaction.execute(
        "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'text', 0, ?3)",
        rusqlite::params![block_id, id, content],
    )).and_then(|_| transaction.execute(
        "UPDATE assistant_conversations SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, conversation_id],
    )).and_then(|_| transaction.commit()) {
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

async fn handle_conversation_update_model(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
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
    if let Err(e) = conn.execute("DELETE FROM assistant_messages WHERE conversation_id = ?1", rusqlite::params![id]) {
        return error_response("DB_DELETE_ERROR", &e.to_string());
    }
    if let Err(e) = conn.execute("DELETE FROM assistant_conversations WHERE id = ?1", rusqlite::params![id]) {
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
    let content = params.get("content").and_then(Value::as_str);
    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = data_store.conn();
    let transaction = match conn.unchecked_transaction() {
        Ok(transaction) => transaction,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    let trigger_message_id = if let Some(content) = content {
        let message_id = uuid::Uuid::new_v4().to_string();
        let block_id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = transaction.execute(
            "INSERT INTO assistant_messages (id, conversation_id, role, status, created_at) VALUES (?1, ?2, 'user', 'complete', ?3)",
            rusqlite::params![message_id, conversation_id, now],
        ).and_then(|_| transaction.execute(
            "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'text', 0, ?3)",
            rusqlite::params![block_id, message_id, content],
        )) {
            return error_response("DB_INSERT_ERROR", &e.to_string());
        }
        Some(message_id)
    } else {
        params.get("trigger_message_id").and_then(Value::as_str).map(str::to_string)
    };
    if let Err(e) = transaction.execute(
        "INSERT INTO assistant_runs (id, conversation_id, status, trigger_message_id, provider_id, model_id, started_at)
         VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6)",
        rusqlite::params![run_id, conversation_id, trigger_message_id, provider_id, model_id, now],
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
        Some("completed" | "failed" | "interrupted") => params.get("status").and_then(Value::as_str).unwrap(),
        _ => return error_response("INVALID_PARAM", "status must be completed, failed, or interrupted"),
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
            "SELECT id, conversation_id, status, provider_id, model_id, started_at, finished_at, error_code, step_count FROM assistant_runs WHERE conversation_id = ?1 ORDER BY started_at DESC"
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
            "SELECT id, conversation_id, status, provider_id, model_id, started_at, finished_at, error_code, step_count FROM assistant_runs ORDER BY started_at DESC"
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
        "started_at": row.get::<_, Option<String>>(5)?,
        "finished_at": row.get::<_, Option<String>>(6)?,
        "error_code": row.get::<_, Option<String>>(7)?,
        "step_count": row.get::<_, Option<i64>>(8)?
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
         ORDER BY sequence ASC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };

    let after_sequence = params.get("after_sequence").and_then(Value::as_i64).unwrap_or(0);
    let events: Vec<Value> = match stmt.query_map(rusqlite::params![run_id, after_sequence], |row| {
        let payload: Value = serde_json::from_str(&row.get::<_, String>(4)?)
            .unwrap_or(Value::Null);
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
    let approved = params.get("approved").and_then(|v| v.as_bool()).unwrap_or(false);

    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_permission_requests SET responded = 1, approved = ?1, responded_at = ?2 WHERE id = ?3",
        rusqlite::params![approved as i32, now, request_id],
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
        success_response(serde_json::json!(rows.filter_map(|row| row.ok()).collect::<Vec<Value>>()))
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
        let created = dispatch_rpc(&store, "conversation.create", &serde_json::json!({
            "mode": "agent", "title": "Test", "provider_id": "provider", "model_id": "model"
        })).await;
        assert!(created.success);
        let conversation_id = created.data.unwrap()["id"].as_str().unwrap().to_string();

        let appended = dispatch_rpc(&store, "conversation.appendMessage", &serde_json::json!({
            "conversation_id": conversation_id, "role": "user", "content": "Hello"
        })).await;
        assert!(appended.success);

        let messages = dispatch_rpc(&store, "conversation.getMessages", &serde_json::json!({
            "conversation_id": conversation_id
        })).await;
        assert!(messages.success);
        let messages = messages.data.unwrap();
        assert_eq!(messages[0]["content_blocks"][0]["content"]["text"], "Hello");
    }
}
