//! Daemon-side Credential Broker: **pure UDS lease client** (NE-P0-02 / 19.1).
//!
//! The Agent Daemon no longer opens the Host-authoritative `natives.db` and no
//! longer reads `provider_kek` / decrypts keys itself. Every credential-bearing
//! read is a short-TTL, Run-bound, revocable **lease request** sent over the
//! authenticated broker Unix socket to the Tauri Host broker
//! (`src-tauri/src/credential_broker.rs`).
//!
//! Security:
//! - Plaintext key material is only ever the *response* of a lease request and
//!   is dropped when the caller drops the value. It is never cached in daemon
//!   memory, never written to assistant.db, never logged, and never emitted as
//!   an engine event.
//! - Errors are redacted before they leave this module.
//! - The daemon never writes natives.db: the Host is the only writer.

use assistant_protocol::v2::credential::{
    CredentialBrokerRequest, CredentialBrokerResponse, CredentialLeaseEnvelope,
    CredentialLeaseReply, CredentialLeaseRevokeRequest, CredentialPoolLeaseRequest,
    CredentialPoolLeaseResponse, CredentialSecretLeaseRequest, CredentialSecretLeaseResponse,
    CredentialSettingLeaseRequest, CredentialSettingLeaseResponse, HostSubagentRow,
    HostSubagentsLeaseRequest, HostSubagentsLeaseResponse, LoopbackSettingsLeaseRequest,
    LoopbackSettingsLeaseResponse, RoutingPlanLeaseRequest, RoutingPlanLeaseResponse,
};
use assistant_protocol::v2::methods::names;
use provider_adapters::capabilities::Credential;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// UDS lease client for the Host Credential Broker. Holds no database handle
/// and no key material — only the broker socket endpoint.
pub struct NativesDbBroker {
    endpoint: PathBuf,
}

/// One decrypted Sub2API account lease. It is constructed per request and must
/// remain memory-only; the daemon never serializes this value into assistant.db.
#[derive(Debug, Clone)]
pub struct Sub2ApiAccountCredential {
    pub id: String,
    pub provider_id: String,
    pub platform: String,
    pub account_type: String,
    pub credentials: Value,
    pub extra: Value,
    pub priority: i64,
    pub concurrency: u32,
    pub expires_at: Option<String>,
    /// Account proxy wins over the global proxy. Both remain daemon-memory only.
    pub proxy_url: Option<String>,
}

/// Small, credential-safe projection of the Host-owned local routing settings.
/// The token only lives in this per-request value and is never persisted by the
/// daemon or emitted as an engine event.
#[derive(Debug, Clone)]
pub struct LoopbackSettings {
    pub enabled: bool,
    pub port: u16,
    pub bearer_token: Option<String>,
    pub rectifier: Value,
}

impl NativesDbBroker {
    /// Open a lease client for the Host Credential Broker.
    ///
    /// API-compatibility signature retained: callers pass the (now unused)
    /// natives.db path; the client actually resolves the broker socket from
    /// `NATIVES_BROKER_SOCKET` or the runtime directory. No database is opened.
    pub fn open(_path: impl AsRef<Path>) -> Result<Self, String> {
        let endpoint = default_broker_socket_path()?;
        Ok(Self { endpoint })
    }

    /// Open a lease client against the default broker socket endpoint.
    pub fn open_default() -> Result<Self, String> {
        let endpoint = default_broker_socket_path()?;
        Ok(Self { endpoint })
    }

    pub fn path(&self) -> &Path {
        &self.endpoint
    }

    pub fn endpoint(&self) -> &Path {
        &self.endpoint
    }

    /// Fail-closed write guard: the daemon must NOT write natives.db (P0-007).
    /// OAuth refresh persistence is the Host's job via broker lease; a call
    /// that reaches the daemon with a write intent is a caller bug.
    pub fn update_sub2api_credentials(
        &self,
        _account_id: &str,
        _credentials: &Value,
        _expires_at: Option<&str>,
    ) -> Result<(), String> {
        Err(
            "natives.db write blocked: daemon holds only credential leases (T104); \
             OAuth credential persistence must go through the Host broker"
                .into(),
        )
    }

