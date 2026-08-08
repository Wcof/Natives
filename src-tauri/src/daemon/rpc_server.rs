//! RPC Server — Unix Domain Socket listener with authentication and dispatch.
//!
//! Protocol:
//!   1. Client connects, sends HandshakeRequest (bootstrap token, client version)
//!   2. Server validates bootstrap token, generates session token, responds HandshakeResponse
//!   3. Client sends RpcRequest with session token, method, params
//!   4. Server dispatches to handler, returns RpcResponse
//!   5. Client can subscribe to run events via SubscribeRequest
//!
//! Messages are newline-delimited JSON (one JSON object per line, terminated by \n).

use crate::daemon::data::DataStore;
use crate::daemon::event_bus::EventBus;
use crate::daemon::provider::provider_service::ProviderService;
use crate::Result;
use assistant_protocol::error::{DaemonError, ErrorCategory};
use assistant_protocol::v1::daemon::{
    DaemonCapabilities, DaemonHealth, DaemonStatus, HandshakeRequest, HandshakeResponse,
    RpcRequest, RpcResponse,
};
use assistant_protocol::version::{negotiate, ProtocolVersion};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// RPC server that listens for client connections.
pub struct RpcServer {
    socket_path: String,
    bootstrap_token: String,
    protocol_version: ProtocolVersion,
    daemon_version: String,
    data_store: Arc<DataStore>,
    event_bus: Arc<EventBus>,
    /// Active sessions: session_token -> client_id
    sessions: Arc<Mutex<HashMap<String, String>>>,
}

impl RpcServer {
    /// Create a new RPC server.
    pub fn new(
        socket_path: &str,
        bootstrap_token: &str,
        protocol_version: &str,
        daemon_version: &str,
        data_store: Arc<DataStore>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        RpcServer {
            socket_path: socket_path.to_string(),
            bootstrap_token: bootstrap_token.to_string(),
            protocol_version: ProtocolVersion::from(protocol_version),
            daemon_version: daemon_version.to_string(),
            data_store,
            event_bus,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start listening for connections. Returns a join handle.
    pub async fn start(&self) -> Result<JoinHandle<()>> {
        // Remove any existing socket file
        let _ = std::fs::remove_file(&self.socket_path);

        let listener = UnixListener::bind(&self.socket_path)
            .map_err(|e| crate::Error::Internal(format!("Failed to bind socket: {e}")))?;

        // Set permissions to 0700 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o700))
                .ok();
        }

        let data_store = self.data_store.clone();
        let event_bus = self.event_bus.clone();
        let bootstrap_token = self.bootstrap_token.clone();
        let protocol_version = self.protocol_version.clone();
        let daemon_version = self.daemon_version.clone();
        let sessions = self.sessions.clone();

        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let data_store = data_store.clone();
                        let event_bus = event_bus.clone();
                        let bootstrap_token = bootstrap_token.clone();
                        let protocol_version = protocol_version.clone();
                        let daemon_version = daemon_version.clone();
                        let sessions = sessions.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(
                                stream,
                                &bootstrap_token,
                                &protocol_version,
                                &daemon_version,
                                &data_store,
                                &event_bus,
                                &sessions,
                            )
                            .await
                            {
                                eprintln!("RPC connection error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("RPC server accept error: {e}");
                        break;
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Get the number of active sessions.
    pub async fn active_sessions(&self) -> usize {
        self.sessions.lock().await.len()
    }
}

/// Handle a single client connection.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    bootstrap_token: &str,
    protocol_version: &ProtocolVersion,
    daemon_version: &str,
    data_store: &Arc<DataStore>,
    event_bus: &Arc<EventBus>,
    sessions: &Arc<Mutex<HashMap<String, String>>>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // ── Step 1: Read handshake ──────────────────────────────────
    line.clear();
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| crate::Error::Internal(format!("Failed to read handshake: {e}")))?;

    // The supervisor performs bootstrap authentication once and keeps that
    // connection alive. Tauri command invocations then open short-lived
    // connections authenticated by the resulting session token.
    if let Ok(request) = serde_json::from_str::<RpcRequest>(line.trim()) {
        let authorized = {
            let sessions = sessions.lock().await;
            sessions.contains_key(&request.session_token)
        };
        let response = if authorized {
            dispatch_request(&request, data_store, event_bus, &request.session_token).await
        } else {
            error_response(&request, "UNAUTHORIZED", "Invalid or expired session token")
        };
        let mut json =
            serde_json::to_string(&response).map_err(|e| crate::Error::Internal(e.to_string()))?;
        json.push('\n');
        writer
            .write_all(json.as_bytes())
            .await
            .map_err(|e| crate::Error::Internal(format!("Failed to write response: {e}")))?;
        return Ok(());
    }

    let handshake: HandshakeRequest = serde_json::from_str(line.trim())
        .map_err(|e| crate::Error::Internal(format!("Invalid handshake format: {e}")))?;

    // ── Step 2: Validate bootstrap token ────────────────────────
    if handshake.bootstrap_token != bootstrap_token {
        let response = HandshakeResponse {
            session_token: String::new(),
            daemon_version: daemon_version.to_string(),
            protocol_version: protocol_version.to_string(),
            accepted: false,
            upgrade_required: Some("Invalid bootstrap token".to_string()),
        };
        let mut json =
            serde_json::to_string(&response).map_err(|e| crate::Error::Internal(e.to_string()))?;
        json.push('\n');
        writer.write_all(json.as_bytes()).await.ok();
        return Err(crate::Error::Internal(
            "Invalid bootstrap token".to_string(),
        ));
    }

    // ── Step 3: Version negotiation ─────────────────────────────
    let client_version = ProtocolVersion::from(handshake.client_version.as_str());
    let negotiation = negotiate(&client_version, protocol_version);

    // ── Step 4: Generate session token ──────────────────────────
    let session_token = Uuid::new_v4().to_string();
    {
        let mut sessions = sessions.lock().await;
        sessions.insert(session_token.clone(), handshake.client_id.clone());
    }

    let response = HandshakeResponse {
        session_token: session_token.clone(),
        daemon_version: daemon_version.to_string(),
        protocol_version: protocol_version.to_string(),
        accepted: negotiation.compatible,
        upgrade_required: negotiation.upgrade_required,
    };
    {
        let mut json =
            serde_json::to_string(&response).map_err(|e| crate::Error::Internal(e.to_string()))?;
        json.push('\n');
        writer.write_all(json.as_bytes()).await.map_err(|e| {
            crate::Error::Internal(format!("Failed to send handshake response: {e}"))
        })?;
    }

    if !negotiation.compatible {
        return Err(crate::Error::Internal(
            "Protocol version incompatible".to_string(),
        ));
    }

    // ── Step 5: Handle RPC requests ─────────────────────────────
    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .await
            .map_err(|e| crate::Error::Internal(format!("Failed to read request: {e}")))?;

        if bytes_read == 0 {
            // Connection closed
            break;
        }

        let request: RpcRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                // Send error response for malformed request
                let error_resp = RpcResponse {
                    protocol_version: protocol_version.to_string(),
                    request_id: "unknown".to_string(),
                    success: false,
                    data: None,
                    error: Some(serde_json::json!({
                        "code": "PARSE_ERROR",
                        "category": "protocol",
                        "retryable": false,
                        "user_message_key": "error.parse_error",
                        "technical_message": format!("Failed to parse request: {e}"),
                        "recovery_actions": ["Check the request format and try again"],
                        "correlation_id": String::new(),
                    })),
                };
                let mut json = serde_json::to_string(&error_resp)
                    .map_err(|e| crate::Error::Internal(e.to_string()))?;
                json.push('\n');
                writer.write_all(json.as_bytes()).await.ok();
                continue;
            }
        };

        // Validate session token
        {
            let sessions = sessions.lock().await;
            if !sessions.contains_key(&request.session_token) {
                let error_resp = RpcResponse {
                    protocol_version: protocol_version.to_string(),
                    request_id: request.request_id.clone(),
                    success: false,
                    data: None,
                    error: Some(serde_json::json!({
                        "code": "UNAUTHORIZED",
                        "category": "auth",
                        "retryable": false,
                        "user_message_key": "error.session_expired",
                        "technical_message": "Invalid or expired session token".to_string(),
                        "recovery_actions": ["Re-authenticate by restarting the client"],
                        "correlation_id": String::new(),
                    })),
                };
                let mut json = serde_json::to_string(&error_resp)
                    .map_err(|e| crate::Error::Internal(e.to_string()))?;
                json.push('\n');
                writer.write_all(json.as_bytes()).await.ok();
                continue;
            }
        }

        // Dispatch to handler
        let response = dispatch_request(&request, data_store, event_bus, &session_token).await;

        let mut json =
            serde_json::to_string(&response).map_err(|e| crate::Error::Internal(e.to_string()))?;
        json.push('\n');
        if let Err(e) = writer.write_all(json.as_bytes()).await {
            eprintln!("Failed to write response: {e}");
            break;
        }
    }

    Ok(())
}

