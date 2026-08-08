//! UDS client for talking to an independent Agent Daemon process.
//!
//! Production target (G4): Tauri host is a client only — it must not share an
//! in-process `global_run_manager()` with a separate sidecar binary.
//!
//! Usage:
//! ```ignore
//! let mut c = DaemonClient::connect("/tmp/natives-agent.sock", "bootstrap-token", "2.0.0").await?;
//! let run = c.call("run.start", serde_json::json!({ ... })).await?;
//! ```

use crate::rpc::{read_frame, FrameError, FRAME_READ_TIMEOUT, MAX_FRAME_BYTES};
use assistant_protocol::v1::daemon::{
    HandshakeRequest, HandshakeResponse, RpcRequest, RpcResponse,
};
use assistant_protocol::v2::{RunEventV2, PROTOCOL_V2};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use uuid::Uuid;

/// Errors from the UDS daemon client.
#[derive(Debug, thiserror::Error)]
pub enum DaemonClientError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("handshake rejected: {0}")]
    Handshake(String),
    #[error("rpc failed: {0}")]
    Rpc(String),
    #[error("protocol error: {0}")]
    Protocol(String),
}

/// Parse one newline-delimited handshake response body.
pub(crate) fn parse_handshake_response_line(
    line: &str,
) -> Result<HandshakeResponse, DaemonClientError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(DaemonClientError::Handshake(
            "empty handshake response".into(),
        ));
    }

    // Prefer explicit JSON inspection so DaemonError bodies (legacy rejects)
    // never coerce into an empty HandshakeResponse via serde defaults.
    let value: Value =
        serde_json::from_str(trimmed).map_err(|e| DaemonClientError::Handshake(e.to_string()))?;

    // Daemon auth/protocol failures historically serialized as DaemonError.
    if let Some(msg) = value
        .get("technical_message")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        let code = value
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("handshake_error");
        return Err(DaemonClientError::Handshake(format!("{code}: {msg}")));
    }

    // Structured reject (accepted=false) — always a Handshake error for UI.
    if value.get("accepted") == Some(&Value::Bool(false)) {
        let upgrade = value
            .get("upgrade_required")
            .or_else(|| value.get("upgradeRequired"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("handshake not accepted");
        return Err(DaemonClientError::Handshake(upgrade.to_string()));
    }

    // Success path requires a non-empty session token.
    let token = value
        .get("session_token")
        .or_else(|| value.get("sessionToken"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if token.trim().is_empty() {
        return Err(DaemonClientError::Handshake(
            "missing field `session_token` in handshake response".into(),
        ));
    }

    match serde_json::from_value::<HandshakeResponse>(value) {
        Ok(mut resp) => {
            // Normalize aliases if deserializer left defaults.
            if resp.session_token.trim().is_empty() {
                resp.session_token = token;
            }
            if !resp.accepted {
                return Err(DaemonClientError::Handshake(
                    resp.upgrade_required
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| "handshake not accepted".into()),
                ));
            }
            Ok(resp)
        }
        Err(e) => Err(DaemonClientError::Handshake(e.to_string())),
    }
}

/// Client for an independent Agent Daemon over Unix Domain Socket.
///
/// Stores bootstrap material so a dropped connection can re-handshake once
/// without the caller re-supplying secrets from ordinary logs.
pub struct DaemonClient {
    socket_path: PathBuf,
    writer: tokio::net::unix::OwnedWriteHalf,
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    session_token: String,
    client_id: String,
    protocol_version: String,
    bootstrap_token: String,
    client_version: String,
    /// `Some` once [`DaemonClient::begin_watch`] has consumed the ACK. While in
    /// stream mode only [`DaemonClient::read_stream_frame`] may be used — the
    /// connection is dedicated to the `run.watch` stream.
    stream_mode: Option<StreamWatchState>,
}

/// Per-stream bookkeeping while the client is reading a `run.watch` stream.
pub struct StreamWatchState {
    pub run_id: String,
    pub last_activity: std::time::Instant,
}

impl DaemonClient {
    /// Connect, handshake with bootstrap token, return authenticated client.
    pub async fn connect(
        socket_path: impl AsRef<Path>,
        bootstrap_token: &str,
        client_version: &str,
    ) -> Result<Self, DaemonClientError> {
        let socket_path = socket_path.as_ref().to_path_buf();
        let client_id = format!("tauri-{}", Uuid::new_v4());
        let (writer, reader, session_token, protocol_version) =
            Self::handshake_stream(&socket_path, bootstrap_token, client_version, &client_id)
                .await?;

        Ok(Self {
            socket_path,
            writer,
            reader,
            session_token,
            client_id,
            protocol_version,
            bootstrap_token: bootstrap_token.to_string(),
            client_version: client_version.to_string(),
            stream_mode: None,
        })
    }

    async fn handshake_stream(
        socket_path: &Path,
        bootstrap_token: &str,
        client_version: &str,
        client_id: &str,
    ) -> Result<
        (
            tokio::net::unix::OwnedWriteHalf,
            BufReader<tokio::net::unix::OwnedReadHalf>,
            String,
            String,
        ),
        DaemonClientError,
    > {
        let stream = UnixStream::connect(socket_path).await?;
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = write_half;

        let handshake = HandshakeRequest {
            client_version: client_version.to_string(),
            client_id: client_id.to_string(),
            bootstrap_token: bootstrap_token.to_string(),
        };
        let line = serde_json::to_string(&handshake)?;
        writer.write_all(line.as_bytes()).await?;
        writer.write_all(b"\n").await?;

        let resp_frame = match read_frame(&mut reader, MAX_FRAME_BYTES, FRAME_READ_TIMEOUT).await {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                return Err(DaemonClientError::Handshake(
                    "empty handshake response".into(),
                ))
            }
            Err(FrameError::Oversize) => {
                return Err(DaemonClientError::Protocol(format!(
                    "handshake response exceeds {MAX_FRAME_BYTES} bytes"
                )))
            }
            Err(FrameError::Timeout) => {
                return Err(DaemonClientError::Protocol(
                    "handshake response frame read timed out".into(),
                ))
            }
            Err(FrameError::Io) => {
                return Err(DaemonClientError::Io(std::io::Error::other(
                    "frame read failed",
                )))
            }
        };
        let resp_line = String::from_utf8_lossy(&resp_frame);
        let handshake_resp = parse_handshake_response_line(resp_line.trim())?;
        if !handshake_resp.accepted {
            return Err(DaemonClientError::Handshake(
                handshake_resp
                    .upgrade_required
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "handshake not accepted".into()),
            ));
        }
        if handshake_resp.session_token.trim().is_empty() {
            return Err(DaemonClientError::Handshake(
                "handshake accepted but session_token is empty".into(),
            ));
        }
        Ok((
            writer,
            reader,
            handshake_resp.session_token,
            handshake_resp.protocol_version,
        ))
    }

    /// Re-handshake after disconnect (uses stored bootstrap; never logs it).
    pub async fn reconnect(&mut self) -> Result<(), DaemonClientError> {
        let (writer, reader, session_token, protocol_version) = Self::handshake_stream(
            &self.socket_path,
            &self.bootstrap_token,
            &self.client_version,
            &self.client_id,
        )
        .await?;
        self.writer = writer;
        self.reader = reader;
        self.session_token = session_token;
        self.protocol_version = protocol_version;
        self.stream_mode = None;
        Ok(())
    }

    pub fn session_token(&self) -> &str {
        &self.session_token
    }

    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Call one RPC method and wait for a single response line.
    /// On connection closed, attempts one reconnect + retry (session refresh).
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, DaemonClientError> {
        match self.call_once(method, params.clone()).await {
            Ok(v) => Ok(v),
            Err(DaemonClientError::Protocol(msg)) if msg.contains("connection closed") => {
                match self.reconnect().await {
                    Ok(()) => self.call_once(method, params).await,
                    Err(DaemonClientError::Handshake(_)) => {
                        let mut fresh = self.reconnect_from_current_env().await?;
                        let result = fresh.call_once(method, params).await;
                        if result.is_ok() {
                            *self = fresh;
                        }
                        result
                    }
                    Err(error) => Err(error),
                }
            }
            Err(DaemonClientError::Handshake(_)) => {
                let mut fresh = self.reconnect_from_current_env().await?;
                let result = fresh.call_once(method, params).await;
                if result.is_ok() {
                    *self = fresh;
                }
                result
            }
            Err(e) => Err(e),
        }
    }

    /// Reconnect once with the supervisor's current bootstrap after a sidecar
    /// restart. The cached client may only know the previous bootstrap.
    async fn reconnect_from_current_env(&self) -> Result<DaemonClient, DaemonClientError> {
        let socket = std::env::var_os("NATIVES_DAEMON_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.socket_path.clone());
        let bootstrap = std::env::var("NATIVES_DAEMON_BOOTSTRAP")
            .unwrap_or_else(|_| self.bootstrap_token.clone());
        DaemonClient::connect(socket, &bootstrap, &self.client_version).await
    }

    async fn call_once(&mut self, method: &str, params: Value) -> Result<Value, DaemonClientError> {
        if self.stream_mode.is_some() {
            return Err(DaemonClientError::Protocol(
                "client is in run.watch stream mode; call() is not available".into(),
            ));
        }
        let request_id = Uuid::new_v4().to_string();
        let req = RpcRequest {
            protocol_version: self.protocol_version.clone(),
            request_id: request_id.clone(),
            client_id: self.client_id.clone(),
            session_token: self.session_token.clone(),
            method: method.to_string(),
            params,
        };
        let line = serde_json::to_string(&req)?;
        if self.writer.write_all(line.as_bytes()).await.is_err()
            || self.writer.write_all(b"\n").await.is_err()
        {
            return Err(DaemonClientError::Protocol("connection closed".into()));
        }

        let resp_frame =
            match read_frame(&mut self.reader, MAX_FRAME_BYTES, FRAME_READ_TIMEOUT).await {
                Ok(Some(frame)) => frame,
                Ok(None) => return Err(DaemonClientError::Protocol("connection closed".into())),
                Err(FrameError::Oversize) => {
                    return Err(DaemonClientError::Protocol(format!(
                        "response frame exceeds {MAX_FRAME_BYTES} bytes"
                    )))
                }
                Err(FrameError::Timeout) => {
                    return Err(DaemonClientError::Protocol(
                        "response frame read timed out".into(),
                    ))
                }
                Err(FrameError::Io) => {
                    return Err(DaemonClientError::Io(std::io::Error::other(
                        "frame read failed",
                    )))
                }
            };
        let resp_line = String::from_utf8_lossy(&resp_frame);
        if let Ok(resp) = serde_json::from_str::<RpcResponse>(resp_line.trim()) {
            if resp.success {
                return Ok(resp.data.unwrap_or(Value::Null));
            }
            let msg = resp
                .error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "rpc failed".into());
            return Err(DaemonClientError::Rpc(msg));
        }
        Err(DaemonClientError::Rpc(resp_line.trim().to_string()))
    }

    /// Read the next event line on a `run.watch` connection (A3 EventClient).
    ///
    /// The server writes one `V2EventEnvelope` per line and closes the stream
    /// on terminal / cancel / disconnect. Returns `None` on clean close and an
    /// error on transport failure so the caller can decide whether to reconnect
    /// with `after_sequence`.
    pub async fn read_event(&mut self) -> Option<Result<RunEventV2, DaemonClientError>> {
        use assistant_protocol::v2::RunEventV2;
        let frame = match read_frame(&mut self.reader, MAX_FRAME_BYTES, FRAME_READ_TIMEOUT).await {
            Ok(Some(frame)) => frame,
            Ok(None) => return None, // clean close
            Err(FrameError::Oversize) => {
                return Some(Err(DaemonClientError::Protocol(format!(
                    "event frame exceeds {MAX_FRAME_BYTES} bytes"
                ))))
            }
            Err(FrameError::Timeout) => {
                return Some(Err(DaemonClientError::Protocol(
                    "event frame read timed out".into(),
                )))
            }
            Err(FrameError::Io) => {
                return Some(Err(DaemonClientError::Io(std::io::Error::other(
                    "event frame read failed",
                ))))
            }
        };
        let line = String::from_utf8_lossy(&frame);
        // The server writes V2EventEnvelope lines. Decode into a RunEventV2
        // shape (run_id/sequence/type/payload) so callers see one uniform view.
        match serde_json::from_str::<serde_json::Value>(line.trim()) {
            Ok(value) => {
                let run_id = value
                    .get("run_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let sequence = value.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0);
                let payload_value = value.get("payload").cloned().unwrap_or(Value::Null);
                // A2's server serializes the RunEventKind payload with the
                // serde `type` tag, so it decodes straight back into the enum.
                match serde_json::from_value::<assistant_protocol::v2::RunEventKind>(payload_value)
                {
                    Ok(kind) => Some(Ok(RunEventV2::new(run_id, sequence, kind))),
                    Err(e) => Some(Err(DaemonClientError::Json(e))),
                }
            }
            Err(e) => Some(Err(DaemonClientError::Json(e))),
        }
    }

    /// Open a persistent `run.watch` (RunWatchStreamV2) connection and consume
    /// the ordinary RPC ACK (`STREAM-CONTRACT-V2`).
    ///
    /// After this call the client is in **stream mode**: only
    /// [`DaemonClient::read_stream_frame`] may be used. The ACK is a normal
    /// `RpcResponse`; every later line is a [`RunStreamFrameV2`]. The 30s
    /// frame timeout is safe because the daemon sends heartbeats on idle.
    pub async fn begin_watch(
        &mut self,
        run_id: &str,
        after_durable_sequence: u64,
        after_live_sequence: u64,
    ) -> Result<(), DaemonClientError> {
        if self.stream_mode.is_some() {
            return Err(DaemonClientError::Protocol(
                "begin_watch called on a client already in stream mode".into(),
            ));
        }
        let request_id = Uuid::new_v4().to_string();
        let req = RpcRequest {
            protocol_version: self.protocol_version.clone(),
            request_id: request_id.clone(),
            client_id: self.client_id.clone(),
            session_token: self.session_token.clone(),
            method: "run.watch".to_string(),
            params: serde_json::json!({
                "run_id": run_id,
                "after_durable_sequence": after_durable_sequence,
                "after_live_sequence": after_live_sequence,
            }),
        };
        let line = serde_json::to_string(&req)?;
        if self.writer.write_all(line.as_bytes()).await.is_err()
            || self.writer.write_all(b"\n").await.is_err()
        {
            return Err(DaemonClientError::Protocol("connection closed".into()));
        }

        // First response must be the ordinary RPC ACK.
        let ack_frame =
            match read_frame(&mut self.reader, MAX_FRAME_BYTES, FRAME_READ_TIMEOUT).await {
                Ok(Some(frame)) => frame,
                Ok(None) => return Err(DaemonClientError::Protocol("connection closed".into())),
                Err(FrameError::Oversize) => {
                    return Err(DaemonClientError::Protocol(format!(
                        "run.watch ACK frame exceeds {MAX_FRAME_BYTES} bytes"
                    )))
                }
                Err(FrameError::Timeout) => {
                    return Err(DaemonClientError::Protocol(
                        "run.watch ACK frame read timed out".into(),
                    ))
                }
                Err(FrameError::Io) => {
                    return Err(DaemonClientError::Io(std::io::Error::other(
                        "run.watch ACK frame read failed",
                    )))
                }
            };
        let ack_line = String::from_utf8_lossy(&ack_frame);
        let resp: RpcResponse =
            serde_json::from_str(ack_line.trim()).map_err(DaemonClientError::Json)?;
        if !resp.success {
            let msg = resp
                .error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "run.watch failed".into());
            return Err(DaemonClientError::Rpc(msg));
        }
        // Validate the ACK advertises stream version 2 (frozen contract).
        let data = resp.data.unwrap_or(Value::Null);
        let stream = data.get("stream").and_then(|v| v.as_str()).unwrap_or("");
        let version = data
            .get("streamVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if stream != crate::stream_protocol::STREAM_METHOD
            || version != crate::stream_protocol::STREAM_VERSION
        {
            return Err(DaemonClientError::Protocol(format!(
                "unexpected run.watch ACK: stream={stream:?} version={version}"
            )));
        }
        self.stream_mode = Some(StreamWatchState {
            run_id: run_id.to_string(),
            last_activity: std::time::Instant::now(),
        });
        Ok(())
    }

    /// Read the next frame on a `run.watch` (RunWatchStreamV2) connection.
    ///
    /// Returns `None` on clean close, or the decoded [`RunStreamFrameV2`].
    /// Heartbeats reset the idle clock so a healthy stream never trips the 30s
    /// frame timeout — only a truly silent stream does.
    pub async fn read_stream_frame(
        &mut self,
    ) -> Option<Result<crate::stream_protocol::RunStreamFrameV2, DaemonClientError>> {
        let frame = match read_frame(&mut self.reader, MAX_FRAME_BYTES, FRAME_READ_TIMEOUT).await {
            Ok(Some(frame)) => frame,
            Ok(None) => return None, // clean close (terminal / cancel / disconnect)
            Err(FrameError::Oversize) => {
                return Some(Err(DaemonClientError::Protocol(format!(
                    "run.watch frame exceeds {MAX_FRAME_BYTES} bytes"
                ))))
            }
            Err(FrameError::Timeout) => {
                return Some(Err(DaemonClientError::Protocol(
                    "run.watch frame read timed out (no heartbeat)".into(),
                )))
            }
            Err(FrameError::Io) => {
                return Some(Err(DaemonClientError::Io(std::io::Error::other(
                    "run.watch frame read failed",
                ))))
            }
        };
        let line = String::from_utf8_lossy(&frame);
        match serde_json::from_str::<serde_json::Value>(line.trim()) {
            Ok(value) => {
                if value
                    .get("frame_type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t == "heartbeat")
                {
                    if let Some(st) = self.stream_mode.as_mut() {
                        st.last_activity = std::time::Instant::now();
                    }
                }
                match serde_json::from_value::<crate::stream_protocol::RunStreamFrameV2>(value) {
                    Ok(frame) => Some(Ok(frame)),
                    Err(e) => Some(Err(DaemonClientError::Json(e))),
                }
            }
            Err(e) => Some(Err(DaemonClientError::Json(e))),
        }
    }
}

/// Resolve how the host should reach Run Authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunAuthorityMode {
    /// In-process `global_run_manager()` (embedded / tests / transitional).
    Embedded,
    /// Independent sidecar via UDS (`NATIVES_DAEMON_SOCKET` + bootstrap).
    Uds {
        socket: PathBuf,
        bootstrap_token: String,
    },
}

fn default_daemon_socket() -> PathBuf {
    std::env::var("NATIVES_DAEMON_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir().join("natives"));
            runtime_dir.join("natives-agent.sock")
        })
}