    /// Resolve one provider key by requesting a Run-bound short-TTL lease from
    /// the Host broker over UDS. `key_id` may be `None` / empty / `_primary_`
    /// to select the primary active key for the provider.
    pub fn resolve(
        &self,
        provider_id: &str,
        key_id: Option<&str>,
        run_id: &str,
    ) -> Result<Credential, String> {
        if provider_id.trim().is_empty() {
            return Err("provider_id is required".into());
        }
        let key_id = key_id.unwrap_or("").trim().to_string();
        let req = CredentialBrokerRequest {
            key_id: if key_id.is_empty() || key_id == "_primary_" {
                "_primary_".into()
            } else {
                key_id
            },
            provider_id: provider_id.to_string(),
            run_id: run_id.to_string(),
            session_id: None,
        };
        let payload = serde_json::to_value(&req)
            .map_err(|e| redact_err(&format!("credential request serialize failed: {e}")))?;
        let data = self.lease_request(names::CREDENTIAL_LEASE_ACQUIRE, &payload)?;
        let resp: CredentialBrokerResponse = serde_json::from_value(data)
            .map_err(|e| redact_err(&format!("credential broker response parse failed: {e}")))?;
        credential_from_response(resp, run_id)
    }

    /// Request the active Sub2API account pool as a lease from the Host broker.
    pub fn resolve_sub2api_pool(
        &self,
        provider_id: &str,
    ) -> Result<Vec<Sub2ApiAccountCredential>, String> {
        if provider_id.trim().is_empty() {
            return Err("provider_id is required".into());
        }
        let req = CredentialPoolLeaseRequest {
            provider_id: provider_id.to_string(),
            run_id: "sub2api-pool".into(),
        };
        let payload = serde_json::to_value(&req)
            .map_err(|e| redact_err(&format!("pool request serialize failed: {e}")))?;
        let data = self.lease_request(names::CREDENTIAL_POOL_ACQUIRE, &payload)?;
        let resp: CredentialPoolLeaseResponse = serde_json::from_value(data)
            .map_err(|e| redact_err(&format!("broker pool response parse failed: {e}")))?;
        Ok(resp
            .accounts
            .into_iter()
            .map(|a| Sub2ApiAccountCredential {
                id: a.id,
                provider_id: a.provider_id,
                platform: a.platform,
                account_type: a.account_type,
                credentials: a.credentials,
                extra: a.extra,
                priority: a.priority,
                concurrency: a.concurrency,
                expires_at: a.expires_at,
                proxy_url: a.proxy_url,
            })
            .collect())
    }

    /// Request the Host-owned loopback routing settings (bearer token included)
    /// as a lease. Memory-only; the token is never persisted or logged.
    pub fn loopback_settings(&self) -> Result<LoopbackSettings, String> {
        let req = LoopbackSettingsLeaseRequest {
            run_id: "loopback".into(),
        };
        let payload = serde_json::to_value(&req)
            .map_err(|e| redact_err(&format!("routing request serialize failed: {e}")))?;
        let data = self.lease_request(names::CREDENTIAL_ROUTING_SETTINGS, &payload)?;
        let resp: LoopbackSettingsLeaseResponse = serde_json::from_value(data)
            .map_err(|e| redact_err(&format!("broker routing settings parse failed: {e}")))?;
        Ok(LoopbackSettings {
            enabled: resp.enabled,
            port: resp.port,
            bearer_token: resp.bearer_token,
            rectifier: resp.rectifier,
        })
    }

    /// Acquire the Host-owned routing plan as a lease (Daemon → Host). Serves
    /// `provider_routing_settings` + enabled `provider_route_bindings` so the
    /// daemon never opens natives.db (T104 / modular remediation W1).
    pub fn routing_plan(&self, run_id: &str) -> Result<RoutingPlanLeaseResponse, String> {
        let req = RoutingPlanLeaseRequest {
            run_id: run_id.to_string(),
        };
        let payload = serde_json::to_value(&req)
            .map_err(|e| redact_err(&format!("routing plan request serialize failed: {e}")))?;
        let data = self.lease_request(names::CREDENTIAL_ROUTING_PLAN, &payload)?;
        let resp: RoutingPlanLeaseResponse = serde_json::from_value(data)
            .map_err(|e| redact_err(&format!("routing plan response parse failed: {e}")))?;
        Ok(resp)
    }

