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

/// Dispatch RPC method to the appropriate handler
async fn dispatch_rpc(data_store: &Arc<DataStore>, method: &str, params: &Value) -> RpcResponse {
    if method == "daemon.getCapabilities" {
        return handle_host_get_capabilities().await;
    }

    if daemon_owned_method(method) {
        return match daemon_authority::request(method, params.clone()).await {
            Ok(data) => success_response(data),
            Err(error) => error_response("DAEMON_RPC_ERROR", &error),
        };
    }

    match method {
        "provider.list" => handle_provider_list(data_store, params).await,
        "conversation.list" => handle_conversation_list(data_store, params).await,
        "conversation.create" => handle_conversation_create(data_store, params).await,
        "conversation.get" => handle_conversation_get(data_store, params).await,
        "conversation.fork" => handle_conversation_fork(data_store, params).await,
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
        "run.listChildren" => handle_run_list_children(data_store, params).await,
        "run.getEvents" => handle_run_get_events(data_store, params).await,
        "permission.respond" => handle_permission_respond(data_store, params).await,
        "permission.listPending" => handle_permission_list_pending(data_store, params).await,
        "interaction.listPending" => handle_permission_list_pending(data_store, params).await,
        "interaction.respond" => handle_permission_respond(data_store, params).await,
        "promptQueue.list" => handle_prompt_queue_list(data_store, params).await,
        "promptQueue.enqueue" => handle_prompt_queue_enqueue(data_store, params).await,
        "promptQueue.update" => handle_prompt_queue_update(data_store, params).await,
        "promptQueue.remove" => handle_prompt_queue_remove(data_store, params).await,
        "promptQueue.reorder" => handle_prompt_queue_reorder(data_store, params).await,
        "promptQueue.sendNow" => handle_prompt_queue_send_now(data_store, params).await,
        "artifact.list" => handle_artifact_list(data_store, params).await,
        "artifact.open" => handle_artifact_open(data_store, params).await,
        "artifact.reveal" => handle_artifact_open(data_store, params).await,
        _ if assistant_protocol::v2::is_implemented_method(method) => {
            match daemon_authority::request(method, params.clone()).await {
                Ok(data) => success_response(data),
                Err(error) => error_response("DAEMON_RPC_ERROR", &error),
            }
        }
        _ if assistant_protocol::v2::is_known_method(method) => {
            error_response("UNSUPPORTED", &format!("RPC method is not implemented: {method}"))
        }
        _ => error_response("METHOD_NOT_FOUND", &format!("Unknown RPC method: {method}")),
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

fn daemon_owned_method(method: &str) -> bool {
    // provider.list is host-owned: it must read user-configured providers from
    // assistant.db (mirrored from Settings). Agent Daemon's provider.list only
    // returns built-in adapter types, which is useless for the model picker.
    if method == "provider.list" {
        return false;
    }
    if method == "daemon.getCapabilities" {
        return false; // host-mediated honest runtimes
    }
    // Conversation CRUD lives in host assistant.db (assistant_conversations).
    // Routing conversation.* to the UDS daemon in the past dual-stored sessions:
    // UI listed host rows while delete hit daemon tables → "delete has no effect".
    // Runs / engine still go through daemon authority.
    if method.starts_with("conversation.") {
        return false;
    }
    // run.start MUST stay host-owned even in UDS mode:
    // host validates provider/model + project_path, writes user message to assistant.db,
    // then orchestrates daemon create_run + start_run. Raw UDS run.start skips that and
    // fails with run_start_failed (daemon conversation FK / missing host preflight).
    if method == "run.start" {
        return false;
    }
    method.starts_with("run.")
        || method.starts_with("daemon.")
        || method.starts_with("provider.")
        || method.starts_with("tool.")
        || method.starts_with("mcp.")
        || method.starts_with("scheduler.")
        || method.starts_with("extension.")
        || method.starts_with("skill.")
        || method.starts_with("memory.")
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

async fn handle_conversation_get(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").or_else(|| params.get("conversation_id")).and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "id is required"),
    };
    let conn = data_store.conn();
    match conn.query_row(
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at, archived_at
         FROM assistant_conversations WHERE id = ?1",
        rusqlite::params![id],
        |row| {
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
                "archived_at": row.get::<_, Option<String>>(9)?
            }))
        },
    ) {
        Ok(conversation) => success_response(conversation),
        Err(rusqlite::Error::QueryReturnedNoRows) => error_response("NOT_FOUND", "conversation not found"),
        Err(error) => error_response("DB_QUERY_ERROR", &error.to_string()),
    }
}