/// Dispatch an RPC request to the appropriate handler.
async fn dispatch_request(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
    event_bus: &Arc<EventBus>,
    _session_token: &str,
) -> RpcResponse {
    let start = std::time::Instant::now();

    if production_daemon_owned_method(&request.method) {
        return match crate::daemon_authority::request(&request.method, request.params.clone()).await
        {
            Ok(data) => success_response(request, data),
            Err(error) => error_response(
                request,
                "DAEMON_RPC_ERROR",
                &format!("Daemon RPC failed for {}: {error}", request.method),
            ),
        };
    }

    let result = match request.method.as_str() {
        // ── Daemon ──
        "daemon.getStatus" => handle_get_status(request, data_store).await,
        "daemon.getCapabilities" => handle_get_capabilities(request, event_bus).await,

        // ── Conversation ──
        "conversation.list" => handle_conversation_list(request, data_store).await,
        "conversation.create" => handle_conversation_create(request, data_store).await,
        "conversation.get" => handle_conversation_get(request, data_store).await,
        "conversation.update" => handle_conversation_update(request, data_store).await,
        "conversation.update_model" => handle_conversation_update_model(request, data_store).await,
        "conversation.archive" => handle_conversation_archive(request, data_store).await,
        "conversation.delete" => handle_conversation_delete(request, data_store).await,
        "conversation.getMessages" => handle_conversation_get_messages(request, data_store).await,

        // ── Run ──
        "run.start" => handle_run_start(request, data_store, event_bus).await,
        "run.cancel" => handle_run_cancel(request, data_store, event_bus).await,
        "run.list" => handle_run_list(request, data_store).await,
        "run.getStatus" => handle_run_get_status(request, data_store).await,
        "run.getEvents" => handle_run_get_events(request, data_store).await,

        // ── Permission ──
        "permission.respond" => handle_permission_respond(request, data_store).await,

        // ── Provider ──
        "provider.list" => handle_provider_list(request, data_store).await,
        "provider.create" => handle_provider_create(request, data_store).await,
        "provider.addKey" => handle_provider_add_key(request, data_store).await,
        "provider.delete" => handle_provider_delete(request, data_store).await,
        "provider.testKey" => handle_provider_test_key(request, data_store).await,
        "provider.setPrimaryKey" => handle_provider_set_primary_key(request, data_store).await,
        "provider.deleteKey" => handle_provider_delete_key(request, data_store).await,
        "provider.updateDefaults" => handle_provider_update_defaults(request, data_store).await,

        // ── Artifact ──
        "artifact.list" => handle_artifact_list(request, data_store).await,
        "artifact.open" => handle_artifact_open(request, data_store).await,
        "workspace.inspect" => handle_workspace_inspect(request).await,

        // ── Context ──
        "context.preview" => handle_context_preview(request, data_store).await,

        // ── Project ──
        "project.register" => handle_project_register(request).await,
        "project.list" => handle_project_list(request, data_store).await,

        _ if assistant_protocol::v2::is_implemented_method(&request.method) => {
            match crate::daemon_authority::request(&request.method, request.params.clone()).await {
                Ok(data) => success_response(request, data),
                Err(error) => error_response(
                    request,
                    "DAEMON_RPC_ERROR",
                    &format!("Daemon RPC failed for {}: {error}", request.method),
                ),
            }
        }
        _ if assistant_protocol::v2::is_known_method(&request.method) => error_response(
            request,
            "METHOD_UNSUPPORTED",
            &format!(
                "RPC method is not implemented by this server: {}",
                request.method
            ),
        ),

        // ── Unknown ──
        _ => RpcResponse {
            protocol_version: request.protocol_version.clone(),
            request_id: request.request_id.clone(),
            success: false,
            data: None,
            error: Some(serde_json::json!({
                "code": "METHOD_NOT_FOUND",
                "category": "protocol",
                "retryable": false,
                "user_message_key": "error.method_not_found",
                "technical_message": format!("Unknown method: {}", request.method),
                "correlation_id": String::new(),
            })),
        },
    };

    // Log slow requests (>500ms)
    let elapsed = start.elapsed();
    if elapsed.as_millis() > 500 {
        eprintln!(
            "Slow RPC: {} took {}ms",
            request.method,
            elapsed.as_millis()
        );
    }

    result
}

fn production_daemon_owned_method(method: &str) -> bool {
    // User-configured providers live in assistant.db and must be listed by the
    // host handler. Daemon's provider.list only returns built-in adapter types.
    if method == "provider.list" {
        return false;
    }
    crate::daemon_authority::authority_mode_label() == "uds"
        && assistant_protocol::v2::is_implemented_method(method)
}

// ─── Handler Implementations ─────────────────────────────────

async fn handle_get_status(request: &RpcRequest, _data_store: &Arc<DataStore>) -> RpcResponse {
    RpcResponse {
        protocol_version: request.protocol_version.clone(),
        request_id: request.request_id.clone(),
        success: true,
        data: Some(serde_json::json!(DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: "0.1.0".to_string(),
            uptime_secs: 0,
            pid: std::process::id() as u64,
            active_runs: 0,
            active_extensions: 0,
            provider_count: 0,
            memory_usage_mb: 0,
            health: DaemonHealth::Healthy,
        })),
        error: None,
    }
}

async fn handle_get_capabilities(request: &RpcRequest, _event_bus: &Arc<EventBus>) -> RpcResponse {
    RpcResponse {
        protocol_version: request.protocol_version.clone(),
        request_id: request.request_id.clone(),
        success: true,
        data: Some(serde_json::json!(DaemonCapabilities {
            protocol_version: "0.1.0".to_string(),
            features: vec![
                "chat".to_string(),
                "agent".to_string(),
                "streaming".to_string(),
                "tool_calling".to_string(),
                "permission_gateway".to_string(),
            ],
            max_concurrent_runs: 5,
            max_concurrent_sub_agents: 3,
            supported_providers: vec![
                "openai".to_string(),
                "anthropic".to_string(),
                "gemini".to_string(),
                "deepseek".to_string(),
                "openai_compatible".to_string(),
                "ollama".to_string(),
            ],
            has_plugin_host: false,
            has_mcp_support: true,
        })),
        error: None,
    }
}

async fn handle_conversation_list(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let conn = data_store.conn();
    let project_id = request.params.get("project_id").and_then(|v| v.as_str());
    let mode = request.params.get("mode").and_then(|v| v.as_str());
    let include_archived = request
        .params
        .get("include_archived")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut sql = "SELECT id, mode, COALESCE(project_id,''), title, provider_id, model_id, permission_profile_id, created_at, updated_at, archived_at FROM assistant_conversations WHERE 1=1".to_string();
    let mut param_values: Vec<String> = Vec::new();

    if let Some(pid) = project_id {
        param_values.push(pid.to_string());
        sql.push_str(&format!(" AND project_id = ?{}", param_values.len()));
    }
    if let Some(m) = mode {
        param_values.push(m.to_string());
        sql.push_str(&format!(" AND mode = ?{}", param_values.len()));
    }
    if !include_archived {
        sql.push_str(" AND archived_at IS NULL");
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };

    let params: Vec<&dyn rusqlite::types::ToSql> = param_values
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();

    let rows: Vec<serde_json::Value> = match stmt.query_map(params.as_slice(), |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "mode": row.get::<_, String>(1)?,
            "project_id": row.get::<_, String>(2)?,
            "title": row.get::<_, String>(3)?,
            "provider_id": row.get::<_, String>(4)?,
            "model_id": row.get::<_, String>(5)?,
            "created_at": row.get::<_, String>(7)?,
            "updated_at": row.get::<_, String>(8)?,
        }))
    }) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };

    success_response(request, serde_json::json!({ "conversations": rows }))
}

async fn handle_conversation_create(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let mode = match request.params.get("mode").and_then(|value| value.as_str()) {
        Some("chat") => "chat",
        Some("agent") => "agent",
        Some("goal") => "goal",
        _ => {
            return error_response(
                request,
                "INVALID_INPUT",
                "Mode must be 'chat', 'agent', or 'goal'",
            )
        }
    };
    let title = match request
        .params
        .get("title")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'title' parameter"),
    };
    let provider_id = match request
        .params
        .get("provider_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'provider_id' parameter"),
    };
    let model_id = match request
        .params
        .get("model_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'model_id' parameter"),
    };

    let project_id_value = match request
        .params
        .get("project_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => {
            let path = std::path::Path::new(value);
            if !path.exists() {
                return error_response(
                    request,
                    "PROJECT_NOT_FOUND",
                    &format!("Project directory not found: {value}"),
                );
            }
            if !path.is_dir() {
                return error_response(
                    request,
                    "PROJECT_NOT_DIRECTORY",
                    &format!("Not a directory: {value}"),
                );
            }
            value.to_string()
        }
        None => return error_response(request, "INVALID_INPUT", "Missing 'project_id' parameter"),
    };

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = data_store.conn();
    let pair_is_available = conn
        .query_row(
            "SELECT EXISTS(
            SELECT 1 FROM assistant_provider_configs provider
            JOIN assistant_model_cache model ON model.provider_id = provider.id
            WHERE provider.id = ?1 AND model.model_id = ?2
              AND EXISTS(
                SELECT 1 FROM assistant_provider_keys key
                WHERE key.provider_id = provider.id AND key.is_active = 1
              )
         )",
            rusqlite::params![provider_id, model_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if !pair_is_available {
        return error_response(
            request,
            "INVALID_INPUT",
            "Provider/model pair is not available",
        );
    }

    if let Err(e) = conn.execute(
        "INSERT INTO assistant_conversations (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![id, mode, project_id_value, title, provider_id, model_id, now, now],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }

    success_response(
        request,
        serde_json::json!({
            "id": id,
            "mode": mode,
            "project_id": project_id_value,
            "title": title,
            "provider_id": provider_id,
            "model_id": model_id,
            "created_at": now,
            "updated_at": now,
        }),
    )
}

async fn handle_conversation_get(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let conn = data_store.conn();
    let result = conn.query_row(
        "SELECT id, mode, COALESCE(project_id,''), title, provider_id, model_id, permission_profile_id, created_at, updated_at, archived_at
         FROM assistant_conversations WHERE id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "mode": row.get::<_, String>(1)?,
                "project_id": row.get::<_, String>(2)?,
                "title": row.get::<_, String>(3)?,
                "provider_id": row.get::<_, String>(4)?,
                "model_id": row.get::<_, String>(5)?,
                "created_at": row.get::<_, String>(7)?,
                "updated_at": row.get::<_, String>(8)?,
                "archived_at": row.get::<_, Option<String>>(9)?,
            }))
        },
    );

    match result {
        Ok(conv) => success_response(request, conv),
        Err(_) => error_response(request, "NOT_FOUND", "Conversation not found"),
    }
}

async fn handle_conversation_update(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let title = request.params.get("title").and_then(|v| v.as_str());
    let now = chrono::Utc::now().to_rfc3339();

    let conn = data_store.conn();
    if let Some(t) = title {
        if let Err(e) = conn.execute(
            "UPDATE assistant_conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![t, now, id],
        ) {
            return error_response(request, "INTERNAL_ERROR", &e.to_string());
        }
    }

    success_response(request, serde_json::json!({ "updated": true }))
}

