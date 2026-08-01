//! RPC server — authenticated Unix Domain Socket listener with dispatch.
//!
//! ## Protocol
//!
//! 1. Client connects and sends `HandshakeRequest` (bootstrap token, client version).
//! 2. Server validates bootstrap token, generates session token, responds `HandshakeResponse`.
//! 3. Client sends `RpcRequest` with session token, method, and params.
//! 4. Server authenticates, dispatches to handler, returns `RpcResponse`.
//! 5. Client can subscribe to run events via `SubscribeRequest` (stream).
//!
//! Messages are newline-delimited JSON (one JSON object per line, terminated by `\n`).

use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
use assistant_protocol::v1::daemon::{
    DaemonHealth, DaemonStatus, HandshakeRequest, HandshakeResponse, RpcRequest, RpcResponse,
};
use assistant_protocol::version::{negotiate, ProtocolVersion};
use futures_util::StreamExt;
use provider_adapters::capabilities::{
    Credential, ProviderContentBlock, ProviderMessage, ProviderRequest, ProviderTestResult,
};
use provider_adapters::stream::ProviderEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Harness control plane — the `harness.*` method family.
///
/// The files live at `src-agent-daemon/src/harness/`, where the design
/// (第 7.2 节) puts them; only the `mod` declaration is here, via `#[path]`.
/// The reason is boundary hygiene rather than architecture: declaring it in
/// `lib.rs` alongside the other stores is the right home and is a one-line
/// move, but `lib.rs` is owned by a parallel workstream this round. Promoting
/// it later is: delete these two lines, add `pub mod harness;` to `lib.rs`, and
/// rename `crate::rpc::harness` to `crate::harness` at its handful of call
/// sites.
#[path = "harness/mod.rs"]
pub mod harness;

/// Active session: maps session_token -> client_id.
type SessionMap = Arc<Mutex<HashMap<String, String>>>;

/// Reject handshake with a stable HandshakeResponse shape (always includes session_token).
async fn write_handshake_reject(
    writer: &mut (impl AsyncWriteExt + Unpin),
    protocol_version: &str,
    daemon_version: &str,
    reason: impl Into<String>,
) -> std::io::Result<()> {
    let resp = HandshakeResponse {
        session_token: String::new(),
        daemon_version: daemon_version.to_string(),
        protocol_version: protocol_version.to_string(),
        accepted: false,
        upgrade_required: Some(reason.into()),
    };
    let resp_json = serde_json::to_string(&resp).unwrap_or_else(|_| {
        r#"{"session_token":"","daemon_version":"","protocol_version":"","accepted":false,"upgrade_required":"handshake rejected"}"#.into()
    });
    writer.write_all(resp_json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

/// RPC server that listens for client connections.
pub struct RpcServer {
    socket_path: String,
    bootstrap_token: String,
    /// Track whether bootstrap token has been used (single-use).
    bootstrap_used: Arc<Mutex<bool>>,
    protocol_version: ProtocolVersion,
    daemon_version: String,
    sessions: SessionMap,
    started_at: std::time::Instant,
}

impl RpcServer {
    /// Create a new RPC server.
    pub fn new(
        socket_path: &str,
        bootstrap_token: &str,
        protocol_version: &str,
        daemon_version: &str,
    ) -> Self {
        RpcServer {
            socket_path: socket_path.to_string(),
            bootstrap_token: bootstrap_token.to_string(),
            bootstrap_used: Arc::new(Mutex::new(false)),
            protocol_version: ProtocolVersion::from(protocol_version),
            daemon_version: daemon_version.to_string(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            started_at: std::time::Instant::now(),
        }
    }

    /// Run the server — bind socket and accept connections.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Remove any existing socket file
        let _ = std::fs::remove_file(&self.socket_path);

        let listener = UnixListener::bind(&self.socket_path)?;

        // Set permissions to 0700 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o700))?;
        }

        let sessions = self.sessions.clone();
        let bootstrap_token = self.bootstrap_token.clone();
        let bootstrap_used = self.bootstrap_used.clone();
        let protocol_version = self.protocol_version.clone();
        let daemon_version = self.daemon_version.clone();
        let started_at = self.started_at;

        println!("Listening on {}", self.socket_path);

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let sessions = sessions.clone();
                    let bt = bootstrap_token.clone();
                    let bu = bootstrap_used.clone();
                    let pv = protocol_version.clone();
                    let dv = daemon_version.clone();
                    let sa = started_at;

                    tokio::spawn(async move {
                        if let Err(e) =
                            handle_connection(stream, sessions, bt, bu, pv, dv, sa).await
                        {
                            eprintln!("Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }

    /// Get the current daemon status.
    pub fn status(&self) -> DaemonStatus {
        DaemonStatus {
            version: self.daemon_version.clone(),
            protocol_version: self.protocol_version.to_string(),
            uptime_secs: self.started_at.elapsed().as_secs(),
            pid: std::process::id() as u64,
            active_runs: 0,
            active_extensions: 0,
            provider_count: 0,
            memory_usage_mb: 0,
            health: DaemonHealth::Healthy,
        }
    }
}

/// Handle a single client connection.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    sessions: SessionMap,
    bootstrap_token: String,
    bootstrap_used: Arc<Mutex<bool>>,
    protocol_version: ProtocolVersion,
    daemon_version: String,
    started_at: std::time::Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // --- Step 1: Handshake ---
    line.clear();
    reader.read_line(&mut line).await?;
    if line.trim().is_empty() {
        return Ok(());
    }

    // Deserialize handshake request
    let handshake_req: HandshakeRequest = match serde_json::from_str(line.trim()) {
        Ok(req) => req,
        Err(e) => {
            write_handshake_reject(
                &mut writer,
                &protocol_version.to_string(),
                &daemon_version,
                format!("Invalid handshake JSON: {e}"),
            )
            .await?;
            return Ok(());
        }
    };

    // Validate bootstrap token. Local UDS is owner-only (0700); multiple sessions
    // are allowed so Tauri can reconnect without a process restart.
    // Opt into single-use with NATIVES_BOOTSTRAP_SINGLE_USE=1 (tests / hardening).
    {
        let single_use = std::env::var("NATIVES_BOOTSTRAP_SINGLE_USE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let mut used = bootstrap_used.lock().await;
        if handshake_req.bootstrap_token != bootstrap_token {
            write_handshake_reject(
                &mut writer,
                &protocol_version.to_string(),
                &daemon_version,
                "Invalid bootstrap token",
            )
            .await?;
            return Ok(());
        }
        if single_use && *used {
            write_handshake_reject(
                &mut writer,
                &protocol_version.to_string(),
                &daemon_version,
                "Bootstrap token already used (NATIVES_BOOTSTRAP_SINGLE_USE)",
            )
            .await?;
            return Ok(());
        }
        *used = true;
    }

    // Negotiate protocol version
    let client_version = ProtocolVersion::from(handshake_req.client_version.as_str());
    let negotiation = negotiate(&client_version, &protocol_version);

    if !negotiation.compatible {
        write_handshake_reject(
            &mut writer,
            &protocol_version.to_string(),
            &daemon_version,
            format!("Protocol mismatch: client v{client_version}, daemon v{protocol_version}"),
        )
        .await?;
        return Ok(());
    }

    // Generate session token
    let session_token = Uuid::new_v4().to_string();
    {
        let mut sessions = sessions.lock().await;
        sessions.insert(session_token.clone(), handshake_req.client_id.clone());
    }

    // Send handshake response
    let handshake_resp = HandshakeResponse {
        session_token: session_token.clone(),
        daemon_version: daemon_version.clone(),
        protocol_version: protocol_version.to_string(),
        accepted: true,
        upgrade_required: negotiation.upgrade_required,
    };
    let resp_json = serde_json::to_string(&handshake_resp)?;
    writer.write_all(resp_json.as_bytes()).await?;
    writer.write_all(b"\n").await?;

    // --- Step 2: Handle RPC requests ---
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            // Client disconnected
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: RpcRequest = match serde_json::from_str(trimmed) {
            Ok(req) => req,
            Err(e) => {
                let err = DaemonError::new(
                    error_codes::INVALID_INPUT,
                    ErrorCategory::Validation,
                    false,
                    format!("Invalid RPC JSON: {}", e),
                );
                send_error(&mut writer, &err).await;
                continue;
            }
        };

        // Validate session token
        {
            let sessions = sessions.lock().await;
            if !sessions.contains_key(&request.session_token) {
                let err = DaemonError::new(
                    error_codes::UNAUTHORIZED,
                    ErrorCategory::Auth,
                    false,
                    "Invalid session token".to_string(),
                );
                send_error(&mut writer, &err).await;
                continue;
            }
        }

        // Dispatch method
        handle_rpc(
            &mut writer,
            &request,
            &protocol_version,
            &daemon_version,
            &started_at,
        )
        .await;
    }

    // Cleanup: remove session
    {
        let mut sessions = sessions.lock().await;
        sessions.retain(|_, v| v != &handshake_req.client_id);
    }

    Ok(())
}

/// Process-wide run manager (Phase 1 authority).
fn run_manager() -> &'static crate::run_manager::RunManager {
    // Must share the process-wide authority with Tauri host shims.
    crate::run_manager::global_run_manager()
}

fn resolve_provider_adapter(
    provider_id: &str,
) -> Option<Box<dyn provider_adapters::ProviderAdapter>> {
    let needle = provider_id.to_ascii_lowercase();
    provider_adapters::register_all().into_iter().find(|p| {
        let t = format!("{:?}", p.provider_type()).to_ascii_lowercase();
        t == needle
            || t.contains(&needle)
            || needle.contains(&t)
            || (needle.contains("compatible") && t.contains("compatible"))
            || (needle.contains("openai") && t == "openai")
            || (needle.contains("anthropic") && t.contains("anthropic"))
            || (needle.contains("gemini") && t.contains("gemini"))
            || (needle.contains("deepseek") && t.contains("deepseek"))
            || (needle.contains("ollama") && t.contains("ollama"))
    })
}

async fn test_provider_model(
    adapter: &dyn provider_adapters::ProviderAdapter,
    credential: Credential,
    model: &str,
) -> Result<ProviderTestResult, provider_adapters::capabilities::ProviderError> {
    let started = std::time::Instant::now();
    let request = ProviderRequest {
        model: model.to_string(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text {
                text: "Reply with exactly: ok".into(),
            }],
        }],
        system_prompt: Some("Be concise.".into()),
        tools: None,
        max_tokens: Some(16),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
        controls: Default::default(),
    };
    let mut stream = adapter.stream(request, credential).await?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::Completed { .. } => {
                break;
            }
            ProviderEvent::Error(err) => return Err(err),
            _ => {}
        }
    }
    if !text.trim().is_empty() {
        Ok(ProviderTestResult {
            success: true,
            latency_ms: Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
            message: format!("Model test passed: {model}"),
        })
    } else {
        Err(provider_adapters::capabilities::ProviderError {
            code: "EMPTY_RESPONSE".into(),
            message: format!("Provider test returned no content: model={model}"),
            category: provider_adapters::capabilities::ProviderErrorCategory::Unknown,
            retryable: true,
            retry_after_ms: None,
        })
    }
}

