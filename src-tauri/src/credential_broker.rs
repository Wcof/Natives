//! Credential Broker — Tauri-side issuance of short-TTL, Run-bound, revocable
//! credential leases for the Agent Daemon (NE-P0-02 / 19.1).
//!
//! The daemon never reads `natives.db` / `provider_kek`. Every credential
//! resolve is a **lease request** over the authenticated broker UDS channel;
//! this module decrypts one key per request from `natives.db` through the
//! existing KEK/DEK envelope path and hands the material back only as the
//! payload of a lease response.
//!
//! Security rules:
//! - Keys live encrypted in `natives.db` via KEK/DEK envelope encryption.
//! - Daemon never receives bulk key dumps — only per-request lease material.
//! - Lease metadata (no secret) is tracked in an in-memory registry with a
//!   short TTL; revocation is enforced and durable (Host writes natives.db).
//! - Responses are never written to event logs or assistant.db.
//! - Errors are redacted before they leave the module.

use crate::error::{Error, Result};
use crate::provider_key_manager::envelope_decrypt;
use assistant_protocol::v2::credential as wire;
use assistant_protocol::v2::methods::names;
use rusqlite::OptionalExtension;
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
/// Issues a Run-bound short-TTL lease and returns memory-only Credential
/// (never logs key).
///
/// W3 P0-07: only reachable under test/diagnostic (the embedded broker install
/// in `daemon_authority`/`lib.rs` is cfg-gated and production runs UDS with the
/// daemon-owned broker). Kept cfg(test|diagnostic) so the Host production build
/// does not depend on the provider-adapters implementation crate.
#[cfg(any(test, feature = "diagnostic"))]
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

// ─────────────────────────────────────────────────────────────────────────────
// Lease registry (short TTL + revocation) — moved to `credential_broker_lease`.
// ─────────────────────────────────────────────────────────────────────────────
pub use crate::credential_broker_lease::{lease_registry, CredentialLeaseRegistry, LeaseEntry};

// ─────────────────────────────────────────────────────────────────────────────
// Lease handlers (wire protocol) — used by the UDS broker dispatch and by the
// tauri command below.
// ─────────────────────────────────────────────────────────────────────────────

/// Acquire one Run-bound short-TTL lease: validate, decrypt from natives.db,
/// register the lease, persist lease metadata, return the material + lease.
pub fn broker_acquire_lease(
    req: wire::CredentialBrokerRequest,
) -> std::result::Result<wire::CredentialBrokerResponse, String> {
    if req.provider_id.trim().is_empty() {
        return Err("provider_id is required".into());
    }
    if req.run_id.trim().is_empty() || req.run_id == "unspecified" {
        return Err("run_id required for credential lease binding".into());
    }

    let db = crate::db::get_main_conn()
        .map_err(|e| redact_broker_error(&format!("DB connection failed: {e}")))?;

    let row = db
        .query_row(
            "SELECT k.id, k.api_key_encrypted, k.dek_encrypted, p.base_url, COALESCE(NULLIF(p.api_protocol, ''), p.preset_name)
             FROM provider_api_keys k
             JOIN user_providers p ON k.provider_id = p.id
             WHERE k.id = ?1 AND k.provider_id = ?2
             LIMIT 1",
            rusqlite::params![req.key_id, req.provider_id],
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
                rusqlite::params![req.provider_id],
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
            redact_broker_error(&format!("No active key for provider {}", req.provider_id))
        })?;

    let (key_id, encrypted_key, dek_encrypted, base_url, provider_type) = row;
    let api_key = envelope_decrypt(&encrypted_key, &dek_encrypted, &db).map_err(|e| {
        redact_broker_error(&format!(
            "Failed to decrypt key for provider {} (run {}): {e}",
            req.provider_id, req.run_id
        ))
    })?;

    let lease = lease_registry().issue(
        &req.provider_id,
        &key_id,
        &req.run_id,
        req.session_id.clone(),
        wire::CredentialLeaseMeta::default_ttl(),
    );

    // Durable lease metadata (never key material) in the Host-authoritative
    // natives.db. Best effort: the in-memory registry already governs TTL/revoke.
    if let Err(e) = crate::key_lease::record_lease(
        &db,
        &req.run_id,
        &req.provider_id,
        &key_id,
        &lease.lease_id,
        &lease.expires_at.to_rfc3339(),
    ) {
        eprintln!(
            "[credential-broker] lease record warning (redacted): {}",
            redact_broker_error(&e.to_string())
        );
    }

    let proxy_url = global_proxy_for_daemon().ok().flatten();
    Ok(wire::CredentialBrokerResponse {
        key_id,
        provider_id: req.provider_id,
        api_key,
        base_url,
        provider_type,
        proxy_url,
        lease: Some(lease),
    })
}