async fn handle_conversation_update_model(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let id = match request
        .params
        .get("id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };
    let provider_id = match request
        .params
        .get("provider_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'provider_id' parameter"),
    };
    let model_id = match request
        .params
        .get("model_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'model_id' parameter"),
    };

    let conn = data_store.conn();
    let pair_is_available = conn
        .query_row(
            "SELECT EXISTS(
            SELECT 1
            FROM assistant_provider_configs provider
            JOIN assistant_model_cache model ON model.provider_id = provider.id
            WHERE provider.id = ?1
              AND model.model_id = ?2
              AND EXISTS(
                SELECT 1 FROM assistant_provider_keys key
                WHERE key.provider_id = provider.id AND key.is_active = 1
              )
         )",
            rusqlite::params![provider_id, model_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if !pair_is_available {
        return error_response(
            request,
            "INVALID_INPUT",
            "Provider/model pair is not available",
        );
    }

    let now = chrono::Utc::now().to_rfc3339();
    match conn.execute(
        "UPDATE assistant_conversations
         SET provider_id = ?1, model_id = ?2, updated_at = ?3
         WHERE id = ?4",
        rusqlite::params![provider_id, model_id, now, id],
    ) {
        Ok(0) => error_response(request, "NOT_FOUND", "Conversation not found"),
        Ok(_) => success_response(
            request,
            serde_json::json!({
                "updated": true,
                "provider_id": provider_id,
                "model_id": model_id,
                "updated_at": now,
            }),
        ),
        Err(error) => error_response(request, "INTERNAL_ERROR", &error.to_string()),
    }
}

async fn handle_conversation_archive(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let now = chrono::Utc::now().to_rfc3339();
    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_conversations SET archived_at = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![now, now, id],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }

    success_response(request, serde_json::json!({ "archived": true }))
}

async fn handle_conversation_delete(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "DELETE FROM assistant_conversations WHERE id = ?1",
        rusqlite::params![id],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }

    success_response(request, serde_json::json!({ "deleted": true }))
}

async fn handle_conversation_get_messages(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let conversation_id = match request
        .params
        .get("conversation_id")
        .and_then(|v| v.as_str())
    {
        Some(id) => id,
        None => {
            return error_response(
                request,
                "INVALID_INPUT",
                "Missing 'conversation_id' parameter",
            )
        }
    };

    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT m.id, m.conversation_id, m.parent_message_id, m.role, m.status, m.input_tokens, m.output_tokens, m.reasoning_tokens, m.created_at
         FROM assistant_messages m
         WHERE m.conversation_id = ?1
         ORDER BY m.created_at ASC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };

    let rows: Vec<serde_json::Value> =
        match stmt.query_map(rusqlite::params![conversation_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "conversation_id": row.get::<_, String>(1)?,
                "parent_message_id": row.get::<_, Option<String>>(2)?,
                "role": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "input_tokens": row.get::<_, Option<i64>>(5)?,
                "output_tokens": row.get::<_, Option<i64>>(6)?,
                "reasoning_tokens": row.get::<_, Option<i64>>(7)?,
                "created_at": row.get::<_, String>(8)?,
            }))
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
        };

    // Get content blocks for each message
    let mut messages_with_blocks: Vec<serde_json::Value> = Vec::new();
    for msg in &rows {
        let msg_id = msg["id"].as_str().unwrap_or("");
        let mut stmt_blocks = match conn.prepare(
            "SELECT block_type, block_index, content, metadata FROM assistant_message_blocks WHERE message_id = ?1 ORDER BY block_index ASC"
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let blocks: Vec<serde_json::Value> =
            match stmt_blocks.query_map(rusqlite::params![msg_id], |row| {
                let content_str: String = row.get::<_, String>(2)?;
                let content: serde_json::Value =
                    serde_json::from_str(&content_str).unwrap_or(serde_json::Value::Null);
                Ok(serde_json::json!({
                    "type": row.get::<_, String>(0)?,
                    "index": row.get::<_, i32>(1)?,
                    "content": content,
                }))
            }) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => vec![],
            };

        let mut m = msg.clone();
        m["content_blocks"] = serde_json::Value::Array(blocks);
        messages_with_blocks.push(m);
    }

    success_response(
        request,
        serde_json::json!({ "messages": messages_with_blocks }),
    )
}

async fn handle_run_start(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
    event_bus: &Arc<EventBus>,
) -> RpcResponse {
    let _ = data_store.migrate_legacy_provider_keys();
    let conversation_id = match request
        .params
        .get("conversation_id")
        .and_then(|v| v.as_str())
    {
        Some(id) => id,
        None => {
            return error_response(
                request,
                "INVALID_INPUT",
                "Missing 'conversation_id' parameter",
            )
        }
    };

    let provider_id = match request
        .params
        .get("provider_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'provider_id' parameter"),
    };
    let model_id = match request
        .params
        .get("model_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'model_id' parameter"),
    };
    let trigger_message_id = request
        .params
        .get("trigger_message_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let mut conn = data_store.conn();
    let conversation_pair = conn.query_row(
        "SELECT provider_id, model_id FROM assistant_conversations WHERE id = ?1",
        rusqlite::params![conversation_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    );
    let (stored_provider_id, stored_model_id) = match conversation_pair {
        Ok(pair) => pair,
        Err(_) => return error_response(request, "NOT_FOUND", "Conversation not found"),
    };
    if stored_provider_id != provider_id || stored_model_id != model_id {
        return error_response(
            request,
            "INVALID_INPUT",
            "Provider/model selection is not persisted on the conversation",
        );
    }

    let pair_is_available = conn
        .query_row(
            "SELECT EXISTS(
            SELECT 1 FROM assistant_provider_configs provider
            JOIN assistant_model_cache model ON model.provider_id = provider.id
            WHERE provider.id = ?1 AND model.model_id = ?2
              AND EXISTS(
                SELECT 1 FROM assistant_provider_keys key
                WHERE key.provider_id = provider.id AND key.is_active = 1
              )
         )",
            rusqlite::params![provider_id, model_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if !pair_is_available {
        return error_response(
            request,
            "INVALID_INPUT",
            "Provider/model pair is not available",
        );
    }

    let (message_id, content, insert_message) = if let Some(trigger_id) = trigger_message_id {
        let belongs_to_conversation = conn
            .query_row(
                "SELECT EXISTS(
                SELECT 1 FROM assistant_messages
                WHERE id = ?1 AND conversation_id = ?2 AND role = 'user'
             )",
                rusqlite::params![trigger_id, conversation_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);
        if !belongs_to_conversation {
            return error_response(
                request,
                "INVALID_INPUT",
                "Retry trigger message is not a user message in this conversation",
            );
        }
        let mut statement = match conn.prepare(
            "SELECT content FROM assistant_message_blocks
             WHERE message_id = ?1 AND block_type = 'text'
             ORDER BY block_index ASC",
        ) {
            Ok(statement) => statement,
            Err(error) => return error_response(request, "INTERNAL_ERROR", &error.to_string()),
        };
        let text_parts = match statement
            .query_map(rusqlite::params![trigger_id], |row| row.get::<_, String>(0))
        {
            Ok(rows) => rows
                .filter_map(|row| row.ok())
                .filter_map(|raw| {
                    serde_json::from_str::<serde_json::Value>(&raw)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("text")
                                .and_then(|text| text.as_str())
                                .map(ToOwned::to_owned)
                        })
                })
                .collect::<Vec<_>>(),
            Err(error) => return error_response(request, "INTERNAL_ERROR", &error.to_string()),
        };
        let content = text_parts.join("").trim().to_string();
        if content.is_empty() {
            return error_response(
                request,
                "INVALID_INPUT",
                "Retry trigger message has no text content",
            );
        }
        (trigger_id.to_string(), content, false)
    } else {
        let content = match request
            .params
            .get("content")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => value.to_string(),
            None => return error_response(request, "INVALID_INPUT", "Missing 'content' parameter"),
        };
        let message_id = Uuid::new_v4().to_string();
        (message_id, content, true)
    };

    let transaction = match conn.transaction() {
        Ok(transaction) => transaction,
        Err(error) => return error_response(request, "INTERNAL_ERROR", &error.to_string()),
    };
    if insert_message {
        if let Err(error) = transaction.execute(
            "INSERT INTO assistant_messages (id, conversation_id, role, status, created_at) VALUES (?1, ?2, 'user', 'complete', ?3)",
            rusqlite::params![message_id, conversation_id, now],
        ) {
            return error_response(request, "INTERNAL_ERROR", &error.to_string());
        }
        if let Err(error) = transaction.execute(
            "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'text', 0, ?3)",
            rusqlite::params![Uuid::new_v4().to_string(), message_id, serde_json::json!({"text": content}).to_string()],
        ) {
            return error_response(request, "INTERNAL_ERROR", &error.to_string());
        }
    }
    if let Err(error) = transaction.execute(
        "INSERT INTO assistant_runs (id, conversation_id, status, trigger_message_id, provider_id, model_id, started_at)
         VALUES (?1, ?2, 'running', ?3, ?4, ?5, datetime('now'))",
        rusqlite::params![id, conversation_id, message_id, provider_id, model_id],
    ) {
        return error_response(request, "INTERNAL_ERROR", &error.to_string());
    }
    if let Err(error) = transaction.execute(
        "UPDATE assistant_conversations SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, conversation_id],
    ) {
        return error_response(request, "INTERNAL_ERROR", &error.to_string());
    }
    if let Err(error) = transaction.commit() {
        return error_response(request, "INTERNAL_ERROR", &error.to_string());
    }
    drop(conn);

    // Sole Run Authority: Protocol v2 Daemon RunManager (same crate as the
    // sidecar). The legacy in-process AgentLoop path is hard-disabled.
    let run_id = id.clone();
    let conversation_id_owned = conversation_id.to_string();
    let provider_id_owned = provider_id.to_string();
    let model_id_owned = model_id.to_string();
    let data_store = data_store.clone();
    let event_bus = event_bus.clone();
    let content_owned = content.clone();
    tokio::spawn(async move {
        // S3 Execution Policy: resolve the Settings V2 policy for this Run
        // (explicit override → application default → safe default native).
        // No hardcoded max_steps / silent runtime default here.
        let settings = crate::execution_engine_settings::load_execution_engine_settings();
        let runtimes = settings
            .as_ref()
            .map(crate::execution_engine_settings::build_runtime_descriptors)
            .unwrap_or_default();
        let policy = settings.as_ref().ok().and_then(|s| {
            crate::execution_engine_settings::resolve_execution_policy(
                s,
                &runtimes,
                None, // this legacy RPC path has no explicit runtime override
                None, // no conversation-level override yet
                None, // no explicit maxSteps override
            )
            .ok()
        });
        let (max_steps, runtime_id) = match &policy {
            Some(p) => (Some(p.max_steps), Some(p.runtime_id.clone())),
            None => {
                // Settings unavailable → degrade explicitly with defaults that
                // are documented, never a silent fake success. The daemon still
                // enforces its own honest runtime gate.
                (Some(crate::execution_engine_settings::DEFAULT_MAX_STEPS), None)
            }
        };
        // G4: same Run Authority façade as assistant_service (embedded or UDS).
        let create =
            crate::daemon_authority::create_run(assistant_protocol::v2::CreateRunRequest {
                conversation_id: conversation_id_owned.clone(),
                provider_id: provider_id_owned.clone(),
                model_id: model_id_owned.clone(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some(content_owned.clone()),
                attachments: None,
                max_steps,
                parent_run_id: None,
                project_path: None,
                idempotency_key: Some(run_id.clone()),
                effort: None,
                runtime_id: runtime_id.clone(),
                capability_selection: None,
            })
            .await;
        if let Err(error) = create {
            let _ = event_bus
                .publish(
                    &run_id,
                    "failed",
                    serde_json::json!({"code":"daemon_create_failed","error":error}),
                )
                .await;
            let conn = data_store.conn();
            let _ = conn.execute(
                "UPDATE assistant_runs SET status='failed', error_code='daemon_create_failed', finished_at=datetime('now') WHERE id=?1",
                rusqlite::params![run_id],
            );
            return;
        }

        let start_result =
            crate::daemon_authority::start_run(assistant_protocol::v2::StartRunRequest {
                run_id: Some(run_id.clone()),
                conversation_id: Some(conversation_id_owned),
                provider_id: Some(provider_id_owned),
                model_id: Some(model_id_owned),
                key_id: None,
                content: Some(content_owned),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("ask".into()),
                max_steps,
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id,
                agent_profile_id: None,
                capability_selection: None,
            })
            .await;

        let (status, error_code) = match start_result {
            Ok(run) => (run.status.as_str().to_string(), run.error_code),
            Err(error) => {
                let _ = event_bus
                    .publish(
                        &run_id,
                        "failed",
                        serde_json::json!({"code":"agent_error","error":error}),
                    )
                    .await;
                ("failed".into(), Some("agent_error".into()))
            }
        };

        // Mirror Protocol v2 events into the legacy event bus for any old subscribers.
        let events = crate::daemon_authority::replay_events(&run_id, 0)
            .await
            .unwrap_or_default();
        for event in events {
            let _ = event_bus
                .publish(
                    &run_id,
                    event.payload.type_name(),
                    serde_json::to_value(&event.payload).unwrap_or_default(),
                )
                .await;
        }

        let conn = data_store.conn();
        let _ = conn.execute(
            "UPDATE assistant_runs SET status=?1, error_code=?2, finished_at=datetime('now') WHERE id=?3",
            rusqlite::params![status, error_code, run_id],
        );
    });

    success_response(
        request,
        serde_json::json!({
            "id": id,
            "conversation_id": conversation_id,
            "trigger_message_id": message_id,
            "status": "running",
            "execution": "agent_daemon_run_manager",
        }),
    )
}