    /// Export legacy Host `subagents` rows once via the broker lease
    /// (ADR-0016 / W1): the daemon imports Host-owned data without opening
    /// natives.db. Memory-only; instructions/tools only, never secrets.
    pub fn host_subagents(&self, run_id: &str) -> Result<HostSubagentsLeaseResponse, String> {
        let req = HostSubagentsLeaseRequest {
            run_id: run_id.to_string(),
        };
        let payload = serde_json::to_value(&req)
            .map_err(|e| redact_err(&format!("subagents request serialize failed: {e}")))?;
        let data = self.lease_request(names::HOST_SUBAGENTS_EXPORT, &payload)?;
        let resp: HostSubagentsLeaseResponse = serde_json::from_value(data)
            .map_err(|e| redact_err(&format!("subagents response parse failed: {e}")))?;
        Ok(resp)
    }

    /// Explicitly revoke a lease this daemon no longer needs (run finished or
    /// cancelled). Revocation is enforced by the Host; a revoked lease is
    /// rejected on any status/refresh probe.
    pub fn revoke_lease(&self, lease_id: &str, run_id: &str) -> Result<(), String> {
        let req = CredentialLeaseRevokeRequest {
            lease_id: lease_id.to_string(),
            run_id: run_id.to_string(),
        };
        let payload = serde_json::to_value(&req)
            .map_err(|e| redact_err(&format!("revoke request serialize failed: {e}")))?;
        self.lease_request(names::CREDENTIAL_LEASE_REVOKE, &payload)?;
        Ok(())
    }

    /// Send one typed lease request over the authenticated broker socket and
    /// return the typed payload `Value`. Errors are redacted and fail closed.
    #[cfg(unix)]
    fn lease_request(
        &self,
        method: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let stream = UnixStream::connect(&self.endpoint).map_err(|e| {
            redact_err(&format!(
                "credential broker unreachable at {}: {e}",
                self.endpoint.display()
            ))
        })?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut writer = stream;

        let envelope = CredentialLeaseEnvelope {
            method: method.to_string(),
            payload: payload.clone(),
        };
        let line = serde_json::to_string(&envelope)
            .map_err(|e| redact_err(&format!("credential broker request serialize failed: {e}")))?;
        writer
            .write_all(line.as_bytes())
            .map_err(|e| redact_err(&format!("credential broker write failed: {e}")))?;
        writer
            .write_all(b"\n")
            .map_err(|e| redact_err(&format!("credential broker write failed: {e}")))?;
        writer
            .flush()
            .map_err(|e| redact_err(&format!("credential broker flush failed: {e}")))?;

        let mut reply_line = String::new();
        let n = reader
            .read_line(&mut reply_line)
            .map_err(|e| redact_err(&format!("credential broker read failed: {e}")))?;
        if n == 0 {
            return Err("credential broker closed the connection".into());
        }
        let reply: CredentialLeaseReply = serde_json::from_str(reply_line.trim())
            .map_err(|e| redact_err(&format!("credential broker reply parse failed: {e}")))?;
        if reply.ok {
            reply
                .data
                .ok_or_else(|| "credential broker returned an empty reply".into())
        } else {
            Err(redact_err(
                reply
                    .error
                    .as_deref()
                    .unwrap_or("credential broker rejected the request"),
            ))
        }
    }

    /// Non-Unix stub: the broker lease channel requires a Unix socket endpoint.
    #[cfg(not(unix))]
    fn lease_request(
        &self,
        _method: &str,
        _payload: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err(
            "credential lease broker requires a Unix socket endpoint on this platform; \
             configure NATIVES_BROKER_SOCKET"
                .into(),
        )
    }
}

/// Convert a broker lease response into a provider `Credential`, enforcing the
/// lease contract fail-closed: an expired lease or a lease bound to a different
/// run is rejected even if the payload contains a valid key. The key material
/// is returned per-call and dropped by the caller; it is never cached here.
pub(crate) fn credential_from_response(
    resp: CredentialBrokerResponse,
    run_id: &str,
) -> Result<Credential, String> {
    if let Some(lease) = &resp.lease {
        if lease.is_expired() {
            return Err("credential lease expired; re-request a fresh lease".into());
        }
        if !lease.binds_run(run_id) {
            return Err("credential lease is bound to a different run".into());
        }
    }
    if resp.api_key.trim().is_empty() {
        return Err(format!(
            "Broker returned empty key for provider '{}'",
            resp.provider_id
        ));
    }
    Ok(Credential {
        api_key: resp.api_key,
        base_url: resp.base_url,
        proxy_url: resp.proxy_url,
        key_id: Some(resp.key_id),
        provider_type: resp.provider_type,
    })
}

