use crate::daemon::data::DataStore;
use crate::daemon_authority;
use crate::runtime::AgentRuntime;

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
            "daemon.getCapabilities" => handle_host_get_capabilities().await,
            "provider.list" => handle_provider_list(data_store, params).await,
            "run.start" => handle_run_start(data_store, params).await,
            "run.subscribe" => handle_run_subscribe(data_store, params).await,
            "permission.respond" => handle_permission_respond(data_store, params).await,
            "permission.listPending" => handle_permission_list_pending(data_store, params).await,
            "artifact.list" => handle_artifact_list(data_store, params).await,
            "artifact.open" | "artifact.reveal" => handle_artifact_open(data_store, params).await,
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
fn is_host_owned_method(method: &str) -> bool {
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
fn daemon_owned_method(method: &str) -> bool {
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


/// Honest per-runtime status for settings / RuntimePanel (REQ-T03).
async fn handle_host_get_capabilities() -> RpcResponse {
    use assistant_protocol::v2::{DaemonCapabilities, RuntimeAvailability, RuntimeCapability};

    let mut runtimes = vec![RuntimeCapability {
        id: "native".into(),
        display_name: "Native Daemon".into(),
        status: RuntimeAvailability::Executable,
        reason: None,
        methods: vec![],
    }];

    let meta = crate::runtime::registry::list_runtime_metadata().await;
    let mut seen = std::collections::HashSet::new();
    for m in &meta {
        seen.insert(m.id.clone());
        let (status, reason) = if m.id == "codex_cli" {
            (
                RuntimeAvailability::Unavailable,
                Some("app-server not implemented".into()),
            )
        } else if m.available {
            (RuntimeAvailability::Executable, None)
        } else {
            (
                RuntimeAvailability::Unavailable,
                Some(format!("{} binary not found", m.display_name)),
            )
        };
        runtimes.push(RuntimeCapability {
            id: m.id.clone(),
            display_name: m.display_name.clone(),
            status,
            reason,
            methods: vec![],
        });
    }
    if !seen.contains("claude_cli") {
        let claude = crate::runtime::claude_cli::ClaudeCliRuntime::new();
        runtimes.push(RuntimeCapability {
            id: "claude_cli".into(),
            display_name: "Claude CLI".into(),
            status: if claude.is_available() {
                RuntimeAvailability::Executable
            } else {
                RuntimeAvailability::Unavailable
            },
            reason: if claude.is_available() {
                None
            } else {
                Some("claude binary not found".into())
            },
            methods: vec![],
        });
    }
    if !seen.contains("codex_cli") {
        runtimes.push(RuntimeCapability {
            id: "codex_cli".into(),
            display_name: "Codex CLI".into(),
            status: RuntimeAvailability::Unavailable,
            reason: Some("app-server not implemented".into()),
            methods: vec![],
        });
    }

    let caps = DaemonCapabilities::host_mediated(runtimes);
    success_response(serde_json::to_value(caps).unwrap_or_default())
}


// ─── Provider catalog (host-owned) ───

async fn handle_provider_list(data_store: &Arc<DataStore>, _params: &Value) -> RpcResponse {
    // Prefer natives.db (Settings SoT). Fall back to assistant.db mirror only when
    // the main pool is unavailable (e.g. unit tests with :memory: DataStore).
    match list_providers_from_natives_db() {
        Ok(providers) => return success_response(serde_json::json!({ "providers": providers })),
        Err(err) => {
            // Soft fallback keeps fixture/unit tests working without a main pool.
            eprintln!("provider.list natives.db unavailable, falling back to mirror: {err}");
        }
    }

    // Fallback: assistant.db mirror (legacy / test paths). Still filter to
    // providers that currently have an active key so deleted settings rows
    // mirrored earlier do not reappear as selectable ghosts.
    let _ = data_store.migrate_legacy_provider_keys();
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, provider_type, display_name, api_base_url, health_status, default_model, created_at, updated_at
         FROM assistant_provider_configs ORDER BY display_name ASC",
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };

    let provider_rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )> = match stmt.query_map([], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
        ))
    }) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
    };

    let mut providers = Vec::with_capacity(provider_rows.len());
    for (
        id,
        provider_type,
        display_name,
        api_base_url,
        health_status,
        default_model,
        created_at,
        updated_at,
    ) in provider_rows
    {
        let has_active_key = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM assistant_provider_keys WHERE provider_id = ?1 AND is_active = 1)",
                rusqlite::params![id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);
        // Stale mirror rows without a live key must not appear in the picker.
        if !has_active_key {
            continue;
        }

        let mut model_stmt = match conn.prepare(
            "SELECT model_id, display_name, capabilities, context_window, max_output, source, discovered_at
             FROM assistant_model_cache
             WHERE provider_id = ?1 ORDER BY model_id ASC",
        ) {
            Ok(stmt) => stmt,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let mut models: Vec<Value> = match model_stmt.query_map(rusqlite::params![id], |row| {
            let capabilities = row.get::<_, String>(2)?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "display_name": row.get::<_, Option<String>>(1)?,
                "capabilities": serde_json::from_str::<Value>(&capabilities)
                    .unwrap_or_else(|_| serde_json::json!({})),
                "context_window": row.get::<_, i64>(3)?,
                "max_output": row.get::<_, i64>(4)?,
                "source": row.get::<_, String>(5)?,
                "discovered_at": row.get::<_, String>(6)?,
            }))
        }) {
            Ok(rows) => rows.filter_map(|row| row.ok()).collect(),
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };

        // Model cache empty → surface default_model only (never invent catalog entries).
        if models.is_empty() {
            if let Some(dm) = default_model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                models.push(serde_json::json!({
                    "id": dm,
                    "display_name": dm,
                    "capabilities": {},
                    "context_window": 0,
                    "max_output": 0,
                    "source": "default_model",
                    "discovered_at": created_at,
                }));
            }
        }

        providers.push(serde_json::json!({
            "id": id,
            "provider_type": provider_type,
            "display_name": display_name,
            "api_base_url": api_base_url,
            "health_status": health_status,
            "default_model": default_model,
            "has_active_key": true,
            "models": models,
            "created_at": created_at,
            "updated_at": updated_at,
        }));
    }

    success_response(serde_json::json!({ "providers": providers }))
}