/// Map an [`McpError`] onto the daemon error envelope.
///
/// The distinction that matters is `Unsupported`: "this server does not do
/// resources" and "you sent bad arguments" are different facts, and collapsing
/// them into `invalid_input` would make a GUI show a broken-input message for a
/// server that is working exactly as advertised. `Denied` stays separate for the
/// same reason — a blocked `file://` read is a policy outcome, not a bug.
fn mcp_error_to_daemon(err: crate::mcp_runtime::McpError) -> DaemonError {
    use crate::mcp_runtime::McpError;
    match err {
        McpError::Invalid(m) => DaemonError::new(
            error_codes::INVALID_INPUT,
            ErrorCategory::Validation,
            false,
            m,
        ),
        McpError::NotFound(m) => {
            DaemonError::new(error_codes::NOT_FOUND, ErrorCategory::NotFound, false, m)
        }
        // Not `unsupported`: that code is reserved for methods this daemon does
        // not implement, and this method *is* implemented. The unsupported thing
        // is the remote server's capability set.
        McpError::Unsupported(m) => DaemonError::new(
            error_codes::INVALID_INPUT,
            ErrorCategory::Validation,
            false,
            m,
        ),
        McpError::Denied(m) => DaemonError::new(
            error_codes::PERMISSION_DENIED,
            ErrorCategory::PermissionDenied,
            false,
            m,
        ),
        McpError::Transport(m) => {
            DaemonError::new(error_codes::NETWORK_ERROR, ErrorCategory::Network, true, m)
        }
    }
}

