use crate::{db, Error, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rusqlite::Connection;
use serde::Serialize;

const ENCRYPTION_KEY_SETTING: &str = "env_encryption_key";

/// Ciphertext format prefix — identifies AES-256-GCM v2 payloads
const V2_PREFIX: &str = "v2:";
/// Old XOR format was just raw base64 (no prefix)

#[derive(Debug, Serialize)]
pub struct EnvProfile {
    pub id: i64,
    pub name: String,
    pub is_default: i32,
    pub created_at: String,
}

/// Get or create the encryption key.
///
/// Primary: macOS Keychain / Windows Credential Manager / Linux Secret Service
/// via the `keyring` crate. Falls back to SQLite `settings` table if keyring
/// is unavailable (e.g., headless CI), and migrates the key on first successful
/// keyring access.
pub fn get_encryption_key(conn: &Connection) -> Result<String> {
    const SERVICE: &str = "natives";
    const USERNAME: &str = "env_encryption_key";

    // 1. Try OS keyring first
    if let Ok(entry) = keyring::Entry::new(SERVICE, USERNAME) {
        match entry.get_password() {
            Ok(key) => {
                // Key found in keyring — if SQLite still has the old key, migrate & delete
                if let Ok(Some(db_key)) = db::get_setting(conn, ENCRYPTION_KEY_SETTING) {
                    if db_key == key {
                        // Same key — safe to remove from SQLite
                        let _ = db::delete_setting(conn, ENCRYPTION_KEY_SETTING);
                    }
                    // If different, keyring takes precedence; don't delete SQLite key
                    // (it may be needed for a rollback)
                }
                return Ok(key);
            }
            Err(keyring::Error::NoEntry) => {
                // Not in keyring yet — check SQLite for migration
                if let Ok(Some(db_key)) = db::get_setting(conn, ENCRYPTION_KEY_SETTING) {
                    // Migrate existing SQLite key to keyring
                    if entry.set_password(&db_key).is_ok() {
                        let _ = db::delete_setting(conn, ENCRYPTION_KEY_SETTING);
                    }
                    return Ok(db_key);
                }
                // No key anywhere — generate new one, store in keyring
                let new_key = generate_random_hex(32);
                if entry.set_password(&new_key).is_err() {
                    // Keyring write failed — fall back to SQLite
                    let _ = db::set_setting(conn, ENCRYPTION_KEY_SETTING, &new_key);
                }
                return Ok(new_key);
            }
            Err(_) => {
                // Keyring access error — fall through to SQLite fallback
            }
        }
    }

    // 2. Fallback: SQLite settings table (for headless/CI environments)
    match db::get_setting(conn, ENCRYPTION_KEY_SETTING)? {
        Some(key) => Ok(key),
        None => {
            let new_key = generate_random_hex(32);
            db::set_setting(conn, ENCRYPTION_KEY_SETTING, &new_key)?;
            Ok(new_key)
        }
    }
}

fn generate_random_hex(bytes: usize) -> String {
    use std::io::Read;
    let mut rng = std::fs::File::open("/dev/urandom")
        .expect("failed to open /dev/urandom");
    let mut buf = vec![0u8; bytes];
    rng.read_exact(&mut buf)
        .expect("failed to read from /dev/urandom");
    hex::encode(buf)
}

/// Encrypt text using AES-256-GCM with random nonce.
///
/// Format: `v2:<nonce_hex>:<ciphertext_hex>:<tag_hex>`
///
/// The nonce (12 bytes) and tag (16 bytes) are randomly generated per encryption.
pub fn encrypt(text: &str, encryption_key: &str) -> Result<String> {
    let key_bytes =
        hex::decode(encryption_key).map_err(|e| Error::Internal(format!("invalid key hex: {e}")))?;
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher =
        Aes256Gcm::new(key);

    // Generate random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, text.as_bytes())
        .map_err(|e| Error::Internal(format!("encryption failed: {e}")))?;

    // ciphertext = actual_ciphertext (len=plaintext) ++ tag (16 bytes)
    // aes-gcm's encrypt() returns ciphertext || tag concatenated
    let actual_ct = &ciphertext[..ciphertext.len() - 16];
    let tag = &ciphertext[ciphertext.len() - 16..];

    Ok(format!(
        "{}{}:{}:{}",
        V2_PREFIX,
        hex::encode(nonce_bytes),
        hex::encode(actual_ct),
        hex::encode(tag)
    ))
}

/// Decrypt text encrypted with AES-256-GCM.
///
/// Also handles legacy XOR + base64 format (no `v2:` prefix) for backward
/// compatibility — on read, old values are transparently re-encrypted as v2
/// by the caller (set_variable stores newly encrypted values).
pub fn decrypt(encoded: &str, encryption_key: &str) -> Result<String> {
    let key_bytes =
        hex::decode(encryption_key).map_err(|e| Error::Internal(format!("invalid key hex: {e}")))?;

    if encoded.starts_with(V2_PREFIX) {
        // New AES-256-GCM format: v2:<nonce_hex>:<ciphertext_hex>:<tag_hex>
        let inner = encoded.strip_prefix(V2_PREFIX).unwrap_or("");
        let parts: Vec<&str> = inner.split(':').collect();
        if parts.len() != 3 {
            return Err(Error::Internal("invalid v2 ciphertext format".into()));
        }
        let nonce_bytes = hex::decode(parts[0])
            .map_err(|e| Error::Internal(format!("invalid nonce hex: {e}")))?;
        let ct_bytes = hex::decode(parts[1])
            .map_err(|e| Error::Internal(format!("invalid ciphertext hex: {e}")))?;
        let tag_bytes = hex::decode(parts[2])
            .map_err(|e| Error::Internal(format!("invalid tag hex: {e}")))?;

        if nonce_bytes.len() != 12 {
            return Err(Error::Internal("nonce must be 12 bytes".into()));
        }
        if tag_bytes.len() != 16 {
            return Err(Error::Internal("tag must be 16 bytes".into()));
        }

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Reconstruct the ciphertext || tag for decrypt
        let mut combined = ct_bytes;
        combined.extend_from_slice(&tag_bytes);

        let plaintext = cipher
            .decrypt(nonce, combined.as_slice())
            .map_err(|_| Error::Internal("decryption failed (wrong key or corrupted data)".into()))?;

        String::from_utf8(plaintext)
            .map_err(|e| Error::Internal(format!("decrypted bytes not valid UTF-8: {e}")))
    } else {
        // Legacy XOR + base64 format — attempt migration
        let encrypted = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            encoded,
        )
        .map_err(|e| {
            Error::Internal(format!(
                "legacy base64 decode failed (corrupted data): {e}"
            ))
        })?;
        let mut decrypted = Vec::with_capacity(encrypted.len());
        for (i, &byte) in encrypted.iter().enumerate() {
            decrypted.push(byte ^ key_bytes[i % key_bytes.len()]);
        }
        String::from_utf8(decrypted)
            .map_err(|e| Error::Internal(format!("decrypted bytes not valid UTF-8: {e}")))
    }
}