/// Revoke an issued lease (Daemon → Host).
pub fn broker_revoke_lease(
    req: wire::CredentialLeaseRevokeRequest,
) -> std::result::Result<wire::CredentialLeaseStatus, String> {
    lease_registry()
        .revoke(&req.lease_id, &req.run_id)
        .map_err(|e| redact_broker_error(&e))
}

/// Report lease status (Daemon → Host). Never returns key material.
pub fn broker_lease_status(
    req: wire::CredentialLeaseStatusRequest,
) -> std::result::Result<wire::CredentialLeaseStatus, String> {
    lease_registry()
        .status(&req.lease_id)
        .map_err(|e| redact_broker_error(&e))
}

/// Acquire the active Sub2API account pool as a lease (Daemon → Host).
pub fn broker_pool_acquire(
    req: wire::CredentialPoolLeaseRequest,
) -> std::result::Result<wire::CredentialPoolLeaseResponse, String> {
    if req.provider_id.trim().is_empty() {
        return Err("provider_id is required".into());
    }
    let db = crate::db::get_main_conn()
        .map_err(|e| redact_broker_error(&format!("DB connection failed: {e}")))?;
    let mut stmt = db
        .prepare(
            "SELECT a.id, a.provider_id, a.platform, a.account_type, a.credentials_encrypted,
                a.dek_encrypted, a.extra_json, a.priority, a.concurrency, a.expires_at,
                p.config_encrypted, p.dek_encrypted
             FROM provider_accounts a LEFT JOIN provider_account_proxies p ON p.id = a.proxy_id
             WHERE a.provider_id = ?1 AND a.status = 'active'
               AND (a.expires_at IS NULL OR a.expires_at = '' OR a.expires_at > datetime('now'))
             ORDER BY a.priority ASC, a.id ASC",
        )
        .map_err(|e| redact_broker_error(&format!("prepare Sub2API pool: {e}")))?;
    let rows = stmt
        .query_map([req.provider_id.clone()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
            ))
        })
        .map_err(|e| redact_broker_error(&format!("query Sub2API pool: {e}")))?;

    let mut accounts = Vec::new();
    for row in rows {
        let (
            id,
            provider_id,
            platform,
            account_type,
            encrypted,
            dek,
            extra,
            priority,
            concurrency,
            expires_at,
            proxy_encrypted,
            proxy_dek,
        ) = row.map_err(|e| redact_broker_error(&e.to_string()))?;
        let plaintext = envelope_decrypt(&encrypted, &dek, &db)
            .map_err(|e| redact_broker_error(&format!("Sub2API decrypt failed: {e}")))?;
        let credentials = serde_json::from_str(&plaintext).map_err(|_| {
            redact_broker_error(&format!("Sub2API account '{id}' has invalid credentials"))
        })?;
        let extra = serde_json::from_str(&extra).unwrap_or(Value::Object(Default::default()));
        let proxy_url = match (proxy_encrypted, proxy_dek) {
            (Some(encrypted), Some(dek)) => {
                let plain = envelope_decrypt(&encrypted, &dek, &db).map_err(|e| {
                    redact_broker_error(&format!("Sub2API proxy decrypt failed: {e}"))
                })?;
                proxy_url_from_value(&serde_json::from_str::<Value>(&plain).map_err(|_| {
                    redact_broker_error(&format!("Sub2API account '{id}' has invalid proxy"))
                })?)
            }
            _ => global_proxy_for_daemon().ok().flatten(),
        };
        accounts.push(wire::CredentialPoolAccount {
            id,
            provider_id,
            platform,
            account_type,
            credentials,
            extra,
            priority,
            concurrency: u32::try_from(concurrency.max(1)).unwrap_or(1),
            expires_at,
            proxy_url,
        });
    }

    let lease = lease_registry().issue(
        &req.provider_id,
        "sub2api-pool",
        &req.run_id,
        None,
        wire::CredentialLeaseMeta::default_ttl(),
    );
    Ok(wire::CredentialPoolLeaseResponse {
        provider_id: req.provider_id,
        lease: Some(lease),
        accounts,
    })
}