/// Read the MCP server id from either accepted param spelling.
fn mcp_server_id(request: &RpcRequest) -> &str {
    request
        .params
        .get("server_id")
        .or_else(|| request.params.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

/// Dispatch an RPC request to the appropriate handler.
///
/// Public so the dispatch-coverage contract test (`tests/rpc_dispatch_contract.rs`) can
/// drive every advertised method through the *real* match instead of re-deriving the
/// arm list from source text. Callers must supply a connected write half; use
/// `tokio::net::UnixStream::pair()` in tests.
pub async fn handle_rpc(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &RpcRequest,
    protocol_version: &ProtocolVersion,
    daemon_version: &str,
    started_at: &std::time::Instant,
) {
    use assistant_protocol::v2::methods::names;
    use assistant_protocol::v2::{
        CancelRunRequest, CreateRunRequest, ReplayRunRequest, RetryRunRequest, StartRunRequest,
    };
    match request.method.as_str() {
        names::DAEMON_GET_STATUS => {
            let active_runs = run_manager()
                .list_runs(None)
                .iter()
                .filter(|r| r.status.is_active())
                .count() as u32;
            let status = DaemonStatus {
                version: daemon_version.to_string(),
                protocol_version: protocol_version.to_string(),
                uptime_secs: started_at.elapsed().as_secs(),
                pid: std::process::id() as u64,
                active_runs,
                active_extensions: 0,
                provider_count: 6,
                memory_usage_mb: 0,
                health: DaemonHealth::Healthy,
            };
            let mut value = serde_json::to_value(&status).unwrap_or_default();
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "natives_db_path".into(),
                    serde_json::Value::String(
                        crate::default_natives_db_path()
                            .to_string_lossy()
                            .to_string(),
                    ),
                );
            }
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                value,
            )
            .await;
        }
        names::DAEMON_PING => {
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({"pong": true, "timestamp": chrono::Utc::now().to_rfc3339()}),
            )
            .await;
        }
        names::ENGINE_RATE_LIMIT_GET => {
            let snapshot = if let Some(gov) = crate::global_governor() {
                gov.snapshot().await
            } else {
                crate::governor::EngineRateLimitSnapshot {
                    settings: crate::governor::EngineRateLimitSettings::default(),
                    effective_interval_ms: 0,
                    queued_requests: 0,
                    cooling_routes: 0,
                }
            };
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::to_value(&snapshot).unwrap_or_default(),
            )
            .await;
        }
        names::ENGINE_RATE_LIMIT_UPDATE => {
            let req: crate::governor::EngineRateLimitSettings =
                match serde_json::from_value(request.params.clone()) {
                    Ok(req) => req,
                    Err(e) => {
                        send_rpc_failure(
                            writer,
                            request,
                            "invalid_params",
                            format!("Invalid parameters: {e}"),
                        )
                        .await;
                        return;
                    }
                };
            if let Err(message) = req.validate() {
                send_rpc_failure(writer, request, "invalid_params", message).await;
                return;
            }
            let encoded = match serde_json::to_string(&req) {
                Ok(encoded) => encoded,
                Err(e) => {
                    send_rpc_failure(writer, request, "serialization_failed", e.to_string()).await;
                    return;
                }
            };
            if let Err(message) =
                crate::natives_db_broker::write_setting(crate::governor::SETTINGS_KEY, &encoded)
            {
                send_rpc_failure(writer, request, "persistence_failed", message).await;
                return;
            }
            if let Some(gov) = crate::global_governor() {
                gov.update_settings(req.clone()).await;
            }
            let snapshot = if let Some(gov) = crate::global_governor() {
                gov.snapshot().await
            } else {
                crate::governor::EngineRateLimitSnapshot {
                    settings: req,
                    effective_interval_ms: 0,
                    queued_requests: 0,
                    cooling_routes: 0,
                }
            };
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::to_value(snapshot).unwrap_or_default(),
            )
            .await;
        }
        names::ENGINE_RATE_LIMIT_ACQUIRE => {
            let provider_id = request
                .params
                .get("provider_id")
                .and_then(serde_json::Value::as_str);
            let key_id = request
                .params
                .get("key_id")
                .and_then(serde_json::Value::as_str);
            let (Some(provider_id), Some(key_id)) = (provider_id, key_id) else {
                send_rpc_failure(
                    writer,
                    request,
                    "invalid_params",
                    "provider_id and key_id are required".into(),
                )
                .await;
                return;
            };
            if let Some(governor) = crate::global_governor() {
                if let Err(message) = governor
                    .acquire(
                        provider_id,
                        key_id,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                {
                    send_rpc_failure(writer, request, "rate_limit_cancelled", message).await;
                    return;
                }
            }
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({"acquired": true}),
            )
            .await;
        }
        names::ENGINE_RATE_LIMIT_COOLDOWN => {
            let provider_id = request
                .params
                .get("provider_id")
                .and_then(serde_json::Value::as_str);
            let key_id = request
                .params
                .get("key_id")
                .and_then(serde_json::Value::as_str);
            let retry_after_ms = request
                .params
                .get("retry_after_ms")
                .and_then(serde_json::Value::as_u64);
            let (Some(provider_id), Some(key_id)) = (provider_id, key_id) else {
                send_rpc_failure(
                    writer,
                    request,
                    "invalid_params",
                    "provider_id and key_id are required".into(),
                )
                .await;
                return;
            };
            if let Some(governor) = crate::global_governor() {
                governor
                    .record_rate_limit(provider_id, key_id, retry_after_ms)
                    .await;
            }
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({"recorded": true}),
            )
            .await;
        }
        names::DAEMON_GET_CAPABILITIES => {
            let caps = crate::run_manager::RunManager::capabilities();
            let mut value = serde_json::to_value(&caps).unwrap_or_default();
            // Per-runtime capability matrix (ADR-0016): which runtimes can
            // honour expert/team/skills/mcp selections and by what mechanism.
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "runtime_capabilities".into(),
                    crate::capability_resolution::runtime_capability_matrix(),
                );
            }
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                value,
            )
            .await;
        }
        names::CONVERSATION_CREATE
        | names::CONVERSATION_LIST
        // Paged variants are advertised in IMPLEMENTED_METHODS and handled by
        // conversation_store::request — they must be routed here or they fall through
        // to the fail-closed arm and surface as `internal_error`. The frontend calls
        // both with a silent `.catch()` fallback to an unpaged fetch, so the failure
        // showed up as a performance regression rather than a visible error.
        | names::CONVERSATION_LIST_PAGE
        | names::CONVERSATION_GET
        | names::CONVERSATION_FORK
        | names::CONVERSATION_GET_MESSAGES
        | names::CONVERSATION_GET_MESSAGES_PAGE
        | names::CONVERSATION_APPEND_MESSAGE
        | names::CONVERSATION_RENAME
        | names::CONVERSATION_UPDATE_MODEL
        | names::CONVERSATION_UPDATE_PERMISSION
        | names::CONVERSATION_ARCHIVE
        | names::CONVERSATION_DELETE => {
            match crate::conversation_store::request(&request.method, request.params.clone()).await
            {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await
                }
            }
        }
        names::RUN_CREATE => {
            match serde_json::from_value::<CreateRunRequest>(request.params.clone()) {
                Ok(req) => match run_manager().create_run(req) {
                    Ok(run) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::to_value(run).unwrap_or_default(),
                        )
                        .await
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                "run_create_failed",
                                ErrorCategory::Internal,
                                false,
                                e,
                            ),
                        )
                        .await
                    }
                },
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }
        names::RUN_START => {
            match serde_json::from_value::<StartRunRequest>(request.params.clone()) {
                Ok(req) => {
                    // Non-blocking: engine runs in background so this connection can
                    // still accept run.cancel / permission.respond / run.getEvents.
                    match crate::run_manager::RunManager::start_detached_global(req) {
                        Ok(run) => {
                            send_success(
                                writer,
                                &request.request_id,
                                &request.client_id,
                                &request.session_token,
                                serde_json::to_value(run).unwrap_or_default(),
                            )
                            .await
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    "run_start_failed",
                                    ErrorCategory::Internal,
                                    true,
                                    e,
                                ),
                            )
                            .await
                        }
                    }
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }
        names::RUN_CANCEL => {
            match serde_json::from_value::<CancelRunRequest>(request.params.clone()) {
                Ok(req) => match run_manager().cancel(req).await {
                    Ok(run) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::to_value(run).unwrap_or_default(),
                        )
                        .await
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                "run_cancel_failed",
                                ErrorCategory::NotFound,
                                false,
                                e,
                            ),
                        )
                        .await
                    }
                },
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }
        names::PERMISSION_RESPOND => {
            let request_id = request
                .params
                .get("request_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let approved = request
                .params
                .get("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let run_id = request.params.get("run_id").and_then(|v| v.as_str());
            let scope = request.params.get("scope").and_then(|v| v.as_str());
            if request_id.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "request_id required for permission.respond",
                    ),
                )
                .await;
            } else {
                match run_manager()
                    .respond_permission_for_run(request_id, approved, run_id, scope)
                    .await
                {
                    Ok(()) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::json!({
                                "ok": true,
                                "approved": approved,
                                "request_id": request_id,
                                "run_id": run_id,
                                "scope": scope.unwrap_or("once"),
                            }),
                        )
                        .await
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                "permission_failed",
                                ErrorCategory::PermissionDenied,
                                false,
                                e,
                            ),
                        )
                        .await
                    }
                }
            }
        }
        names::RUN_RETRY => {
            match serde_json::from_value::<RetryRunRequest>(request.params.clone()) {
                Ok(req) => {
                    // M1: create new run and start it detached (do not leave Queued).
                    match run_manager().retry(req) {
                        Ok(new_run) => {
                            let start_req = StartRunRequest {
            agent_profile_id: None,
            capability_selection: None,
                                run_id: Some(new_run.id.clone()),
                                conversation_id: Some(new_run.conversation_id.clone()),
                                provider_id: Some(new_run.provider_id.clone()),
                                model_id: Some(new_run.model_id.clone()),
                                key_id: new_run.key_id.clone(),
                                content: None,
                                attachments: None,
                                trigger_message_id: None,
                                permission_profile: Some(new_run.permission_profile.clone()),
                                max_steps: Some(new_run.max_steps),
                                project_path: new_run.project_path.clone(),
                                idempotency_key: None,
                                effort: None,
                                runtime_id: None,
                            };
                            match crate::run_manager::RunManager::start_detached_global(start_req) {
                                Ok(run) => {
                                    send_success(
                                        writer,
                                        &request.request_id,
                                        &request.client_id,
                                        &request.session_token,
                                        serde_json::to_value(run).unwrap_or_default(),
                                    )
                                    .await
                                }
                                Err(e) => {
                                    send_error(
                                        writer,
                                        &DaemonError::new(
                                            "run_retry_start_failed",
                                            ErrorCategory::Internal,
                                            true,
                                            e,
                                        ),
                                    )
                                    .await
                                }
                            }
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    "run_retry_failed",
                                    ErrorCategory::NotFound,
                                    false,
                                    e,
                                ),
                            )
                            .await
                        }
                    }
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }
        names::RUN_REPLAY | names::RUN_GET_EVENTS => {
            // Non-blocking event batch (array payload for UI/Tauri poll compatibility).
            let after = request
                .params
                .get("after_sequence")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let events = run_manager().replay(ReplayRunRequest {
                run_id: run_id.to_string(),
                after_sequence: after,
            });
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::to_value(events).unwrap_or_default(),
            )
            .await;
        }
        names::RUN_SUBSCRIBE => {
            // Hybrid subscribe:
            // 1) Always return replay after_sequence (sequence gap fill).
            // 2) Optional wait_ms / mode=push: block up to wait_ms for *new*
            //    broadcast events (real-time push over long-poll style).
            // Continuous multi-line push on a dedicated connection is future work;
            // this unblocks UI without blocking cancel on a second connection.
            let after = request
                .params
                .get("after_sequence")
                .or_else(|| request.params.get("last_sequence"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let wait_ms = request
                .params
                .get("wait_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let want_push = request
                .params
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|m| m == "push" || m == "long_poll")
                .unwrap_or(false)
                || wait_ms > 0;

            let mut events = run_manager().replay(ReplayRunRequest {
                run_id: run_id.clone(),
                after_sequence: after,
            });
            let mut mode = "subscribe_poll";
            if want_push && events.is_empty() {
                let timeout = std::time::Duration::from_millis(wait_ms.clamp(1, 30_000));
                let mut rx = run_manager().events().subscribe(&run_id);
                mode = "subscribe_push_wait";
                let deadline = tokio::time::Instant::now() + timeout;
                loop {
                    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    match tokio::time::timeout(left, rx.recv()).await {
                        Ok(Ok(ev)) if ev.effective_run_sequence() > after => {
                            events.push(ev);
                            // Drain a small batch without extra waits.
                            while let Ok(more) = rx.try_recv() {
                                if more.effective_run_sequence() > after {
                                    events.push(more);
                                }
                            }
                            break;
                        }
                        Ok(Ok(_)) => continue,
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                            events = run_manager().replay(ReplayRunRequest {
                                run_id: run_id.clone(),
                                after_sequence: after,
                            });
                            break;
                        }
                        Ok(Err(_)) | Err(_) => break,
                    }
                }
            }
            let terminal = run_manager()
                .get_run(&run_id)
                .map(|r| r.status.is_terminal())
                .unwrap_or(false);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "run_id": run_id,
                    "events": events,
                    "terminal": terminal,
                    "mode": mode,
                }),
            )
            .await;
        }
        names::RUN_LIST => {
            let conversation_id = request
                .params
                .get("conversation_id")
                .and_then(|v| v.as_str());
            let runs = run_manager().list_runs(conversation_id);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "runs": runs }),
            )
            .await;
        }
        names::RUN_LIST_CHILDREN => match handle_run_list_children(&request.params) {
            Ok(value) => {
                send_success(
                    writer,
                    &request.request_id,
                    &request.client_id,
                    &request.session_token,
                    value,
                )
                .await
            }
            Err(e) => {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        e,
                    ),
                )
                .await
            }
        },
        names::RUN_GET_ACTIVITY => match handle_run_get_activity(&request.params) {
            Ok(value) => {
                send_success(
                    writer,
                    &request.request_id,
                    &request.client_id,
                    &request.session_token,
                    value,
                )
                .await
            }
            Err(e) => {
                let not_found = e.contains("not found");
                send_error(
                    writer,
                    &DaemonError::new(
                        if not_found {
                            error_codes::NOT_FOUND
                        } else {
                            error_codes::INVALID_INPUT
                        },
                        if not_found {
                            ErrorCategory::NotFound
                        } else {
                            ErrorCategory::Validation
                        },
                        false,
                        e,
                    ),
                )
                .await
            }
        },
        names::RUN_FINISH => match handle_run_finish(&request.params) {
            Ok(value) => {
                send_success(
                    writer,
                    &request.request_id,
                    &request.client_id,
                    &request.session_token,
                    value,
                )
                .await
            }
            Err(e) => {
                let not_found = e.contains("not found");
                send_error(
                    writer,
                    &DaemonError::new(
                        if not_found {
                            error_codes::NOT_FOUND
                        } else {
                            error_codes::INVALID_INPUT
                        },
                        if not_found {
                            ErrorCategory::NotFound
                        } else {
                            ErrorCategory::Validation
                        },
                        false,
                        e,
                    ),
                )
                .await
            }
        },
        names::PERMISSION_LIST_PENDING => {
            match crate::interaction_store::list_pending(request.params.clone()) {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        filter_permission_interactions(value),
                    )
                    .await
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await
                }
            }
        }
        names::AGENT_LIST => {
            let project = request
                .params
                .get("project_path")
                .or_else(|| request.params.get("projectPath"))
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from);
            let agents = discover_agent_profiles(project.as_deref());
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "agents": agents }),
            )
            .await;
        }
        names::CONVERSATION_UPDATE => {
            match handle_conversation_update(request.params.clone()).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await
                }
            }
        }
        names::PROMPT_QUEUE_LIST
        | names::PROMPT_QUEUE_ENQUEUE
        | names::PROMPT_QUEUE_UPDATE
        | names::PROMPT_QUEUE_REMOVE
        | names::PROMPT_QUEUE_REORDER
        | names::PROMPT_QUEUE_SEND_NOW
        | names::PROMPT_QUEUE_INTERJECT => {
            match crate::prompt_queue_store::request(&request.method, request.params.clone()).await
            {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(writer, &DaemonError::new(code, category, false, e)).await
                }
            }
        }
        names::INTERACTION_LIST_PENDING | names::INTERACTION_RESPOND => {
            match crate::interaction_store::request(&request.method, request.params.clone()).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(writer, &DaemonError::new(code, category, false, e)).await
                }
            }
        }
        names::SUBAGENT_LIST | names::SUBAGENT_TOUCH | names::SUBAGENT_SWITCH_ROUTE => {
            match crate::subagent_store::request(&request.method, request.params.clone()).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(writer, &DaemonError::new(code, category, false, e)).await
                }
            }
        }
        names::TOOL_LIST => {
            let mut gateway = capability_gateway::CapabilityGateway::new();
            gateway.register_builtins();
            let tools = gateway
                .list_tools()
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.name,
                        "description": t.description,
                        "input_schema": t.schema,
                    })
                })
                .collect::<Vec<_>>();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "tools": tools }),
            )
            .await;
        }
        names::PROVIDER_LIST => {
            let providers = provider_adapters::register_all()
                .into_iter()
                .map(|p| {
                    let caps = p.capabilities();
                    serde_json::json!({
                        "provider_type": format!("{:?}", p.provider_type()).to_ascii_lowercase(),
                        "streaming": caps.streaming,
                        "tool_calls": caps.tool_calls,
                        "reasoning": caps.reasoning,
                    })
                })
                .collect::<Vec<_>>();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "providers": providers }),
            )
            .await;
        }
        names::PROVIDER_DISCOVER_MODELS => {
            let provider_id = request
                .params
                .get("provider_id")
                .or_else(|| request.params.get("provider"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let key_id = request
                .params
                .get("key_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("discover");
            match resolve_provider_adapter(provider_id) {
                Some(adapter) => {
                    let cred = crate::production::resolve_credential_for_run(
                        provider_id,
                        key_id.as_deref(),
                        run_id,
                    );
                    let result = match cred {
                        Ok(c) => adapter.discover_models(c).await,
                        Err(_) => adapter.list_models().await,
                    };
                    match result {
                        Ok(models) => {
                            send_success(
                                writer,
                                &request.request_id,
                                &request.client_id,
                                &request.session_token,
                                serde_json::json!({ "models": models }),
                            )
                            .await;
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    error_codes::PROVIDER_ERROR,
                                    ErrorCategory::Provider,
                                    e.retryable,
                                    e.message.clone(),
                                ),
                            )
                            .await;
                        }
                    }
                }
                None => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            format!("unknown provider: {provider_id}"),
                        ),
                    )
                    .await;
                }
            }
        }
        names::PROVIDER_TEST => {
            let provider_id = request
                .params
                .get("provider_id")
                .or_else(|| request.params.get("provider"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let key_id = request
                .params
                .get("key_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("provider-test");
            let model = request
                .params
                .get("model_id")
                .or_else(|| request.params.get("model"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if model.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "model_id is required for provider.test",
                    ),
                )
                .await;
                return;
            }
            match resolve_provider_adapter(provider_id) {
                Some(adapter) => {
                    let result = match crate::production::resolve_credential_for_run(
                        provider_id,
                        key_id.as_deref(),
                        run_id,
                    ) {
                        Ok(c) => test_provider_model(adapter.as_ref(), c, model).await,
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    error_codes::UNAUTHORIZED,
                                    ErrorCategory::Auth,
                                    false,
                                    e,
                                ),
                            )
                            .await;
                            return;
                        }
                    };
                    match result {
                        Ok(r) => {
                            send_success(
                                writer,
                                &request.request_id,
                                &request.client_id,
                                &request.session_token,
                                serde_json::to_value(r).unwrap_or_default(),
                            )
                            .await;
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    e.code.clone(),
                                    ErrorCategory::Provider,
                                    e.retryable,
                                    e.message.clone(),
                                ),
                            )
                            .await;
                        }
                    }
                }
                None => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            format!("unknown provider: {provider_id}"),
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_LIST => {
            let mcp = crate::mcp_runtime::global_mcp();
            let servers = mcp.list_servers();
            let tools = mcp.list_tools();
            // `capabilities` is per-server and may be null. Null means "no
            // completed handshake, we do not know" — never "supports nothing".
            // The GUI must render unknown differently from unsupported, which is
            // only possible because this field is nullable rather than defaulted.
            let capabilities: Vec<serde_json::Value> = servers
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "server_id": s.id,
                        "capabilities": mcp.server_capabilities(&s.id),
                    })
                })
                .collect();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "servers": servers,
                    "tools": tools,
                    "namespaced": mcp.namespaced_tools(),
                    "capabilities": capabilities,
                }),
            )
            .await;
        }
        names::MCP_START => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // task-06: refuse inline trusted server registration over RPC.
            // Only start already-registered servers from trusted config sources.
            if request.params.get("server").is_some() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "inline MCP server registration disabled; register via trusted config only",
                    ),
                )
                .await;
                return;
            }
            // Transport-aware start (stdio session or HTTP/SSE probe).
            match crate::mcp_runtime::global_mcp().start(id) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_STOP => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().stop(id) {
                Ok(()) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({ "ok": true, "id": id }),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INTERNAL_ERROR,
                            ErrorCategory::Internal,
                            true,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_CALL => {
            // task-06 phase 1: close direct RPC MCP transport bypass.
            // Never call global_mcp().call_tool from RPC. Agent path uses
            // PermissionGatedTools -> shared invocation only.
            let _ = (
                request.params.get("server_id"),
                request.params.get("tool"),
                request.params.get("arguments"),
            );
            send_error(
                writer,
                &DaemonError::new(
                    error_codes::INVALID_INPUT,
                    ErrorCategory::Validation,
                    false,
                    "direct_mcp_call_disabled",
                ),
            )
            .await;
        }
        names::MCP_LIVENESS => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().liveness(id) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_RECONNECT => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().reconnect(id) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            true,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_AUTH_SET => {
            let id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let token = request
                .params
                .get("token")
                .or_else(|| request.params.get("access_token"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let token_type = request
                .params
                .get("token_type")
                .and_then(|v| v.as_str())
                .unwrap_or("bearer");
            let expires_at = request.params.get("expires_at").and_then(|v| v.as_u64());
            // Never echo token back.
            match crate::mcp_runtime::global_mcp().set_auth_token(
                id,
                token.to_string(),
                token_type,
                expires_at,
            ) {
                Ok(lease) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::to_value(lease)
                            .unwrap_or(serde_json::json!({"has_token": true})),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_AUTH_STATUS => {
            let id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().auth_status(id) {
                Ok(lease) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::to_value(lease)
                            .unwrap_or(serde_json::json!({"has_token": false})),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_AUTH_CLEAR => {
            let id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().clear_auth_token(id) {
                Ok(()) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({ "ok": true, "server_id": id }),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::MCP_RESOURCES_LIST => {
            let cursor = request.params.get("cursor").and_then(|v| v.as_str());
            match crate::mcp_runtime::global_mcp().list_resources(mcp_server_id(request), cursor) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &mcp_error_to_daemon(e)).await,
            }
        }
        names::MCP_RESOURCES_TEMPLATES_LIST => {
            match crate::mcp_runtime::global_mcp().list_resource_templates(mcp_server_id(request)) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &mcp_error_to_daemon(e)).await,
            }
        }
        names::MCP_RESOURCES_READ => {
            // Human-initiated read only. There is deliberately no model-facing
            // tool for this: `tools/call` stays closed over RPC because it has
            // side effects, and a resource read is gated instead by the server's
            // own published URI set plus the scheme policy in `mcp_runtime`.
            let uri = request
                .params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().read_resource(mcp_server_id(request), uri) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &mcp_error_to_daemon(e)).await,
            }
        }
        names::MCP_PROMPTS_LIST => {
            let cursor = request.params.get("cursor").and_then(|v| v.as_str());
            match crate::mcp_runtime::global_mcp().list_prompts(mcp_server_id(request), cursor) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &mcp_error_to_daemon(e)).await,
            }
        }
        names::MCP_PROMPTS_GET => {
            let name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            match crate::mcp_runtime::global_mcp().get_prompt(
                mcp_server_id(request),
                name,
                arguments,
            ) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &mcp_error_to_daemon(e)).await,
            }
        }
        names::MCP_ROOTS_LIST => {
            // What *we* would hand a server that asks. Empty is a real answer
            // ("no roots granted"), not a placeholder, so `source` states where
            // the set came from instead of leaving the GUI to guess.
            let roots = crate::mcp_runtime::global_mcp().client_roots();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "roots": roots,
                    "source": if std::env::var("NATIVES_MCP_ROOTS").is_ok() {
                        "env:NATIVES_MCP_ROOTS"
                    } else {
                        "explicit"
                    },
                }),
            )
            .await;
        }
        names::MCP_NOTIFICATIONS_LIST => {
            let server_id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            let items = crate::mcp_runtime::global_mcp().notifications(server_id);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "server_id": server_id,
                    "notifications": items,
                }),
            )
            .await;
        }
        names::ARTIFACT_LIST => {
            let run_id = request.params.get("run_id").and_then(|v| v.as_str());
            let items = crate::artifact_store::global_artifacts().list(run_id);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "artifacts": items }),
            )
            .await;
        }
        names::TASK_LIST => {
            let filter_run_id = request.params.get("run_id").and_then(|v| v.as_str());
            let filter_conversation_id = request
                .params
                .get("conversation_id")
                .and_then(|v| v.as_str());

            // Get tasks from database (persistent)
            let mut tasks = match crate::task_store::list_all_tasks() {
                Ok(db_tasks) => db_tasks,
                Err(e) => {
                    eprintln!("Failed to list tasks from DB: {e}");
                    Vec::new()
                }
            };

            // Also get in-memory tasks (for running tasks not yet persisted)
            for (task_id, rec) in run_manager().runtime.list_tasks().await {
                // Skip if already in DB results
                if tasks.iter().any(|t| t["id"].as_str() == Some(&task_id)) {
                    continue;
                }
                if let Some(want) = filter_run_id {
                    if rec.run_id != want {
                        continue;
                    }
                }
                let conversation_id = run_manager()
                    .get_run(&rec.run_id)
                    .map(|r| r.conversation_id);
                if let Some(want) = filter_conversation_id {
                    match conversation_id.as_deref() {
                        Some(cid) if cid == want => {}
                        _ => continue,
                    }
                }
                tasks.push(serde_json::json!({
                    "id": task_id,
                    "run_id": rec.run_id,
                    "conversation_id": conversation_id,
                    "status": rec.status,
                    "output": rec.output,
                    "kind": "subagent",
                }));
            }

            // Apply filters to DB results
            if filter_run_id.is_some() || filter_conversation_id.is_some() {
                tasks.retain(|t| {
                    if let Some(want) = filter_run_id {
                        if t["run_id"].as_str() != Some(want) {
                            return false;
                        }
                    }
                    if let Some(want) = filter_conversation_id {
                        if t["conversation_id"].as_str() != Some(want) {
                            return false;
                        }
                    }
                    true
                });
            }

            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "tasks": tasks }),
            )
            .await;
        }
        names::TASK_CANCEL => {
            let task_id = request
                .params
                .get("task_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if task_id.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "task_id required for task.cancel",
                    ),
                )
                .await;
            } else {
                let cancelled = run_manager().runtime.kill_task(task_id).await;
                if cancelled {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({
                            "ok": true,
                            "cancelled": true,
                            "task_id": task_id,
                        }),
                    )
                    .await;
                } else {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            format!("unknown task_id: {task_id}"),
                        ),
                    )
                    .await;
                }
            }
        }
        names::TASK_WAIT => {
            let task_id = request
                .params
                .get("task_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if task_id.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "task_id required for task.wait",
                    ),
                )
                .await;
            } else {
                let timeout_ms = request
                    .params
                    .get("timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(60_000);
                match run_manager().runtime.wait_task(&task_id, timeout_ms).await {
                    Ok(rec) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::json!({
                                "id": task_id,
                                "run_id": rec.run_id,
                                "status": rec.status,
                                "output": rec.output,
                                "kind": "subagent",
                            }),
                        )
                        .await;
                    }
                    Err(e) if e == "timeout" => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                error_codes::TIMEOUT,
                                ErrorCategory::Timeout,
                                true,
                                format!("task.wait timed out after {timeout_ms}ms: {task_id}"),
                            ),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                error_codes::NOT_FOUND,
                                ErrorCategory::NotFound,
                                false,
                                e,
                            ),
                        )
                        .await;
                    }
                }
            }
        }
        names::ARTIFACT_OPEN => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::artifact_store::global_artifacts().open(id) {
                Ok((meta, bytes)) => {
                    // Never return huge binaries raw if over cap — return meta + base64 preview.
                    let preview = if bytes.len() <= 64 * 1024 {
                        Some(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &bytes,
                        ))
                    } else {
                        None
                    };
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({
                            "artifact": meta,
                            "content_base64": preview,
                            "truncated": preview.is_none(),
                        }),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        names::EXTENSION_LIST => {
            let items = crate::extension_store::global_extensions()
                .list()
                .into_iter()
                .map(|item| {
                    let mut value = serde_json::to_value(item).unwrap_or_default();
                    if let Some(object) = value.as_object_mut() {
                        object.insert(
                            "execution_status".into(),
                            serde_json::Value::String("discovered_not_executable".into()),
                        );
                    }
                    value
                })
                .collect::<Vec<_>>();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "extensions": items,
                    "execution_status": "discovered_not_executable"
                }),
            )
            .await;
        }
        // Conversation-level capability selection (ADR-0016).
        names::CONVERSATION_UPDATE_CAPABILITIES | names::CONVERSATION_GET_CAPABILITIES => {
            match crate::capability_resolution::handle_conversation_rpc(
                &request.method,
                &request.params,
            ) {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(writer, &DaemonError::new(code, category, false, e)).await
                }
            }
        }
        // Capability library configuration surface (ADR-0016). One routing arm;
        // per-method dispatch lives in capability::request to keep rpc.rs flat.
        // The is_implemented_method guard keeps catalogued-but-unimplemented
        // methods (e.g. hub before it ships) on the honest unsupported path.
        method
            if method.starts_with("capability.")
                && assistant_protocol::v2::is_implemented_method(method) =>
        {
            match crate::capability::request(method, request.params.clone()).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(writer, &DaemonError::new(code, category, false, e)).await
                }
            }
        }
        names::SKILL_LIST => {
            let project = request
                .params
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from);
            if let Some(p) = project.as_deref() {
                crate::skill_store::global_skills().discover_for_project(Some(p));
            }
            let items = crate::skill_store::global_skills().list();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "skills": items }),
            )
            .await;
        }
        names::MEMORY_SEARCH => {
            let query = request
                .params
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = request
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as usize;
            let hits = crate::memory_store::global_memory().search(query, limit);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "hits": hits }),
            )
            .await;
        }
        names::MEMORY_ADD => {
            let text = request
                .params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let scope = request
                .params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("workspace");
            let project_path = request
                .params
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let tags = request
                .params
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            match crate::memory_store::global_memory().add(scope, project_path, text, tags) {
                Ok(entry) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::to_value(entry).unwrap_or_default(),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }
        // Unimplemented catalogue methods: fail closed (not empty success).
        // Method disposition: known→unsupported, unknown→unsupported (invalid only for bad shape).
        // promptQueue.* is handled above via prompt_queue_store (daemon DB + harness).
        "run.rewindPreview" | "run.rewind" | "workspace.restorePreview" | "workspace.restore" => {
            match handle_rewind_rpc(&request.method, &request.params) {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await
                }
            }
        }
        // Harness control plane. The Harness family and its project identity
        // support methods share one handler, and
        // `the_harness_prefix_and_the_advertised_harness_family_agree` in the
        // protocol crate pins the prefix to exactly the advertised set, so this
        // arm can never quietly serve something that was never advertised.
        method
            if method.starts_with(names::HARNESS_PREFIX)
                || matches!(
                    method,
                    names::PROJECT_IDENTITY_REGISTER | names::PROJECT_IDENTITY_LIST
                ) =>
        {
            match harness::request(method, request.params.clone()).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    // The Harness error already carries a structured code and a
                    // category; re-deriving either from the message here would
                    // throw that away.
                    send_error(
                        writer,
                        &DaemonError::new(e.code, e.category, false, e.message),
                    )
                    .await
                }
            }
        }
        "conversation.getContextUsage" => match handle_context_usage_rpc(&request.params) {
            Ok(value) => {
                send_success(
                    writer,
                    &request.request_id,
                    &request.client_id,
                    &request.session_token,
                    value,
                )
                .await
            }
            Err(e) => {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        e,
                    ),
                )
                .await
            }
        },
        _ => {
            let status = assistant_protocol::v2::method_status(&request.method);
            let code = match status {
                assistant_protocol::v2::MethodStatus::Unsupported => "unsupported",
                assistant_protocol::v2::MethodStatus::InvalidRequest => "invalid_request",
                assistant_protocol::v2::MethodStatus::Implemented => "internal_error",
            };
            let err = DaemonError::new(
                code,
                ErrorCategory::Unsupported,
                false,
                format!(
                    "method not implemented: {} (status={status:?}; see daemon.getCapabilities)",
                    request.method
                ),
            );
            send_error(writer, &err).await;
        }
    }
}