/// Default path: `$NATIVES_DB_PATH` or `~/.natives/natives.db`.
/// Credentials only — never the assistant conversation authority store.
/// Retained for diagnostics / call-site compatibility; the daemon no longer
/// opens this file.
pub fn default_natives_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("NATIVES_DB_PATH") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs_next_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".natives")
        .join("natives.db")
}

/// Daemon authority DB: `$NATIVES_ASSISTANT_DB_PATH` or `~/.natives/assistant.db`.
/// Phase 0: conversation/run/event authority lives here (not natives.db).
pub fn default_assistant_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("NATIVES_ASSISTANT_DB_PATH") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs_next_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".natives")
        .join("assistant.db")
}

/// Broker socket endpoint: `$NATIVES_BROKER_SOCKET`, else the runtime
/// directory (`NATIVES_RUNTIME_DIR` / `XDG_RUNTIME_DIR` / `~/.natives/runtime`)
/// joined with `natives-broker.sock`. The Host creates this socket and passes
/// the env var to the sidecar.
pub fn default_broker_socket_path() -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        if let Ok(p) = std::env::var("NATIVES_BROKER_SOCKET") {
            if !p.trim().is_empty() {
                return Ok(PathBuf::from(p));
            }
        }
        let runtime_dir = std::env::var_os("NATIVES_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
            .unwrap_or_else(|| {
                dirs_next_home()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".natives")
                    .join("runtime")
            });
        Ok(runtime_dir.join("natives-broker.sock"))
    }
    #[cfg(not(unix))]
    {
        let _ = std::env::var("NATIVES_BROKER_SOCKET");
        Err(
            "credential lease broker requires a Unix socket endpoint on this platform; \
             configure NATIVES_BROKER_SOCKET"
                .into(),
        )
    }
}

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Install the process-wide UDS lease broker. Returns true only when a broker
/// socket endpoint is configured AND reachable (socket file present) — the
/// daemon never falls back to reading natives.db.
pub fn try_install_natives_db_broker() -> bool {
    let endpoint = match default_broker_socket_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[agent-daemon] UDS credential broker not configured: {e}");
            return false;
        }
    };
    if !endpoint.exists() {
        eprintln!(
            "[agent-daemon] UDS credential broker not reachable at {} (fail closed)",
            endpoint
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<broker-sock>")
        );
        return false;
    }
    match crate::production_credentials::install_uds_lease_broker() {
        Ok(()) => {
            eprintln!(
                "[agent-daemon] UDS credential lease broker installed at {}",
                endpoint
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<broker-sock>")
            );
            true
        }
        Err(e) => {
            eprintln!("[agent-daemon] UDS credential broker install failed: {e}");
            false
        }
    }
}

/// Read and decrypt one capability secret by row id (ADR-0016 decision 7) via
/// a Host broker lease. The value is memory-only: the caller must use it and
/// drop it; it is never logged and never persisted by the daemon.
pub fn read_capability_secret(id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("capability secret id is required".into());
    }
    let broker = NativesDbBroker::open_default()?;
    let req = CredentialSecretLeaseRequest {
        secret_id: id.to_string(),
        run_id: "capability-secret".into(),
    };
    let payload = serde_json::to_value(&req)
        .map_err(|e| redact_err(&format!("secret request serialize failed: {e}")))?;
    let data = broker.lease_request(names::CREDENTIAL_SECRET_ACQUIRE, &payload)?;
    let resp: CredentialSecretLeaseResponse = serde_json::from_value(data)
        .map_err(|e| redact_err(&format!("broker secret response parse failed: {e}")))?;
    Ok(resp.value)
}

