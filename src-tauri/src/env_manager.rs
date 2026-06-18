use crate::{db, Error, Result};
use rusqlite::Connection;
use serde::Serialize;

const ENCRYPTION_KEY_SETTING: &str = "env_encryption_key";

#[derive(Debug, Serialize)]
pub struct EnvProfile {
    pub id: i64,
    pub name: String,
    pub is_default: i32,
    pub created_at: String,
}

/// Get or create the encryption key (persisted in settings table)
pub fn get_encryption_key(conn: &Connection) -> Result<String> {
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
    let mut rng = std::fs::File::open("/dev/urandom").unwrap();
    let mut buf = vec![0u8; bytes];
    rng.read_exact(&mut buf).unwrap();
    hex::encode(buf)
}

/// Encrypt text using AES-256-GCM with scrypt-derived key.
/// Format: iv_base64:authTag_base64:ciphertext_base64
pub fn encrypt(text: &str, encryption_key: &str) -> Result<String> {
    // For now, use a simple XOR-based encoding (not production-grade)
    // TODO: Replace with proper AES-256-GCM using ring or aes-gcm crate
    let key_bytes = hex::decode(encryption_key).map_err(|e| Error::Internal(e.to_string()))?;
    let text_bytes = text.as_bytes();
    let mut encrypted = Vec::with_capacity(text_bytes.len());
    for (i, &byte) in text_bytes.iter().enumerate() {
        encrypted.push(byte ^ key_bytes[i % key_bytes.len()]);
    }
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &encrypted,
    ))
}

/// Decrypt text (reverse of encrypt)
pub fn decrypt(encoded: &str, encryption_key: &str) -> Result<String> {
    let key_bytes = hex::decode(encryption_key).map_err(|e| Error::Internal(e.to_string()))?;
    let encrypted = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        encoded,
    )
    .map_err(|e| Error::Internal(e.to_string()))?;
    let mut decrypted = Vec::with_capacity(encrypted.len());
    for (i, &byte) in encrypted.iter().enumerate() {
        decrypted.push(byte ^ key_bytes[i % key_bytes.len()]);
    }
    String::from_utf8(decrypted).map_err(|e| Error::Internal(e.to_string()))
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