async fn handle_conversation_fork(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let source_id = match params.get("conversation_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };
    let source = {
        let conn = data_store.conn();
        match conn.query_row(
            "SELECT mode, project_id, title, provider_id, model_id, permission_profile_id FROM assistant_conversations WHERE id = ?1",
            rusqlite::params![source_id],
            |row| Ok(serde_json::json!({
                "mode": row.get::<_, String>(0)?,
                "project_id": row.get::<_, Option<String>>(1)?,
                "title": row.get::<_, String>(2)?,
                "provider_id": row.get::<_, String>(3)?,
                "model_id": row.get::<_, String>(4)?,
                "permission_profile_id": row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "ask".into()),
            })),
        ) {
            Ok(source) => source,
            Err(rusqlite::Error::QueryReturnedNoRows) => return error_response("NOT_FOUND", "conversation not found"),
            Err(error) => return error_response("DB_QUERY_ERROR", &error.to_string()),
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let title = format!("Fork of {}", source.get("title").and_then(Value::as_str).unwrap_or("Conversation"));
    let conn = data_store.conn();
    if let Err(error) = conn.execute(
        "INSERT INTO assistant_conversations (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        rusqlite::params![
            id,
            source.get("mode").and_then(Value::as_str).unwrap_or("agent"),
            source.get("project_id").and_then(Value::as_str),
            title.clone(),
            source.get("provider_id").and_then(Value::as_str).unwrap_or_default(),
            source.get("model_id").and_then(Value::as_str).unwrap_or_default(),
            source.get("permission_profile_id").and_then(Value::as_str).unwrap_or("ask"),
            now.clone(),
        ],
    ) {
        return error_response("DB_INSERT_ERROR", &error.to_string());
    }
    success_response(serde_json::json!({
        "id": id,
        "mode": source.get("mode"),
        "project_id": source.get("project_id"),
        "title": title,
        "provider_id": source.get("provider_id"),
        "model_id": source.get("model_id"),
        "permission_profile_id": source.get("permission_profile_id"),
        "created_at": now,
        "updated_at": now,
        "archived_at": null
    }))
}

async fn handle_conversation_create(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let project_id = params.get("project_id").and_then(|v| v.as_str());
    let mode = match params.get("mode").and_then(|v| v.as_str()).unwrap_or("agent") {
        m @ ("chat" | "agent" | "goal") => m,
        _ => return error_response("INVALID_PARAM", "mode must be chat, agent, or goal"),
    };
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
    let id = match params
        .get("id")
        .or_else(|| params.get("conversation_id"))
        .and_then(|v| v.as_str())
    {
        Some(i) => i,
        None => return error_response("MISSING_PARAM", "id is required"),
    };

    let runs = match daemon_authority::list_runs(Some(id)).await {
        Ok(runs) => runs,
        Err(error) => return error_response("DAEMON_DELETE_FAILED", &error),
    };
    for run in runs.iter().filter(|run| !run.status.is_terminal()) {
        if let Err(error) = daemon_authority::cancel_run(&run.id).await {
            return error_response("DAEMON_DELETE_FAILED", &error);
        }
    }

    let conn = data_store.conn();
    match conn.execute(
        "DELETE FROM assistant_conversations WHERE id = ?1",
        rusqlite::params![id],
    ) {
        Ok(0) => return error_response("NOT_FOUND", "conversation not found"),
        Ok(_) => {}
        Err(e) => return error_response("DB_DELETE_ERROR", &e.to_string()),
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
    // Validate the execution boundary before reserving a local run row. A
    // failed preflight must never leave an active-looking ghost run behind.
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

    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // All DB work in a block so MutexGuard is dropped before any await (Send).
    let (trigger_message_id, permission_profile) = {
        let conn = data_store.conn();
        // Provider/model must be currently available in natives.db (or mirror fallback).
        // Accept: active key + (model in cache OR model == default_model).
        let pair_available = provider_model_pair_available(provider_id, model_id, &conn);
        if !pair_available {
            return error_response("INVALID_PARAM", "Provider/model pair is not available");
        }
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
                    rusqlite::params![
                        uuid::Uuid::new_v4().to_string(),
                        message_id,
                        block_index,
                        content
                    ],
                ) {
                    return error_response("DB_INSERT_ERROR", &e.to_string());
                }
                block_index += 1;
            }
            for payload in &normalized_attachments {
                if let Err(e) = transaction.execute(
                    "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'file_reference', ?3, ?4)",
                    rusqlite::params![
                        uuid::Uuid::new_v4().to_string(),
                        message_id,
                        block_index,
                        payload.to_string()
                    ],
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
        let permission_profile = transaction
            .query_row(
                "SELECT COALESCE(permission_profile_id, 'ask') FROM assistant_conversations WHERE id = ?1",
                rusqlite::params![conversation_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "ask".to_string());
        let active_primary: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM assistant_runs
             WHERE conversation_id = ?1 AND parent_run_id IS NULL
               AND status IN ('queued','preparing','running','waiting_permission','cancelling'))",
                rusqlite::params![conversation_id],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if active_primary {
            return error_response(
                "RUN_ALREADY_ACTIVE",
                "This conversation already has an active run",
            );
        }
        if let Err(e) = transaction
            .execute(
                "INSERT INTO assistant_runs (id, conversation_id, status, trigger_message_id, provider_id, model_id, permission_profile, started_at)
         VALUES (?1, ?2, 'queued', ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    run_id,
                    conversation_id,
                    trigger_message_id,
                    provider_id,
                    model_id,
                    permission_profile,
                    now
                ],
            )
            .and_then(|_| {
                transaction.execute(
                    "UPDATE assistant_conversations SET updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, conversation_id],
                )
            })
            .and_then(|_| transaction.commit())
        {
            return error_response("DB_INSERT_ERROR", &e.to_string());
        }
        (trigger_message_id, permission_profile)
    };

    // Protocol v2: Run Authority via embedded RunManager or UDS sidecar (G4).
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
        idempotency_key: Some(run_id.clone()),
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
        trigger_message_id: trigger_message_id.clone(),
        permission_profile: Some(permission_profile.clone()),
        max_steps: Some(50),
        project_path,
        idempotency_key: None,
        effort: effort.clone(),
        runtime_id: runtime_id.clone(),
    };
    let db_run_id = run_id.clone();
    let daemon_run_id = daemon_run.id.clone();
    let daemon_run_id_resp = daemon_run.id.clone();
    let mode_label = daemon_authority::authority_mode_label();
    let started_daemon = match daemon_authority::start_run(start_req).await {
        Ok(run) => run,
        Err(error) => {
            let conn = data_store.conn();
            let _ = conn.execute(
                "UPDATE assistant_runs SET status = 'failed', error_code = ?1, finished_at = ?2 WHERE id = ?3",
                rusqlite::params![error.to_string(), chrono::Utc::now().to_rfc3339(), run_id],
            );
            return error_response("DAEMON_START_FAILED", &error);
        }
    };
    let daemon_status = started_daemon.status.as_str().to_string();
    let db_status = if started_daemon.status.is_terminal() {
        daemon_status.clone()
    } else {
        "running".to_string()
    };
    let conn = data_store.conn();
    let _ = conn.execute(
        "UPDATE assistant_runs SET status = ?1 WHERE id = ?2",
        rusqlite::params![db_status, run_id],
    );
    tokio::spawn(async move {
        // Brief settle so fixture/fast engines finish before first mirror.
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            if let Ok(Some(r)) = daemon_authority::get_run(&daemon_run_id).await {
                if r.status.is_terminal() {
                    break;
                }
            }
        }
        if let Ok(events) = daemon_authority::replay_events(&daemon_run_id, 0).await {
            if let Ok(conn) = crate::db::get_assistant_db_conn() {
                for event in events {
                    let payload =
                        serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".into());
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            db_run_id,
                            event.sequence as i64,
                            event.timestamp.to_rfc3339(),
                            event.payload.type_name(),
                            payload
                        ],
                    );
                }
                if let Ok(Some(r)) = daemon_authority::get_run(&daemon_run_id).await {
                    let _ = conn.execute(
                        "UPDATE assistant_runs SET status = ?1, finished_at = CASE WHEN ?1 IN ('completed','failed','cancelled','interrupted') THEN ?2 ELSE finished_at END WHERE id = ?3",
                        rusqlite::params![r.status.as_str(), chrono::Utc::now().to_rfc3339(), db_run_id],
                    );
                }
            }
        }
    });

    // Prefer daemon run id for wire `id` so UI subscribe addresses EventSequencer key.
    let response_id = if daemon_run_id_resp.is_empty() {
        run_id.clone()
    } else {
        daemon_run_id_resp.clone()
    };
    success_response(serde_json::json!({
        "id": response_id,
        "conversation_id": conversation_id,
        "status": db_status,
        "provider_id": provider_id,
        "model_id": model_id,
        "permission_profile": permission_profile,
        "started_at": now,
        "execution": "agent_daemon_run_manager",
        "authority_mode": mode_label,
        "daemon_run_id": daemon_run_id_resp,
        "host_run_id": run_id,
    }))
}

