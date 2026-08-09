//! Credential reference + short-lived lease types.
//!
//! Plaintext keys travel only on authenticated host↔daemon IPC and must never
//! appear in events, logs, errors, or frontend payloads.
//!
//! NE-P0-02 (19.1): the Agent Daemon never reads `natives.db` / `provider_kek`
//! directly. It requests **Run-bound, short-TTL, revocable credential leases**
//! from the Host Credential Broker over UDS. Every wire type for that lease
//! RPC lives here (single protocol source of truth).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Opaque reference a daemon run holds while calling the Credential Broker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CredentialRef {
    pub provider_id: String,
    pub key_id: String,
}

/// In-memory credential material returned only over authenticated IPC
/// between Daemon and Tauri Credential Broker. Never written to events,
/// profiles, or the frontend.
#[derive(Debug, Clone)]
pub struct CredentialMaterial {
    pub provider_id: String,
    pub key_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
}

impl Drop for CredentialMaterial {
    fn drop(&mut self) {
        // Best-effort zeroization of the key buffer.
        let key = self.api_key.as_mut_str();
        // SAFETY: we only overwrite the UTF-8 bytes with zeros before drop.
        unsafe {
            for byte in key.as_bytes_mut() {
                *byte = 0;
            }
        }
    }
}

/// Request from Daemon → Tauri for decrypted key material.
/// Must bind session + run so a child cannot silently reuse a parent lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialBrokerRequest {
    pub key_id: String,
    pub provider_id: String,
    pub run_id: String,
    /// Authenticated session that owns this resolve (optional on embedded inject).
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Response Tauri → Daemon (authenticated IPC only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialBrokerResponse {
    pub key_id: String,
    pub provider_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
    /// Provider protocol identifier (e.g. `anthropic_messages`) — Host-owned,
    /// returned in the same lease round trip so the daemon never performs a
    /// second Host-side lookup.
    #[serde(default)]
    pub provider_type: Option<String>,
    /// Host-resolved proxy URL for the provider call (memory-only).
    #[serde(default)]
    pub proxy_url: Option<String>,
    /// Short-lived lease metadata (no secret in lease id).
    #[serde(default)]
    pub lease: Option<CredentialLeaseMeta>,
}

/// Public lease metadata (safe to log without key material).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialLeaseMeta {
    pub lease_id: String,
    pub provider_id: String,
    pub key_id: String,
    pub run_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub expires_at: DateTime<Utc>,
}

impl CredentialLeaseMeta {
    pub fn new(
        provider_id: impl Into<String>,
        key_id: impl Into<String>,
        run_id: impl Into<String>,
        session_id: Option<String>,
        ttl: Duration,
    ) -> Self {
        Self {
            lease_id: uuid::Uuid::new_v4().to_string(),
            provider_id: provider_id.into(),
            key_id: key_id.into(),
            run_id: run_id.into(),
            session_id,
            expires_at: Utc::now() + ttl,
        }
    }

    pub fn default_ttl() -> Duration {
        Duration::seconds(120)
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Child runs must not reuse a parent lease without re-resolve.
    pub fn binds_run(&self, run_id: &str) -> bool {
        self.run_id == run_id
    }
}

/// One JSON-line request envelope on the authenticated broker UDS channel
/// (Daemon → Host). `method` is a `credential.*` name from [`super::names`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialLeaseEnvelope {
    pub method: String,
    pub payload: serde_json::Value,
}

/// One JSON-line reply envelope on the authenticated broker UDS channel
/// (Host → Daemon). `error` is always redacted — it must never carry key
/// material. `data` carries the typed response payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialLeaseReply {
    pub ok: bool,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

impl CredentialLeaseReply {
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

/// Explicit revocation of a previously issued lease (Daemon → Host).
/// Binds the revoke to the run that owns the lease; mismatched runs are
/// rejected so a child cannot revoke a parent's lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialLeaseRevokeRequest {
    pub lease_id: String,
    pub run_id: String,
}