/// Read a plain Host setting (e.g. governor rate-limit JSON) via a broker
/// lease. Replaces the former direct natives.db `settings` read: the daemon
/// boots from Host-pushed defaults when the broker is unreachable (Ok(None)).
pub fn read_setting(key: &str) -> Result<Option<String>, String> {
    let key = key.trim();
    if key.is_empty() {
        return Ok(None);
    }
    let broker = NativesDbBroker::open_default()?;
    let req = CredentialSettingLeaseRequest {
        key: key.to_string(),
        run_id: "daemon-boot".into(),
    };
    let payload = serde_json::to_value(&req)
        .map_err(|e| redact_err(&format!("setting request serialize failed: {e}")))?;
    let data = broker.lease_request(names::CREDENTIAL_SETTING_GET, &payload)?;
    let resp: CredentialSettingLeaseResponse = serde_json::from_value(data)
        .map_err(|e| redact_err(&format!("broker setting response parse failed: {e}")))?;
    Ok(resp.value)
}

/// T104 (P0-007): daemon-side writes to the Host-authoritative natives.db are
/// forbidden. Settings are written by the Host (which broadcasts changes);
/// this function exists only to make that boundary explicit and fail closed.
pub fn write_setting(_key: &str, _value: &str) -> Result<(), String> {
    Err(
        "natives.db write blocked: settings persistence belongs to the Host \
         broker (T104); daemon must not write the Host-authoritative database"
            .into(),
    )
}

/// Redact secret-looking material from any daemon-side broker error.
fn redact_err(msg: &str) -> String {
    crate::production_credentials::redact_cred_err(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_with_lease(
        api_key: &str,
        run_id: &str,
        lease_run_id: &str,
        ttl_secs: i64,
    ) -> CredentialBrokerResponse {
        use assistant_protocol::v2::CredentialLeaseMeta;
        use chrono::Duration;
        CredentialBrokerResponse {
            key_id: "k1".into(),
            provider_id: "openai".into(),
            api_key: api_key.into(),
            base_url: Some("https://example.test".into()),
            provider_type: Some("anthropic_messages".into()),
            proxy_url: None,
            lease: Some(CredentialLeaseMeta::new(
                "openai",
                "k1",
                lease_run_id,
                None,
                Duration::seconds(ttl_secs),
            )),
        }
    }

    #[test]
    fn lease_contract_is_enforced_fail_closed() {
        // Valid: run matches, lease not expired.
        let cred = credential_from_response(
            response_with_lease("sk-ok-key", "run-1", "run-1", 120),
            "run-1",
        )
        .expect("valid lease must resolve");
        assert_eq!(cred.api_key, "sk-ok-key");
        assert_eq!(cred.key_id.as_deref(), Some("k1"));
        assert_eq!(cred.provider_type.as_deref(), Some("anthropic_messages"));

        // Run mismatch: a child must not reuse a parent lease.
        let err = credential_from_response(
            response_with_lease("sk-parent", "run-2", "run-1", 120),
            "run-2",
        )
        .unwrap_err();
        assert!(err.contains("different run"), "{err}");
        assert!(!err.contains("sk-parent"), "error must not leak key");

        // Already-expired lease (negative TTL): fail closed even with key.
        let err = credential_from_response(
            response_with_lease("sk-expired", "run-3", "run-3", -5),
            "run-3",
        )
        .unwrap_err();
        assert!(err.contains("expired"), "{err}");
        assert!(!err.contains("sk-expired"), "error must not leak key");
    }

    #[test]
    fn empty_key_fails_closed() {
        let mut resp = response_with_lease("", "run-1", "run-1", 120);
        resp.api_key = "   ".into();
        let err = credential_from_response(resp, "run-1").unwrap_err();
        assert!(err.contains("empty key"));
    }

    #[test]
    fn redaction_strips_key_prefixes_from_errors() {
        let msg = redact_err("upstream 401 Bearer sk-ant-secret-token-value-here");
        assert!(!msg.contains("sk-ant-secret-token-value-here"));
        assert!(msg.contains("REDACTED") || msg.contains("[REDACTED_KEY]"));
    }

    #[test]
    fn broker_endpoint_defaults_to_runtime_dir() {
        std::env::set_var("NATIVES_BROKER_SOCKET", "/tmp/natives-test-broker.sock");
        let ep = default_broker_socket_path().unwrap();
        assert_eq!(ep, PathBuf::from("/tmp/natives-test-broker.sock"));
        std::env::remove_var("NATIVES_BROKER_SOCKET");
    }
}