/// Read providers exclusively from natives.db (`user_providers` + active keys).
/// Model list prefers assistant_model_cache when present; otherwise uses `default_model`.
fn list_providers_from_natives_db() -> std::result::Result<Vec<Value>, String> {
    let natives = crate::db::get_main_conn().map_err(|e| e.to_string())?;

    let mut pstmt = natives
        .prepare(
            "SELECT id, preset_name, api_protocol, name, website_url, base_url, default_model, created_at, updated_at
             FROM user_providers ORDER BY name ASC",
        )
        .map_err(|e| e.to_string())?;

    let provider_rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )> = pstmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Active key presence from natives.db only.
    let mut kstmt = natives
        .prepare(
            "SELECT provider_id FROM provider_api_keys WHERE COALESCE(is_active, 1) = 1",
        )
        .map_err(|e| e.to_string())?;
    let active_providers: std::collections::HashSet<String> = kstmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Optional model cache from assistant.db (discovery results); never required.
    let model_rows: std::collections::HashMap<String, Vec<Value>> = match crate::db::get_assistant_db_conn()
    {
        Ok(assistant) => {
            let mut stmt = match assistant.prepare(
                "SELECT provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at
                 FROM assistant_model_cache ORDER BY model_id ASC",
            ) {
                Ok(s) => s,
                Err(_) => {
                    return Ok(assemble_natives_providers(provider_rows, &active_providers, &std::collections::HashMap::new()));
                }
            };
            let mut grouped: std::collections::HashMap<String, Vec<Value>> =
                std::collections::HashMap::new();
            if let Ok(rows) = stmt.query_map([], |row| {
                let capabilities = row.get::<_, String>(3).unwrap_or_else(|_| "{}".into());
                Ok((
                    row.get::<_, String>(0)?,
                    serde_json::json!({
                        "id": row.get::<_, String>(1)?,
                        "display_name": row.get::<_, Option<String>>(2)?,
                        "capabilities": serde_json::from_str::<Value>(&capabilities)
                            .unwrap_or_else(|_| serde_json::json!({})),
                        "context_window": row.get::<_, i64>(4).unwrap_or(0),
                        "max_output": row.get::<_, i64>(5).unwrap_or(0),
                        "source": row.get::<_, String>(6).unwrap_or_else(|_| "cache".into()),
                        "discovered_at": row.get::<_, String>(7).unwrap_or_default(),
                    }),
                ))
            }) {
                for row in rows.flatten() {
                    grouped.entry(row.0).or_default().push(row.1);
                }
            }
            grouped
        }
        Err(_) => std::collections::HashMap::new(),
    };

    Ok(assemble_natives_providers(
        provider_rows,
        &active_providers,
        &model_rows,
    ))
}