async fn handle_run_cancel(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };

    let now = chrono::Utc::now().to_rfc3339();
    // The local projection changes only after the Daemon confirms cancellation.
    let daemon_run = match daemon_authority::cancel_run(run_id).await {
        Ok(run) => run,
        Err(error) => return error_response("DAEMON_CANCEL_FAILED", &error),
    };
    let status = daemon_run.status.as_str();
    // Older assistant.db files have no cancelled CHECK value yet; keep their
    // rebuildable projection compatible while the daemon remains authoritative.
    let local_status = if status == "cancelled" { "interrupted" } else { status };

    let conn = data_store.conn();
    let changed = match conn.execute(
        "UPDATE assistant_runs SET status = ?1, finished_at = ?2 WHERE id = ?3 AND status IN ('queued', 'preparing', 'running', 'waiting_permission', 'cancelling')",
        rusqlite::params![local_status, now, run_id],
    ) {
        Ok(changed) => changed,
        Err(e) => return error_response("DB_UPDATE_ERROR", &e.to_string()),
    };

    if changed > 0 {
        let sequence: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM assistant_run_events WHERE run_id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )
            .unwrap_or(1);
        let _ = conn.execute(
            "INSERT INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                run_id,
                sequence,
                now,
                status,
                serde_json::json!({ "reason": "cancelled" }).to_string()
            ],
        );
    }

    success_response(serde_json::json!({ "id": run_id, "status": status, "cancelled": changed > 0 }))
}

