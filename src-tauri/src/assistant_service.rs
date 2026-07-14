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
        "conversation.rename" => handle_conversation_rename(data_store, params).await,
        "conversation.archive" => handle_conversation_archive(data_store, params).await,
        "conversation.delete" => handle_conversation_delete(data_store, params).await,
        "run.start" => handle_run_start(data_store, params).await,
        "run.cancel" => handle_run_cancel(data_store, params).await,
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
        "SELECT id, project_id, title, provider_id, model_id, created_at, updated_at, archived
         FROM assistant_conversations ORDER BY updated_at DESC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    let rows = match stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "project_id": row.get::<_, Option<String>>(1)?,
            "title": row.get::<_, String>(2)?,
            "provider_id": row.get::<_, String>(3)?,
            "model_id": row.get::<_, String>(4)?,
            "created_at": row.get::<_, String>(5)?,
            "updated_at": row.get::<_, String>(6)?,
            "archived": row.get::<_, bool>(7)?
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
        "INSERT INTO assistant_conversations (id, project_id, title, provider_id, model_id, created_at, updated_at, archived)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
        rusqlite::params![id, project_id, title, provider_id, model_id, now, now],
    ) {
        return error_response("DB_INSERT_ERROR", &e.to_string());
    }

    success_response(serde_json::json!({
        "id": id,
        "project_id": project_id,
        "title": title,
        "provider_id": provider_id,
        "model_id": model_id,
        "created_at": now,
        "updated_at": now,
        "archived": false
    }))
}

async fn handle_conversation_get_messages(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };

    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, role, conversation_id, created_at, token_count
         FROM assistant_messages
         WHERE conversation_id = ?1
         ORDER BY created_at ASC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };

    let rows = match stmt.query_map(rusqlite::params![conversation_id], |row| {
        let msg_id: String = row.get(0)?;
        Ok(serde_json::json!({
            "id": msg_id,
            "role": row.get::<_, String>(1)?,
            "conversation_id": row.get::<_, String>(2)?,
            "created_at": row.get::<_, String>(3)?,
            "token_count": row.get::<_, Option<i64>>(4)?
        }))
    }) {
        Ok(r) => r,
        Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
    };

    let messages: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
    success_response(serde_json::json!(messages))
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

async fn handle_conversation_archive(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => return error_response("MISSING_PARAM", "id is required"),
    };

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_conversations SET archived = 1 WHERE id = ?1",
        rusqlite::params![id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": id, "archived": true }))
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

// ─── Run handlers (stubs — will be expanded with executor integration) ───

async fn handle_run_start(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };

    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "INSERT INTO assistant_runs (id, conversation_id, status, created_at, updated_at)
         VALUES (?1, ?2, 'running', ?3, ?4)",
        rusqlite::params![run_id, conversation_id, now, now],
    ) {
        return error_response("DB_INSERT_ERROR", &e.to_string());
    }

    success_response(serde_json::json!({
        "id": run_id,
        "conversation_id": conversation_id,
        "status": "running",
        "created_at": now
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
        "UPDATE assistant_runs SET status = 'cancelled', updated_at = ?1 WHERE id = ?2 AND status = 'running'",
        rusqlite::params![now, run_id],
    ) {
        return error_response("DB_UPDATE_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "id": run_id, "status": "cancelled" }))
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
            "SELECT id, conversation_id, status, created_at, updated_at FROM assistant_runs WHERE conversation_id = ?1 ORDER BY created_at DESC"
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
            "SELECT id, conversation_id, status, created_at, updated_at FROM assistant_runs ORDER BY created_at DESC"
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
        "created_at": row.get::<_, String>(3)?,
        "updated_at": row.get::<_, String>(4)?
    }))
}

async fn handle_run_get_events(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };

    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, run_id, event_type, data, created_at
         FROM assistant_run_events
         WHERE run_id = ?1
         ORDER BY created_at ASC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };

    let events: Vec<Value> = match stmt.query_map(rusqlite::params![run_id], |row| {
        let data_val: Option<Value> = row.get::<_, Option<String>>(3)?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "run_id": row.get::<_, String>(1)?,
            "event_type": row.get::<_, String>(2)?,
            "data": data_val,
            "created_at": row.get::<_, String>(4)?
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

    if let Some(cid) = conversation_id {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, run_id, path, mime_type, created_at FROM assistant_artifacts WHERE conversation_id = ?1 ORDER BY created_at DESC"
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
            "SELECT id, conversation_id, run_id, path, mime_type, created_at FROM assistant_artifacts ORDER BY created_at DESC"
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
        "created_at": row.get::<_, String>(5)?
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