/// Read env:
/// - `NATIVES_DAEMON_MODE=embedded` → in-process (tests / explicit dev only)
/// - `uds` / `sidecar` / `remote` → UDS required (**no** silent embedded fallback)
/// - `auto` → UDS if socket+bootstrap present, else embedded (**dev only**, never default)
/// - unset → `embedded` under `cfg(test)`; otherwise **`uds`** (production default)
///
/// Production rule: missing socket/bootstrap under UDS mode is a hard failure at
/// the call site — this resolver never rewrites UDS → Embedded.
pub fn resolve_run_authority_mode() -> RunAuthorityMode {
    let mode = std::env::var("NATIVES_DAEMON_MODE").unwrap_or_else(|_| {
        if cfg!(test) {
            "embedded".into()
        } else {
            // Production default: UDS only. Dev without sidecar must set
            // NATIVES_DAEMON_MODE=embedded or auto explicitly.
            "uds".into()
        }
    });
    let mode = mode.to_ascii_lowercase();
    let socket = default_daemon_socket();
    let bootstrap_token =
        std::env::var("NATIVES_DAEMON_BOOTSTRAP").unwrap_or_else(|_| String::new());

    match mode.as_str() {
        "embedded" | "inprocess" | "in-process" => RunAuthorityMode::Embedded,
        "uds" | "sidecar" | "remote" => RunAuthorityMode::Uds {
            socket,
            bootstrap_token,
        },
        "auto" => {
            // Explicit opt-in for local `cargo run` without installer.
            // Must NEVER be the production default.
            if socket.exists() && !bootstrap_token.is_empty() {
                RunAuthorityMode::Uds {
                    socket,
                    bootstrap_token,
                }
            } else {
                RunAuthorityMode::Embedded
            }
        }
        other => {
            // Unknown values fail closed to UDS (production-safe) rather than embedded.
            let _ = other;
            RunAuthorityMode::Uds {
                socket,
                bootstrap_token,
            }
        }
    }
}