async fn handle_run_finish(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };
    let status = match params.get("status").and_then(Value::as_str) {
        Some("completed" | "failed" | "cancelled" | "interrupted") => {
            params.get("status").and_then(Value::as_str).unwrap()
        }
        _ => {
            return error_response(
                "INVALID_PARAM",
                "status must be completed, failed, cancelled, or interrupted",
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

async fn handle_run_retry(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let run_id = match params.get("run_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "run_id is required"),
    };

    // Prefer Daemon memory retry+start; if missing, rehydrate from DB then create+start.
    let new_run = match daemon_authority::retry_and_start(run_id).await {
        Ok(r) => r,
        Err(_) => {
            let rehydrated = {
                let conn = data_store.conn();
                let row = conn.query_row(
                    "SELECT conversation_id, provider_id, model_id, permission_profile
                 FROM assistant_runs WHERE id = ?1",
                    rusqlite::params![run_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    },
                );
                let (conversation_id, provider_id, model_id, perm) = match row {
                    Ok(r) => r,
                    Err(_) => return error_response("NOT_FOUND", "run not found"),
                };
                let content: Option<String> = conn
                    .query_row(
                        "SELECT b.content FROM assistant_messages m
                     JOIN assistant_message_blocks b ON b.message_id = m.id
                     WHERE m.conversation_id = ?1 AND m.role = 'user' AND b.block_type = 'text'
                     ORDER BY m.created_at DESC LIMIT 1",
                        rusqlite::params![conversation_id],
                        |row| row.get::<_, String>(0),
                    )
                    .ok()
                    .and_then(|raw| {
                        serde_json::from_str::<Value>(&raw)
                            .ok()
                            .and_then(|v| {
                                v.get("text").and_then(|t| t.as_str()).map(str::to_string)
                            })
                            .or(Some(raw))
                    });
                (conversation_id, provider_id, model_id, perm, content)
            };
            let (conversation_id, provider_id, model_id, perm, content) = rehydrated;
            let created = match daemon_authority::create_run(
                assistant_protocol::v2::CreateRunRequest {
                    conversation_id,
                    provider_id,
                    model_id,
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: perm,
                    content: content.clone(),
                    attachments: None,
                    max_steps: Some(50),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: None,
                        effort: None,
        runtime_id: None,
},
            )
            .await
            {
                Ok(r) => r,
                Err(e) => return error_response("DAEMON_RETRY_FAILED", &e),
            };
            match daemon_authority::start_run(assistant_protocol::v2::StartRunRequest {
                run_id: Some(created.id.clone()),
                conversation_id: Some(created.conversation_id.clone()),
                provider_id: Some(created.provider_id.clone()),
                model_id: Some(created.model_id.clone()),
                key_id: created.key_id.clone(),
                content,
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some(created.permission_profile.clone()),
                max_steps: Some(created.max_steps),
                project_path: None,
                idempotency_key: None,
                    effort: None,
        runtime_id: None,
})
            .await
            {
                Ok(r) => r,
                Err(e) => return error_response("DAEMON_RETRY_START_FAILED", &e),
            }
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    {
        let conn = data_store.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO assistant_runs (id, conversation_id, status, provider_id, model_id, permission_profile, started_at)
             VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6)",
            rusqlite::params![
                new_run.id,
                new_run.conversation_id,
                new_run.provider_id,
                new_run.model_id,
                new_run.permission_profile,
                now
            ],
        ) {
            return error_response("DB_INSERT_ERROR", &e.to_string());
        }
    }

    let new_id = new_run.id.clone();
    let db_id = new_run.id.clone();
    tokio::spawn(async move {
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            if let Ok(Some(r)) = daemon_authority::get_run(&new_id).await {
                if r.status.is_terminal() {
                    break;
                }
            }
        }
        if let Ok(events) = daemon_authority::replay_events(&new_id, 0).await {
            for event in events {
                if let Ok(conn) = crate::db::get_assistant_db_conn() {
                    let payload =
                        serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".into());
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            db_id,
                            event.sequence as i64,
                            event.timestamp.to_rfc3339(),
                            event.payload.type_name(),
                            payload
                        ],
                    );
                }
            }
        }
        if let Ok(conn) = crate::db::get_assistant_db_conn() {
            if let Ok(Some(run)) = daemon_authority::get_run(&new_id).await {
                let status = run.status.as_str().to_string();
                let _ = conn.execute(
                    "UPDATE assistant_runs SET status = ?1, finished_at = ?2 WHERE id = ?3",
                    rusqlite::params![status, chrono::Utc::now().to_rfc3339(), db_id],
                );
            }
        }
    });

    success_response(serde_json::json!({
        "id": new_run.id,
        "conversation_id": new_run.conversation_id,
        "status": "running",
        "provider_id": new_run.provider_id,
        "model_id": new_run.model_id,
        "permission_profile": new_run.permission_profile,
        "started_at": now,
        "execution": "agent_daemon_run_manager",
        "authority_mode": daemon_authority::authority_mode_label(),
        "retried_from": run_id,
        "ids_differ": new_run.id != run_id,
    }))
}