fn assemble_natives_providers(
    provider_rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )>,
    active_providers: &std::collections::HashSet<String>,
    model_rows: &std::collections::HashMap<String, Vec<Value>>,
) -> Vec<Value> {
    let mut providers = Vec::new();
    for (
        id,
        preset_name,
        api_protocol,
        name,
        _website_url,
        base_url,
        default_model,
        created_at,
        updated_at,
    ) in provider_rows
    {
        if !active_providers.contains(&id) {
            continue;
        }
        let mut models = model_rows.get(&id).cloned().unwrap_or_default();
        if models.is_empty() {
            if let Some(dm) = default_model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                models.push(serde_json::json!({
                    "id": dm,
                    "display_name": dm,
                    "capabilities": {},
                    "context_window": 0,
                    "max_output": 0,
                    "source": "default_model",
                    "discovered_at": created_at,
                }));
            }
        }
        let provider_type = if !api_protocol.trim().is_empty() {
            api_protocol
        } else {
            preset_name
        };
        providers.push(serde_json::json!({
            "id": id,
            "provider_type": provider_type,
            "display_name": name,
            "api_base_url": base_url,
            "health_status": "unknown",
            "default_model": default_model,
            "has_active_key": true,
            "models": models,
            "created_at": created_at,
            "updated_at": updated_at,
        }));
    }
    providers
}