async fn handle_run_cancel(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
    event_bus: &Arc<EventBus>,
) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let changed = {
        let conn = data_store.conn();
        match conn.execute(
            "UPDATE assistant_runs SET status = 'interrupted', finished_at = datetime('now') WHERE id = ?1 AND status IN ('running', 'queued', 'preparing')",
            rusqlite::params![id],
        ) {
            Ok(changed) => changed,
            Err(error) => return error_response(request, "INTERNAL_ERROR", &error.to_string()),
        }
    };

    if changed == 0 {
        return error_response(request, "NOT_FOUND", "Active run not found");
    }

    let event_published = event_bus
        .publish(
            id,
            "interrupted",
            serde_json::json!({ "reason": "cancelled" }),
        )
        .await
        .is_ok();

    success_response(
        request,
        serde_json::json!({
            "cancelled": true,
            "event_published": event_published,
        }),
    )
}

async fn handle_run_get_status(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let conn = data_store.conn();
    match conn.query_row(
        "SELECT id, conversation_id, status, provider_id, model_id, started_at, finished_at, error_code, step_count
         FROM assistant_runs WHERE id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "conversation_id": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "provider_id": row.get::<_, String>(3)?,
                "model_id": row.get::<_, String>(4)?,
                "started_at": row.get::<_, Option<String>>(5)?,
                "finished_at": row.get::<_, Option<String>>(6)?,
                "error_code": row.get::<_, Option<String>>(7)?,
                "step_count": row.get::<_, Option<i32>>(8)?,
            }))
        },
    ) {
        Ok(run) => success_response(request, run),
        Err(_) => error_response(request, "NOT_FOUND", "Run not found"),
    }
}

async fn handle_run_list(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let conversation_id = match request
        .params
        .get("conversation_id")
        .and_then(|v| v.as_str())
    {
        Some(id) => id,
        None => {
            return error_response(
                request,
                "INVALID_INPUT",
                "Missing 'conversation_id' parameter",
            )
        }
    };
    let limit = request
        .params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .clamp(1, 100) as i64;
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, conversation_id, status, provider_id, model_id, started_at, finished_at,
                error_code, step_count, total_input_tokens, total_output_tokens
         FROM assistant_runs
         WHERE conversation_id = ?1
         ORDER BY COALESCE(started_at, '') DESC, rowid DESC
         LIMIT ?2",
    ) {
        Ok(stmt) => stmt,
        Err(error) => return error_response(request, "INTERNAL_ERROR", &error.to_string()),
    };
    let runs = match stmt.query_map(rusqlite::params![conversation_id, limit], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "conversation_id": row.get::<_, String>(1)?,
            "status": row.get::<_, String>(2)?,
            "provider_id": row.get::<_, String>(3)?,
            "model_id": row.get::<_, String>(4)?,
            "started_at": row.get::<_, Option<String>>(5)?,
            "finished_at": row.get::<_, Option<String>>(6)?,
            "error_code": row.get::<_, Option<String>>(7)?,
            "step_count": row.get::<_, Option<i64>>(8)?,
            "total_input_tokens": row.get::<_, Option<i64>>(9)?,
            "total_output_tokens": row.get::<_, Option<i64>>(10)?,
        }))
    }) {
        Ok(rows) => rows.filter_map(|row| row.ok()).collect::<Vec<_>>(),
        Err(error) => return error_response(request, "INTERNAL_ERROR", &error.to_string()),
    };
    success_response(request, serde_json::json!({ "runs": runs }))
}

async fn handle_run_get_events(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let run_id = match request.params.get("run_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'run_id' parameter"),
    };
    let after_sequence = request
        .params
        .get("after_sequence")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT sequence, timestamp, event_type, payload
         FROM assistant_run_events
         WHERE run_id = ?1 AND sequence > ?2
         ORDER BY sequence ASC",
    ) {
        Ok(s) => s,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };

    let events: Vec<serde_json::Value> =
        match stmt.query_map(rusqlite::params![run_id, after_sequence], |row| {
            let payload_str: String = row.get::<_, String>(3)?;
            let payload: serde_json::Value =
                serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
            Ok(serde_json::json!({
                "sequence": row.get::<_, i64>(0)?,
                "timestamp": row.get::<_, String>(1)?,
                "type": row.get::<_, String>(2)?,
                "payload": payload,
            }))
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
        };

    success_response(request, serde_json::json!({ "events": events }))
}

async fn handle_permission_respond(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let request_id = match request.params.get("request_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'request_id' parameter"),
    };
    let approved = request
        .params
        .get("approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let scope = request
        .params
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("once");

    let conn = data_store.conn();
    if let Err(e) = conn.execute(
        "UPDATE assistant_permission_requests SET status = ?1, scope = ?2, responded_at = datetime('now') WHERE id = ?3",
        rusqlite::params![if approved { "approved" } else { "rejected" }, scope, request_id],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }

    success_response(request, serde_json::json!({ "responded": true }))
}

async fn handle_provider_list(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let _ = data_store.migrate_legacy_provider_keys();
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, provider_type, display_name, api_base_url, health_status, default_model, created_at, updated_at
         FROM assistant_provider_configs ORDER BY display_name ASC"
    ) {
        Ok(s) => s,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
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
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
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
        let mut model_stmt = match conn.prepare(
            "SELECT model_id, display_name, capabilities, context_window, max_output, source, discovered_at
             FROM assistant_model_cache
             WHERE provider_id = ?1 ORDER BY model_id ASC"
        ) {
            Ok(stmt) => stmt,
            Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
        };
        let models: Vec<serde_json::Value> =
            match model_stmt.query_map(rusqlite::params![id], |row| {
                let capabilities = row.get::<_, String>(2)?;
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "display_name": row.get::<_, Option<String>>(1)?,
                    "capabilities": serde_json::from_str::<serde_json::Value>(&capabilities)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    "context_window": row.get::<_, i64>(3)?,
                    "max_output": row.get::<_, i64>(4)?,
                    "source": row.get::<_, String>(5)?,
                    "discovered_at": row.get::<_, String>(6)?,
                }))
            }) {
                Ok(rows) => rows.filter_map(|row| row.ok()).collect(),
                Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
            };
        let has_active_key = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM assistant_provider_keys WHERE provider_id = ?1 AND is_active = 1)",
            rusqlite::params![id],
            |row| row.get::<_, bool>(0),
        ).unwrap_or(false);

        providers.push(serde_json::json!({
            "id": id,
            "provider_type": provider_type,
            "display_name": display_name,
            "api_base_url": api_base_url,
            "health_status": health_status,
            "default_model": default_model,
            "has_active_key": has_active_key,
            "models": models,
            "created_at": created_at,
            "updated_at": updated_at,
        }));
    }

    success_response(request, serde_json::json!({ "providers": providers }))
}