async fn handle_run_list(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = params.get("conversation_id").and_then(|v| v.as_str());

    if let Some(cid) = conversation_id {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, status, provider_id, model_id, permission_profile, started_at, finished_at, error_code, step_count FROM assistant_runs WHERE conversation_id = ?1 AND parent_run_id IS NULL ORDER BY started_at DESC"
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
            "SELECT id, conversation_id, status, provider_id, model_id, permission_profile, started_at, finished_at, error_code, step_count FROM assistant_runs WHERE parent_run_id IS NULL ORDER BY started_at DESC"
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

async fn handle_run_list_children(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let parent_id = match params.get("parent_run_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "parent_run_id is required"),
    };
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, conversation_id, status, provider_id, model_id, permission_profile, started_at, finished_at, error_code, step_count
         FROM assistant_runs WHERE parent_run_id = ?1 ORDER BY started_at ASC",
    ) {
        Ok(stmt) => stmt,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };
    let rows = match stmt.query_map(rusqlite::params![parent_id], row_to_run) {
        Ok(rows) => rows,
        Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
    };
    success_response(serde_json::json!(rows.filter_map(|row| row.ok()).collect::<Vec<_>>()))
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

    let after_sequence = params
        .get("after_sequence")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    // Prefer live Run Authority events (embedded or UDS), then durable DB.
    if let Ok(daemon_events) =
        daemon_authority::replay_events(run_id, after_sequence as u64).await
    {
        if !daemon_events.is_empty() {
            if let Ok(conn) = crate::db::get_assistant_db_conn() {
                for event in &daemon_events {
                    let payload =
                        serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".into());
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            run_id,
                            event.sequence as i64,
                            event.timestamp.to_rfc3339(),
                            event.payload.type_name(),
                            payload
                        ],
                    );
                }
            }
            let events: Vec<Value> = daemon_events
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
            return success_response(serde_json::json!(events));
        }
    }

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

