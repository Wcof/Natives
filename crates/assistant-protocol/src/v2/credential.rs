//! Credential reference + short-lived lease types.
//!
//! Plaintext keys travel only on authenticated host↔daemon IPC and must never
//! appear in events, logs, errors, or frontend payloads.

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
}