/// Status probe for an issued lease (no secret material).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialLeaseStatusRequest {
    pub lease_id: String,
}

/// Public lease status — safe to log / persist (never contains key material).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialLeaseStatus {
    pub lease_id: String,
    pub provider_id: String,
    pub key_id: String,
    pub run_id: String,
    pub active: bool,
    pub revoked: bool,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Sub2API pool lease request (Daemon → Host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialPoolLeaseRequest {
    pub provider_id: String,
    pub run_id: String,
}

/// One decrypted Sub2API account, lease-bound and memory-only. The daemon
/// never persists this value into assistant.db.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialPoolAccount {
    pub id: String,
    pub provider_id: String,
    pub platform: String,
    pub account_type: String,
    pub credentials: serde_json::Value,
    pub extra: serde_json::Value,
    pub priority: i64,
    pub concurrency: u32,
    pub expires_at: Option<String>,
    /// Account proxy wins over the global proxy. Both stay daemon-memory only.
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialPoolLeaseResponse {
    pub provider_id: String,
    #[serde(default)]
    pub lease: Option<CredentialLeaseMeta>,
    #[serde(default)]
    pub accounts: Vec<CredentialPoolAccount>,
}

/// Loopback routing-settings lease (Daemon → Host). The bearer token rides in
/// plaintext over the authenticated socket only; it is never persisted by the
/// daemon and never emitted as an engine event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopbackSettingsLeaseRequest {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopbackSettingsLeaseResponse {
    pub enabled: bool,
    pub port: u16,
    #[serde(default)]
    pub bearer_token: Option<String>,
    pub rectifier: serde_json::Value,
    #[serde(default)]
    pub lease: Option<CredentialLeaseMeta>,
}

/// Routing-plan lease (Daemon → Host). Serves the Host-owned
/// `provider_routing_settings` + `provider_route_bindings` so the daemon never
/// opens natives.db (T104 / modular remediation W1). `run_id` binds the lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingPlanLeaseRequest {
    pub run_id: String,
}

/// One route binding entry from the Host `provider_route_bindings` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingPlanTarget {
    pub provider_id: String,
    pub credential_kind: String,
    #[serde(default)]
    pub credential_id: Option<String>,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingPlanLeaseResponse {
    pub enabled: bool,
    #[serde(default)]
    pub targets: Vec<RoutingPlanTarget>,
    #[serde(default)]
    pub lease: Option<CredentialLeaseMeta>,
}

/// Legacy Host `subagents` export lease (Daemon → Host). ADR-0016 retirement:
/// the daemon imports the Host-owned `subagents` table once via the broker
/// channel instead of opening natives.db. Read-only rows; memory-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSubagentsLeaseRequest {
    pub run_id: String,
}

/// One legacy Host subagent row (no secrets — instructions/tools only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSubagentRow {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub instructions: Option<String>,
    pub tools: Option<String>,
    pub provider_id: Option<String>,
    pub provider_key_id: Option<String>,
    pub model_id: Option<String>,
    pub enabled: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSubagentsLeaseResponse {
    #[serde(default)]
    pub rows: Vec<HostSubagentRow>,
    #[serde(default)]
    pub lease: Option<CredentialLeaseMeta>,
}

/// Capability-secret lease (Daemon → Host), e.g. MCP env / bearer / OAuth
/// refresh material stored in the Host `capability_secrets` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSecretLeaseRequest {
    pub secret_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSecretLeaseResponse {
    pub secret_id: String,
    pub value: String,
    #[serde(default)]
    pub lease: Option<CredentialLeaseMeta>,
}