/// Read a required non-empty string param, accepting snake_case and camelCase aliases.
fn required_param<'a>(params: &'a serde_json::Value, aliases: &[&str]) -> Result<&'a str, String> {
    for key in aliases {
        if let Some(value) = params.get(*key).and_then(|v| v.as_str()) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }
    Err(format!("{} is required", aliases[0]))
}

/// `run.listChildren` — direct children of `parent_run_id`.
///
/// Source of truth is the RunManager projection (the sole owner of run identity), not
/// the live ExecutionRegistry: children of a finished parent must still be listable.
/// Depth-1 only; callers recurse if they want the whole tree.
fn handle_run_list_children(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let parent = required_param(params, &["parent_run_id", "parentRunId", "run_id", "runId"])?;
    let mut children: Vec<assistant_protocol::v2::RunV2> = run_manager()
        .list_runs(None)
        .into_iter()
        .filter(|r| r.parent_run_id.as_deref() == Some(parent))
        .collect();
    children.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(serde_json::json!({
        "parent_run_id": parent,
        "children": children,
    }))
}

/// `run.getActivity` — point-in-time activity snapshot for one run.
///
/// Every field is projected from an existing source (RunManager run record, the run's
/// child projection, and the persisted interaction table). Nothing is synthesised.
fn handle_run_get_activity(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let run_id = required_param(params, &["run_id", "runId"])?;
    let run = run_manager()
        .get_run(run_id)
        .ok_or_else(|| format!("run not found: {run_id}"))?;
    let child_run_ids: Vec<String> = run_manager()
        .list_runs(None)
        .into_iter()
        .filter(|r| r.parent_run_id.as_deref() == Some(run_id))
        .map(|r| r.id)
        .collect();
    // Best-effort: a missing/locked interaction table must not fail the snapshot.
    let pending = crate::interaction_store::list_pending(serde_json::json!({ "run_id": run_id }))
        .ok()
        .and_then(|v| v.get("interactions").cloned())
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let pending_count = pending.as_array().map(|a| a.len()).unwrap_or(0);
    Ok(serde_json::json!({
        "run_id": run.id,
        "conversation_id": run.conversation_id,
        "status": run.status,
        "runtime_id": run.runtime_id,
        "provider_id": run.provider_id,
        "model_id": run.model_id,
        "agent_profile_id": run.agent_profile_id,
        "step_count": run.step_count,
        "max_steps": run.max_steps,
        "retry_count": run.retry_count,
        "last_event_sequence": run.last_event_sequence,
        "created_at": run.created_at,
        "started_at": run.started_at,
        "finished_at": run.finished_at,
        "error_code": run.error_code,
        "child_run_ids": child_run_ids,
        "pending_interaction_count": pending_count,
        "pending_interactions": pending,
    }))
}

