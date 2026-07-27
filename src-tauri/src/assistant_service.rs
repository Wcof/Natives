//! Assistant Host RPC facade: wire types and host-owned router (capabilities/provider/run/artifacts).
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
            "artifact.list" => artifacts::handle_artifact_list(data_store, params).await,
            "artifact.open" | "artifact.reveal" => {
                artifacts::handle_artifact_open(data_store, params).await
            }
            _ => error_response(
                "METHOD_NOT_FOUND",
                &format!("Unknown host-owned method: {method}"),
            ),
        };
    }

    if assistant_protocol::v2::is_implemented_method(method)
        || method.starts_with("conversation.")
        || method.starts_with("promptQueue.")
        || method.starts_with("interaction.")
        || method.starts_with("permission.")
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
            || method.starts_with("permission.")
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