async fn handle_provider_create(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let provider_type = match request.params.get("provider_type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return error_response(
                request,
                "INVALID_INPUT",
                "Missing 'provider_type' parameter",
            )
        }
    };
    let display_name = match request.params.get("display_name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return error_response(request, "INVALID_INPUT", "Missing 'display_name' parameter")
        }
    };
    let api_base_url = request
        .params
        .get("api_base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let id = request
        .params
        .get("id")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let default_model = request
        .params
        .get("default_model")
        .and_then(|v| v.as_str())
        .map(str::trim);
    let mut models: Vec<(String, Option<String>)> = request
        .params
        .get("models")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model.get("id")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let display_name = model
                .get("display_name")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            Some((id.to_string(), display_name))
        })
        .collect();
    models.sort_by(|left, right| left.0.cmp(&right.0));
    models.dedup_by(|left, right| left.0 == right.0);
    let default_model = match default_model {
        Some(model) if !model.is_empty() && models.iter().any(|(id, _)| id == model) => model,
        _ => {
            return error_response(
                request,
                "INVALID_INPUT",
                "Default model must be present in discovered models",
            )
        }
    };
    let now = chrono::Utc::now().to_rfc3339();

    let mut conn = data_store.conn();
    let transaction = match conn.transaction() {
        Ok(transaction) => transaction,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };
    if let Err(e) = transaction.execute(
        "INSERT INTO assistant_provider_configs (id, provider_type, display_name, api_base_url, default_model, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, provider_type, display_name, api_base_url, default_model, now, now],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }

    if let Some(keys) = request.params.get("keys").and_then(|v| v.as_array()) {
        let encryption_key = match crate::env_manager::get_encryption_key(&transaction) {
            Ok(key) => key,
            Err(e) => {
                return error_response(
                    request,
                    "INTERNAL_ERROR",
                    &format!("Credential encryption unavailable: {e}"),
                )
            }
        };
        for (index, key) in keys.iter().enumerate() {
            let plaintext = match key.get("api_key").and_then(|v| v.as_str()) {
                Some(value) if !value.trim().is_empty() => value.trim(),
                _ => continue,
            };
            let encrypted = match crate::env_manager::encrypt(plaintext, &encryption_key) {
                Ok(value) => value,
                Err(e) => {
                    return error_response(
                        request,
                        "INTERNAL_ERROR",
                        &format!("Credential encryption failed: {e}"),
                    )
                }
            };
            let chars: Vec<char> = plaintext.chars().collect();
            let masked = if chars.len() > 8 {
                format!(
                    "{}…{}",
                    chars[..4].iter().collect::<String>(),
                    chars[chars.len() - 4..].iter().collect::<String>()
                )
            } else {
                "***".to_string()
            };
            let key_id = Uuid::new_v4().to_string();
            let label = key
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("API Key");
            if let Err(e) = transaction.execute(
                "INSERT INTO assistant_provider_keys (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![key_id, id, encrypted, masked, label, if index == 0 { 1 } else { 0 }, now],
            ) {
                return error_response(request, "INTERNAL_ERROR", &e.to_string());
            }
        }
    }

    for (model_id, display_name) in models {
        if let Err(e) = transaction.execute(
            "INSERT INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'api_discovery', ?5)",
            rusqlite::params![Uuid::new_v4().to_string(), id, model_id, display_name, now],
        ) {
            return error_response(request, "INTERNAL_ERROR", &e.to_string());
        }
    }

    if let Err(e) = transaction.commit() {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }

    success_response(request, serde_json::json!({ "id": id }))
}

async fn handle_provider_add_key(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let provider_id = match request.params.get("provider_id").and_then(|v| v.as_str()) {
        Some(value) => value,
        None => return error_response(request, "INVALID_INPUT", "Missing 'provider_id' parameter"),
    };
    let plaintext = match request.params.get("api_key").and_then(|v| v.as_str()) {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => return error_response(request, "INVALID_INPUT", "Missing 'api_key' parameter"),
    };
    let conn = data_store.conn();
    let encryption_key = match crate::env_manager::get_encryption_key(&conn) {
        Ok(key) => key,
        Err(e) => {
            return error_response(
                request,
                "INTERNAL_ERROR",
                &format!("Credential encryption unavailable: {e}"),
            )
        }
    };
    let encrypted = match crate::env_manager::encrypt(plaintext, &encryption_key) {
        Ok(value) => value,
        Err(e) => {
            return error_response(
                request,
                "INTERNAL_ERROR",
                &format!("Credential encryption failed: {e}"),
            )
        }
    };
    let chars: Vec<char> = plaintext.chars().collect();
    let masked = if chars.len() > 8 {
        format!(
            "{}…{}",
            chars[..4].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    } else {
        "***".to_string()
    };
    let id = Uuid::new_v4().to_string();
    let label = request
        .params
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("API Key");
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = conn.execute(
        "INSERT INTO assistant_provider_keys (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
        rusqlite::params![id, provider_id, encrypted, masked, label, now],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }
    success_response(
        request,
        serde_json::json!({ "id": id, "masked_key": masked }),
    )
}

async fn handle_provider_delete(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let mut conn = data_store.conn();

    // Check for conversations using this provider
    let conv_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_conversations WHERE provider_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let force = request
        .params
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if conv_count > 0 && !force {
        // Get the affected conversation titles for the impact report
        let mut stmt = conn
            .prepare("SELECT id, title FROM assistant_conversations WHERE provider_id = ?1 LIMIT 5")
            .map_err(|e| crate::Error::Internal(e.to_string()));

        let conversations: Vec<serde_json::Value> = if let Ok(mut stmt) = stmt {
            stmt.query_map(rusqlite::params![id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                }))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
        } else {
            vec![]
        };

        return success_response(
            request,
            serde_json::json!({
                "requires_confirmation": true,
                "conversation_count": conv_count,
                "conversations": conversations,
                "message": format!("This provider is used by {conv_count} conversation(s). Use 'force: true' to delete anyway."),
            }),
        );
    }

    let transaction = match conn.transaction() {
        Ok(transaction) => transaction,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };
    if let Err(e) = transaction.execute(
        "DELETE FROM assistant_model_cache WHERE provider_id = ?1",
        rusqlite::params![id],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }
    if let Err(e) = transaction.execute(
        "DELETE FROM assistant_provider_configs WHERE id = ?1",
        rusqlite::params![id],
    ) {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }
    if let Err(e) = transaction.commit() {
        return error_response(request, "INTERNAL_ERROR", &e.to_string());
    }

    success_response(
        request,
        serde_json::json!({ "deleted": true, "affected_conversations": conv_count }),
    )
}

/// Test a provider key via RPC.
/// Uses ProviderService to prepare, execute, and save the test result.
/// Never exposes the plaintext key.
async fn handle_provider_test_key(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let provider_id = match request.params.get("provider_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'provider_id' parameter"),
    };
    let key_id = match request.params.get("key_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'key_id' parameter"),
    };

    // Phase 1: Prepare (sync)
    let prep = {
        let conn = data_store.conn();
        ProviderService::prepare_key_test(&conn, provider_id, key_id)
    };

    let result = match prep {
        Ok(crate::daemon::provider::provider_service::KeyTestPreparation::ConfigError(msg)) => {
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let err_result = crate::daemon::provider::provider_service::KeyTestResult {
                success: false,
                status: "unavailable".to_string(),
                tested_at: now,
                error_code: Some("config_error".to_string()),
                user_message: Some(msg),
            };
            // Save error result
            {
                let conn = data_store.conn();
                ProviderService::save_test_result(&conn, key_id, &err_result);
            }
            err_result
        }
        Ok(crate::daemon::provider::provider_service::KeyTestPreparation::Ready {
            base_url,
            api_key,
            model,
        }) => {
            // Phase 2: execute async
            let test_result = ProviderService::execute_key_test(&base_url, &api_key, &model).await;
            // Phase 3: save
            {
                let conn = data_store.conn();
                ProviderService::save_test_result(&conn, key_id, &test_result);
            }
            test_result
        }
        Err(e) => {
            return error_response(request, "INTERNAL_ERROR", &e.to_string());
        }
    };

    success_response(
        request,
        serde_json::json!({
            "success": result.success,
            "status": result.status,
            "tested_at": result.tested_at,
            "error_code": result.error_code,
            "user_message": result.user_message,
        }),
    )
}

/// Set a key as the primary key for its provider.
async fn handle_provider_set_primary_key(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let key_id = match request.params.get("key_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'key_id' parameter"),
    };

    let conn = data_store.conn();
    match ProviderService::set_primary_key(&conn, key_id) {
        Ok(summary) => {
            success_response(request, serde_json::to_value(&summary).unwrap_or_default())
        }
        Err(e) => error_response(request, "INTERNAL_ERROR", &e.to_string()),
    }
}

/// Delete a key. Primary keys cannot be deleted.
async fn handle_provider_delete_key(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let key_id = match request.params.get("key_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'key_id' parameter"),
    };

    let conn = data_store.conn();
    match ProviderService::delete_key(&conn, key_id) {
        Ok(_) => success_response(request, serde_json::json!({ "deleted": true })),
        Err(e) => error_response(request, "INVALID_INPUT", &e.to_string()),
    }
}

/// Update provider defaults (default_model).
async fn handle_provider_update_defaults(
    request: &RpcRequest,
    data_store: &Arc<DataStore>,
) -> RpcResponse {
    let provider_id = match request.params.get("provider_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'provider_id' parameter"),
    };
    let default_model = request
        .params
        .get("default_model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let conn = data_store.conn();
    let input = crate::daemon::provider::provider_service::UpdateDefaultsInput {
        provider_id: provider_id.to_string(),
        default_model,
    };

    match ProviderService::update_defaults(&conn, &input) {
        Ok(summary) => {
            success_response(request, serde_json::to_value(&summary).unwrap_or_default())
        }
        Err(e) => error_response(request, "INTERNAL_ERROR", &e.to_string()),
    }
}

async fn handle_artifact_list(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let run_id = request.params.get("run_id").and_then(|v| v.as_str());
    let conversation_id = request
        .params
        .get("conversation_id")
        .and_then(|v| v.as_str());

    let conn = data_store.conn();
    let sql = if run_id.is_some() {
        "SELECT id, run_id, conversation_id, source_tool, path, sha256, size, mime_type, label, kind, created_at
         FROM assistant_artifacts WHERE run_id = ?1 ORDER BY created_at DESC"
    } else if conversation_id.is_some() {
        "SELECT id, run_id, conversation_id, source_tool, path, sha256, size, mime_type, label, kind, created_at
         FROM assistant_artifacts WHERE conversation_id = ?1 ORDER BY created_at DESC"
    } else {
        "SELECT id, run_id, conversation_id, source_tool, path, sha256, size, mime_type, label, kind, created_at
         FROM assistant_artifacts ORDER BY created_at DESC LIMIT 50"
    };

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };

    let map_row = |row: &rusqlite::Row<'_>| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "run_id": row.get::<_, String>(1)?,
            "conversation_id": row.get::<_, String>(2)?,
            "source_tool": row.get::<_, String>(3)?,
            "path": row.get::<_, String>(4)?,
            "sha256": row.get::<_, String>(5)?,
            "size": row.get::<_, i64>(6)?,
            "mime_type": row.get::<_, String>(7)?,
            "label": row.get::<_, Option<String>>(8)?,
            "kind": row.get::<_, String>(9)?,
            "created_at": row.get::<_, String>(10)?,
        }))
    };
    let artifacts: Vec<serde_json::Value> = if let Some(rid) = run_id {
        match stmt.query_map(rusqlite::params![rid], map_row) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
        }
    } else if let Some(cid) = conversation_id {
        match stmt.query_map(rusqlite::params![cid], map_row) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
        }
    } else {
        match stmt.query_map([], map_row) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
        }
    };

    success_response(request, serde_json::json!({ "artifacts": artifacts }))
}