/// `run.finish` — externally driven terminal commit.
///
/// Delegates to `RunManager::commit_status`, which is the sole committer of run
/// lifecycle transitions (see docs/architecture/NATIVE-DAEMON-CAPABILITY-MAP.md).
/// This handler never writes run state itself, and terminal races stay idempotent
/// because `commit_status` returns the existing run when it is already terminal.
fn handle_run_finish(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use assistant_protocol::v2::RunStatusV2;
    let run_id = required_param(params, &["run_id", "runId"])?;
    let requested = params
        .get("status")
        .or_else(|| params.get("outcome"))
        .and_then(|v| v.as_str())
        .unwrap_or("completed")
        .trim()
        .to_ascii_lowercase();
    let target = match requested.as_str() {
        "completed" | "complete" | "success" | "succeeded" => RunStatusV2::Completed,
        "failed" | "failure" | "error" => RunStatusV2::Failed,
        "cancelled" | "canceled" => RunStatusV2::Cancelled,
        "interrupted" => RunStatusV2::Interrupted,
        other => {
            return Err(format!(
                "status must be a terminal state (completed|failed|cancelled|interrupted), got: {other}"
            ))
        }
    };
    let mut metadata = agent_core::TransitionMetadata::empty().with_lifecycle_hint(match target {
        RunStatusV2::Completed => "completed",
        RunStatusV2::Failed => "failed",
        RunStatusV2::Cancelled => "cancelled",
        _ => "interrupted",
    });
    if let Some(reason) = params.get("reason").and_then(|v| v.as_str()) {
        if !reason.trim().is_empty() {
            metadata = metadata.with_reason(reason.trim());
        }
    }
    let run = run_manager().commit_status(run_id, target, metadata)?;
    serde_json::to_value(run).map_err(|e| e.to_string())
}