async fn handle_prompt_queue_list(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").or_else(|| params.get("conversationId")).and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, conversation_id, content, source, attachments, position, client_temp_id, created_at
         FROM assistant_prompt_queue WHERE conversation_id = ?1 ORDER BY position ASC, created_at ASC",
    ) {
        Ok(stmt) => stmt,
        Err(error) => return error_response("DB_ERROR", &error.to_string()),
    };
    let rows = stmt.query_map(rusqlite::params![conversation_id], |row| {
        let attachments: Option<String> = row.get(4)?;
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "conversation_id": row.get::<_, String>(1)?,
            "content": row.get::<_, String>(2)?,
            "source": row.get::<_, String>(3)?,
            "attachments": attachments.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()),
            "order": row.get::<_, i64>(5)?,
            "client_temp_id": row.get::<_, Option<String>>(6)?,
            "created_at": row.get::<_, String>(7)?
        }))
    });
    match rows {
        Ok(rows) => success_response(serde_json::json!(rows.filter_map(|row| row.ok()).collect::<Vec<_>>())),
        Err(error) => error_response("DB_QUERY_ERROR", &error.to_string()),
    }
}

async fn handle_prompt_queue_enqueue(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "conversation_id is required"),
    };
    let content = match params.get("content").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
        Some(content) => content,
        None => return error_response("MISSING_PARAM", "content is required"),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM assistant_prompt_queue WHERE conversation_id = ?1",
        rusqlite::params![conversation_id],
        |row| row.get(0),
    ).unwrap_or(0);
    if let Err(error) = conn.execute(
        "INSERT INTO assistant_prompt_queue (id, conversation_id, content, source, attachments, position, client_temp_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        rusqlite::params![id, conversation_id, content, params.get("source").and_then(Value::as_str).unwrap_or("user"), params.get("attachments").map(Value::to_string), position, params.get("client_temp_id").or_else(|| params.get("clientTempId")).and_then(Value::as_str), now],
    ) {
        return error_response("DB_INSERT_ERROR", &error.to_string());
    }
    success_response(serde_json::json!({"id": id, "conversation_id": conversation_id, "content": content, "order": position, "created_at": now}))
}