/// True when provider has an active key and model is either cached or the provider default.
fn provider_model_pair_available(
    provider_id: &str,
    model_id: &str,
    assistant_conn: &rusqlite::Connection,
) -> bool {
    // Prefer natives.db SoT.
    if let Ok(natives) = crate::db::get_main_conn() {
        let has_key: bool = natives
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM provider_api_keys
                    WHERE provider_id = ?1 AND COALESCE(is_active, 1) = 1
                )",
                rusqlite::params![provider_id],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !has_key {
            return false;
        }
        let default_model: Option<String> = natives
            .query_row(
                "SELECT default_model FROM user_providers WHERE id = ?1",
                rusqlite::params![provider_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        if default_model
            .as_deref()
            .map(str::trim)
            .is_some_and(|dm| dm == model_id)
        {
            return true;
        }
        // Model cache is optional discovery data in assistant.db.
        if let Ok(assistant) = crate::db::get_assistant_db_conn() {
            return assistant
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM assistant_model_cache
                        WHERE provider_id = ?1 AND model_id = ?2
                    )",
                    rusqlite::params![provider_id, model_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
        }
        return false;
    }

    // Unit-test / no-main-pool fallback: assistant mirror tables.
    assistant_conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM assistant_model_cache model
                JOIN assistant_provider_keys key ON key.provider_id = model.provider_id AND key.is_active = 1
                WHERE model.provider_id = ?1 AND model.model_id = ?2
            )",
            rusqlite::params![provider_id, model_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
        || assistant_conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM assistant_provider_configs p
                    JOIN assistant_provider_keys k ON k.provider_id = p.id AND k.is_active = 1
                    WHERE p.id = ?1 AND p.default_model = ?2
                )",
                rusqlite::params![provider_id, model_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
}

// ─── Run gateway (host preflight + daemon orchestration) ───

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
    let effort = params
        .get("effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let runtime_id = params
        .get("runtime_id")
        .or_else(|| params.get("runtimeId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let attachments = params
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if attachments.len() > 10 {
        return error_response("INVALID_INPUT", "At most 10 attachments are allowed");
    }
    let mut normalized_attachments = Vec::with_capacity(attachments.len());
    for attachment in &attachments {
        let path = match attachment.get("path").and_then(Value::as_str) {
            Some(path) if !path.trim().is_empty() => path.trim(),
            _ => return error_response("INVALID_INPUT", "attachment path is required"),
        };
        let metadata = match crate::file_manager::read_file(path) {
            Ok(metadata) if !metadata.truncated => metadata,
            Ok(_) => {
                return error_response(
                    "INVALID_INPUT",
                    "attachments must be UTF-8 files smaller than 2 MB",
                )
            }
            Err(error) => {
                return error_response(
                    "INVALID_INPUT",
                    &format!("Attachment cannot be read: {error}"),
                )
            }
        };
        let name = attachment
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path)
                    .to_string()
            });
        // Frontend historically sent camelCase `mimeType`; accept both wire shapes.
        let mime_type = attachment
            .get("mime_type")
            .or_else(|| attachment.get("mimeType"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("text/plain");
        let size = attachment
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or(metadata.size);
        normalized_attachments.push(serde_json::json!({
            "path": path,
            "name": name,
            "mime_type": mime_type,
            "size": size,
        }));
    }
    // Host preflight only: project_path + provider/model availability.
    // Runtime writes (messages / runs / events) are Daemon-only (assistant.db).
    let project_path = params
        .get("project_path")
        .or_else(|| params.get("workspace_path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("NATIVES_PROJECT_PATH").ok())
        .filter(|path| !path.trim().is_empty());
    if project_path.is_none()
        && std::env::var("NATIVES_REQUIRE_PROJECT_PATH")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true)
    {
        return error_response(
            "PROJECT_PATH_REQUIRED",
            "project_path must be provided by UI (daemon cwd is not a valid default)",
        );
    }

    {
        let conn = data_store.conn();
        if !provider_model_pair_available(provider_id, model_id, &conn) {
            return error_response("INVALID_PARAM", "Provider/model pair is not available");
        }
    }

    // Permission profile from Daemon conversation row (canonical), not host mirror.
    let permission_profile = match daemon_authority::request(
        "conversation.get",
        serde_json::json!({ "id": conversation_id }),
    )
    .await
    {
        Ok(conv) => conv
            .get("permission_profile_id")
            .and_then(Value::as_str)
            .filter(|p| matches!(*p, "readonly" | "ask" | "full_access"))
            .unwrap_or("ask")
            .to_string(),
        Err(_) => "ask".to_string(),
    };

    // Active-run gate from Daemon (no host assistant_runs).
    if let Ok(runs) = daemon_authority::list_runs(Some(conversation_id)).await {
        if runs.iter().any(|r| !r.status.is_terminal()) {
            return error_response(
                "RUN_ALREADY_ACTIVE",
                "This conversation already has an active run",
            );
        }
    }

    let user_content = content.unwrap_or("").to_string();
    let daemon_attachments: Vec<assistant_protocol::v2::AttachmentRef> = normalized_attachments
        .iter()
        .filter_map(|attachment| {
            let path = attachment.get("path")?.as_str()?.to_string();
            Some(assistant_protocol::v2::AttachmentRef {
                path,
                name: attachment
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                mime_type: attachment
                    .get("mime_type")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                size: attachment.get("size").and_then(Value::as_u64),
            })
        })
        .collect();
    let daemon_attachments_opt = if daemon_attachments.is_empty() {
        None
    } else {
        Some(daemon_attachments)
    };

    // Idempotency key for this start attempt (Daemon create/start, not host row id).
    let idempotency_key = uuid::Uuid::new_v4().to_string();
    let daemon_run = match daemon_authority::create_run(assistant_protocol::v2::CreateRunRequest {
        conversation_id: conversation_id.to_string(),
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        key_id: None,
        agent_profile_id: None,
        permission_profile: Some(permission_profile.clone()),
        content: Some(user_content.clone()),
        attachments: daemon_attachments_opt.clone(),
        max_steps: Some(50),
        parent_run_id: None,
        project_path: project_path.clone(),
        idempotency_key: Some(idempotency_key.clone()),
        effort: effort.clone(),
        runtime_id: runtime_id.clone(),
    })
    .await
    {
        Ok(r) => r,
        Err(e) => return error_response("DAEMON_CREATE_FAILED", &e),
    };
    let start_req = assistant_protocol::v2::StartRunRequest {
        run_id: Some(daemon_run.id.clone()),
        conversation_id: Some(conversation_id.to_string()),
        provider_id: Some(provider_id.to_string()),
        model_id: Some(model_id.to_string()),
        key_id: None,
        content: Some(user_content),
        attachments: daemon_attachments_opt,
        trigger_message_id: None,
        permission_profile: Some(permission_profile.clone()),
        max_steps: Some(50),
        project_path,
        idempotency_key: None,
        effort: effort.clone(),
        runtime_id: runtime_id.clone(),
    };
    let mode_label = daemon_authority::authority_mode_label();
    let started_daemon = match daemon_authority::start_run(start_req).await {
        Ok(run) => run,
        Err(error) => return error_response("DAEMON_START_FAILED", &error),
    };
    let status = if started_daemon.status.is_terminal() {
        started_daemon.status.as_str().to_string()
    } else {
        "running".to_string()
    };
    let started_at = started_daemon
        .started_at
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    // No host projection loop: UI reads runs/events/messages from Daemon.
    success_response(serde_json::json!({
        "id": started_daemon.id,
        "conversation_id": conversation_id,
        "status": status,
        "provider_id": provider_id,
        "model_id": model_id,
        "permission_profile": started_daemon.permission_profile,
        "started_at": started_at,
        "execution": "agent_daemon_run_manager",
        "authority_mode": mode_label,
        "daemon_run_id": started_daemon.id,
    }))
}

async fn handle_run_subscribe(_data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };
    let after_sequence = params
        .get("after_sequence")
        .or_else(|| params.get("last_sequence"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let wait_ms = params
        .get("wait_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .clamp(0, 30_000);
    let want_push = params
        .get("mode")
        .and_then(Value::as_str)
        .map(|m| m == "push" || m == "long_poll")
        .unwrap_or(false)
        || wait_ms > 0;

    // Forward long-poll to daemon authority; then project host state before terminal:true.
    let mut params_forward = params.clone();
    if let Some(obj) = params_forward.as_object_mut() {
        obj.insert("run_id".into(), Value::String(run_id.to_string()));
        obj.insert("after_sequence".into(), serde_json::json!(after_sequence));
        if want_push && wait_ms > 0 {
            obj.insert("wait_ms".into(), serde_json::json!(wait_ms));
            obj.insert("mode".into(), Value::String("push".into()));
        }
    }

    // Forward only — no host projection of events/messages/runs.
    let data = match daemon_authority::request("run.subscribe", params_forward).await {
        Ok(data) => data,
        Err(error) => {
            match daemon_authority::replay_events(run_id, after_sequence).await {
                Ok(events) => {
                    let daemon_terminal = daemon_authority::get_run(run_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|r| r.status.is_terminal())
                        .unwrap_or(false);
                    let event_values: Vec<Value> = events
                        .into_iter()
                        .map(|e| {
                            serde_json::json!({
                                "run_id": e.run_id,
                                "sequence": e.sequence,
                                "timestamp": e.timestamp.to_rfc3339(),
                                "type": e.payload.type_name(),
                                "payload": e.payload,
                            })
                        })
                        .collect();
                    return success_response(serde_json::json!({
                        "run_id": run_id,
                        "events": event_values,
                        "terminal": daemon_terminal,
                        "mode": "subscribe_fallback_replay",
                        "error": error,
                    }));
                }
                Err(e2) => return error_response("DAEMON_RPC_ERROR", &format!("{error}; {e2}")),
            }
        }
    };

    let daemon_terminal = data
        .get("terminal")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || daemon_authority::get_run(run_id)
            .await
            .ok()
            .flatten()
            .map(|r| r.status.is_terminal())
            .unwrap_or(false);

    let out_events = data
        .get("events")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));

    success_response(serde_json::json!({
        "run_id": run_id,
        "events": out_events,
        "terminal": daemon_terminal,
        "mode": data.get("mode").cloned().unwrap_or(Value::String("subscribe_host".into())),
    }))
}


// ─── Permission (host still dual-writes until task-04 integration) ───

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

async fn handle_artifact_list(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    if daemon_authority::authority_mode_label() == "uds" {
        return match daemon_authority::request("artifact.list", params.clone()).await {
            Ok(data) => success_response(data),
            Err(error) => error_response("DAEMON_ARTIFACT_FAILED", &error),
        };
    }

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
    use std::sync::{Mutex, OnceLock};

    /// Serialise tests that mutate NATIVES_DAEMON_MODE / NATIVES_ASSISTANT_DB_PATH.
    fn daemon_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn host_owned_router_table() {
        assert!(is_host_owned_method("provider.list"));
        assert!(is_host_owned_method("daemon.getCapabilities"));
        assert!(is_host_owned_method("run.start"));
        assert!(is_host_owned_method("run.subscribe"));
        assert!(is_host_owned_method("permission.respond"));
        assert!(is_host_owned_method("permission.listPending"));
        assert!(is_host_owned_method("artifact.open"));
        assert!(is_host_owned_method("artifact.reveal"));
        assert!(is_host_owned_method("artifact.list"));
        assert!(!is_host_owned_method("run.cancel"));
        assert!(!is_host_owned_method("conversation.list"));
        assert!(!is_host_owned_method("promptQueue.list"));
        assert!(!is_host_owned_method("mcp.list"));
        assert!(daemon_owned_method("conversation.list"));
        assert!(daemon_owned_method("run.cancel"));
        assert!(!daemon_owned_method("run.start"));
        assert!(!daemon_owned_method("provider.list"));
    }

    #[test]
    fn provider_list_is_host_owned_not_daemon_owned() {
        let previous = std::env::var("NATIVES_DAEMON_MODE").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "uds");
        assert!(
            !daemon_owned_method("provider.list"),
            "provider.list must stay host-owned so user configs are visible"
        );
        assert!(daemon_owned_method("provider.test"));
        if let Some(value) = previous {
            std::env::set_var("NATIVES_DAEMON_MODE", value);
        } else {
            std::env::remove_var("NATIVES_DAEMON_MODE");
        }
    }

    #[tokio::test]
    async fn provider_list_returns_mirrored_user_provider_models() {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        store
            .conn()
            .execute(
                "INSERT INTO assistant_provider_configs
                 (id, provider_type, display_name, api_base_url, default_model, health_status, created_at, updated_at)
                 VALUES ('p1', 'openai_compatible', 'SenseNova', 'https://api.example/v1', 'deepseek-v4-flash', 'unknown', 'now', 'now')",
                [],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO assistant_provider_keys
                 (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
                 VALUES ('k1', 'p1', 'enc', 'sk-…abcd', 'API Key', 1, 'now')",
                [],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO assistant_model_cache
                 (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
                 VALUES ('p1:deepseek-v4-flash', 'p1', 'deepseek-v4-flash', 'DeepSeek V4 Flash', '{}', 0, 0, 'manual', 'now')",
                [],
            )
            .unwrap();

        let response = dispatch_rpc(&store, "provider.list", &serde_json::json!({})).await;
        assert!(
            response.success,
            "provider.list failed: {:?}",
            response.error
        );
        let data = response.data.expect("provider.list data");
        let providers = data
            .get("providers")
            .and_then(|v| v.as_array())
            .expect("providers array");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["id"], "p1");
        assert_eq!(providers[0]["has_active_key"], true);
        assert_eq!(providers[0]["models"][0]["id"], "deepseek-v4-flash");
    }

    #[test]
    fn run_start_is_always_host_owned() {
        let previous = std::env::var("NATIVES_DAEMON_MODE").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
        assert!(!daemon_owned_method("run.start"));
        std::env::set_var("NATIVES_DAEMON_MODE", "uds");
        assert!(!daemon_owned_method("run.start"));
        if let Some(value) = previous {
            std::env::set_var("NATIVES_DAEMON_MODE", value);
        } else {
            std::env::remove_var("NATIVES_DAEMON_MODE");
        }
        assert!(daemon_owned_method("run.list"));
        assert!(daemon_owned_method("run.cancel"));
        assert!(!daemon_owned_method("run.subscribe"));
        assert!(daemon_owned_method("provider.test"));
        assert!(daemon_owned_method("mcp.list"));
        assert!(daemon_owned_method("conversation.list"));
        assert!(daemon_owned_method("promptQueue.list"));
        assert!(!daemon_owned_method("artifact.open"));
    }

    #[tokio::test]
    async fn implemented_daemon_method_is_not_rejected_by_legacy_dispatch() {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        let response = dispatch_rpc(&store, "mcp.list", &serde_json::json!({})).await;
        assert_ne!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("METHOD_NOT_FOUND")
        );
    }

    #[tokio::test]
    async fn conversation_permission_and_attachments_round_trip() {
        // Serialise env mutations so parallel natives tests cannot clobber the
        // embedded daemon DB path mid-flight.
        let _env_guard = daemon_env_lock();
        let previous_daemon_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        let previous_db = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let tmp_db = std::env::temp_dir().join(format!(
            "natives-asst-test-{}.db",
            uuid::Uuid::new_v4()
        ));
        std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
        std::env::set_var(
            "NATIVES_ASSISTANT_DB_PATH",
            tmp_db.to_string_lossy().as_ref(),
        );
        crate::daemon_authority::reset_authority_cache().await;
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        let attachment_path = std::path::PathBuf::from(format!(
            "/tmp/natives-assistant-test-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&attachment_path, "example attachment").unwrap();
        let created = dispatch_rpc(
            &store,
            "conversation.create",
            &serde_json::json!({
                "mode": "agent",
                "title": "Attachment test",
                "provider_id": "provider",
                "model_id": "model",
                "permission_profile_id": "readonly"
            }),
        )
        .await;
        assert!(created.success, "create failed: {:?}", created.error);
        let created_data = created.data.as_ref().expect("create data");
        let conversation_id = created_data["id"].as_str().unwrap().to_string();
        assert_eq!(created_data["permission_profile_id"], "readonly");

        let got = dispatch_rpc(
            &store,
            "conversation.get",
            &serde_json::json!({ "id": conversation_id }),
        )
        .await;
        assert!(got.success, "conversation.get failed: {:?}", got.error);
        assert_eq!(got.data.as_ref().unwrap()["permission_profile_id"], "readonly");

        store.conn().execute("INSERT INTO assistant_provider_configs (id, provider_type, display_name, api_base_url, created_at, updated_at) VALUES ('provider', 'openai', 'Provider', 'https://example.com', datetime('now'), datetime('now'))", []).unwrap();
        store.conn().execute("INSERT INTO assistant_provider_keys (id, provider_id, encrypted_key, masked_key, created_at) VALUES ('key', 'provider', 'encrypted', '***', datetime('now'))", []).unwrap();
        store.conn().execute("INSERT INTO assistant_model_cache (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at) VALUES ('model-cache', 'provider', 'model', 'model', '{}', 0, 0, 'manual', datetime('now'))", []).unwrap();

        let started = dispatch_rpc(
            &store,
            "run.start",
            &serde_json::json!({
                "conversation_id": conversation_id,
                "provider_id": "provider",
                "model_id": "model",
                "project_path": "/tmp",
                "content": "Inspect this file",
                "attachments": [{
                    "path": attachment_path.to_string_lossy().to_string(),
                    "name": "example.txt",
                    "mime_type": "text/plain",
                    "size": 12
                }]
            }),
        )
        .await;
        assert!(started.success, "run.start failed: {:?}", started.error);
        let run = started.data.as_ref().unwrap();
        assert_eq!(
            run["permission_profile"], "readonly",
            "run payload: {run}"
        );
        let run_id = run["id"].as_str().unwrap();

        let conversations = dispatch_rpc(&store, "conversation.list", &Value::Null)
            .await
            .data
            .unwrap();
        let listed = conversations
            .as_array()
            .cloned()
            .unwrap_or_else(|| {
                conversations
                    .get("conversations")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
            });
        let found = listed.iter().find(|c| c["id"] == conversation_id);
        assert!(found.is_some(), "conversation list missing id: {listed:?}");
        assert_eq!(found.unwrap()["permission_profile_id"], "readonly");

        let runs = dispatch_rpc(
            &store,
            "run.list",
            &serde_json::json!({ "conversation_id": conversation_id }),
        )
        .await
        .data
        .unwrap();
        let run_list = runs
            .get("runs")
            .and_then(|v| v.as_array())
            .or_else(|| runs.as_array())
            .expect("runs array");
        assert!(!run_list.is_empty(), "expected at least one run: {runs}");
        assert_eq!(run_list[0]["permission_profile"], "readonly");
        assert_eq!(run_list[0]["id"], run_id);

        let messages = dispatch_rpc(
            &store,
            "conversation.getMessages",
            &serde_json::json!({ "conversation_id": conversation_id }),
        )
        .await
        .data
        .unwrap();
        let msg_list = messages
            .as_array()
            .cloned()
            .unwrap_or_else(|| {
                messages
                    .get("messages")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
            });
        assert!(!msg_list.is_empty(), "expected user message: {messages}");

        let _ = std::fs::remove_file(&attachment_path);
        let _ = std::fs::remove_file(&tmp_db);
        crate::daemon_authority::reset_authority_cache().await;
        if let Some(mode) = previous_daemon_mode {
            std::env::set_var("NATIVES_DAEMON_MODE", mode);
        } else {
            std::env::remove_var("NATIVES_DAEMON_MODE");
        }
        if let Some(db) = previous_db {
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", db);
        } else {
            std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        }
    }

    #[tokio::test]
    async fn structured_assistant_blocks_round_trip() {
        let _env_guard = daemon_env_lock();
        let previous_daemon_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        let previous_db = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let tmp_db = std::env::temp_dir().join(format!(
            "natives-blocks-test-{}.db",
            uuid::Uuid::new_v4()
        ));
        std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
        std::env::set_var(
            "NATIVES_ASSISTANT_DB_PATH",
            tmp_db.to_string_lossy().as_ref(),
        );
        crate::daemon_authority::reset_authority_cache().await;
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        let created = dispatch_rpc(
            &store,
            "conversation.create",
            &serde_json::json!({
                "mode": "agent", "title": "Blocks", "provider_id": "p", "model_id": "m"
            }),
        )
        .await;
        assert!(created.success, "create failed: {:?}", created.error);
        let conversation_id = created.data.unwrap()["id"].as_str().unwrap().to_string();
        let appended = dispatch_rpc(&store, "conversation.appendMessage", &serde_json::json!({
            "conversation_id": conversation_id,
            "role": "assistant",
            "blocks": [{ "type": "reasoning", "reasoning": "checked" }, { "type": "text", "text": "done" }]
        })).await;
        assert!(appended.success, "append failed: {:?}", appended.error);
        let messages = dispatch_rpc(
            &store,
            "conversation.getMessages",
            &serde_json::json!({ "conversation_id": conversation_id }),
        )
        .await
        .data
        .unwrap();
        let msg_list = messages
            .as_array()
            .cloned()
            .unwrap_or_else(|| {
                messages
                    .get("messages")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
            });
        assert!(!msg_list.is_empty(), "messages: {messages}");
        assert_eq!(
            msg_list[0]["content_blocks"][0]["content"]["reasoning"],
            "checked"
        );
        assert_eq!(msg_list[0]["content_blocks"][1]["content"]["text"], "done");
        let _ = std::fs::remove_file(&tmp_db);
        crate::daemon_authority::reset_authority_cache().await;
        if let Some(mode) = previous_daemon_mode {
            std::env::set_var("NATIVES_DAEMON_MODE", mode);
        } else {
            std::env::remove_var("NATIVES_DAEMON_MODE");
        }
        if let Some(db) = previous_db {
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", db);
        } else {
            std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        }
    }
}