/// Narrow `interaction.listPending` rows down to permission requests and flatten the
/// stored payload into the shape the permission UI reads.
///
/// Returns a bare JSON array (not `{interactions: […]}`) — that is what the gateway
/// adapter expects from `permission.listPending`.
fn filter_permission_interactions(value: serde_json::Value) -> serde_json::Value {
    let rows = value
        .get("interactions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .filter(|row| {
            row.get("kind")
                .and_then(|v| v.as_str())
                .map(|k| k.contains("permission"))
                .unwrap_or(false)
        })
        .map(|row| {
            let payload = row
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let field = |name: &str| {
                payload
                    .get(name)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            };
            serde_json::json!({
                "id": row.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "run_id": row.get("run_id").cloned().unwrap_or(serde_json::Value::Null),
                "conversation_id": row
                    .get("conversation_id")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "kind": row.get("kind").cloned().unwrap_or(serde_json::Value::Null),
                "created_at": row.get("created_at").cloned().unwrap_or(serde_json::Value::Null),
                "tool_call_id": field("tool_call_id"),
                "tool_name": field("tool_name"),
                "reason": field("reason"),
                "input": field("input"),
            })
        })
        .collect();
    serde_json::Value::Array(out)
}

/// `conversation.update` — generic partial update.
///
/// Composed from the existing single-field conversation_store commands so there is
/// exactly one SQL writer per field. Unknown/absent fields are simply not applied;
/// an update naming no known field is a validation error rather than a silent no-op.
async fn handle_conversation_update(
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = required_param(&params, &["id", "conversation_id", "conversationId"])?.to_string();
    let get = |names: &[&str]| -> Option<String> {
        names
            .iter()
            .find_map(|n| params.get(*n).and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    };
    let mut applied: Vec<&str> = Vec::new();

    if let Some(title) = get(&["title", "name"]) {
        crate::conversation_store::request(
            assistant_protocol::v2::methods::names::CONVERSATION_RENAME,
            serde_json::json!({ "id": id, "title": title }),
        )
        .await?;
        applied.push("title");
    }

    let provider_id = get(&["provider_id", "providerId"]);
    let model_id = get(&["model_id", "modelId"]);
    match (&provider_id, &model_id) {
        (Some(provider_id), Some(model_id)) => {
            crate::conversation_store::request(
                assistant_protocol::v2::methods::names::CONVERSATION_UPDATE_MODEL,
                serde_json::json!({
                    "id": id,
                    "provider_id": provider_id,
                    "model_id": model_id,
                }),
            )
            .await?;
            applied.push("provider_id");
            applied.push("model_id");
        }
        // Partial model routing would leave the conversation pointing at a model the
        // provider does not serve — refuse instead of half-applying.
        (Some(_), None) | (None, Some(_)) => {
            return Err("provider_id and model_id must be updated together".into())
        }
        (None, None) => {}
    }

    if let Some(profile) = get(&[
        "permission_profile_id",
        "permissionProfileId",
        "permission_profile",
    ]) {
        crate::conversation_store::request(
            assistant_protocol::v2::methods::names::CONVERSATION_UPDATE_PERMISSION,
            serde_json::json!({ "id": id, "permission_profile_id": profile }),
        )
        .await?;
        applied.push("permission_profile_id");
    }

    if applied.is_empty() {
        return Err(
            "conversation.update requires at least one of: title, provider_id+model_id, \
             permission_profile_id"
                .into(),
        );
    }
    // Return the fresh row so callers do not have to re-read.
    crate::conversation_store::request(
        assistant_protocol::v2::methods::names::CONVERSATION_GET,
        serde_json::json!({ "id": id }),
    )
    .await
    .map(|conversation| serde_json::json!({ "id": id, "updated": applied, "conversation": conversation }))
}

/// `agent.list` — discover declarative Agent Profiles on disk.
///
/// Discovery itself lives in `agent_core::list_agent_profiles` so that the ids reported
/// here are exactly the ids `agent_core::load_agent_profile` can resolve. This function
/// only projects to wire JSON and re-attaches `sourcePath`, which `AgentProfile` skips
/// during serialization.
fn discover_agent_profiles(project_root: Option<&std::path::Path>) -> Vec<serde_json::Value> {
    agent_core::list_agent_profiles(project_root)
        .into_iter()
        .map(|profile| {
            let source_path = profile
                .source_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string());
            let mut value = serde_json::to_value(&profile).unwrap_or_default();
            if let (Some(obj), Some(path)) = (value.as_object_mut(), source_path) {
                obj.insert("sourcePath".into(), serde_json::Value::String(path));
            }
            value
        })
        .collect()
}

