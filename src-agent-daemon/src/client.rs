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

use assistant_protocol::v1::daemon::{HandshakeRequest, HandshakeResponse, RpcRequest, RpcResponse};
use assistant_protocol::v2::PROTOCOL_V2;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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
        let (writer, reader, session_token, protocol_version) = Self::handshake_stream(
            &socket_path,
            bootstrap_token,
            client_version,
            &client_id,
        )
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

        let mut resp_line = String::new();
        let n = reader.read_line(&mut resp_line).await?;
        if n == 0 {
            return Err(DaemonClientError::Handshake(
                "empty handshake response".into(),
            ));
        }
        let handshake_resp: HandshakeResponse = serde_json::from_str(resp_line.trim())
            .map_err(|e| DaemonClientError::Handshake(e.to_string()))?;
        if !handshake_resp.accepted {
            return Err(DaemonClientError::Handshake(
                handshake_resp
                    .upgrade_required
                    .unwrap_or_else(|| "handshake not accepted".into()),
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
    async fn reconnect_from_current_env(
        &self,
    ) -> Result<DaemonClient, DaemonClientError> {
        let socket = std::env::var_os("NATIVES_DAEMON_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.socket_path.clone());
        let bootstrap = std::env::var("NATIVES_DAEMON_BOOTSTRAP")
            .unwrap_or_else(|_| self.bootstrap_token.clone());
        DaemonClient::connect(socket, &bootstrap, &self.client_version).await
    }

    async fn call_once(&mut self, method: &str, params: Value) -> Result<Value, DaemonClientError> {
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

        let mut resp_line = String::new();
        let n = self.reader.read_line(&mut resp_line).await?;
        if n == 0 {
            return Err(DaemonClientError::Protocol("connection closed".into()));
        }
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
}

/// Resolve how the host should reach Run Authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunAuthorityMode {
    /// In-process `global_run_manager()` (embedded / tests / transitional).
    Embedded,
    /// Independent sidecar via UDS (`NATIVES_DAEMON_SOCKET` + bootstrap).
    Uds { socket: PathBuf, bootstrap_token: String },
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
    matches!(
        resolve_run_authority_mode(),
        RunAuthorityMode::Uds { .. }
    ) && {
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
        std::env::set_var(
            "NATIVES_DAEMON_SOCKET",
            "/tmp/natives-auto-no-socket.sock",
        );
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
}