async fn handle_workspace_inspect(request: &RpcRequest) -> RpcResponse {
    // NOTE: Git branch/dirty state is intentionally NOT probed here.
    // The single authoritative Git implementation lives in `src-tauri/src/git.rs`
    // and is exposed to the renderer through `window.nativesAPI.git.*`. The daemon
    // must not run a parallel Git workflow (Assistant Workspace Integration Design, Section 1).
    // This RPC only reports filesystem-level workspace identity; branch/dirty come
    // from the shared Tauri Git adapter on the consumer side.
    let project_path = match request.params.get("project_path").and_then(|v| v.as_str()) {
        Some(path) if !path.trim().is_empty() => path,
        _ => return error_response(request, "INVALID_INPUT", "Missing 'project_path' parameter"),
    };
    let canonical = match std::fs::canonicalize(project_path) {
        Ok(path) if path.is_dir() => path,
        _ => return error_response(request, "NOT_FOUND", "Project directory not found"),
    };
    success_response(
        request,
        serde_json::json!({
            "project_path": canonical.to_string_lossy(),
            "repository_root": canonical.to_string_lossy(),
        }),
    )
}

async fn handle_artifact_open(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let id = match request.params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(request, "INVALID_INPUT", "Missing 'id' parameter"),
    };

    let conn = data_store.conn();
    match conn.query_row(
        "SELECT path, mime_type FROM assistant_artifacts WHERE id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(serde_json::json!({
                "path": row.get::<_, String>(0)?,
                "mime_type": row.get::<_, String>(1)?,
            }))
        },
    ) {
        Ok(artifact) => success_response(request, artifact),
        Err(_) => error_response(request, "NOT_FOUND", "Artifact not found"),
    }
}

async fn handle_context_preview(request: &RpcRequest, _data_store: &Arc<DataStore>) -> RpcResponse {
    error_response(
        request,
        "METHOD_NOT_FOUND",
        "Context preview is unavailable until real token accounting is connected",
    )
}

// ─── Project Handlers ─────────────────────────────────────

async fn handle_project_register(request: &RpcRequest) -> RpcResponse {
    let path_str = match request
        .params
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(p) => p,
        None => {
            return error_response(request, "PROJECT_PATH_REQUIRED", "Project path is required")
        }
    };

    let path = std::path::Path::new(path_str);
    if !path.exists() {
        return error_response(
            request,
            "PROJECT_NOT_FOUND",
            &format!("Project directory not found: {path_str}"),
        );
    }
    let canonical = match std::fs::canonicalize(path) {
        Ok(c) if c.is_dir() => c,
        Ok(_) => {
            return error_response(
                request,
                "PROJECT_NOT_DIRECTORY",
                &format!("Not a directory: {path_str}"),
            )
        }
        Err(_) => {
            return error_response(
                request,
                "PROJECT_NOT_FOUND",
                &format!("Cannot resolve path: {path_str}"),
            )
        }
    };

    let canonical_str = canonical.to_string_lossy().to_string();

    success_response(
        request,
        serde_json::json!({
            "canonical_path": canonical_str,
            "name": canonical.file_name().and_then(|n| n.to_str()).unwrap_or("Project"),
        }),
    )
}

async fn handle_project_list(request: &RpcRequest, data_store: &Arc<DataStore>) -> RpcResponse {
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT project_id FROM assistant_conversations WHERE project_id IS NOT NULL AND project_id != ''"
    ) {
        Ok(s) => s,
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };

    let paths: Vec<String> = match stmt.query_map([], |row| row.get::<_, String>(0)) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(e) => return error_response(request, "INTERNAL_ERROR", &e.to_string()),
    };

    success_response(request, serde_json::json!({ "projects": paths }))
}

// ─── Response Helpers ───────────────────────────────────────

fn success_response(request: &RpcRequest, data: serde_json::Value) -> RpcResponse {
    RpcResponse {
        protocol_version: request.protocol_version.clone(),
        request_id: request.request_id.clone(),
        success: true,
        data: Some(data),
        error: None,
    }
}