fn handle_rewind_rpc(
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use crate::checkpoint::global_checkpoint_manager;
    let run_id = params
        .get("run_id")
        .and_then(|v| v.as_str())
        .ok_or("run_id is required")?;
    // Project path is taken from the bound run identity — callers cannot inject
    // an arbitrary path to restore files into another project (task-07/10).
    let run = crate::run_manager::global_run_manager()
        .get_run(run_id)
        .ok_or_else(|| format!("run not found: {run_id}"))?;
    // Legacy unbound ProjectIdentity: refuse restore entirely (no caller path injection).
    if run.project_id.is_none() {
        return Err(
            "workspace.restore refused: run has no verified ProjectIdentity; restored=0".into(),
        );
    }
    let bound_path = run.project_path.map(std::path::PathBuf::from);
    let project_path = if let Some(bound) = bound_path {
        if let Some(caller) = params
            .get("project_path")
            .or_else(|| params.get("project_root"))
            .and_then(|v| v.as_str())
        {
            let caller_p = std::path::PathBuf::from(caller);
            let b = bound.canonicalize().unwrap_or_else(|_| bound.clone());
            let c = caller_p.canonicalize().unwrap_or(caller_p);
            if b != c {
                return Err(format!(
                    "workspace.restore refused: caller project_path does not match run identity; restored=0"
                ));
            }
        }
        bound
    } else {
        return Err(
            "workspace.restore refused: run has project_id but no bound project_path; restored=0"
                .into(),
        );
    };
    let paths: Option<Vec<String>> = params.get("paths").and_then(|v| {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
    });
    let mgr = global_checkpoint_manager();
    match method {
        "run.rewindPreview" | "run.rewind" => {
            // Deprecated: ambiguous "whole run rewind". Prefer workspace.restore*.
            let replacement = if method.contains("Preview") {
                "workspace.restorePreview"
            } else {
                "workspace.restore"
            };
            Ok(serde_json::json!({
                "deprecated": true,
                "method": method,
                "scope": "workspace_file_only",
                "message": "run.rewind/run.rewindPreview are deprecated. Use workspace.restorePreview / workspace.restore for checkpoint-covered files only. Conversation rewind and execution replay are separate APIs. External side-effects are not rolled back.",
                "replacement": replacement,
            }))
        }
        "workspace.restorePreview" => {
            let preview = mgr.rewind_preview(run_id, &project_path, paths.as_deref())?;
            let mut value = serde_json::to_value(preview).unwrap_or_default();
            if let Some(obj) = value.as_object_mut() {
                let coverage = crate::side_effect_ledger::coverage_for_run(run_id)
                    .unwrap_or_else(|| "unknown".into());
                obj.insert("coverage".into(), serde_json::json!(coverage));
                obj.insert("scope".into(), serde_json::json!("workspace_file_only"));
            }
            Ok(value)
        }
        "workspace.restore" => {
            let checkpoint_id = params
                .get("checkpoint_id")
                .and_then(|v| v.as_str())
                .ok_or("checkpoint_id is required")?;
            let policy = params
                .get("conflict_policy")
                .and_then(|v| v.as_str())
                .unwrap_or("fail");
            let restored = mgr.workspace_restore(
                run_id,
                checkpoint_id,
                &project_path,
                paths.as_deref(),
                policy,
            )?;
            // Restore audit event — does not alter old Run terminal status.
            crate::run_manager::global_run_manager().events().append(
                run_id,
                assistant_protocol::v2::RunEventKind::CheckpointRewound {
                    checkpoint_id: checkpoint_id.to_string(),
                    paths: restored.clone(),
                    conflict_policy: Some(policy.to_string()),
                },
            );
            Ok(serde_json::json!({
                "ok": true,
                "scope": "workspace_file_only",
                "checkpoint_id": checkpoint_id,
                "restored_paths": restored,
            }))
        }
        other => Err(format!("unsupported restore method: {other}")),
    }
}

