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

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use provider_adapters::capabilities::Credential;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Open natives.db and resolve provider keys for the daemon process.
pub struct NativesDbBroker {
    path: PathBuf,
    /// Serialized access; rusqlite Connection is not Sync.
    conn: Mutex<Connection>,
}

impl NativesDbBroker {
    /// Open existing natives.db (read-write is required for SQLite WAL open on some
    /// systems; we never write key material — only SELECT).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(format!("natives.db not found at {}", path.display()));
        }
        let conn = Connection::open(&path)
            .map_err(|e| format!("open natives.db failed: {e}"))?;
        // Best-effort read-only pragma (may fail if already WAL-opened elsewhere).
        let _ = conn.execute_batch("PRAGMA query_only=ON; PRAGMA busy_timeout=3000;");
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
            key_id: Some(id),
            provider_type,
        })
    }
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
    let kek_cipher =
        Aes256Gcm::new_from_slice(&kek).map_err(|e| format!("KEK cipher: {e}"))?;
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

/// Default path: `$NATIVES_DB_PATH` or `~/.natives/natives.db`.
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
        let cred = broker
            .resolve("provider-uuid", None, "run-1")
            .unwrap();
        assert_eq!(cred.api_key, "sk-test-secret-key-value");
        assert_eq!(cred.key_id.as_deref(), Some("k1"));
        assert_eq!(
            cred.base_url.as_deref(),
            Some("https://example.test")
        );
        assert_eq!(cred.provider_type.as_deref(), Some("anthropic_messages"));
    }
}