fn error_response(request: &RpcRequest, code: &str, message: &str) -> RpcResponse {
    let category = match code {
        "INVALID_INPUT" => ErrorCategory::Validation,
        "NOT_FOUND" => ErrorCategory::NotFound,
        "UNAUTHORIZED" => ErrorCategory::Auth,
        "PERMISSION_DENIED" => ErrorCategory::PermissionDenied,
        "RATE_LIMITED" => ErrorCategory::RateLimited,
        "TIMEOUT" => ErrorCategory::Timeout,
        "METHOD_NOT_FOUND" | "METHOD_UNSUPPORTED" => ErrorCategory::Unsupported,
        "PROJECT_PATH_REQUIRED"
        | "PROJECT_NOT_FOUND"
        | "PROJECT_NOT_DIRECTORY"
        | "PROJECT_REGISTER_FAILED" => ErrorCategory::Validation,
        _ => ErrorCategory::Internal,
    };

    let retryable = matches!(
        code,
        "INTERNAL_ERROR" | "TIMEOUT" | "RATE_LIMITED" | "PROJECT_REGISTER_FAILED"
    );

    let daemon_error =
        DaemonError::new(code, category, retryable, message).with_recovery(match code {
            "INVALID_INPUT" => vec!["Check the request parameters and try again".to_string()],
            "NOT_FOUND" => vec!["Verify the resource ID is correct".to_string()],
            "INTERNAL_ERROR" => {
                vec!["Retry the request. If the problem persists, restart the daemon.".to_string()]
            }
            "UNAUTHORIZED" => vec!["Re-authenticate by restarting the client".to_string()],
            "METHOD_NOT_FOUND" | "METHOD_UNSUPPORTED" => {
                vec!["Check the API version compatibility".to_string()]
            }
            "PROJECT_PATH_REQUIRED" => vec!["Provide a valid project folder path".to_string()],
            "PROJECT_NOT_FOUND" => {
                vec!["The project folder may have been moved or deleted".to_string()]
            }
            "PROJECT_NOT_DIRECTORY" => vec!["Select a folder, not a file".to_string()],
            "PROJECT_REGISTER_FAILED" => vec!["Retry registering the project".to_string()],
            _ => vec!["Retry the request".to_string()],
        });

    RpcResponse {
        protocol_version: request.protocol_version.clone(),
        request_id: request.request_id.clone(),
        success: false,
        data: None,
        error: Some(serde_json::to_value(&daemon_error).unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::data::DataStore;
    use crate::daemon::event_bus::EventBus;
    use std::sync::Arc;

    fn setup() -> (Arc<DataStore>, Arc<EventBus>) {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        // Ensure legacy table exists before migration v7 references it
        store
            .conn()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS user_providers (
                id TEXT PRIMARY KEY, name TEXT, preset_name TEXT,
                base_url TEXT, website_url TEXT, api_key TEXT,
                default_model TEXT, is_primary INTEGER, is_active INTEGER,
                created_at TEXT, updated_at TEXT
            );",
            )
            .ok();
        store.run_migrations().unwrap();
        let bus = Arc::new(EventBus::new(store.clone()));
        (store, bus)
    }

    fn provider_request(method: &str, params: serde_json::Value) -> RpcRequest {
        RpcRequest {
            protocol_version: "0.1.0".to_string(),
            request_id: Uuid::new_v4().to_string(),
            client_id: "provider-model-test".to_string(),
            session_token: "test".to_string(),
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn uds_mode_forwards_implemented_methods_to_agent_daemon_before_legacy_handlers() {
        let previous_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "uds");

        assert!(production_daemon_owned_method("mcp.list"));
        assert!(production_daemon_owned_method("run.start"));
        assert!(production_daemon_owned_method("provider.test"));
        assert!(
            !production_daemon_owned_method("provider.list"),
            "provider.list must remain host-owned"
        );
        assert!(!production_daemon_owned_method("provider.create"));

        if let Some(value) = previous_mode {
            std::env::set_var("NATIVES_DAEMON_MODE", value);
        } else {
            std::env::remove_var("NATIVES_DAEMON_MODE");
        }
    }

    #[tokio::test]
    async fn provider_models_are_persisted_and_listed_from_discovery_cache() {
        let (store, _) = setup();
        let create = provider_request(
            "provider.create",
            serde_json::json!({
                "id": "provider-1",
                "provider_type": "openai_compatible",
                "display_name": "Custom",
                "api_base_url": "https://example.com/v1",
                "default_model": "model-b",
                "models": [
                    { "id": "model-b" },
                    { "id": "" },
                    { "id": "model-a", "display_name": "Model A" },
                    { "id": "model-a" }
                ]
            }),
        );

        let created = handle_provider_create(&create, &store).await;
        assert!(
            created.success,
            "provider create failed: {:?}",
            created.error
        );

        store
            .conn()
            .execute(
                "INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
             VALUES ('key-1', 'provider-1', 'encrypted', '***', 'API Key', 1, 'now')",
                [],
            )
            .unwrap();

        let listed = handle_provider_list(
            &provider_request("provider.list", serde_json::json!({})),
            &store,
        )
        .await;
        assert!(listed.success, "provider list failed: {:?}", listed.error);
        let provider = &listed.data.unwrap()["providers"][0];
        assert_eq!(provider["default_model"], "model-b");
        assert_eq!(provider["has_active_key"], true);
        assert_eq!(
            provider["models"],
            serde_json::json!([
                {
                    "id": "model-a", "display_name": "Model A", "capabilities": {},
                    "context_window": 0, "max_output": 0, "source": "api_discovery",
                    "discovered_at": provider["models"][0]["discovered_at"]
                },
                {
                    "id": "model-b", "display_name": null, "capabilities": {},
                    "context_window": 0, "max_output": 0, "source": "api_discovery",
                    "discovered_at": provider["models"][1]["discovered_at"]
                }
            ])
        );

        let deleted = handle_provider_delete(
            &provider_request(
                "provider.delete",
                serde_json::json!({
                    "id": "provider-1",
                    "force": true
                }),
            ),
            &store,
        )
        .await;
        assert!(
            deleted.success,
            "provider delete failed: {:?}",
            deleted.error
        );
        let cached_models: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assistant_model_cache WHERE provider_id = 'provider-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cached_models, 0);
    }

    #[tokio::test]
    async fn conversation_project_list_returns_all_projects_and_create_echoes_project_id() {
        let (store, _) = setup();
        store.conn().execute_batch(
            "INSERT INTO assistant_provider_configs
             (id, provider_type, display_name, api_base_url, health_status, created_at, updated_at)
             VALUES ('provider', 'openai_compatible', 'Provider', 'https://example.com/v1', 'healthy', 'now', 'now');
             INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
             VALUES ('key-project', 'provider', 'encrypted', '***', 'API Key', 1, 'now');
             INSERT INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES ('cache-project', 'provider', 'model', 'Model', '{}', 0, 0, 'api_discovery', 'now');",
        ).unwrap();
        let now = "2026-07-12T12:00:00Z";
        for index in 0..51 {
            let project = if index % 2 == 0 {
                "/work/alpha"
            } else {
                "/work/beta"
            };
            store
                .conn()
                .execute(
                    "INSERT INTO assistant_conversations
                 (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
                 VALUES (?1, 'chat', ?2, ?3, 'provider', 'model', ?4, ?4)",
                    rusqlite::params![
                        format!("project-{index}"),
                        project,
                        format!("Conversation {index}"),
                        now
                    ],
                )
                .unwrap();
        }
        store
            .conn()
            .execute(
                "INSERT INTO assistant_conversations
             (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
             VALUES ('unassigned', 'chat', NULL, 'Unassigned', 'provider', 'model', ?1, ?1)",
                rusqlite::params![now],
            )
            .unwrap();

        let listed = handle_conversation_list(
            &provider_request(
                "conversation.list",
                serde_json::json!({ "include_archived": false }),
            ),
            &store,
        )
        .await;
        assert!(
            listed.success,
            "conversation list failed: {:?}",
            listed.error
        );
        let conversations = listed.data.unwrap()["conversations"]
            .as_array()
            .unwrap()
            .to_vec();
        assert_eq!(conversations.len(), 52);
        assert!(conversations
            .iter()
            .any(|conversation| conversation["project_id"] == "/work/alpha"));
        assert!(conversations
            .iter()
            .any(|conversation| conversation["project_id"] == "/work/beta"));
        assert!(conversations
            .iter()
            .any(|conversation| conversation["project_id"] == ""));

        let created = handle_conversation_create(
            &provider_request(
                "conversation.create",
                serde_json::json!({
                    "mode": "agent",
                    "title": "Project agent",
                    "provider_id": "provider",
                    "model_id": "model",
                    "project_id": "/tmp"
                }),
            ),
            &store,
        )
        .await;
        assert!(
            created.success,
            "conversation create failed: {:?}",
            created.error
        );
        assert_eq!(created.data.unwrap()["project_id"], "/tmp");
    }

    #[tokio::test]
    async fn conversation_create_requires_project_and_rejects_unavailable_pair() {
        let (store, _) = setup();
        // Create provider and model
        store.conn().execute_batch(
            "INSERT INTO assistant_provider_configs
             (id, provider_type, display_name, api_base_url, health_status, created_at, updated_at)
             VALUES ('provider', 'openai_compatible', 'Provider', 'https://example.com/v1', 'healthy', 'now', 'now');
             INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
             VALUES ('key-null', 'provider', 'encrypted', '***', 'API Key', 1, 'now');
             INSERT INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES ('cache-null', 'provider', 'model', 'Model', '{}', 0, 0, 'api_discovery', 'now');",
        ).unwrap();

        // Missing, empty, and null project IDs must never create an unassigned conversation.
        let no_project = handle_conversation_create(
            &provider_request(
                "conversation.create",
                serde_json::json!({
                    "mode": "chat", "title": "No Project", "provider_id": "provider",
                    "model_id": "model"
                }),
            ),
            &store,
        )
        .await;
        assert!(!no_project.success);

        let empty_project = handle_conversation_create(
            &provider_request(
                "conversation.create",
                serde_json::json!({
                    "mode": "chat", "title": "Empty Project", "provider_id": "provider",
                    "model_id": "model", "project_id": ""
                }),
            ),
            &store,
        )
        .await;
        assert!(!empty_project.success);

        let null_project = handle_conversation_create(
            &provider_request(
                "conversation.create",
                serde_json::json!({
                    "mode": "chat", "title": "Null Project", "provider_id": "provider",
                    "model_id": "model", "project_id": null
                }),
            ),
            &store,
        )
        .await;
        assert!(!null_project.success);

        // Unavailable model pair should still fail
        let unavailable_pair = handle_conversation_create(
            &provider_request(
                "conversation.create",
                serde_json::json!({
                    "mode": "chat", "title": "Invalid", "provider_id": "provider",
                    "model_id": "invented", "project_id": "/tmp"
                }),
            ),
            &store,
        )
        .await;
        assert!(!unavailable_pair.success);

        // Invalid requests never reach storage.
        assert_eq!(
            store
                .conn()
                .query_row("SELECT COUNT(*) FROM assistant_conversations", [], |row| {
                    row.get::<_, i64>(0)
                },)
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn project_register_returns_canonical_path() {
        let (store, bus) = setup();
        // /tmp exists on all Unix systems — use it as a valid directory
        let result = handle_project_register(&provider_request(
            "project.register",
            serde_json::json!({ "path": "/tmp" }),
        ))
        .await;
        assert!(
            result.success,
            "project.register should succeed: {:?}",
            result.error
        );
        let canonical = result.data.as_ref().unwrap()["canonical_path"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(canonical.ends_with("/tmp") || canonical.ends_with("/private/tmp"));

        // Missing path should fail
        let missing = handle_project_register(&provider_request(
            "project.register",
            serde_json::json!({ "path": "/definitely/does/not/exist" }),
        ))
        .await;
        assert!(!missing.success);
        assert!(missing.error.as_ref().unwrap()["code"]
            .as_str()
            .unwrap()
            .contains("PROJECT_NOT_FOUND"));

        // Empty path should fail
        let empty =
            handle_project_register(&provider_request("project.register", serde_json::json!({})))
                .await;
        assert!(!empty.success);

        // File path should fail (not a directory)
        let file_path = handle_project_register(&provider_request(
            "project.register",
            serde_json::json!({ "path": "/tmp" }),
        ))
        .await;
        // /tmp is a dir so this succeeds; we test with a known file via /dev/null
        let dev_null = handle_project_register(&provider_request(
            "project.register",
            serde_json::json!({ "path": "/dev/null" }),
        ))
        .await;
        // /dev/null is a file, not a directory
        if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
            // /dev/null exists but is not a directory
            let code = dev_null.error.as_ref().unwrap()["code"].as_str().unwrap();
            assert!(
                code.contains("PROJECT_NOT_DIRECTORY") || code.contains("PROJECT_NOT_FOUND"),
                "Expected directory error for /dev/null, got: {code}"
            );
            assert!(!dev_null.success);
        }
    }

    #[tokio::test]
    async fn project_list_returns_unique_project_paths() {
        let (store, _) = setup();
        store.conn().execute_batch(
            "INSERT INTO assistant_provider_configs
             (id, provider_type, display_name, api_base_url, health_status, created_at, updated_at)
             VALUES ('provider-pl', 'openai_compatible', 'Provider', 'https://example.com/v1', 'healthy', 'now', 'now');
             INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
             VALUES ('key-pl', 'provider-pl', 'encrypted', '***', 'API Key', 1, 'now');
             INSERT INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES ('cache-pl', 'provider-pl', 'model', 'Model', '{}', 0, 0, 'api_discovery', 'now');
             INSERT INTO assistant_conversations
             (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
             VALUES ('c1', 'chat', '/work/alpha', 'A', 'provider-pl', 'model', 'now', 'now'),
                    ('c2', 'chat', '/work/beta', 'B', 'provider-pl', 'model', 'now', 'now'),
                    ('c3', 'chat', '/work/alpha', 'A2', 'provider-pl', 'model', 'now', 'now'),
                    ('c4', 'chat', NULL, 'Null', 'provider-pl', 'model', 'now', 'now');",
        ).unwrap();

        let result = handle_project_list(
            &provider_request("project.list", serde_json::json!({})),
            &store,
        )
        .await;
        assert!(
            result.success,
            "project.list should succeed: {:?}",
            result.error
        );
        let projects = result.data.unwrap()["projects"]
            .as_array()
            .unwrap()
            .to_vec();
        assert_eq!(projects.len(), 2);
        let paths: Vec<&str> = projects.iter().map(|p| p.as_str().unwrap()).collect();
        assert!(paths.contains(&"/work/alpha"));
        assert!(paths.contains(&"/work/beta"));
    }

    #[tokio::test]
    async fn assistant_workspace_rpc_restores_runs_blocks_artifacts_and_environment() {
        let (store, _) = setup();
        store.conn().execute_batch(
            "INSERT INTO assistant_conversations
             (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
             VALUES ('conv-workspace', 'agent', '/tmp', 'Workspace', 'provider', 'model', '2026-07-12T00:00:00Z', '2026-07-12T00:00:00Z');
             INSERT INTO assistant_runs
             (id, conversation_id, status, provider_id, model_id, started_at, finished_at, step_count)
             VALUES ('run-old', 'conv-workspace', 'completed', 'provider', 'model', '2026-07-12T00:00:00Z', '2026-07-12T00:00:01Z', 1),
                    ('run-new', 'conv-workspace', 'interrupted', 'provider', 'model', '2026-07-12T00:01:00Z', '2026-07-12T00:01:02Z', 2);
             INSERT INTO assistant_messages
             (id, conversation_id, role, status, reasoning_tokens, created_at)
             VALUES ('message-reasoning', 'conv-workspace', 'assistant', 'complete', 17, '2026-07-12T00:01:01Z');
             INSERT INTO assistant_message_blocks
             (id, message_id, block_index, block_type, content)
             VALUES ('block-reasoning', 'message-reasoning', 0, 'reasoning', '{\"text\":\"真实思考\"}');
             INSERT INTO assistant_artifacts
             (id, run_id, conversation_id, source_tool, path, sha256, size, mime_type, label, kind, created_at)
             VALUES ('artifact-1', 'run-new', 'conv-workspace', 'write_file', '/tmp/a.ts', 'hash', 12, 'text/plain', 'a.ts', 'file', '2026-07-12T00:01:02Z');"
        ).unwrap();

        let runs = handle_run_list(
            &provider_request(
                "run.list",
                serde_json::json!({ "conversation_id": "conv-workspace", "limit": 1 }),
            ),
            &store,
        )
        .await;
        assert!(runs.success, "run.list failed: {:?}", runs.error);
        let runs_data = runs.data.unwrap();
        let listed_runs = runs_data["runs"].as_array().unwrap();
        assert_eq!(listed_runs.len(), 1);
        assert_eq!(listed_runs[0]["id"], "run-new");

        let messages = handle_conversation_get_messages(
            &provider_request(
                "conversation.getMessages",
                serde_json::json!({ "conversation_id": "conv-workspace" }),
            ),
            &store,
        )
        .await;
        let messages_data = messages.data.unwrap();
        let message = &messages_data["messages"][0];
        assert_eq!(message["reasoning_tokens"], 17);
        assert_eq!(message["content_blocks"][0]["type"], "reasoning");
        assert_eq!(message["content_blocks"][0]["content"]["text"], "真实思考");

        let artifacts = handle_artifact_list(
            &provider_request(
                "artifact.list",
                serde_json::json!({ "conversation_id": "conv-workspace" }),
            ),
            &store,
        )
        .await;
        assert_eq!(
            artifacts.data.unwrap()["artifacts"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        let environment = handle_workspace_inspect(&provider_request(
            "workspace.inspect",
            serde_json::json!({ "project_path": "/tmp" }),
        ))
        .await;
        assert!(
            environment.success,
            "workspace.inspect failed: {:?}",
            environment.error
        );
        assert_eq!(
            environment.data.unwrap()["project_path"],
            std::fs::canonicalize("/tmp")
                .unwrap()
                .to_string_lossy()
                .as_ref(),
        );
    }

    #[tokio::test]
    async fn conversation_model_update_persists_only_a_real_configured_pair() {
        let (store, _) = setup();
        store.conn().execute_batch(
            "INSERT INTO assistant_provider_configs
             (id, provider_type, display_name, api_base_url, health_status, created_at, updated_at)
             VALUES ('provider-real', 'openai_compatible', 'Real', 'https://example.com/v1', 'healthy', 'now', 'now');
             INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
             VALUES ('key-real', 'provider-real', 'encrypted', '***', 'API Key', 1, 'now');
             INSERT INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES ('cache-real', 'provider-real', 'model-real', 'Model Real', '{}', 0, 0, 'api_discovery', 'now');
             INSERT INTO assistant_conversations
             (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
             VALUES ('conv-model', 'chat', '/tmp', 'Test', 'provider-real', 'model-real', 'now', 'now');",
        ).unwrap();

        let update = provider_request(
            "conversation.update_model",
            serde_json::json!({
                "id": "conv-model",
                "provider_id": "provider-real",
                "model_id": "model-real"
            }),
        );
        let response = handle_conversation_update_model(&update, &store).await;
        assert!(
            response.success,
            "model update failed: {:?}",
            response.error
        );

        let pair = store
            .conn()
            .query_row(
                "SELECT provider_id, model_id FROM assistant_conversations WHERE id='conv-model'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(
            pair,
            ("provider-real".to_string(), "model-real".to_string())
        );

        let invalid = provider_request(
            "conversation.update_model",
            serde_json::json!({
                "id": "conv-model",
                "provider_id": "provider-real",
                "model_id": "invented-model"
            }),
        );
        assert!(
            !handle_conversation_update_model(&invalid, &store)
                .await
                .success
        );
    }

    #[tokio::test]
    async fn run_start_rejects_missing_provider_and_model_instead_of_inventing_defaults() {
        let (store, bus) = setup();
        store
            .conn()
            .execute(
                "INSERT INTO assistant_conversations
             (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
             VALUES ('conv-no-default', 'chat', '/tmp', 'Test', '', '', 'now', 'now')",
                [],
            )
            .unwrap();

        let response = handle_run_start(
            &provider_request(
                "run.start",
                serde_json::json!({
                    "conversation_id": "conv-no-default",
                    "content": "hello"
                }),
            ),
            &store,
            &bus,
        )
        .await;

        assert!(!response.success);
        assert_eq!(store.conn().query_row(
            "SELECT COUNT(*) FROM assistant_messages WHERE conversation_id='conv-no-default'",
            [],
            |row| row.get::<_, i64>(0),
        ).unwrap(), 0);
    }

    #[tokio::test]
    async fn retry_run_reuses_trigger_message_without_inserting_a_duplicate_user_message() {
        let (store, bus) = setup();
        store.conn().execute_batch(
            "INSERT INTO assistant_provider_configs
             (id, provider_type, display_name, api_base_url, health_status, created_at, updated_at)
             VALUES ('provider-real', 'openai_compatible', 'Real', 'https://example.com/v1', 'healthy', 'now', 'now');
             INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
             VALUES ('key-real', 'provider-real', 'encrypted', '***', 'API Key', 1, 'now');
             INSERT INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES ('cache-retry', 'provider-real', 'model-real', 'Model Real', '{}', 0, 0, 'api_discovery', 'now');
             INSERT INTO assistant_conversations
             (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
             VALUES ('conv-retry', 'chat', '/tmp', 'Retry', 'provider-real', 'model-real', '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z');
             INSERT INTO assistant_messages
             (id, conversation_id, role, status, created_at)
             VALUES ('message-original', 'conv-retry', 'user', 'complete', 'now');
             INSERT INTO assistant_message_blocks
             (id, message_id, block_type, block_index, content)
             VALUES ('block-original', 'message-original', 'text', 0, '{\"text\":\"retry me\"}');",
        ).unwrap();

        let response = handle_run_start(
            &provider_request(
                "run.start",
                serde_json::json!({
                    "conversation_id": "conv-retry",
                    "provider_id": "provider-real",
                    "model_id": "model-real",
                    "trigger_message_id": "message-original"
                }),
            ),
            &store,
            &bus,
        )
        .await;
        assert!(response.success, "retry start failed: {:?}", response.error);

        assert_eq!(store.conn().query_row(
            "SELECT COUNT(*) FROM assistant_messages WHERE conversation_id='conv-retry' AND role='user'",
            [],
            |row| row.get::<_, i64>(0),
        ).unwrap(), 1);
        assert_eq!(
            response.data.unwrap()["trigger_message_id"],
            "message-original"
        );
        assert_ne!(
            store
                .conn()
                .query_row(
                    "SELECT updated_at FROM assistant_conversations WHERE id='conv-retry'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "2000-01-01T00:00:00Z"
        );
    }

    #[tokio::test]
    async fn context_preview_never_returns_fabricated_token_sections() {
        let (store, _) = setup();
        let response = handle_context_preview(
            &provider_request(
                "context.preview",
                serde_json::json!({
                    "conversation_id": "missing"
                }),
            ),
            &store,
        )
        .await;

        assert!(!response.success);
        assert!(response.data.is_none());
    }

    #[tokio::test]
    async fn run_cancel_publishes_an_interrupted_event_for_the_active_run() {
        let (store, bus) = setup();
        store
            .conn()
            .execute_batch(
                "INSERT INTO assistant_conversations
             (id, mode, project_id, title, provider_id, model_id, created_at, updated_at)
             VALUES ('conv-cancel', 'chat', '/tmp', 'Cancel', 'provider', 'model', 'now', 'now');
             INSERT INTO assistant_runs
             (id, conversation_id, status, provider_id, model_id, started_at)
             VALUES ('run-cancel', 'conv-cancel', 'running', 'provider', 'model', 'now');",
            )
            .unwrap();

        let response = handle_run_cancel(
            &provider_request("run.cancel", serde_json::json!({ "id": "run-cancel" })),
            &store,
            &bus,
        )
        .await;
        assert!(response.success, "cancel failed: {:?}", response.error);
        let events = bus.replay("run-cancel", 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "interrupted");
        assert_eq!(events[0].payload["reason"], "cancelled");
    }

    #[tokio::test]
    async fn test_handshake_rejects_bad_token() {
        let sock_path = format!(
            "/tmp/test_natives_rpc_bad_token_{}.sock",
            std::process::id()
        );
        let _ = std::fs::remove_file(&sock_path);

        let (store, bus) = setup();
        let server = RpcServer::new(&sock_path, "correct-token", "0.1.0", "1.0.0", store, bus);

        let server_handle = server.start().await.unwrap();

        // Small delay to let server start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Connect as client
        let stream = tokio::net::UnixStream::connect(&sock_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(reader);

        // Send bad handshake
        let bad_handshake = HandshakeRequest {
            client_version: "0.1.0".to_string(),
            client_id: "test-client".to_string(),
            bootstrap_token: "wrong-token".to_string(),
        };
        let mut json = serde_json::to_string(&bad_handshake).unwrap();
        json.push('\n');
        writer.write_all(json.as_bytes()).await.unwrap();

        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let response: HandshakeResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(!response.accepted);

        server_handle.abort();
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn test_handshake_success() {
        let sock_path = format!("/tmp/test_natives_rpc_success_{}.sock", std::process::id());
        let _ = std::fs::remove_file(&sock_path);

        let (store, bus) = setup();
        let server = RpcServer::new(&sock_path, "correct-token", "0.1.0", "1.0.0", store, bus);

        let server_handle = server.start().await.unwrap();

        // Small delay to let server start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let stream = tokio::net::UnixStream::connect(&sock_path).await;
        if let Ok(stream) = stream {
            let (reader, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(reader);

            let handshake = HandshakeRequest {
                client_version: "0.1.0".to_string(),
                client_id: "test-client".to_string(),
                bootstrap_token: "correct-token".to_string(),
            };
            let mut json = serde_json::to_string(&handshake).unwrap();
            json.push('\n');
            writer.write_all(json.as_bytes()).await.unwrap();

            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let response: HandshakeResponse = serde_json::from_str(line.trim()).unwrap();
            assert!(response.accepted);
            assert!(!response.session_token.is_empty());
        }

        server_handle.abort();
        let _ = std::fs::remove_file(&sock_path);
    }
}