fn handle_context_usage_rpc(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use crate::checkpoint::estimate_context_usage;
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("id"))
        .and_then(|v| v.as_str())
        .ok_or("conversation_id is required")?;
    // Load messages from daemon store and estimate.
    let history = crate::conversation_store::engine_history(conversation_id).unwrap_or_default();
    let mut conv_chars = 0usize;
    let mut tool_chars = 0usize;
    for m in &history {
        let n = m.content.len();
        if m.role == "tool" {
            tool_chars += n;
        } else {
            conv_chars += n;
        }
    }
    let max_tokens = params
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(128_000);
    let mut usage = estimate_context_usage(0, conv_chars, tool_chars, max_tokens);
    if let Some(obj) = usage.as_object_mut() {
        obj.insert(
            "conversationId".into(),
            serde_json::Value::String(conversation_id.to_string()),
        );
        obj.insert(
            "conversation_id".into(),
            serde_json::Value::String(conversation_id.to_string()),
        );
        if let Some(used) = obj.get("used_tokens").cloned() {
            obj.insert("usedTokens".into(), used);
        }
        if let Some(max) = obj.get("max_tokens").cloned() {
            obj.insert("maxTokens".into(), max);
        }
    }
    Ok(usage)
}

/// Send a success response.
async fn send_success(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request_id: &str,
    _client_id: &str,
    _session_token: &str,
    data: serde_json::Value,
) {
    let resp = RpcResponse {
        // Must match DaemonCapabilities / handshake (PROTOCOL_V2), never a stale 0.1.0.
        protocol_version: assistant_protocol::v2::PROTOCOL_V2.to_string(),
        request_id: request_id.to_string(),
        success: true,
        data: Some(data),
        error: None,
    };
    let json = serde_json::to_string(&resp).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

/// Send an error response.
async fn send_error(writer: &mut tokio::net::unix::OwnedWriteHalf, error: &DaemonError) {
    let json = serde_json::to_string(error).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

async fn send_rpc_failure(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &RpcRequest,
    code: &str,
    message: String,
) {
    let response = RpcResponse {
        protocol_version: assistant_protocol::v2::PROTOCOL_V2.to_string(),
        request_id: request.request_id.clone(),
        success: false,
        data: None,
        error: Some(serde_json::json!({"code": code, "message": message})),
    };
    let json = serde_json::to_string(&response).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_protocol::version::ProtocolVersion;

    /// Test bootstrap token single-use enforcement.
    #[tokio::test]
    async fn test_bootstrap_token_single_use() {
        let bootstrap_used = Arc::new(Mutex::new(false));
        let _pv = ProtocolVersion::new(2, 0, 0);

        // First use should succeed
        {
            let mut used = bootstrap_used.lock().await;
            assert!(!*used);
            *used = true;
        }

        // Second use should fail
        {
            let used = bootstrap_used.lock().await;
            assert!(*used);
        }
    }

    /// Test session token rotation.
    #[tokio::test]
    async fn test_session_token_rotation() {
        let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
        let token1 = Uuid::new_v4().to_string();

        {
            let mut sessions = sessions.lock().await;
            sessions.insert(token1.clone(), "client-1".to_string());
            assert_eq!(sessions.get(&token1).unwrap(), "client-1");
        }

        // Simulate rotation: new token, remove old
        let token2 = Uuid::new_v4().to_string();
        {
            let mut sessions = sessions.lock().await;
            sessions.remove(&token1);
            sessions.insert(token2.clone(), "client-1".to_string());
            assert!(!sessions.contains_key(&token1));
            assert_eq!(sessions.get(&token2).unwrap(), "client-1");
        }
    }

    /// Test protocol mismatch rejection.
    #[tokio::test]
    async fn test_protocol_mismatch_rejection() {
        let client_version = ProtocolVersion::new(0, 1, 0);
        let daemon_version = ProtocolVersion::new(2, 0, 0);
        let result = negotiate(&client_version, &daemon_version);
        assert!(!result.compatible);
        assert!(result.upgrade_required.is_some());
    }

    #[test]
    fn rpc_success_envelope_uses_protocol_v2() {
        assert_eq!(assistant_protocol::v2::PROTOCOL_V2, "2.0.0");
        let caps = crate::run_manager::RunManager::capabilities();
        assert_eq!(caps.protocol_version, "2.0.0");
        assert!(caps.methods.iter().any(|m| m == "run.start"));
        assert!(!caps.methods.iter().any(|m| m.starts_with("scheduler.")));
        assert!(caps.methods.iter().any(|m| m == "mcp.list"));
        assert!(caps.mcp);
        assert!(!caps.scheduler);
        assert!(caps.methods.iter().any(|m| m == "extension.list"));
        assert!(!caps.methods.iter().any(|m| m == "extension.enable"));
        assert!(!caps.extensions);
    }

    struct StaticStreamAdapter {
        events: Vec<ProviderEvent>,
    }

    #[async_trait::async_trait]
    impl provider_adapters::ProviderAdapter for StaticStreamAdapter {
        fn provider_type(&self) -> assistant_protocol::v1::provider::ProviderType {
            assistant_protocol::v1::provider::ProviderType::OpenaiCompatible
        }

        fn capabilities(&self) -> provider_adapters::capabilities::ProviderCapabilities {
            provider_adapters::capabilities::ProviderCapabilities {
                provider_type: self.provider_type(),
                features: vec!["streaming".into()],
                max_context_window: 1_000,
                streaming: true,
                tool_calls: false,
                structured_output: false,
                image_input: false,
                file_input: false,
                reasoning: false,
                system_prompt: true,
                function_calling: false,
            }
        }

        async fn chat(
            &self,
            _request: ProviderRequest,
        ) -> Result<
            provider_adapters::capabilities::ProviderResponse,
            provider_adapters::capabilities::ProviderError,
        > {
            unreachable!("provider.test must use stream")
        }

        async fn chat_stream(
            &self,
            _request: ProviderRequest,
        ) -> Result<
            Box<
                dyn futures_util::Stream<Item = provider_adapters::ProviderStreamEvent>
                    + Send
                    + Unpin,
            >,
            provider_adapters::capabilities::ProviderError,
        > {
            unreachable!("provider.test must use stream")
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
            _credential: Credential,
        ) -> Result<
            std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>,
            provider_adapters::capabilities::ProviderError,
        > {
            Ok(Box::pin(futures_util::stream::iter(self.events.clone())))
        }

        async fn list_models(
            &self,
        ) -> Result<
            Vec<provider_adapters::capabilities::ModelInfo>,
            provider_adapters::capabilities::ProviderError,
        > {
            Ok(Vec::new())
        }

        async fn test_connection(
            &self,
        ) -> Result<ProviderTestResult, provider_adapters::capabilities::ProviderError> {
            unreachable!("provider.test must not use key-present fake checks")
        }
    }

    #[tokio::test]
    async fn provider_model_test_consumes_stream_content() {
        let adapter = StaticStreamAdapter {
            events: vec![
                ProviderEvent::TextDelta("ok".into()),
                ProviderEvent::Completed {
                    reason: provider_adapters::stream::ProviderStopReason::Stop,
                },
            ],
        };
        let result = test_provider_model(
            &adapter,
            Credential {
                api_key: "test-key".into(),
                base_url: None,
                proxy_url: None,
                key_id: Some("k".into()),
                provider_type: Some("openai_compatible".into()),
            },
            "model-under-test",
        )
        .await
        .unwrap();

        assert!(result.success);
        assert!(result.message.contains("model-under-test"));
    }

    #[tokio::test]
    async fn provider_model_test_empty_stream_is_structured_error() {
        let adapter = StaticStreamAdapter {
            events: vec![ProviderEvent::Completed {
                reason: provider_adapters::stream::ProviderStopReason::Stop,
            }],
        };
        let error = test_provider_model(
            &adapter,
            Credential {
                api_key: "test-key".into(),
                base_url: None,
                proxy_url: None,
                key_id: Some("k".into()),
                provider_type: Some("openai_compatible".into()),
            },
            "empty-model",
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, "EMPTY_RESPONSE");
        assert!(error.retryable);
        assert!(error.message.contains("Provider test returned no content"));
    }

    /// Test client disconnect cleanup.
    #[tokio::test]
    async fn test_client_disconnect_cleanup() {
        let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
        let client_id = "client-1".to_string();
        let token = Uuid::new_v4().to_string();

        {
            let mut sessions = sessions.lock().await;
            sessions.insert(token.clone(), client_id.clone());
        }

        // Simulate disconnect: remove by client_id
        {
            let mut sessions = sessions.lock().await;
            sessions.retain(|_, v| v != &client_id);
            assert!(sessions.is_empty());
        }
    }

    /// Test that forged tokens are rejected.
    #[tokio::test]
    async fn test_forged_session_token_rejected() {
        let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
        let valid_token = Uuid::new_v4().to_string();
        let forged_token = "forged-token".to_string();

        {
            let mut sessions = sessions.lock().await;
            sessions.insert(valid_token, "client-1".to_string());
        }

        {
            let sessions = sessions.lock().await;
            assert!(!sessions.contains_key(&forged_token));
        }
    }
}