async fn handle_prompt_queue_update(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").and_then(Value::as_str) { Some(id) => id, None => return error_response("MISSING_PARAM", "id is required") };
    let content = match params.get("content").and_then(Value::as_str) { Some(content) => content, None => return error_response("MISSING_PARAM", "content is required") };
    let conn = data_store.conn();
    match conn.execute("UPDATE assistant_prompt_queue SET content = ?1, updated_at = ?2 WHERE id = ?3", rusqlite::params![content, chrono::Utc::now().to_rfc3339(), id]) {
        Ok(0) => error_response("NOT_FOUND", "prompt queue item not found"),
        Ok(_) => success_response(serde_json::json!({"id": id, "updated": true})),
        Err(error) => error_response("DB_UPDATE_ERROR", &error.to_string()),
    }
}

async fn handle_prompt_queue_remove(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").and_then(Value::as_str) { Some(id) => id, None => return error_response("MISSING_PARAM", "id is required") };
    let conn = data_store.conn();
    match conn.execute("DELETE FROM assistant_prompt_queue WHERE id = ?1", rusqlite::params![id]) {
        Ok(0) => error_response("NOT_FOUND", "prompt queue item not found"),
        Ok(_) => success_response(serde_json::json!({"id": id, "removed": true})),
        Err(error) => error_response("DB_DELETE_ERROR", &error.to_string()),
    }
}

async fn handle_prompt_queue_reorder(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let conversation_id = match params.get("conversation_id").and_then(Value::as_str) { Some(id) => id, None => return error_response("MISSING_PARAM", "conversation_id is required") };
    let ids = match params.get("ids").and_then(Value::as_array) { Some(ids) => ids, None => return error_response("MISSING_PARAM", "ids is required") };
    let conn = data_store.conn();
    let tx = match conn.unchecked_transaction() { Ok(tx) => tx, Err(error) => return error_response("DB_ERROR", &error.to_string()) };
    for (position, id) in ids.iter().filter_map(Value::as_str).enumerate() {
        if let Err(error) = tx.execute("UPDATE assistant_prompt_queue SET position = ?1, updated_at = ?2 WHERE id = ?3 AND conversation_id = ?4", rusqlite::params![position as i64, chrono::Utc::now().to_rfc3339(), id, conversation_id]) {
            return error_response("DB_UPDATE_ERROR", &error.to_string());
        }
    }
    if let Err(error) = tx.commit() { return error_response("DB_UPDATE_ERROR", &error.to_string()); }
    success_response(serde_json::json!({"conversation_id": conversation_id, "reordered": true}))
}

