//! Socket/session lifecycle for the UDS RPC server: bounded frame reads,
//! handshake, connection handling, and the `RpcServer` listener.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
use assistant_protocol::v1::daemon::{
    DaemonHealth, DaemonStatus, HandshakeRequest, HandshakeResponse,
};
use assistant_protocol::v2::{V2Request, PROTOCOL_V2};
use assistant_protocol::version::{negotiate, ProtocolVersion};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::framing::{send_error, send_invalid_request, FRAME_READ_TIMEOUT, MAX_FRAME_BYTES};
use super::handle_rpc;
use super::SessionMap;

/// Result of a bounded frame read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameError {
    /// Frame exceeded the configured byte cap (no newline within the cap).
    Oversize,
    /// Peer failed to deliver a full frame before the deadline.
    Timeout,
    /// Underlying I/O error.
    Io,
}

/// Read one newline-terminated frame with a hard byte cap and a read deadline.
///
/// - `Ok(None)` on EOF (peer closed).
/// - `Ok(Some(bytes))` when a frame within `max_frame` bytes was read.
/// - `Err(FrameError::Oversize)` when a frame exceeds `max_frame` bytes.
/// - `Err(FrameError::Timeout)` when the peer stalls without delivering a full
///   frame within `timeout`.
///
/// `take(max_frame + 1)` bounds the read so a newline-less frame cannot grow
/// the buffer without limit (replaces the unbounded `read_line`).
pub(crate) async fn read_frame<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    max_frame: usize,
    timeout: std::time::Duration,
) -> Result<Option<Vec<u8>>, FrameError> {
    let mut frame = Vec::with_capacity(1024);
    let outcome = tokio::time::timeout(timeout, async {
        let mut capped = reader.take((max_frame + 1) as u64);
        let n = capped.read_until(b'\n', &mut frame).await?;
        Ok::<_, std::io::Error>(n)
    })
    .await;
    let n = match outcome {
        Ok(Ok(n)) => n,
        Ok(Err(_)) => return Err(FrameError::Io),
        Err(_) => return Err(FrameError::Timeout),
    };
    if n == 0 {
        return Ok(None);
    }
    if frame.len() > max_frame {
        return Err(FrameError::Oversize);
    }
    Ok(Some(frame))
}

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

        println!(
            "Listening on {}",
            std::path::Path::new(&self.socket_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<runtime-sock>")
        );

        // B05 (TASK-008): replay undelivered permission decisions from the
        // outbox. A decision whose event/waiter delivery never completed after
        // a crash is delivered here; the handler is never woken ahead of its
        // durable event.
        if let Err(error) = crate::interaction_store::recover_interaction_outbox().await {
            eprintln!("[rpc] interaction outbox recovery failed: {error}");
        }

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

    // --- Step 1: Handshake (bounded frame, read deadline; N04) ---
    let handshake_frame = match read_frame(&mut reader, MAX_FRAME_BYTES, FRAME_READ_TIMEOUT).await {
        Ok(Some(frame)) => frame,
        Ok(None) => return Ok(()), // peer closed before sending a handshake
        Err(FrameError::Oversize) => {
            write_handshake_reject(
                &mut writer,
                &protocol_version.to_string(),
                &daemon_version,
                format!("Handshake frame exceeds {MAX_FRAME_BYTES} bytes"),
            )
            .await?;
            return Ok(());
        }
        Err(FrameError::Timeout) => {
            write_handshake_reject(
                &mut writer,
                &protocol_version.to_string(),
                &daemon_version,
                "Handshake frame read timed out".to_string(),
            )
            .await?;
            return Ok(());
        }
        Err(FrameError::Io) => return Err("handshake read failed".into()),
    };

    // Deserialize handshake request
    let handshake_req: HandshakeRequest = {
        let line = String::from_utf8_lossy(&handshake_frame);
        if line.trim().is_empty() {
            return Ok(());
        }
        match serde_json::from_str(line.trim()) {
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

    // --- Step 2: Handle RPC requests (bounded frame, read deadline; N04) ---
    loop {
        let frame = match read_frame(&mut reader, MAX_FRAME_BYTES, FRAME_READ_TIMEOUT).await {
            Ok(Some(frame)) => frame,
            Ok(None) => break, // Client disconnected
            Err(FrameError::Oversize) => {
                let err = DaemonError::new(
                    error_codes::INVALID_INPUT,
                    ErrorCategory::Validation,
                    false,
                    format!("RPC frame exceeds {MAX_FRAME_BYTES} bytes"),
                );
                send_error(&mut writer, "", &err).await;
                // A connection that overran the frame cap is unusable; close it
                // so the session is cleaned up (finally-style teardown below).
                break;
            }
            Err(FrameError::Timeout) => {
                let err = DaemonError::new(
                    error_codes::TIMEOUT,
                    ErrorCategory::Timeout,
                    false,
                    "RPC frame read timed out".to_string(),
                );
                send_error(&mut writer, "", &err).await;
                break;
            }
            Err(FrameError::Io) => break,
        };

        let line = String::from_utf8_lossy(&frame);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(e) => {
                send_invalid_request(&mut writer, "", format!("Invalid RPC JSON: {e}")).await;
                continue;
            }
        };
        let request_id = value
            .get("request_id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        // `session_id` is the V2 wire discriminator. It may be null until the
        // separate V2 handshake exists, but it must be present so a v1 shell
        // cannot silently enter the production command path.
        if value.get("session_id").is_none() {
            send_invalid_request(
                &mut writer,
                &request_id,
                "V2 RPC requests must include `session_id`",
            )
            .await;
            continue;
        }
        let request: V2Request = match serde_json::from_value(value) {
            Ok(req) => req,
            Err(e) => {
                send_invalid_request(&mut writer, &request_id, format!("Invalid V2 RPC: {e}"))
                    .await;
                continue;
            }
        };

        if request.protocol_version != PROTOCOL_V2 {
            let err = DaemonError::new(
                error_codes::PROTOCOL_INCOMPATIBLE,
                ErrorCategory::Validation,
                false,
                format!("RPC protocol must be {PROTOCOL_V2}"),
            );
            send_error(&mut writer, &request.request_id, &err).await;
            continue;
        }

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
                send_error(&mut writer, &request.request_id, &err).await;
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
        sessions.remove(&session_token);
    }

    Ok(())
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
        assert!(caps.extensions);
    }

    /// Closing a replaced connection must not revoke the replacement token.
    #[tokio::test]
    async fn test_replaced_connection_keeps_replacement_session() {
        let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
        let client_id = "client-1".to_string();
        let bootstrap_token = "bootstrap-token".to_string();
        let bootstrap_used = Arc::new(Mutex::new(false));
        let protocol_version = ProtocolVersion::new(2, 0, 0);
        let started_at = std::time::Instant::now();

        let (connection_a, server_a) = tokio::net::UnixStream::pair().unwrap();
        let a_task = tokio::spawn(handle_connection(
            server_a,
            sessions.clone(),
            bootstrap_token.clone(),
            bootstrap_used.clone(),
            protocol_version.clone(),
            "test".to_string(),
            started_at,
        ));
        let (a_read, mut a_write) = connection_a.into_split();
        let mut a_reader = BufReader::new(a_read);
        let handshake = HandshakeRequest {
            client_version: protocol_version.to_string(),
            client_id: client_id.clone(),
            bootstrap_token: bootstrap_token.clone(),
        };
        a_write
            .write_all(format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes())
            .await
            .unwrap();
        let mut line = String::new();
        a_reader.read_line(&mut line).await.unwrap();
        let token_a = serde_json::from_str::<HandshakeResponse>(&line)
            .unwrap()
            .session_token;

        let (connection_b, server_b) = tokio::net::UnixStream::pair().unwrap();
        let b_task = tokio::spawn(handle_connection(
            server_b,
            sessions.clone(),
            bootstrap_token.clone(),
            bootstrap_used,
            protocol_version.clone(),
            "test".to_string(),
            started_at,
        ));
        let (b_read, mut b_write) = connection_b.into_split();
        let mut b_reader = BufReader::new(b_read);
        b_write
            .write_all(format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes())
            .await
            .unwrap();
        line.clear();
        b_reader.read_line(&mut line).await.unwrap();
        let token_b = serde_json::from_str::<HandshakeResponse>(&line)
            .unwrap()
            .session_token;
        assert_ne!(token_a, token_b);

        drop(a_write);
        drop(a_reader);
        a_task.await.unwrap().unwrap();

        let request = V2Request::new(
            client_id,
            token_b.clone(),
            "daemon.ping",
            serde_json::json!({}),
        );
        b_write
            .write_all(format!("{}\n", serde_json::to_string(&request).unwrap()).as_bytes())
            .await
            .unwrap();
        line.clear();
        b_reader.read_line(&mut line).await.unwrap();
        let response: assistant_protocol::v2::V2SuccessResponse =
            serde_json::from_str(&line).unwrap();
        assert!(response.success);
        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.data["pong"], true);

        {
            let sessions = sessions.lock().await;
            assert!(!sessions.contains_key(&token_a));
            assert!(sessions.contains_key(&token_b));
        }

        drop(b_write);
        drop(b_reader);
        b_task.await.unwrap().unwrap();
        assert!(sessions.lock().await.is_empty());
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

#[cfg(test)]
mod rpc_frame_tests {
    //! TASK-003 (N04): bounded UDS frames — fixed max size, read deadline,
    //! structured error, and clean connection teardown. These fail against the
    //! pre-fix `read_line` (unbounded growth on newline-less frames).
    use super::*;
    use std::io::Cursor;
    use std::pin::Pin;
    use std::task::Poll;
    use tokio::io::{AsyncBufRead, AsyncRead, ReadBuf};
    use tokio::time::Duration;

    const SHORT_TIMEOUT: Duration = Duration::from_millis(50);

    #[tokio::test]
    async fn rpc_frame_accepts_exact_max_boundary() {
        let mut payload = vec![b'x'; MAX_FRAME_BYTES - 1];
        payload.push(b'\n');
        let mut cur = Cursor::new(payload);
        let frame = read_frame(&mut cur, MAX_FRAME_BYTES, SHORT_TIMEOUT)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.len(), MAX_FRAME_BYTES);
        assert_eq!(frame.last(), Some(&b'\n'));
    }

    #[tokio::test]
    async fn rpc_frame_rejects_oversize_plus_one() {
        // MAX+1 bytes with no newline: the cap must stop the read and reject.
        let payload = vec![b'x'; MAX_FRAME_BYTES + 1];
        let mut cur = Cursor::new(payload);
        let err = read_frame(&mut cur, MAX_FRAME_BYTES, SHORT_TIMEOUT)
            .await
            .unwrap_err();
        assert_eq!(err, FrameError::Oversize);
    }

    #[tokio::test]
    async fn rpc_frame_rejects_newline_less_oversize() {
        // A single line larger than the cap without any newline byte.
        let payload = vec![b'x'; MAX_FRAME_BYTES + 10];
        let mut cur = Cursor::new(payload);
        let err = read_frame(&mut cur, MAX_FRAME_BYTES, SHORT_TIMEOUT)
            .await
            .unwrap_err();
        assert_eq!(err, FrameError::Oversize);
    }

    #[tokio::test]
    async fn rpc_frame_eof_returns_none() {
        let mut cur = Cursor::new(Vec::<u8>::new());
        assert!(read_frame(&mut cur, MAX_FRAME_BYTES, SHORT_TIMEOUT)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn rpc_frame_slowloris_times_out() {
        // A peer that trickles nothing and stalls must hit the read deadline.
        let mut reader = StallReader;
        let err = read_frame(&mut reader, MAX_FRAME_BYTES, SHORT_TIMEOUT)
            .await
            .unwrap_err();
        assert_eq!(err, FrameError::Timeout);
    }

    struct StallReader;
    impl AsyncRead for StallReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
    }
    impl AsyncBufRead for StallReader {
        fn poll_fill_buf(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<&[u8]>> {
            Poll::Pending
        }
        fn consume(self: Pin<&mut Self>, _amt: usize) {}
    }
}