/// Returns true when mode requires UDS and forbids silent embedded fallback.
pub fn mode_requires_uds() -> bool {
    matches!(resolve_run_authority_mode(), RunAuthorityMode::Uds { .. }) && {
        let m = std::env::var("NATIVES_DAEMON_MODE")
            .unwrap_or_else(|_| {
                if cfg!(test) {
                    "embedded".into()
                } else {
                    "uds".into()
                }
            })
            .to_ascii_lowercase();
        !matches!(m.as_str(), "auto" | "embedded" | "inprocess" | "in-process")
    }
}

/// Convenience: PROTOCOL_V2 string for handshake client_version.
pub fn client_protocol_version() -> &'static str {
    PROTOCOL_V2
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; serialize tests that mutate them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn explicit_embedded_mode() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        let prev_sock = std::env::var("NATIVES_DAEMON_SOCKET").ok();
        let prev_boot = std::env::var("NATIVES_DAEMON_BOOTSTRAP").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
        std::env::remove_var("NATIVES_DAEMON_SOCKET");
        std::env::remove_var("NATIVES_DAEMON_BOOTSTRAP");
        assert_eq!(resolve_run_authority_mode(), RunAuthorityMode::Embedded);
        restore_env("NATIVES_DAEMON_MODE", prev_mode);
        restore_env("NATIVES_DAEMON_SOCKET", prev_sock);
        restore_env("NATIVES_DAEMON_BOOTSTRAP", prev_boot);
    }

    #[test]
    fn uds_mode_from_env() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        let prev_sock = std::env::var("NATIVES_DAEMON_SOCKET").ok();
        let prev_boot = std::env::var("NATIVES_DAEMON_BOOTSTRAP").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "uds");
        std::env::set_var("NATIVES_DAEMON_SOCKET", "/tmp/test-natives.sock");
        std::env::set_var("NATIVES_DAEMON_BOOTSTRAP", "tok");
        match resolve_run_authority_mode() {
            RunAuthorityMode::Uds {
                socket,
                bootstrap_token,
            } => {
                assert_eq!(socket, PathBuf::from("/tmp/test-natives.sock"));
                assert_eq!(bootstrap_token, "tok");
            }
            other => panic!("expected Uds, got {other:?}"),
        }
        restore_env("NATIVES_DAEMON_MODE", prev_mode);
        restore_env("NATIVES_DAEMON_SOCKET", prev_sock);
        restore_env("NATIVES_DAEMON_BOOTSTRAP", prev_boot);
    }

    #[test]
    fn uds_mode_does_not_silently_become_embedded_without_socket() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        let prev_sock = std::env::var("NATIVES_DAEMON_SOCKET").ok();
        let prev_boot = std::env::var("NATIVES_DAEMON_BOOTSTRAP").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "uds");
        std::env::set_var(
            "NATIVES_DAEMON_SOCKET",
            "/tmp/natives-missing-uds-socket.sock",
        );
        std::env::remove_var("NATIVES_DAEMON_BOOTSTRAP");
        // Critical: still Uds — never rewrite to Embedded when socket missing.
        assert!(
            matches!(resolve_run_authority_mode(), RunAuthorityMode::Uds { .. }),
            "UDS mode must not silent-fallback to Embedded"
        );
        assert!(mode_requires_uds());
        restore_env("NATIVES_DAEMON_MODE", prev_mode);
        restore_env("NATIVES_DAEMON_SOCKET", prev_sock);
        restore_env("NATIVES_DAEMON_BOOTSTRAP", prev_boot);
    }

    #[test]
    fn auto_is_explicit_dev_only_and_may_embed() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
        let prev_sock = std::env::var("NATIVES_DAEMON_SOCKET").ok();
        let prev_boot = std::env::var("NATIVES_DAEMON_BOOTSTRAP").ok();
        std::env::set_var("NATIVES_DAEMON_MODE", "auto");
        std::env::set_var("NATIVES_DAEMON_SOCKET", "/tmp/natives-auto-no-socket.sock");
        std::env::remove_var("NATIVES_DAEMON_BOOTSTRAP");
        assert_eq!(resolve_run_authority_mode(), RunAuthorityMode::Embedded);
        assert!(!mode_requires_uds());
        restore_env("NATIVES_DAEMON_MODE", prev_mode);
        restore_env("NATIVES_DAEMON_SOCKET", prev_sock);
        restore_env("NATIVES_DAEMON_BOOTSTRAP", prev_boot);
    }

    fn restore_env(key: &str, prev: Option<String>) {
        if let Some(v) = prev {
            std::env::set_var(key, v);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn parse_handshake_response_line_maps_daemon_error() {
        let line = r#"{"code":"unauthorized","category":"auth","retryable":false,"user_message_key":"error.unauthorized","technical_message":"Invalid bootstrap token","recovery_actions":[],"correlation_id":"00000000-0000-0000-0000-000000000001"}"#;
        let err = parse_handshake_response_line(line).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid bootstrap token"), "{msg}");
        assert!(
            !msg.contains("missing field `session_token` at line"),
            "{msg}"
        );
    }

    #[test]
    fn parse_handshake_response_line_accepts_snake_case_success() {
        let line = r#"{"session_token":"tok","daemon_version":"0.1.0","protocol_version":"2.0.0","accepted":true,"upgrade_required":null}"#;
        let resp = parse_handshake_response_line(line).unwrap();
        assert!(resp.accepted);
        assert_eq!(resp.session_token, "tok");
    }

    #[test]
    fn parse_handshake_response_line_accepts_camel_case_aliases() {
        let line = r#"{"sessionToken":"tok2","daemonVersion":"0.1.0","protocolVersion":"2.0.0","accepted":true}"#;
        let resp = parse_handshake_response_line(line).unwrap();
        assert!(resp.accepted);
        assert_eq!(resp.session_token, "tok2");
    }
}