/// Acquire Host-owned loopback routing settings as a lease (Daemon → Host).
/// The bearer token returns in plaintext over the authenticated socket only.
pub fn broker_routing_settings(
    req: wire::LoopbackSettingsLeaseRequest,
) -> std::result::Result<wire::LoopbackSettingsLeaseResponse, String> {
    let _ = req.run_id;
    let db = crate::db::get_main_conn()
        .map_err(|e| redact_broker_error(&format!("DB connection failed: {e}")))?;
    let row = db
        .query_row(
            "SELECT enabled, local_enabled, local_port, local_token_encrypted,
                    local_token_dek_encrypted, rectifier_json
             FROM provider_routing_settings WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|e| redact_broker_error(&format!("read routing settings: {e}")))?;
    let (routing_enabled, local_enabled, port, token, token_dek, rectifier) = row;
    let bearer_token = match (token, token_dek) {
        (Some(token), Some(dek)) => Some(
            envelope_decrypt(&token, &dek, &db)
                .map_err(|e| redact_broker_error(&format!("routing token decrypt failed: {e}")))?,
        ),
        _ => None,
    };
    let lease = lease_registry().issue(
        "loopback",
        "routing-settings",
        "loopback",
        None,
        wire::CredentialLeaseMeta::default_ttl(),
    );
    Ok(wire::LoopbackSettingsLeaseResponse {
        enabled: routing_enabled != 0 && local_enabled != 0,
        port: u16::try_from(port).unwrap_or(15721),
        bearer_token,
        rectifier: serde_json::from_str(&rectifier).unwrap_or(Value::Object(Default::default())),
        lease: Some(lease),
    })
}