/// Plain, non-secret Host setting read (Daemon → Host), e.g. governor
/// rate-limit JSON. This is how the daemon boots without touching natives.db.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSettingLeaseRequest {
    pub key: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSettingLeaseResponse {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Validate broker request fields (fail closed; no secrets in errors).
pub fn validate_broker_request(req: &CredentialBrokerRequest) -> Result<(), String> {
    if req.provider_id.trim().is_empty() {
        return Err("provider_id is required".into());
    }
    if req.run_id.trim().is_empty() || req.run_id == "unspecified" {
        return Err("run_id required for credential lease binding".into());
    }
    Ok(())
}

/// Redact any secret-looking substrings from broker/daemon errors.
pub fn redact_credential_error(msg: &str) -> String {
    let mut out = msg.to_string();
    // Common key prefixes — replacement must NOT re-introduce the prefix
    // (or the loop never terminates).
    for prefix in ["sk-ant-", "sk-", "AIza", "xoxb-", "ghp_"] {
        while let Some(idx) = out.find(prefix) {
            let end = (idx + prefix.len() + 32).min(out.len());
            out.replace_range(idx..end, "[REDACTED_KEY]");
        }
    }
    if let Some(idx) = out.find("Bearer ") {
        let end = (idx + "Bearer ".len() + 40).min(out.len());
        out.replace_range(idx..end, "Bearer [REDACTED]");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_binds_run_and_expires() {
        let lease = CredentialLeaseMeta::new("openai", "k1", "run-1", None, Duration::seconds(60));
        assert!(lease.binds_run("run-1"));
        assert!(!lease.binds_run("run-2"));
        assert!(!lease.is_expired());
    }

    #[test]
    fn validate_requires_run_id() {
        assert!(validate_broker_request(&CredentialBrokerRequest {
            key_id: "k".into(),
            provider_id: "p".into(),
            run_id: "unspecified".into(),
            session_id: None,
        })
        .is_err());
        assert!(validate_broker_request(&CredentialBrokerRequest {
            key_id: "k".into(),
            provider_id: "p".into(),
            run_id: "run-ok".into(),
            session_id: Some("s1".into()),
        })
        .is_ok());
    }

    #[test]
    fn redacts_sk_keys() {
        let msg = redact_credential_error("failed key sk-abc123secretvalue999 extra");
        assert!(!msg.contains("secretvalue"));
        assert!(msg.contains("REDACTED"));
        assert!(!msg.contains("sk-abc"));
    }

    #[test]
    fn lease_reply_envelope_roundtrips_without_secrets() {
        let ok = CredentialLeaseReply::ok(serde_json::json!({"leaseId": "L-1"}));
        let line = serde_json::to_string(&ok).unwrap();
        let back: CredentialLeaseReply = serde_json::from_str(&line).unwrap();
        assert!(back.ok);
        assert_eq!(back.data.unwrap()["leaseId"], "L-1");

        let err = CredentialLeaseReply::err("No active key for provider openai");
        let line = serde_json::to_string(&err).unwrap();
        assert!(!line.contains("sk-"));
        let back: CredentialLeaseReply = serde_json::from_str(&line).unwrap();
        assert!(!back.ok);
        assert!(back.error.unwrap().contains("No active key"));
    }

    #[test]
    fn response_serializes_provider_type_and_proxy_but_never_redacts_wrongly() {
        let resp = CredentialBrokerResponse {
            key_id: "k1".into(),
            provider_id: "openai".into(),
            api_key: "sk-ant-secret-value-here".into(),
            base_url: Some("https://example.test".into()),
            provider_type: Some("anthropic_messages".into()),
            proxy_url: Some("http://127.0.0.1:8080".into()),
            lease: Some(CredentialLeaseMeta::new(
                "openai",
                "k1",
                "run-1",
                None,
                Duration::seconds(120),
            )),
        };
        let json = serde_json::to_value(&resp).unwrap();
        // Lease metadata must carry no key material even when it is logged.
        let lease_json = serde_json::to_value(&json["lease"]).unwrap();
        assert!(!lease_json.to_string().contains("sk-ant"));
        // Wire round trip keeps the new host-owned fields.
        let back: CredentialBrokerResponse = serde_json::from_value(json).unwrap();
        assert_eq!(back.provider_type.as_deref(), Some("anthropic_messages"));
        assert_eq!(back.proxy_url.as_deref(), Some("http://127.0.0.1:8080"));
    }
}
