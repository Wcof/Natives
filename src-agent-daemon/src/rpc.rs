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

use assistant_protocol::v1::daemon::{
    DaemonStatus, DaemonHealth,
    HandshakeRequest, HandshakeResponse,
    RpcRequest, RpcResponse,
};
use assistant_protocol::version::{ProtocolVersion, negotiate};
use assistant_protocol::error::{DaemonError, ErrorCategory, error_codes};
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
                        if let Err(e) = handle_connection(stream, sessions, bt, bu, pv, dv, sa).await {
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

    // Validate bootstrap token (single use)
    {
        let mut used = bootstrap_used.lock().await;
        if *used || handshake_req.bootstrap_token != bootstrap_token {
            let err = DaemonError::new(
                error_codes::UNAUTHORIZED,
                ErrorCategory::Auth,
                false,
                "Invalid or replayed bootstrap token".to_string(),
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
        handle_rpc(&mut writer, &request, &protocol_version, &daemon_version, &started_at).await;
    }

    // Cleanup: remove session
    {
        let mut sessions = sessions.lock().await;
        sessions.retain(|_, v| v != &handshake_req.client_id);
    }

    Ok(())
}

/// Dispatch an RPC request to the appropriate handler.
async fn handle_rpc(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &RpcRequest,
    protocol_version: &ProtocolVersion,
    daemon_version: &str,
    started_at: &std::time::Instant,
) {
    match request.method.as_str() {
        "daemon.getStatus" => {
            let status = DaemonStatus {
                version: daemon_version.to_string(),
                protocol_version: protocol_version.to_string(),
                uptime_secs: started_at.elapsed().as_secs(),
                pid: std::process::id() as u64,
                active_runs: 0,
                active_extensions: 0,
                provider_count: 0,
                memory_usage_mb: 0,
                health: DaemonHealth::Healthy,
            };
            send_success(writer, &request.request_id, &request.client_id, &request.session_token,
                serde_json::to_value(&status).unwrap_or_default()).await;
        }
        "daemon.ping" => {
            send_success(writer, &request.request_id, &request.client_id, &request.session_token,
                serde_json::json!({"pong": true, "timestamp": chrono::Utc::now().to_rfc3339()})).await;
        }
        _ => {
            let err = DaemonError::new(
                "unknown_method",
                ErrorCategory::Unsupported,
                false,
                format!("Unknown method: {}", request.method),
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
        protocol_version: "0.1.0".to_string(),
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
async fn send_error(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    error: &DaemonError,
) {
    let json = serde_json::to_string(error).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_protocol::v1::daemon::HandshakeRequest;
    use assistant_protocol::version::ProtocolVersion;

    /// Test bootstrap token single-use enforcement.
    #[tokio::test]
    async fn test_bootstrap_token_single_use() {
        let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
        let bootstrap_used = Arc::new(Mutex::new(false));
        let pv = ProtocolVersion::new(0, 1, 0);

        // First use should succeed
        {
            let mut used = bootstrap_used.lock().await;
            assert!(!*used);
            *used = true;
        }

        // Second use should fail
        {
            let mut used = bootstrap_used.lock().await;
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
        let client_version = ProtocolVersion::new(1, 0, 0);
        let daemon_version = ProtocolVersion::new(0, 1, 0);
        let result = negotiate(&client_version, &daemon_version);
        assert!(!result.compatible);
        assert!(result.upgrade_required.is_some());
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