// ── Profile CRUD ──

pub fn create_profile(conn: &Connection, name: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO env_profiles (name, is_default, created_at) VALUES (?1, 0, ?2)",
        rusqlite::params![name, now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn delete_profile(conn: &Connection, name: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM env_profiles WHERE name = ?1",
        rusqlite::params![name],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn list_profiles(conn: &Connection) -> Result<Vec<EnvProfile>> {
    let mut stmt = conn
        .prepare("SELECT id, name, is_default, created_at FROM env_profiles ORDER BY id")
        .map_err(Error::Database)?;
    let mut results = Vec::new();
    let mut rows = stmt.query([]).map_err(Error::Database)?;
    while let Some(row) = rows.next().map_err(Error::Database)? {
        results.push(EnvProfile {
            id: row.get(0).map_err(Error::Database)?,
            name: row.get(1).map_err(Error::Database)?,
            is_default: row.get(2).map_err(Error::Database)?,
            created_at: row.get(3).map_err(Error::Database)?,
        });
    }
    Ok(results)
}

pub fn set_default_profile(conn: &Connection, name: &str) -> Result<()> {
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    tx.execute("UPDATE env_profiles SET is_default = 0", [])
        .map_err(Error::Database)?;
    tx.execute(
        "UPDATE env_profiles SET is_default = 1 WHERE name = ?1",
        rusqlite::params![name],
    )
    .map_err(Error::Database)?;
    tx.commit().map_err(Error::Database)?;
    Ok(())
}

pub fn get_default_profile(conn: &Connection) -> Result<Option<EnvProfile>> {
    let mut stmt = conn
        .prepare("SELECT id, name, is_default, created_at FROM env_profiles WHERE is_default = 1 LIMIT 1")
        .map_err(Error::Database)?;
    let mut rows = stmt.query([]).map_err(Error::Database)?;
    match rows.next().map_err(Error::Database)? {
        Some(row) => Ok(Some(EnvProfile {
            id: row.get(0).map_err(Error::Database)?,
            name: row.get(1).map_err(Error::Database)?,
            is_default: row.get(2).map_err(Error::Database)?,
            created_at: row.get(3).map_err(Error::Database)?,
        })),
        None => Ok(None),
    }
}

// ── Variable CRUD ──

pub fn set_variable(
    conn: &Connection,
    profile_id: i64,
    key: &str,
    value: &str,
    encryption_key: &str,
) -> Result<()> {
    let encrypted = encrypt(value, encryption_key)?;
    conn.execute(
        "INSERT INTO env_variables (profile_id, key, value_encrypted) VALUES (?1, ?2, ?3)
         ON CONFLICT(profile_id, key) DO UPDATE SET value_encrypted = excluded.value_encrypted",
        rusqlite::params![profile_id, key, encrypted],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn delete_variable(conn: &Connection, profile_id: i64, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM env_variables WHERE profile_id = ?1 AND key = ?2",
        rusqlite::params![profile_id, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn get_variables(
    conn: &Connection,
    profile_id: i64,
    encryption_key: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt = conn
        .prepare("SELECT key, value_encrypted FROM env_variables WHERE profile_id = ?1")
        .map_err(Error::Database)?;
    let mut result = std::collections::HashMap::new();
    let mut rows = stmt
        .query(rusqlite::params![profile_id])
        .map_err(Error::Database)?;
    while let Some(row) = rows.next().map_err(Error::Database)? {
        let key: String = row.get(0).map_err(Error::Database)?;
        let encrypted: String = row.get(1).map_err(Error::Database)?;
        match decrypt(&encrypted, encryption_key) {
            Ok(value) => {
                // If value was in legacy format, transparently re-encrypt as v2
                if !encrypted.starts_with(V2_PREFIX) {
                    if let Ok(new_encrypted) = encrypt(&value, encryption_key) {
                        let _ = conn.execute(
                            "UPDATE env_variables SET value_encrypted = ?1 WHERE profile_id = ?2 AND key = ?3",
                            rusqlite::params![new_encrypted, profile_id, key],
                        );
                    }
                }
                result.insert(key, value);
            }
            Err(_) => {
                // If decryption fails, skip this variable
                eprintln!("failed to decrypt env variable: {key}");
            }
        }
    }
    Ok(result)
}

/// Inject env variables from a profile into a HashMap (for terminal sessions)
#[allow(dead_code)]
pub fn inject_env(
    conn: &Connection,
    profile_id: i64,
    encryption_key: &str,
    env: &mut std::collections::HashMap<String, String>,
) -> Result<()> {
    let vars = get_variables(conn, profile_id, encryption_key)?;
    for (key, value) in vars {
        // Only set if not already present (caller's existing keys take precedence)
        env.entry(key).or_insert(value);
    }
    Ok(())
}
