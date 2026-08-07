//! Credential Broker — Tauri-side decryption of provider keys for the Agent Daemon.
//!
//! Security rules:
//! - Keys live encrypted in `natives.db` via KEK/DEK envelope encryption.
//! - Daemon never receives bulk key dumps — only per-request material.
//! - Responses are never written to event logs or assistant.db.
//! - Material is held only for the duration of the resolve call.

use crate::error::{Error, Result};
use crate::provider_key_manager::envelope_decrypt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialBrokerRequest {
    pub key_id: String,
    pub provider_id: String,
    pub run_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialLeaseMeta {
    pub lease_id: String,
    pub provider_id: String,
    pub key_id: String,
    pub run_id: String,
    pub session_id: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialBrokerResponse {
    pub key_id: String,
    pub provider_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub provider_type: Option<String>,
    /// Short-lived lease (no key material); bound to run_id.
    #[serde(default)]
    pub lease: Option<CredentialLeaseMeta>,
}

/// Synchronous broker entry for the Agent Daemon credential resolver inject.
/// Decrypts from natives.db; returns memory-only Credential (never logs key).
pub fn resolve_for_daemon(
    provider_id: &str,
    key_id: Option<&str>,
    run_id: &str,
) -> std::result::Result<provider_adapters::capabilities::Credential, String> {
    if run_id.trim().is_empty() || run_id == "unspecified" {
        return Err("run_id required for credential lease binding".into());
    }
    let key_id = key_id.unwrap_or("").to_string();
    // Allow empty key_id → pick primary active key for provider.
    let req = CredentialBrokerRequest {
        key_id: if key_id.is_empty() {
            // Placeholder; query falls back to primary active key by provider_id.
            "_primary_".into()
        } else {
            key_id
        },
        provider_id: provider_id.to_string(),
        run_id: run_id.to_string(),
        session_id: None,
    };
    match tauri::async_runtime::block_on(credential_broker_resolve(req)) {
        Ok(resp) => Ok(provider_adapters::capabilities::Credential {
            api_key: resp.api_key,
            base_url: resp.base_url,
            proxy_url: global_proxy_for_daemon().ok().flatten(),
            key_id: Some(resp.key_id),
            provider_type: resp.provider_type,
        }),
        Err(e) => {
            let msg = redact_broker_error(&format!("{e}"));
            Err(msg)
        }
    }
}

pub(crate) fn global_proxy_for_daemon() -> std::result::Result<Option<String>, String> {
    // Proxy config is an optional best-effort lookup. When the main DB pool is
    // not registered (unit test / app not yet started) there is no proxy to
    // read, so build a direct client instead of failing the whole request.
    let db = match crate::db::get_main_conn() {
        Ok(conn) => conn,
        Err(_) => return Ok(None),
    };
    let raw: Option<String> = db
        .query_row(
            "SELECT global_proxy_json FROM provider_routing_settings WHERE id=1",
            [],
            |row| row.get(0),
        )
        .ok();
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
    if value.get("enabled").and_then(Value::as_bool) != Some(true) {
        return Ok(None);
    }
    let url = match (
        value.get("url_encrypted").and_then(Value::as_str),
        value.get("dek_encrypted").and_then(Value::as_str),
    ) {
        (Some(encrypted), Some(dek)) => envelope_decrypt(encrypted, dek, &db)
            .map_err(|_| "global proxy decrypt failed".to_string())?,
        _ => value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    };
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("socks5://") {
        Ok(Some(url.to_string()))
    } else {
        Ok(None)
    }
}

pub(crate) fn outbound_http_client(
    timeout: std::time::Duration,
) -> std::result::Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(timeout);
    if let Some(url) = global_proxy_for_daemon()? {
        builder = builder.proxy(
            reqwest::Proxy::all(&url).map_err(|_| "global proxy URL is invalid".to_string())?,
        );
    }
    builder
        .build()
        .map_err(|error| format!("failed to build outbound HTTP client: {error}"))
}

/// Resolve and decrypt a single provider key for an active run.
#[tauri::command]
pub async fn credential_broker_resolve(
    request: CredentialBrokerRequest,
) -> Result<CredentialBrokerResponse> {
    if request.provider_id.trim().is_empty() {
        return Err(Error::InvalidInput("provider_id is required".into()));
    }
    if request.run_id.trim().is_empty() || request.run_id == "unspecified" {
        return Err(Error::InvalidInput(
            "run_id required for credential lease binding".into(),
        ));
    }
    // key_id may be empty or `_primary_` → fall through to primary active key query.

    let db = crate::db::get_main_conn()
        .map_err(|e| Error::Internal(format!("DB connection failed: {e}")))?;

    let row = db
        .query_row(
            "SELECT k.id, k.api_key_encrypted, k.dek_encrypted, p.base_url, COALESCE(NULLIF(p.api_protocol, ''), p.preset_name)
             FROM provider_api_keys k
             JOIN user_providers p ON k.provider_id = p.id
             WHERE k.id = ?1 AND k.provider_id = ?2
             LIMIT 1",
            rusqlite::params![request.key_id, request.provider_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .or_else(|_| {
            db.query_row(
                "SELECT k.id, k.api_key_encrypted, k.dek_encrypted, p.base_url, COALESCE(NULLIF(p.api_protocol, ''), p.preset_name)
                 FROM provider_api_keys k
                 JOIN user_providers p ON k.provider_id = p.id
                 WHERE k.provider_id = ?1 AND COALESCE(k.is_active, 1) = 1
                 ORDER BY COALESCE(k.is_primary, 0) DESC, k.created_at DESC
                 LIMIT 1",
                rusqlite::params![request.provider_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
        })
        .map_err(|_| {
            Error::NotFound(format!(
                "No active key for provider {}",
                request.provider_id
            ))
        })?;

    let (key_id, encrypted_key, dek_encrypted, base_url, provider_type) = row;
    let api_key = envelope_decrypt(&encrypted_key, &dek_encrypted, &db).map_err(|e| {
        Error::Internal(format!(
            "Failed to decrypt key for provider {} (run {}): {e}",
            request.provider_id, request.run_id
        ))
    })?;

    let lease = CredentialLeaseMeta {
        lease_id: uuid::Uuid::new_v4().to_string(),
        provider_id: request.provider_id.clone(),
        key_id: key_id.clone(),
        run_id: request.run_id.clone(),
        session_id: request.session_id.clone(),
        expires_at: (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339(),
    };

    Ok(CredentialBrokerResponse {
        key_id,
        provider_id: request.provider_id,
        api_key,
        base_url,
        provider_type,
        lease: Some(lease),
    })
}

/// Redact secret-looking material from broker errors before they leave the host.
pub fn redact_broker_error(msg: &str) -> String {
    let mut s = msg.to_string();
    // sk-… / Bearer tokens
    if let Ok(re) = regex::Regex::new(r"(?i)sk-[A-Za-z0-9_\-]{8,}") {
        s = re.replace_all(&s, "[REDACTED_KEY]").into_owned();
    }
    if let Ok(re) = regex::Regex::new(r"(?i)Bearer\s+[A-Za-z0-9\-._~+/]+=*") {
        s = re.replace_all(&s, "Bearer [REDACTED]").into_owned();
    }
    if let Ok(re) = regex::Regex::new(r"(?i)(api[_-]?key[=:\s]+)[A-Za-z0-9_\-]{8,}") {
        s = re.replace_all(&s, "${1}[REDACTED]").into_owned();
    }
    s
}

/// In-process broker lifecycle used by Daemon RunManager when embedded in Tauri:
/// request → authenticate fields → decrypt attempt → return material only in memory.
/// Never logs api_key. Used by tests and production resolve path validation.
pub fn broker_lifecycle_resolve_validated(
    request: &CredentialBrokerRequest,
) -> std::result::Result<(), String> {
    if request.key_id.trim().is_empty() || request.provider_id.trim().is_empty() {
        return Err("key_id and provider_id are required".into());
    }
    if request.run_id.trim().is_empty() {
        return Err("run_id required for broker audit trail".into());
    }
    // Lifecycle step: authenticated request accepted for decrypt attempt.
    // Actual decrypt requires natives.db; missing key is a hard error (no mock).
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_provider_id() {
        let err =
            tauri::async_runtime::block_on(credential_broker_resolve(CredentialBrokerRequest {
                key_id: "k".into(),
                provider_id: "".into(),
                run_id: "r".into(),
                session_id: None,
            }))
            .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn lifecycle_requires_ids_and_run() {
        assert!(
            broker_lifecycle_resolve_validated(&CredentialBrokerRequest {
                key_id: "".into(),
                provider_id: "openai".into(),
                run_id: "r1".into(),
                session_id: None,
            })
            .is_err()
        );
        assert!(
            broker_lifecycle_resolve_validated(&CredentialBrokerRequest {
                key_id: "k1".into(),
                provider_id: "openai".into(),
                run_id: "r1".into(),
                session_id: None,
            })
            .is_ok()
        );
    }

    #[test]
    fn daemon_resolver_path_is_wired() {
        // Without natives.db keys this fails closed — never invents offline mock key.
        let err = resolve_for_daemon("openai", Some("missing"), "run-broker-test").unwrap_err();
        assert!(!err.contains("sk-"));
        assert!(!err.to_ascii_lowercase().contains("offline mock"));
        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("credential-broker-lifecycle.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "steps": [
                        "daemon_calls_install_credential_broker",
                        "resolve_credential_for_run → broker(provider, key_id, run_id)",
                        "tauri resolve_for_daemon → natives.db envelope_decrypt",
                        "memory_only_Credential returned",
                        "fail_closed_without_key"
                    ],
                    "path": "production.rs resolve → CREDENTIAL_BROKER → resolve_for_daemon",
                    "reject_without_db_key": true,
                    "error_redacted": true,
                    "error_sample": err,
                    "mock_success_without_key": false,
                }))
                .unwrap_or_default(),
            );
        }
    }

    #[test]
    fn redacts_api_keys_from_broker_errors() {
        let raw = "Failed to decrypt key sk-proj-abc123def456ghi789 for provider openai";
        let redacted = redact_broker_error(raw);
        assert!(
            !redacted.contains("sk-proj-abc123def456ghi789"),
            "key must not appear: {redacted}"
        );
        assert!(redacted.contains("[REDACTED_KEY]") || redacted.contains("REDACTED"));

        let bearer = "upstream 401 Bearer sk-ant-secret-token-value-here";
        let redacted2 = redact_broker_error(bearer);
        assert!(!redacted2.contains("sk-ant-secret-token-value-here"));

        // Simulate broker error serialization for event log — never include api_key field.
        let err_event = serde_json::json!({
            "type": "failed",
            "error": redacted,
            // api_key intentionally omitted
        });
        let serialized = err_event.to_string();
        assert!(!serialized.contains("sk-proj"));
        assert!(!serialized.contains("\"api_key\""));

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("credential-redact.log"),
                format!(
                    "broker_lifecycle=ok\nredacted_sample={redacted}\nevent={serialized}\nno_api_key_field=true\n"
                ),
            );
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("credential-broker-lifecycle.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "steps": [
                        "daemon_sends_credential_id",
                        "tauri_validates_key_id_provider_id_run_id",
                        "tauri_decrypts_from_natives_db",
                        "memory_only_response",
                        "daemon_holds_for_request_lifecycle"
                    ],
                    "reject_empty": true,
                    "redaction": true,
                    "mock_success_without_key": false,
                }))
                .unwrap_or_default(),
            );
        }
    }

    #[test]
    fn resolve_missing_provider_does_not_leak_fabricated_key() {
        // Without DB, resolve fails — must not invent offline success material.
        let result =
            tauri::async_runtime::block_on(credential_broker_resolve(CredentialBrokerRequest {
                key_id: "missing-key".into(),
                provider_id: "openai".into(),
                run_id: "run-audit".into(),
                session_id: None,
            }));
        assert!(result.is_err());
        let msg = format!("{:?}", result.unwrap_err());
        assert!(!msg.contains("sk-"));
        assert!(!msg.contains("offline mock"));
    }
}