async fn handle_prompt_queue_send_now(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let id = match params.get("id").and_then(Value::as_str) {
        Some(id) => id,
        None => return error_response("MISSING_PARAM", "id is required"),
    };
    let (conversation_id, content, attachments, provider_id, model_id, project_path) = {
        let conn = data_store.conn();
        match conn.query_row(
            "SELECT q.conversation_id, q.content, q.attachments, c.provider_id, c.model_id, c.project_id
             FROM assistant_prompt_queue q
             JOIN assistant_conversations c ON c.id = q.conversation_id
             WHERE q.id = ?1",
            rusqlite::params![id],
            |row| {
                let attachments: Option<String> = row.get(2)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    attachments
                        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                        .unwrap_or_else(|| Value::Array(Vec::new())),
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        ) {
            Ok(item) => item,
            Err(_) => return error_response("NOT_FOUND", "prompt queue item not found"),
        }
    };

    let runs = match daemon_authority::list_runs(Some(&conversation_id)).await {
        Ok(runs) => runs,
        Err(error) => return error_response("DAEMON_CANCEL_FAILED", &error),
    };
    for run in runs.iter().filter(|run| !run.status.is_terminal()) {
        if let Err(error) = daemon_authority::cancel_run(&run.id).await {
            return error_response("DAEMON_CANCEL_FAILED", &error);
        }
        let local_status = if run.status.as_str() == "cancelled" {
            "interrupted"
        } else {
            run.status.as_str()
        };
        let conn = data_store.conn();
        let _ = conn.execute(
            "UPDATE assistant_runs SET status = ?1, finished_at = ?2 WHERE id = ?3 AND status IN ('queued', 'preparing', 'running', 'waiting_permission', 'cancelling')",
            rusqlite::params![local_status, chrono::Utc::now().to_rfc3339(), run.id],
        );
    }

    let start_response = handle_run_start(
        data_store,
        &serde_json::json!({
            "conversation_id": conversation_id,
            "provider_id": provider_id,
            "model_id": model_id,
            "content": content,
            "attachments": attachments,
            "project_path": project_path,
        }),
    )
    .await;
    if !start_response.success {
        return start_response;
    }

    let conn = data_store.conn();
    if let Err(error) = conn.execute(
        "DELETE FROM assistant_prompt_queue WHERE id = ?1",
        rusqlite::params![id],
    ) {
        return error_response("DB_DELETE_ERROR", &error.to_string());
    }
    start_response
}

// ─── Artifact handlers ───

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
        // Even in UDS, run.start stays host-owned for preflight + message write.
        assert!(!daemon_owned_method("run.start"));
        if let Some(value) = previous {
            std::env::set_var("NATIVES_DAEMON_MODE", value);
        } else {
            std::env::remove_var("NATIVES_DAEMON_MODE");
        }
        assert!(daemon_owned_method("run.list"));
        assert!(daemon_owned_method("run.cancel"));
        assert!(daemon_owned_method("provider.test"));
        assert!(daemon_owned_method("mcp.list"));
        // Conversation CRUD is always host-owned (assistant.db), never UDS dual-store.
        assert!(!daemon_owned_method("conversation.list"));
        assert!(!daemon_owned_method("conversation.delete"));
        assert!(!daemon_owned_method("conversation.create"));
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
        // `natives-agent-daemon` is a dependency, so its `cfg(test)` default is
        // not active when this crate runs tests. Select the embedded authority
        // explicitly; production remains UDS-only by default.
        let previous_daemon_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
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
        let run_id = started.data.as_ref().unwrap()["id"].as_str().unwrap();
        assert_eq!(started.data.as_ref().unwrap()["permission_profile"], "readonly");
        store.conn().execute(
            "INSERT INTO assistant_permission_requests (id, run_id, tool_call_id, tool_name, reason, input, created_at) VALUES ('permission', ?1, 'tool', 'Read', 'test', '{}', ?2)",
            rusqlite::params![run_id, chrono::Utc::now().to_rfc3339()],
        ).unwrap();
        let responded = dispatch_rpc(
            &store,
            "permission.respond",
            &serde_json::json!({
                "request_id": "permission", "approved": true, "scope": "this_run"
            }),
        )
        .await;
        assert!(responded.success);
        let permission: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, scope FROM assistant_permission_requests WHERE id = 'permission'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
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
        assert_eq!(runs["runs"][0]["permission_profile"], "readonly");
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
        let file_block = messages[0]["content_blocks"]
            .as_array()
            .and_then(|blocks| blocks.iter().find(|block| block["type"] == "file_reference"))
            .expect("file_reference block");
        assert_eq!(
            file_block["content"]["path"],
            attachment_path.to_string_lossy().to_string()
        );
        let _ = std::fs::remove_file(attachment_path);
        crate::daemon_authority::reset_authority_cache().await;
        if let Some(mode) = previous_daemon_mode {
            std::env::set_var("NATIVES_DAEMON_MODE", mode);
        } else {
            std::env::remove_var("NATIVES_DAEMON_MODE");
        }
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