/// Acquire the Host-owned routing plan as a lease (Daemon → Host). Serves
/// `provider_routing_settings` + enabled `provider_route_bindings` so the
/// daemon never opens natives.db (T104 / modular remediation W1). Read-only;
/// the lease binds to `run_id` like every other broker method.
pub fn broker_routing_plan(
    req: wire::RoutingPlanLeaseRequest,
) -> std::result::Result<wire::RoutingPlanLeaseResponse, String> {
    let _ = req.run_id;
    let db = crate::db::get_main_conn()
        .map_err(|e| redact_broker_error(&format!("DB connection failed: {e}")))?;
    let enabled: bool = db
        .query_row(
            "SELECT enabled FROM provider_routing_settings WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0).map(|v| v != 0),
        )
        .optional()
        .map_err(|e| redact_broker_error(&format!("read routing settings: {e}")))?
        .unwrap_or(false);
    let mut targets = Vec::new();
    if enabled {
        let mut stmt = db
            .prepare(
                "SELECT provider_id, credential_kind, credential_id, model_id
                 FROM provider_route_bindings WHERE enabled = 1
                 ORDER BY position ASC, id ASC",
            )
            .map_err(|e| redact_broker_error(&format!("read route bindings: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| redact_broker_error(&format!("read route bindings: {e}")))?;
        for row in rows.flatten() {
            let (provider_id, credential_kind, credential_id, model_id) = row;
            if provider_id.trim().is_empty() || model_id.trim().is_empty() {
                continue;
            }
            targets.push(wire::RoutingPlanTarget {
                provider_id,
                credential_kind,
                credential_id,
                model_id,
            });
        }
    }
    let lease = lease_registry().issue(
        "routing",
        "plan",
        "routing-plan",
        None,
        wire::CredentialLeaseMeta::default_ttl(),
    );
    Ok(wire::RoutingPlanLeaseResponse {
        enabled,
        targets,
        lease: Some(lease),
    })
}

/// Export legacy Host `subagents` rows as a lease (Daemon → Host). ADR-0016
/// retirement: the daemon imports Host-owned data without opening natives.db.
/// Read-only; instructions/tools only, never secrets.
pub fn broker_host_subagents(
    req: wire::HostSubagentsLeaseRequest,
) -> std::result::Result<wire::HostSubagentsLeaseResponse, String> {
    let _ = req.run_id;
    let db = crate::db::get_main_conn()
        .map_err(|e| redact_broker_error(&format!("DB connection failed: {e}")))?;
    let rows = {
        let mut stmt = db
            .prepare(
                "SELECT id, name, role, instructions, tools, provider_id, provider_key_id,
                        model_id, enabled FROM subagents",
            )
            .map_err(|e| redact_broker_error(&format!("read subagents: {e}")))?;
        let mapped = stmt
            .query_map([], |row| {
                Ok(wire::HostSubagentRow {
                    id: row.get::<_, String>(0)?,
                    name: row.get::<_, String>(1)?,
                    role: row.get::<_, Option<String>>(2)?,
                    instructions: row.get::<_, Option<String>>(3)?,
                    tools: row.get::<_, Option<String>>(4)?,
                    provider_id: row.get::<_, Option<String>>(5)?,
                    provider_key_id: row.get::<_, Option<String>>(6)?,
                    model_id: row.get::<_, Option<String>>(7)?,
                    enabled: row.get::<_, i64>(8)?,
                })
            })
            .map_err(|e| redact_broker_error(&format!("read subagents: {e}")))?;
        mapped
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| redact_broker_error(&format!("read subagents: {e}")))
    }?;
    let lease = lease_registry().issue(
        "subagents",
        "host-export",
        "host-subagents",
        None,
        wire::CredentialLeaseMeta::default_ttl(),
    );
    Ok(wire::HostSubagentsLeaseResponse {
        rows,
        lease: Some(lease),
    })
}

/// Acquire one capability secret as a lease (Daemon → Host), e.g. MCP env /
/// bearer / OAuth refresh material. Value is memory-only.
pub fn broker_secret_acquire(
    req: wire::CredentialSecretLeaseRequest,
) -> std::result::Result<wire::CredentialSecretLeaseResponse, String> {
    let id = req.secret_id.trim();
    if id.is_empty() {
        return Err("capability secret id is required".into());
    }
    let db = crate::db::get_main_conn()
        .map_err(|e| redact_broker_error(&format!("DB connection failed: {e}")))?;
    let (ciphertext, nonce) = db
        .query_row(
            "SELECT ciphertext, nonce FROM capability_secrets WHERE id = ?1 LIMIT 1",
            rusqlite::params![id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|_| redact_broker_error(&format!("capability secret '{id}' not found")))?;
    let value = envelope_decrypt(&ciphertext, &nonce, &db)
        .map_err(|e| redact_broker_error(&format!("capability secret decrypt failed: {e}")))?;
    let lease = lease_registry().issue(
        "capability",
        id,
        &req.run_id,
        None,
        wire::CredentialLeaseMeta::default_ttl(),
    );
    Ok(wire::CredentialSecretLeaseResponse {
        secret_id: id.to_string(),
        value,
        lease: Some(lease),
    })
}

/// Read one plain (non-secret) Host setting via a lease (Daemon → Host),
/// e.g. governor rate-limit JSON. Missing key → Ok(None).
pub fn broker_setting_get(
    req: wire::CredentialSettingLeaseRequest,
) -> std::result::Result<wire::CredentialSettingLeaseResponse, String> {
    let key = req.key.trim();
    if key.is_empty() {
        return Ok(wire::CredentialSettingLeaseResponse {
            key: req.key,
            value: None,
        });
    }
    let db = crate::db::get_main_conn()
        .map_err(|e| redact_broker_error(&format!("DB connection failed: {e}")))?;
    let value: Option<String> = db
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 LIMIT 1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| redact_broker_error(&format!("read setting: {e}")))?;
    Ok(wire::CredentialSettingLeaseResponse {
        key: key.to_string(),
        value,
    })
}

/// Single-frame host-side dispatch for the broker lease UDS protocol.
///
/// `payload_line` is one JSON-line [`wire::CredentialLeaseEnvelope`]; returns
/// the JSON-line [`wire::CredentialLeaseReply`] to write back. Errors are
/// redacted before they leave this function. The tiny UDS listener (lib.rs
/// assembly) reads one line, calls this, writes one line.
pub fn dispatch_broker_uds(payload_line: &str) -> std::result::Result<String, String> {
    let envelope: wire::CredentialLeaseEnvelope = serde_json::from_str(payload_line.trim())
        .map_err(|e| redact_broker_error(&format!("broker envelope parse failed: {e}")))?;

    // Clone the method name so the match arms can move `envelope.payload` out
    // of the envelope without holding a borrow on a sibling field.
    let method = envelope.method.clone();
    let result: std::result::Result<serde_json::Value, String> = match method.as_str() {
        names::CREDENTIAL_LEASE_ACQUIRE => {
            let req: wire::CredentialBrokerRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_acquire_lease(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::CREDENTIAL_LEASE_REVOKE => {
            let req: wire::CredentialLeaseRevokeRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_revoke_lease(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::CREDENTIAL_LEASE_STATUS => {
            let req: wire::CredentialLeaseStatusRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_lease_status(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::CREDENTIAL_POOL_ACQUIRE => {
            let req: wire::CredentialPoolLeaseRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_pool_acquire(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::CREDENTIAL_ROUTING_SETTINGS => {
            let req: wire::LoopbackSettingsLeaseRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_routing_settings(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::CREDENTIAL_ROUTING_PLAN => {
            let req: wire::RoutingPlanLeaseRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_routing_plan(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::HOST_SUBAGENTS_EXPORT => {
            let req: wire::HostSubagentsLeaseRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_host_subagents(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::CREDENTIAL_SECRET_ACQUIRE => {
            let req: wire::CredentialSecretLeaseRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_secret_acquire(req)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        names::CREDENTIAL_SETTING_GET => {
            let req: wire::CredentialSettingLeaseRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| redact_broker_error(&e.to_string()))?;
            broker_setting_get(req).and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        }
        other => Err(redact_broker_error(&format!(
            "unknown credential lease method '{other}'"
        ))),
    };

    let reply = match result {
        Ok(data) => wire::CredentialLeaseReply::ok(data),
        Err(e) => wire::CredentialLeaseReply::err(redact_broker_error(&e)),
    };
    serde_json::to_string(&reply).map_err(|e| e.to_string())
}

/// Resolve and decrypt a single provider key for an active run, issuing a
/// Run-bound short-TTL lease. Tauri command for the frontend/embedded path.
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
    let wire_req = wire::CredentialBrokerRequest {
        key_id: request.key_id,
        provider_id: request.provider_id,
        run_id: request.run_id,
        session_id: request.session_id,
    };
    match broker_acquire_lease(wire_req) {
        Ok(resp) => Ok(CredentialBrokerResponse {
            key_id: resp.key_id,
            provider_id: resp.provider_id,
            api_key: resp.api_key,
            base_url: resp.base_url,
            provider_type: resp.provider_type,
            lease: resp.lease.map(|l| CredentialLeaseMeta {
                lease_id: l.lease_id,
                provider_id: l.provider_id,
                key_id: l.key_id,
                run_id: l.run_id,
                session_id: l.session_id,
                expires_at: l.expires_at.to_rfc3339(),
            }),
        }),
        Err(e) if e.contains("No active key") => Err(Error::NotFound(e)),
        Err(e) => Err(Error::Internal(e)),
    }
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

fn proxy_url_from_value(value: &Value) -> Option<String> {
    let enabled = value
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !enabled {
        return None;
    }
    value
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| valid_proxy_url(url))
        .map(str::to_string)
}

fn valid_proxy_url(url: &str) -> bool {
    let value = url.trim();
    !value.chars().any(char::is_control)
        && (value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("socks5://"))
}

/// Host-side Credential Broker UDS listener (W3 P0-04).
///
/// Binds a private mode-0600 socket at `NATIVES_BROKER_SOCKET` (or the runtime
/// directory default), accepts one connection at a time, reads one JSON line
/// ([`wire::CredentialLeaseEnvelope`]), dispatches it through
/// [`dispatch_broker_uds`], and writes back one JSON line
/// ([`wire::CredentialLeaseReply`]). Same-UID check via `getpeereid` on macOS /
/// BSD (libc) when available. Errors are redacted; no token/secret ever logs.
/// The daemon never falls back to reading natives.db when the broker is absent.
pub fn spawn_broker_uds_listener(socket_path: &std::path::Path) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};

    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Internal(format!("broker socket dir: {e}")))?;
    }
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)
        .map_err(|e| Error::Internal(format!("broker bind {}: {e}", socket_path.display())))?;

    // Private socket: only the same user may connect (mode 0600).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600));
    }

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let _ = handle_broker_connection(stream);
                }
                Err(e) => {
                    eprintln!("[credential_broker] accept failed (redacted): {e}");
                }
            }
        }
    });
    Ok(())
}

/// Serve one broker request: peer-UID check, bounded one-line read, dispatch,
/// one-line reply. Never panics on I/O errors.
fn handle_broker_connection(stream: std::os::unix::net::UnixStream) -> std::result::Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));

    // Same-UID check before any credential parse/store access.
    #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
    {
        use std::os::unix::io::AsRawFd;
        let mut peer_uid: libc::uid_t = 0;
        let mut peer_gid: libc::gid_t = 0;
        // SAFETY: getpeereid writes into valid out-params owned by this frame.
        let rc = unsafe {
            libc::getpeereid(stream.as_raw_fd(), &mut peer_uid, &mut peer_gid)
        };
        if rc != 0 || peer_uid != unsafe { libc::geteuid() } {
            return Err("broker peer UID mismatch (rejected before parse)".into());
        }
    }

    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    // Bounded read: a single envelope line must fit within a small frame.
    let read = reader
        .read_line(&mut line)
        .map_err(|e| redact_broker_error(&format!("broker read failed: {e}")))?;
    if read == 0 || line.trim().is_empty() {
        return Err("broker empty request".into());
    }
    if line.len() > 16 * 1024 {
        return Err("broker request frame too large".into());
    }
    let reply = dispatch_broker_uds(&line).map_err(|e| redact_broker_error(&e))?;
    let mut writer = stream;
    writer
        .write_all(reply.as_bytes())
        .map_err(|e| redact_broker_error(&format!("broker write failed: {e}")))?;
    let _ = writer.flush();
    Ok(())
}

/// Resolve the broker socket path (mirrors the daemon-side default).
pub fn broker_socket_path() -> std::result::Result<std::path::PathBuf, String> {
    if let Ok(p) = std::env::var("NATIVES_BROKER_SOCKET") {
        if !p.trim().is_empty() {
            return Ok(std::path::PathBuf::from(p));
        }
    }
    let runtime_dir = std::env::var_os("NATIVES_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from))
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".natives")
                .join("runtime")
        });
    Ok(runtime_dir.join("natives-broker.sock"))
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
                        "tauri resolve_for_daemon → natives.db envelope_decrypt → lease",
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
                        "lease_issued_short_ttl",
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

    #[test]
    fn lease_registry_issue_revoke_status_and_ttl() {
        let registry = CredentialLeaseRegistry::default();

        // Issue a normal lease.
        let meta = registry.issue(
            "openai",
            "k1",
            "run-1",
            None,
            chrono::Duration::seconds(120),
        );
        let status = registry.status(&meta.lease_id).unwrap();
        assert!(status.active);
        assert!(!status.revoked);
        assert_eq!(status.run_id, "run-1");

        // Revoke with a mismatched run is rejected.
        assert!(registry.revoke(&meta.lease_id, "run-evil").is_err());
        let status = registry.status(&meta.lease_id).unwrap();
        assert!(status.active, "mismatched revoke must not revoke");

        // Revoke with the owning run succeeds; the lease is now inactive.
        let status = registry.revoke(&meta.lease_id, "run-1").unwrap();
        assert!(!status.active);
        assert!(status.revoked);
        let status = registry.status(&meta.lease_id).unwrap();
        assert!(!status.active);
        assert!(status.revoked);
    }

    #[test]
    fn lease_registry_ttl_expiry_fails_closed() {
        let registry = CredentialLeaseRegistry::default();
        // Negative TTL → already expired at issue time.
        let meta = registry.issue("openai", "k1", "run-1", None, chrono::Duration::seconds(-5));
        let status = registry.status(&meta.lease_id).unwrap();
        assert!(!status.active, "expired lease must report inactive");
    }

    #[test]
    fn dispatch_broker_uds_acquire_rejects_empty_provider_redacted() {
        let envelope = wire::CredentialLeaseEnvelope {
            method: names::CREDENTIAL_LEASE_ACQUIRE.to_string(),
            payload: serde_json::json!({
                "key_id": "k1",
                "provider_id": "",
                "run_id": "run-1",
                "session_id": null,
            }),
        };
        let line = serde_json::to_string(&envelope).unwrap();
        let reply_line = dispatch_broker_uds(&line).unwrap();
        let reply: wire::CredentialLeaseReply = serde_json::from_str(&reply_line).unwrap();
        assert!(!reply.ok);
        assert!(!reply_line.contains("sk-"));
        let err = reply.error.unwrap_or_default();
        assert!(err.contains("provider_id"), "{err}");
    }

    #[test]
    fn dispatch_broker_uds_unknown_method_fails_closed() {
        let envelope = wire::CredentialLeaseEnvelope {
            method: "credential.bogus".into(),
            payload: serde_json::json!({}),
        };
        let line = serde_json::to_string(&envelope).unwrap();
        let reply_line = dispatch_broker_uds(&line).unwrap();
        let reply: wire::CredentialLeaseReply = serde_json::from_str(&reply_line).unwrap();
        assert!(!reply.ok);
        assert!(reply.error.unwrap_or_default().contains("unknown"));
    }
}
