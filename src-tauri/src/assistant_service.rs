//! Assistant Host RPC facade: wire types, router, temporary permission dual-write.
//! Capability modules live under `assistant_service/`.
use crate::daemon::data::DataStore;
use crate::daemon_authority;

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


/// Dispatch RPC method to the appropriate handler.
/// Host-owned methods stay local; everything else implemented goes to Daemon authority.
async fn dispatch_rpc(data_store: &Arc<DataStore>, method: &str, params: &Value) -> RpcResponse {
    if is_host_owned_method(method) {
        return match method {
            "daemon.getCapabilities" => capabilities::handle_host_get_capabilities().await,
            "provider.list" => provider_catalog::handle_provider_list(data_store, params).await,
            "run.start" => run_gateway::handle_run_start(data_store, params).await,
            "run.subscribe" => run_gateway::handle_run_subscribe(data_store, params).await,
            "permission.respond" => handle_permission_respond(data_store, params).await,
            "permission.listPending" => handle_permission_list_pending(data_store, params).await,
            "artifact.list" => artifacts::handle_artifact_list(data_store, params).await,
            "artifact.open" | "artifact.reveal" => artifacts::handle_artifact_open(data_store, params).await,
            _ => error_response("METHOD_NOT_FOUND", &format!("Unknown host-owned method: {method}")),
        };
    }

    if assistant_protocol::v2::is_implemented_method(method)
        || method.starts_with("conversation.")
        || method.starts_with("promptQueue.")
        || method.starts_with("interaction.")
        || method.starts_with("task.")
        || method.starts_with("run.")
        || method.starts_with("daemon.")
        || method.starts_with("provider.")
        || method.starts_with("tool.")
        || method.starts_with("mcp.")
        || method.starts_with("scheduler.")
        || method.starts_with("extension.")
        || method.starts_with("skill.")
        || method.starts_with("memory.")
    {
        return match daemon_authority::request(method, params.clone()).await {
            Ok(data) => success_response(data),
            Err(error) => error_response("DAEMON_RPC_ERROR", &error),
        };
    }

    if assistant_protocol::v2::is_known_method(method) {
        return error_response(
            "UNSUPPORTED",
            &format!("RPC method is not implemented: {method}"),
        );
    }
    error_response("METHOD_NOT_FOUND", &format!("Unknown RPC method: {method}"))
}

/// Host retains only OS-bound / preflight methods. All other implemented methods
/// default to Daemon authority (no parallel Host CRUD).
pub(crate) fn is_host_owned_method(method: &str) -> bool {
    matches!(
        method,
        "daemon.getCapabilities"
            | "provider.list"
            | "run.start"
            | "run.subscribe"
            | "permission.respond"
            | "permission.listPending"
            | "artifact.list"
            | "artifact.open"
            | "artifact.reveal"
    )
}

/// Compatibility alias used by tests and older call sites.
#[cfg(test)]
pub(crate) fn daemon_owned_method(method: &str) -> bool {
    !is_host_owned_method(method)
        && (assistant_protocol::v2::is_implemented_method(method)
            || method.starts_with("conversation.")
            || method.starts_with("promptQueue.")
            || method.starts_with("interaction.")
            || method.starts_with("task.")
            || method.starts_with("run.")
            || method.starts_with("daemon.")
            || method.starts_with("provider.")
            || method.starts_with("tool.")
            || method.starts_with("mcp.")
            || method.starts_with("scheduler.")
            || method.starts_with("extension.")
            || method.starts_with("skill.")
            || method.starts_with("memory."))
}

pub(crate) fn error_response(code: &str, message: &str) -> RpcResponse {
    RpcResponse {
        success: false,
        data: None,
        error: Some(RpcError {
            code: code.to_string(),
            message: message.to_string(),
        }),
    }
}

pub(crate) fn success_response(data: Value) -> RpcResponse {
    RpcResponse {
        success: true,
        data: Some(data),
        error: None,
    }
}


/// Honest per-runtime status for settings / RuntimePanel (REQ-T03).

mod artifacts;
mod capabilities;
mod provider_catalog;
mod run_gateway;

#[cfg(test)]
mod tests;

// ─── Permission (host dual-write until task-04 integration) ───

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
    let run_id = params.get("run_id").and_then(|v| v.as_str());

    // Wake the Run Authority permission waiter (embedded or UDS), bound to run_id when provided.
    if let Err(e) = daemon_authority::respond_permission_for_run(request_id, approved, run_id).await {
        // A legacy host-only pending row has no daemon waiter. Keep that
        // migration path, but never hide errors for a run-bound response.
        if run_id.is_some() {
            return error_response("DAEMON_PERMISSION_FAILED", &e);
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    let changed = match conn.execute(
        "UPDATE assistant_permission_requests SET status = ?1, scope = ?2, responded_at = ?3 WHERE id = ?4 AND status = 'pending'",
        rusqlite::params![if approved { "approved" } else { "rejected" }, scope, now, request_id],
    ) {
        Ok(changed) => changed,
        Err(e) => return error_response("DB_UPDATE_ERROR", &e.to_string()),
    };
    // Daemon-only permission requests may not have a DB row yet — still OK if
    // the waiter was resolved above.
    success_response(serde_json::json!({
        "request_id": request_id,
        "approved": approved,
        "db_updated": changed > 0,
    }))
}

async fn handle_permission_list_pending(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = params.get("conversation_id").and_then(Value::as_str);
    let conn = data_store.conn();
    let sql = if conversation_id.is_some() {
        "SELECT p.id, p.run_id, p.tool_call_id, p.tool_name, p.reason, p.input, p.created_at
         FROM assistant_permission_requests p JOIN assistant_runs r ON r.id = p.run_id
         WHERE p.status = 'pending' AND r.conversation_id = ?1 ORDER BY p.created_at ASC"
    } else {
        "SELECT id, run_id, tool_call_id, tool_name, reason, input, created_at
         FROM assistant_permission_requests WHERE status = 'pending' ORDER BY created_at ASC"
    };
    let mut stmt = match conn.prepare(sql) {
        Ok(stmt) => stmt,
        Err(error) => return error_response("DB_ERROR", &error.to_string()),
    };
    let rows = match if let Some(cid) = conversation_id {
        stmt.query_map(rusqlite::params![cid], pending_permission_row)
    } else {
        stmt.query_map([], pending_permission_row)
    } {
        Ok(rows) => rows.filter_map(|row| row.ok()).collect::<Vec<_>>(),
        Err(error) => return error_response("DB_QUERY_ERROR", &error.to_string()),
    };
    success_response(serde_json::json!(rows))
}

fn pending_permission_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let raw_input: String = row.get(5)?;
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "request_id": row.get::<_, String>(0)?,
        "run_id": row.get::<_, String>(1)?,
        "tool_call_id": row.get::<_, String>(2)?,
        "tool_name": row.get::<_, String>(3)?,
        "reason": row.get::<_, String>(4)?,
        "input": serde_json::from_str::<Value>(&raw_input).unwrap_or(Value::Null),
        "created_at": row.get::<_, String>(6)?
    }))
}

// ─── Artifacts ───

