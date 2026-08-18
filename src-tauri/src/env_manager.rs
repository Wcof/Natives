use crate::{db, Error, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use lazy_static::lazy_static;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::sync::Mutex;

const ENCRYPTION_KEY_SETTING: &str = "env_encryption_key";

/// Ciphertext format prefix — identifies AES-256-GCM v2 payloads
const V2_PREFIX: &str = "v2:";
// Old XOR format was just raw base64 (no prefix).

lazy_static! {
    /// In-memory cache of the env encryption key, loaded once from SQLite.
    static ref ENV_KEY_CACHE: Mutex<Option<String>> = Mutex::new(None);
}

#[cfg(test)]
pub fn reset_env_key_cache_for_tests() {
    let mut cache = ENV_KEY_CACHE.lock().unwrap();
    *cache = None;
}

#[derive(Debug, Serialize)]
pub struct EnvProfile {
    pub id: i64,
    pub name: String,
    pub is_default: i32,
    pub created_at: String,
    pub variables: Vec<EnvVariableMetadata>,
}

#[derive(Debug, Serialize)]
pub struct EnvVariableMetadata {
    pub key: String,
    pub has_value: bool,
    pub masked: bool,
}

/// Get or create the encryption key.
///
/// The key is stored in SQLite `settings`, following docs/standards/technical/02-security.md.
/// Result is cached in `ENV_KEY_CACHE` after first access.
pub fn get_encryption_key(conn: &Connection) -> Result<String> {
    let cache = ENV_KEY_CACHE.lock().unwrap();
    if let Some(key) = cache.as_ref() {
        #[cfg(test)]
        {
            // Tests run against separate in-memory DBs but share the process-
            // global ENV_KEY_CACHE, so a concurrent test can swap in its own DB's
            // key. Verify the cached key belongs to THIS connection before
            // trusting it; re-init (which re-reads this DB's stored key) when it
            // does not. Compiled out of production builds — zero overhead there.
            match db::get_setting(conn, ENCRYPTION_KEY_SETTING) {
                Ok(Some(stored)) if &stored == key => {}
                _ => {
                    drop(cache);
                    return init_env_encryption_key(conn);
                }
            }
        }
        return Ok(key.clone());
    }
    drop(cache);
    init_env_encryption_key(conn)
}

/// Initialize the env encryption key from SQLite and cache it.
/// Called once at app startup (or lazily by `get_encryption_key` on first
/// access). Generates and stores a new key if none exists.
pub fn init_env_encryption_key(conn: &Connection) -> Result<String> {
    let key = match db::get_setting(conn, ENCRYPTION_KEY_SETTING)? {
        Some(key) => {
            validate_hex_key(&key)?;
            key
        }
        None => {
            let new_key = generate_random_hex(32);
            db::set_setting(conn, ENCRYPTION_KEY_SETTING, &new_key)?;
            new_key
        }
    };

    let mut cache = ENV_KEY_CACHE.lock().unwrap();
    *cache = Some(key.clone());
    Ok(key)
}

fn validate_hex_key(key: &str) -> Result<()> {
    let bytes = hex::decode(key)
        .map_err(|e| Error::Internal(format!("invalid encryption key hex: {e}")))?;
    if bytes.len() != 32 {
        return Err(Error::Internal(format!(
            "invalid encryption key length: expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    Ok(())
}

fn generate_random_hex(bytes: usize) -> String {
    // P1-017: use rand::OsRng (OS CSPRNG) instead of /dev/urandom with
    // platform-specific expect. rand's OsRng is cross-platform (macOS/Linux/
    // Windows) and fails closed with a Result instead of panicking.
    let mut buf = vec![0u8; bytes];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut buf);
    hex::encode(buf)
}

/// Encrypt text using AES-256-GCM with random nonce.
///
/// Format: `v2:<nonce_hex>:<ciphertext_hex>:<tag_hex>`
///
/// The nonce (12 bytes) and tag (16 bytes) are randomly generated per encryption.
pub fn encrypt(text: &str, encryption_key: &str) -> Result<String> {
    let key_bytes = hex::decode(encryption_key)
        .map_err(|e| Error::Internal(format!("invalid key hex: {e}")))?;
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

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
    let key_bytes = hex::decode(encryption_key)
        .map_err(|e| Error::Internal(format!("invalid key hex: {e}")))?;

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
        let tag_bytes =
            hex::decode(parts[2]).map_err(|e| Error::Internal(format!("invalid tag hex: {e}")))?;

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

        let plaintext = cipher.decrypt(nonce, combined.as_slice()).map_err(|_| {
            Error::Internal("decryption failed (wrong key or corrupted data)".into())
        })?;

        String::from_utf8(plaintext)
            .map_err(|e| Error::Internal(format!("decrypted bytes not valid UTF-8: {e}")))
    } else {
        // Legacy XOR + base64 format — attempt migration
        let encrypted = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
            .map_err(|e| {
                Error::Internal(format!("legacy base64 decode failed (corrupted data): {e}"))
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

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() || name.trim() != name || name.chars().any(char::is_control) {
        return Err(Error::InvalidInput(
            "profile name must be non-empty and contain no control characters".into(),
        ));
    }
    if name.chars().count() > 128 {
        return Err(Error::InvalidInput(
            "profile name must be 128 characters or fewer".into(),
        ));
    }
    Ok(())
}

pub fn validate_variable_key(key: &str) -> Result<()> {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return Err(Error::InvalidInput(
            "environment key must not be empty".into(),
        ));
    };
    if key.len() > 256
        || !(first == '_' || first.is_ascii_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return Err(Error::InvalidInput(
            "environment key must use ASCII letters, numbers, and underscores and start with a letter or underscore".into(),
        ));
    }
    Ok(())
}

fn profile_id_exists(conn: &Connection, profile_id: i64) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM env_profiles WHERE id = ?1)",
        rusqlite::params![profile_id],
        |row| row.get(0),
    )
    .map_err(Error::Database)
}

fn variable_metadata(conn: &Connection, profile_id: i64) -> Result<Vec<EnvVariableMetadata>> {
    let mut stmt = conn
        .prepare(
            "SELECT key, value_encrypted <> '' FROM env_variables WHERE profile_id = ?1 ORDER BY key",
        )
        .map_err(Error::Database)?;
    let mut rows = stmt
        .query(rusqlite::params![profile_id])
        .map_err(Error::Database)?;
    let mut variables = Vec::new();
    while let Some(row) = rows.next().map_err(Error::Database)? {
        variables.push(EnvVariableMetadata {
            key: row.get(0).map_err(Error::Database)?,
            has_value: row.get(1).map_err(Error::Database)?,
            masked: true,
        });
    }
    Ok(variables)
}

pub fn create_profile(conn: &Connection, name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO env_profiles (name, is_default, created_at) VALUES (?1, 0, ?2)",
        rusqlite::params![name, now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn delete_profile(conn: &Connection, name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let profile = conn
        .query_row(
            "SELECT is_default FROM env_profiles WHERE name = ?1",
            rusqlite::params![name],
            |row| row.get::<_, i32>(0),
        )
        .optional()
        .map_err(Error::Database)?
        .ok_or_else(|| Error::NotFound(format!("environment profile not found: {name}")))?;
    if profile != 0 {
        return Err(Error::Conflict(
            "cannot delete the active default environment profile".into(),
        ));
    }
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
    let mut raw_profiles = Vec::new();
    let mut rows = stmt.query([]).map_err(Error::Database)?;
    while let Some(row) = rows.next().map_err(Error::Database)? {
        raw_profiles.push((
            row.get(0).map_err(Error::Database)?,
            row.get(1).map_err(Error::Database)?,
            row.get(2).map_err(Error::Database)?,
            row.get(3).map_err(Error::Database)?,
        ));
    }
    drop(rows);
    drop(stmt);
    raw_profiles
        .into_iter()
        .map(|(id, name, is_default, created_at)| {
            Ok(EnvProfile {
                id,
                name,
                is_default,
                created_at,
                variables: variable_metadata(conn, id)?,
            })
        })
        .collect()
}

pub fn set_default_profile(conn: &Connection, name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM env_profiles WHERE name = ?1)",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .map_err(Error::Database)?;
    if !exists {
        return Err(Error::NotFound(format!(
            "environment profile not found: {name}"
        )));
    }
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
    let raw_profile = match rows.next().map_err(Error::Database)? {
        Some(row) => Some((
            row.get(0).map_err(Error::Database)?,
            row.get(1).map_err(Error::Database)?,
            row.get(2).map_err(Error::Database)?,
            row.get(3).map_err(Error::Database)?,
        )),
        None => None,
    };
    drop(rows);
    drop(stmt);
    raw_profile
        .map(|(id, name, is_default, created_at)| {
            Ok(EnvProfile {
                id,
                name,
                is_default,
                created_at,
                variables: variable_metadata(conn, id)?,
            })
        })
        .transpose()
}

// ── Variable CRUD ──

pub fn set_variable(
    conn: &Connection,
    profile_id: i64,
    key: &str,
    value: &str,
    encryption_key: &str,
) -> Result<()> {
    validate_variable_key(key)?;
    if !profile_id_exists(conn, profile_id)? {
        return Err(Error::NotFound(format!(
            "environment profile not found: {profile_id}"
        )));
    }
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
    validate_variable_key(key)?;
    if !profile_id_exists(conn, profile_id)? {
        return Err(Error::NotFound(format!(
            "environment profile not found: {profile_id}"
        )));
    }
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
    if !profile_id_exists(conn, profile_id)? {
        return Err(Error::NotFound(format!(
            "environment profile not found: {profile_id}"
        )));
    }
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
                return Err(Error::Internal(
                    "failed to decrypt environment variable".into(),
                ))
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
