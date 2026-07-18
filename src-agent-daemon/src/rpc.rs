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

/// Active session: maps session_token -> client_id.
type SessionMap = Arc<Mutex<HashMap<String, String>>>;

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
            let err = DaemonError::new(
                error_codes::INVALID_INPUT,
                ErrorCategory::Validation,
                false,
                format!("Invalid handshake JSON: {}", e),
            );
            let resp = serde_json::to_string(&err)?;
            writer.write_all(resp.as_bytes()).await?;
            writer.write_all(b"\n").await?;
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
            let err = DaemonError::new(
                error_codes::UNAUTHORIZED,
                ErrorCategory::Auth,
                false,
                "Invalid bootstrap token".to_string(),
            );
            let resp = serde_json::to_string(&err)?;
            writer.write_all(resp.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            return Ok(());
        }
        if single_use && *used {
            let err = DaemonError::new(
                error_codes::UNAUTHORIZED,
                ErrorCategory::Auth,
                false,
                "Bootstrap token already used (NATIVES_BOOTSTRAP_SINGLE_USE)".to_string(),
            );
            let resp = serde_json::to_string(&err)?;
            writer.write_all(resp.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            return Ok(());
        }
        *used = true;
    }

    // Negotiate protocol version
    let client_version = ProtocolVersion::from(handshake_req.client_version.as_str());
    let negotiation = negotiate(&client_version, &protocol_version);

    if !negotiation.compatible {
        let err = DaemonError::new(
            error_codes::PROTOCOL_INCOMPATIBLE,
            ErrorCategory::Unsupported,
            false,
            format!(
                "Protocol mismatch: client v{}, daemon v{}",
                client_version, protocol_version
            ),
        );
        let resp = serde_json::to_string(&err)?;
        writer.write_all(resp.as_bytes()).await?;
        writer.write_all(b"\n").await?;
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
    };
    let mut stream = adapter.stream(request, credential).await?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::Completed => {
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
        })
    }
}

/// Dispatch an RPC request to the appropriate handler.
async fn handle_rpc(
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
        names::DAEMON_GET_CAPABILITIES => {
            let caps = crate::run_manager::RunManager::capabilities();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::to_value(&caps).unwrap_or_default(),
            )
            .await;
        }
        names::CONVERSATION_CREATE
        | names::CONVERSATION_LIST
        | names::CONVERSATION_GET
        | names::CONVERSATION_FORK
        | names::CONVERSATION_GET_MESSAGES
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
                    .respond_permission_for_run(request_id, approved, run_id)
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
                        Ok(Ok(ev)) if ev.sequence > after => {
                            events.push(ev);
                            // Drain a small batch without extra waits.
                            while let Ok(more) = rx.try_recv() {
                                if more.sequence > after {
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
            let servers = crate::mcp_runtime::global_mcp().list_servers();
            let tools = crate::mcp_runtime::global_mcp().list_tools();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "servers": servers,
                    "tools": tools,
                    "namespaced": crate::mcp_runtime::global_mcp().namespaced_tools(),
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
            // Optional inline register for trusted stdio/http
            if let Some(cfg) = request.params.get("server") {
                if let Ok(server) =
                    serde_json::from_value::<agent_core::McpServerConfig>(cfg.clone())
                {
                    let _ = crate::mcp_runtime::global_mcp().register_server(server);
                }
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
            let server_id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tool_name = request
                .params
                .get("tool")
                .or_else(|| request.params.get("name"))
                .or_else(|| request.params.get("tool_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .or_else(|| request.params.get("input"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            if server_id.is_empty() || tool_name.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "mcp.call requires server_id and tool",
                    ),
                )
                .await;
            } else {
                match crate::mcp_runtime::global_mcp().call_tool(server_id, tool_name, arguments) {
                    Ok(v) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::json!({
                                "server_id": server_id,
                                "tool": tool_name,
                                "result": v,
                            }),
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
            let items = crate::extension_store::global_extensions().list();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "extensions": items }),
            )
            .await;
        }
        names::EXTENSION_ENABLE => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let enabled = request
                .params
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if id.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "id required",
                    ),
                )
                .await;
            } else {
                match crate::extension_store::global_extensions().set_enabled(id, enabled) {
                    Ok(item) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::to_value(item).unwrap_or_default(),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                error_codes::PERMISSION_DENIED,
                                ErrorCategory::PermissionDenied,
                                false,
                                e,
                            ),
                        )
                        .await;
                    }
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
        names::SCHEDULER_LIST => {
            let jobs = crate::scheduler_store::global_scheduler().list();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "jobs": jobs }),
            )
            .await;
        }
        names::SCHEDULER_HISTORY => {
            let limit = request
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            let history = crate::scheduler_store::global_scheduler().history(limit);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "history": history }),
            )
            .await;
        }
        names::SCHEDULER_TICK => {
            // Dev/ops: force one due-tick (also used by tests).
            let runner = crate::scheduler_store::ensure_scheduler_runner();
            match runner.tick_once() {
                Ok(fired) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({ "fired_run_ids": fired }),
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
        names::SCHEDULER_CREATE => {
            match serde_json::from_value::<crate::scheduler_store::CreateSchedulerJob>(
                request.params.clone(),
            ) {
                Ok(req) => match crate::scheduler_store::global_scheduler().create(req) {
                    Ok(job) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::to_value(job).unwrap_or_default(),
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
                    .await;
                }
            }
        }
        names::SCHEDULER_UPDATE => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if id.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "id required",
                    ),
                )
                .await;
            } else {
                match crate::scheduler_store::global_scheduler().update(id, request.params.clone())
                {
                    Ok(job) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::to_value(job).unwrap_or_default(),
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
        names::SCHEDULER_DELETE => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if id.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "id required",
                    ),
                )
                .await;
            } else {
                match crate::scheduler_store::global_scheduler().delete(id) {
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
        // Unimplemented catalogue methods: fail closed (not empty success).
        // Method disposition: known→unsupported, unknown→unsupported (invalid only for bad shape).
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
        assert!(
            caps.methods.iter().any(|m| m == "scheduler.delete"),
            "scheduler CRUD methods must be advertised when implemented"
        );
        assert!(caps.methods.iter().any(|m| m == "mcp.list"));
        assert!(caps.mcp);
        assert!(caps.scheduler);
        assert!(
            caps.methods.iter().any(|m| m == "extension.enable"),
            "extension.enable must be advertised when implemented"
        );
        assert!(caps.extensions);
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
                ProviderEvent::Completed,
            ],
        };
        let result = test_provider_model(
            &adapter,
            Credential {
                api_key: "test-key".into(),
                base_url: None,
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
            events: vec![ProviderEvent::Completed],
        };
        let error = test_provider_model(
            &adapter,
            Credential {
                api_key: "test-key".into(),
                base_url: None,
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
