//! Sidecar-local Credential Broker: read encrypted keys from `natives.db`.
//!
//! Independent Agent Daemon processes cannot call Tauri's in-process inject.
//! Production installs this broker at daemon startup when `NATIVES_DB_PATH`
//! (or the default `~/.natives/natives.db`) is readable.
//!
//! Security:
//! - Decrypts one key per request only (same envelope as Tauri host).
//! - Never logs api_key material.
//! - Opens DB read-only.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use provider_adapters::capabilities::Credential;
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Open natives.db and resolve provider keys for the daemon process.
pub struct NativesDbBroker {
    path: PathBuf,
    /// Serialized access; rusqlite Connection is not Sync.
    conn: Mutex<Connection>,
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
    /// Open existing natives.db (read-write is required for SQLite WAL open on some
    /// systems; we never write key material — only SELECT).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(format!("natives.db not found at {}", path.display()));
        }
        let conn = Connection::open(&path).map_err(|e| format!("open natives.db failed: {e}"))?;
        // The daemon only updates a refreshed OAuth lease. It never writes any
        // plaintext credential material and serializes SQLite access via `conn`.
        let _ = conn.execute_batch("PRAGMA busy_timeout=3000;");
        Ok(Self {
            path,
            conn: Mutex::new(conn),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Resolve one credential: exact key_id, or primary/active key for provider.
    pub fn resolve(
        &self,
        provider_id: &str,
        key_id: Option<&str>,
        _run_id: &str,
    ) -> Result<Credential, String> {
        if provider_id.trim().is_empty() {
            return Err("provider_id is required".into());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let key_id = key_id.unwrap_or("").trim();

        let row = if !key_id.is_empty() && key_id != "_primary_" {
            conn.query_row(
                "SELECT k.id, k.api_key_encrypted, k.dek_encrypted, p.base_url, COALESCE(NULLIF(p.api_protocol, ''), p.preset_name)
                 FROM provider_api_keys k
                 JOIN user_providers p ON k.provider_id = p.id
                 WHERE k.id = ?1 AND k.provider_id = ?2
                 LIMIT 1",
                rusqlite::params![key_id, provider_id],
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
            .map_err(|_| {
                format!("No key id '{key_id}' for provider '{provider_id}' in natives.db")
            })?
        } else {
            conn.query_row(
                "SELECT k.id, k.api_key_encrypted, k.dek_encrypted, p.base_url, COALESCE(NULLIF(p.api_protocol, ''), p.preset_name)
                 FROM provider_api_keys k
                 JOIN user_providers p ON k.provider_id = p.id
                 WHERE k.provider_id = ?1 AND COALESCE(k.is_active, 1) = 1
                 ORDER BY COALESCE(k.is_primary, 0) DESC, k.created_at DESC
                 LIMIT 1",
                rusqlite::params![provider_id],
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
            .map_err(|_| format!("No active key for provider '{provider_id}' in natives.db"))?
        };

        let (id, api_key_encrypted, dek_encrypted, base_url, provider_type) = row;
        let api_key = envelope_decrypt(&api_key_encrypted, &dek_encrypted, &conn)?;
        if api_key.trim().is_empty() {
            return Err(format!("Decrypted empty key for provider '{provider_id}'"));
        }
        Ok(Credential {
            api_key,
            base_url: base_url.filter(|s| !s.trim().is_empty()),
            proxy_url: global_proxy_url(&conn)?,
            key_id: Some(id),
            provider_type,
        })
    }

    /// Resolve active pool members in deterministic priority order. Selection and
    /// transient health remain in the routing module; this broker only leases
    /// encrypted material from the Host-owned store.
    pub fn resolve_sub2api_pool(
        &self,
        provider_id: &str,
    ) -> Result<Vec<Sub2ApiAccountCredential>, String> {
        if provider_id.trim().is_empty() {
            return Err("provider_id is required".into());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.provider_id, a.platform, a.account_type, a.credentials_encrypted,
                    a.dek_encrypted, a.extra_json, a.priority, a.concurrency, a.expires_at,
                    p.config_encrypted, p.dek_encrypted
             FROM provider_accounts
             a LEFT JOIN provider_account_proxies p ON p.id = a.proxy_id
             WHERE a.provider_id = ?1 AND a.status = 'active'
               AND (a.expires_at IS NULL OR a.expires_at = '' OR a.expires_at > datetime('now'))
             ORDER BY a.priority ASC, a.id ASC",
            )
            .map_err(|e| format!("prepare Sub2API pool: {e}"))?;
        let rows = stmt
            .query_map([provider_id], |row| {
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
            .map_err(|e| format!("query Sub2API pool: {e}"))?;
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
            ) = row.map_err(|e| e.to_string())?;
            let plaintext = envelope_decrypt(&encrypted, &dek, &conn)?;
            let credentials = serde_json::from_str(&plaintext)
                .map_err(|_| format!("Sub2API account '{id}' has invalid credentials"))?;
            let extra = serde_json::from_str(&extra).unwrap_or(Value::Object(Default::default()));
            let proxy_url = match (proxy_encrypted, proxy_dek) {
                (Some(encrypted), Some(dek)) => proxy_url_from_value(
                    &serde_json::from_str::<Value>(&envelope_decrypt(&encrypted, &dek, &conn)?)
                        .map_err(|_| format!("Sub2API account '{id}' has invalid proxy"))?,
                ),
                _ => global_proxy_url(&conn)?,
            };
            accounts.push(Sub2ApiAccountCredential {
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
        Ok(accounts)
    }

    pub fn loopback_settings(&self) -> Result<LoopbackSettings, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let row = conn
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
            .map_err(|e| format!("read routing settings: {e}"))?;
        let (routing_enabled, local_enabled, port, token, token_dek, rectifier) = row;
        let bearer_token = match (token, token_dek) {
            (Some(token), Some(dek)) => Some(envelope_decrypt(&token, &dek, &conn)?),
            _ => None,
        };
        Ok(LoopbackSettings {
            enabled: routing_enabled != 0 && local_enabled != 0,
            port: u16::try_from(port).unwrap_or(15721),
            bearer_token,
            rectifier: serde_json::from_str(&rectifier)
                .unwrap_or(Value::Object(Default::default())),
        })
    }

    /// Encrypt and atomically replace a refreshed OAuth credential document.
    /// The caller supplies only memory-resident JSON; it is never logged.
    pub fn update_sub2api_credentials(
        &self,
        account_id: &str,
        credentials: &Value,
        expires_at: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let (encrypted, dek) = envelope_encrypt(&credentials.to_string(), &conn)?;
        let changed = conn.execute(
            "UPDATE provider_accounts SET credentials_encrypted=?1, dek_encrypted=?2, expires_at=?3, status='active', updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?4",
            rusqlite::params![encrypted, dek, expires_at, account_id],
        ).map_err(|e| format!("update OAuth account: {e}"))?;
        if changed != 1 {
            return Err("Sub2API account was removed before OAuth refresh completed".into());
        }
        Ok(())
    }
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

fn global_proxy_url(conn: &Connection) -> Result<Option<String>, String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT global_proxy_json FROM provider_routing_settings WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("read global proxy: {e}"))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
    if value.get("enabled").and_then(Value::as_bool) != Some(true) {
        return Ok(None);
    }
    if let (Some(ciphertext), Some(dek)) = (
        value.get("url_encrypted").and_then(Value::as_str),
        value.get("dek_encrypted").and_then(Value::as_str),
    ) {
        let plain = envelope_decrypt(ciphertext, dek, conn)?;
        return Ok(valid_proxy_url(&plain).then_some(plain));
    }
    // Legacy rows have no proxy credentials encryption. They are accepted for
    // upgrade compatibility but are rewritten encrypted on the next settings save.
    Ok(value
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| valid_proxy_url(url))
        .map(str::to_string))
}

fn valid_proxy_url(url: &str) -> bool {
    let value = url.trim();
    !value.chars().any(char::is_control)
        && (value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("socks5://"))
}

fn load_kek(conn: &Connection) -> Result<[u8; 32], String> {
    let hex: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'provider_kek' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "provider_kek not found in natives.db settings".to_string())?;
    let bytes = hex::decode(hex.trim()).map_err(|e| format!("decode KEK: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("KEK wrong length: {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn envelope_decrypt(
    api_key_encrypted: &str,
    dek_encrypted: &str,
    conn: &Connection,
) -> Result<String, String> {
    let kek = load_kek(conn)?;
    let dek_package = BASE64
        .decode(dek_encrypted)
        .map_err(|e| format!("decode DEK package: {e}"))?;
    if dek_package.len() < 12 {
        return Err("DEK package too short".into());
    }
    let (kek_nonce, dek_ct) = dek_package.split_at(12);
    let kek_cipher = Aes256Gcm::new_from_slice(&kek).map_err(|e| format!("KEK cipher: {e}"))?;
    let dek = kek_cipher
        .decrypt(Nonce::from_slice(kek_nonce), dek_ct)
        .map_err(|e| format!("DEK decrypt failed: {e}"))?;

    let payload = BASE64
        .decode(api_key_encrypted)
        .map_err(|e| format!("decode api key: {e}"))?;
    if payload.len() < 12 {
        return Err("API key payload too short".into());
    }
    let (nonce, ct) = payload.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&dek).map_err(|e| format!("DEK cipher: {e}"))?;
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|e| format!("API key decrypt failed: {e}"))?;
    String::from_utf8(plain).map_err(|e| format!("utf8: {e}"))
}

fn envelope_encrypt(plaintext: &str, conn: &Connection) -> Result<(String, String), String> {
    let kek = load_kek(conn)?;
    let mut dek = [0u8; 32];
    OsRng.fill_bytes(&mut dek);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = Aes256Gcm::new_from_slice(&dek)
        .map_err(|e| e.to_string())?
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| "encrypt OAuth credentials failed".to_string())?;
    let mut payload = nonce.to_vec();
    payload.extend(ciphertext);
    let mut kek_nonce = [0u8; 12];
    OsRng.fill_bytes(&mut kek_nonce);
    let dek_ciphertext = Aes256Gcm::new_from_slice(&kek)
        .map_err(|e| e.to_string())?
        .encrypt(Nonce::from_slice(&kek_nonce), dek.as_slice())
        .map_err(|_| "encrypt OAuth DEK failed".to_string())?;
    let mut dek_payload = kek_nonce.to_vec();
    dek_payload.extend(dek_ciphertext);
    Ok((BASE64.encode(payload), BASE64.encode(dek_payload)))
}

/// Default path: `$NATIVES_DB_PATH` or `~/.natives/natives.db`.
/// Credentials only — never the assistant conversation authority store.
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

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Install process-wide broker if natives.db is available. Returns true if installed.
pub fn try_install_natives_db_broker() -> bool {
    let path = default_natives_db_path();
    match NativesDbBroker::open(&path) {
        Ok(broker) => {
            let broker = std::sync::Arc::new(broker);
            crate::production::install_credential_broker(std::sync::Arc::new(
                move |provider_id, key_id, run_id| broker.resolve(provider_id, key_id, run_id),
            ));
            eprintln!(
                "[agent-daemon] Credential broker installed from {}",
                path.display()
            );
            true
        }
        Err(e) => {
            eprintln!(
                "[agent-daemon] natives.db broker not installed ({}): {e}",
                path.display()
            );
            false
        }
    }
}

pub fn read_setting(key: &str) -> Result<Option<String>, String> {
    let path = default_natives_db_path();
    if !path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(&path).map_err(|e| format!("open natives.db failed: {e}"))?;
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1 LIMIT 1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(e.to_string()),
    })
}

pub fn write_setting(key: &str, value: &str) -> Result<(), String> {
    let path = default_natives_db_path();
    let conn = Connection::open(&path).map_err(|e| format!("open natives.db failed: {e}"))?;

    // Ensure settings table exists (fallback for older schema versions)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{Aead, AeadCore, OsRng as AesOsRng};
    use rand::RngCore;

    fn setup_db_with_key(api_key: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("natives.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE user_providers (
               id TEXT PRIMARY KEY, preset_name TEXT, api_protocol TEXT, name TEXT,
               website_url TEXT, base_url TEXT, created_at TEXT, updated_at TEXT
             );
             CREATE TABLE provider_api_keys (
               id TEXT PRIMARY KEY, provider_id TEXT, label TEXT,
               api_key_encrypted TEXT, dek_encrypted TEXT, created_at TEXT,
               is_primary INTEGER DEFAULT 0, is_active INTEGER DEFAULT 1
             );",
        )
        .unwrap();

        let mut kek = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut kek);
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('provider_kek', ?1)",
            [hex::encode(kek)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_providers (id, preset_name, api_protocol, name, website_url, base_url, created_at, updated_at)
             VALUES ('provider-uuid', 'anthropic', 'anthropic_messages', 'x', '', 'https://example.test', 't', 't')",
            [],
        )
        .unwrap();

        let mut dek = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut dek);
        let dek_cipher = Aes256Gcm::new_from_slice(&dek).unwrap();
        let nonce = Aes256Gcm::generate_nonce(&mut AesOsRng);
        let ct = dek_cipher.encrypt(&nonce, api_key.as_bytes()).unwrap();
        let mut api_pkg = nonce.to_vec();
        api_pkg.extend_from_slice(&ct);
        let api_key_encrypted = BASE64.encode(&api_pkg);

        let kek_cipher = Aes256Gcm::new_from_slice(&kek).unwrap();
        let kek_nonce = Aes256Gcm::generate_nonce(&mut AesOsRng);
        let dek_ct = kek_cipher.encrypt(&kek_nonce, dek.as_slice()).unwrap();
        let mut dek_pkg = kek_nonce.to_vec();
        dek_pkg.extend_from_slice(&dek_ct);
        let dek_encrypted = BASE64.encode(&dek_pkg);

        conn.execute(
            "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, created_at, is_primary, is_active)
             VALUES ('k1', 'provider-uuid', 't', ?1, ?2, 't', 1, 1)",
            rusqlite::params![api_key_encrypted, dek_encrypted],
        )
        .unwrap();
        drop(conn);
        (dir, path)
    }

    #[test]
    fn resolves_and_decrypts_primary_key() {
        let (_dir, path) = setup_db_with_key("sk-test-secret-key-value");
        let broker = NativesDbBroker::open(&path).unwrap();
        let cred = broker.resolve("provider-uuid", None, "run-1").unwrap();
        assert_eq!(cred.api_key, "sk-test-secret-key-value");
        assert_eq!(cred.key_id.as_deref(), Some("k1"));
        assert_eq!(cred.base_url.as_deref(), Some("https://example.test"));
        assert_eq!(cred.provider_type.as_deref(), Some("anthropic_messages"));
    }
}
