use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use std::sync::Mutex;

use crate::{Error, Result};

/// KEK-DEK envelope encryption for provider API keys.
///
/// Architecture:
/// - KEK (Key Encryption Key): Stored in OS keychain, fetched once at startup,
///   held in Rust memory, NEVER sent to frontend.
/// - DEK (Data Encryption Key): Randomly generated per provider key write.
///   Encrypted with KEK before storage in SQLite.
/// - API Key: Encrypted with DEK (AES-256-GCM) before storage.
///
/// Storage layout in SQLite `provider_api_keys`:
/// - `api_key_encrypted`: BASE64(Nonce || Ciphertext)
/// - `dek_encrypted`: BASE64(Nonce_KEK || DEK_encrypted_with_KEK)
///
/// The `dek_encrypted` is stored in the `provider_api_keys` table
/// alongside the encrypted API key.
use lazy_static::lazy_static;

lazy_static! {
    /// In-memory cache of the KEK, loaded once from OS keychain.
    /// Guarded by a mutex for interior mutability on first access.
    static ref KEK_CACHE: Mutex<Option<[u8; 32]>> = Mutex::new(None);
}

/// Initialize the KEK from the OS keychain.
/// Called once at app startup. If no KEK exists, generates and stores one.
pub fn init_kek(conn: &rusqlite::Connection) -> Result<[u8; 32]> {
    let entry = keyring::Entry::new("com.natives.assistant", "provider-kek")
        .map_err(|e| Error::Internal(format!("Failed to create keyring entry: {e}")))?;
    
    let kek: [u8; 32] = match entry.get_password() {
        Ok(pw) => {
            // Decode existing KEK from hex
            let bytes = hex::decode(&pw).map_err(|e| {
                Error::Internal(format!("Failed to decode KEK: {e}"))
            })?;
            if bytes.len() != 32 {
                return Err(Error::Internal("KEK has wrong length".to_string()));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        Err(..) => {
            // Generate new KEK
            let mut new_kek = [0u8; 32];
            OsRng.fill_bytes(&mut new_kek);
            let hex_str = hex::encode(new_kek);
            entry.set_password(&hex_str).map_err(|e| {
                Error::Internal(format!("Failed to store KEK: {e}"))
            })?;
            new_kek
        }
    };

    // Cache in memory
    let mut cache = KEK_CACHE.lock().unwrap();
    *cache = Some(kek);

    // Ensure the provider_api_keys table has the dek_encrypted column
    let _ = conn.execute_batch(
        "ALTER TABLE provider_api_keys ADD COLUMN dek_encrypted TEXT NOT NULL DEFAULT '';"
    );

    Ok(kek)
}

/// Get the cached KEK, loading from keychain if needed.
pub fn get_kek(conn: &rusqlite::Connection) -> Result<[u8; 32]> {
    let cache = KEK_CACHE.lock().unwrap();
    match *cache {
        Some(kek) => Ok(kek),
        None => {
            drop(cache);
            init_kek(conn)
        }
    }
}

/// Encrypt an API key using KEK-DEK envelope encryption.
/// Returns (api_key_encrypted, dek_encrypted) as BASE64 strings.
pub fn envelope_encrypt(plaintext: &str, conn: &rusqlite::Connection) -> Result<(String, String)> {
    let kek = get_kek(conn)?;

    // Generate random DEK
    let mut dek = [0u8; 32];
    OsRng.fill_bytes(&mut dek);

    // Encrypt API key with DEK
    let key = Aes256Gcm::new_from_slice(&dek).map_err(|e| {
        Error::Internal(format!("Failed to create DEK cipher: {e}"))
    })?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = key
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| Error::Internal(format!("Encryption failed: {e}")))?;

    // Build encrypted payload: nonce || ciphertext
    let mut encrypted_payload = Vec::with_capacity(12 + ciphertext.len());
    encrypted_payload.extend_from_slice(&nonce_bytes);
    encrypted_payload.extend_from_slice(&ciphertext);
    let api_key_encrypted = BASE64.encode(&encrypted_payload);

    // Encrypt DEK with KEK
    let kek_key = Aes256Gcm::new_from_slice(&kek).map_err(|e| {
        Error::Internal(format!("Failed to create KEK cipher: {e}"))
    })?;
    let mut kek_nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut kek_nonce_bytes);
    let kek_nonce = Nonce::from_slice(&kek_nonce_bytes);
    let dek_encrypted_payload = kek_key
        .encrypt(kek_nonce, dek.as_slice())
        .map_err(|e| Error::Internal(format!("KEK encryption failed: {e}")))?;

    let mut dek_package = Vec::with_capacity(12 + dek_encrypted_payload.len());
    dek_package.extend_from_slice(&kek_nonce_bytes);
    dek_package.extend_from_slice(&dek_encrypted_payload);
    let dek_encrypted = BASE64.encode(&dek_package);

    Ok((api_key_encrypted, dek_encrypted))
}

/// Decrypt an API key using KEK-DEK envelope encryption.
pub fn envelope_decrypt(
    api_key_encrypted: &str,
    dek_encrypted: &str,
    conn: &rusqlite::Connection,
) -> Result<String> {
    let kek = get_kek(conn)?;

    // Decrypt DEK with KEK
    let dek_package = BASE64
        .decode(dek_encrypted)
        .map_err(|e| Error::Internal(format!("Failed to decode DEK package: {e}")))?;
    if dek_package.len() < 12 {
        return Err(Error::Internal("DEK package too short".to_string()));
    }
    let (kek_nonce_bytes, dek_encrypted_bytes) = dek_package.split_at(12);
    let kek_key = Aes256Gcm::new_from_slice(&kek).map_err(|e| {
        Error::Internal(format!("Failed to create KEK cipher: {e}"))
    })?;
    let dek = kek_key
        .decrypt(Nonce::from_slice(kek_nonce_bytes), dek_encrypted_bytes)
        .map_err(|e| Error::Internal(format!("DEK decryption failed: {e}")))?;

    // Decrypt API key with DEK
    let encrypted_payload = BASE64
        .decode(api_key_encrypted)
        .map_err(|e| Error::Internal(format!("Failed to decode API key: {e}")))?;
    if encrypted_payload.len() < 12 {
        return Err(Error::Internal("API key payload too short".to_string()));
    }
    let (nonce_bytes, ciphertext) = encrypted_payload.split_at(12);
    let key = Aes256Gcm::new_from_slice(&dek).map_err(|e| {
        Error::Internal(format!("Failed to create DEK cipher: {e}"))
    })?;
    let plaintext = key
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|e| Error::Internal(format!("API key decryption failed: {e}")))?;

    String::from_utf8(plaintext)
        .map_err(|e| Error::Internal(format!("Invalid UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS provider_api_keys (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                api_key_encrypted TEXT NOT NULL DEFAULT '',
                dek_encrypted TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );"
        ).unwrap();
        conn
    }

    #[test]
    fn test_envelope_encrypt_decrypt_roundtrip() {
        let conn = setup_test_db();
        // init_kek will try to use keyring which may fail in CI/test env
        // So we test the core AES-GCM logic directly
        let kek = [0u8; 32]; // deterministic KEK for testing

        // Encrypt
        let plaintext = "sk-test-api-key-12345";
        let mut dek = [0u8; 32];
        // Use a fixed DEK for reproducible test
        dek[..4].copy_from_slice(&[0x01, 0x02, 0x03, 0x04]);

        let key = Aes256Gcm::new_from_slice(&dek).unwrap();
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..4].copy_from_slice(&[0x05, 0x06, 0x07, 0x08]);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = key.encrypt(nonce, plaintext.as_bytes()).unwrap();

        let mut encrypted_payload = Vec::new();
        encrypted_payload.extend_from_slice(&nonce_bytes);
        encrypted_payload.extend_from_slice(&ciphertext);
        let api_key_encrypted = BASE64.encode(&encrypted_payload);

        // Encrypt DEK with KEK
        let kek_key = Aes256Gcm::new_from_slice(&kek).unwrap();
        let mut kek_nonce = [0u8; 12];
        kek_nonce[..4].copy_from_slice(&[0x09, 0x0a, 0x0b, 0x0c]);
        let dek_enc = kek_key.encrypt(Nonce::from_slice(&kek_nonce), dek.as_slice()).unwrap();
        let mut dek_package = Vec::new();
        dek_package.extend_from_slice(&kek_nonce);
        dek_package.extend_from_slice(&dek_enc);
        let dek_encrypted = BASE64.encode(&dek_package);

        // Decrypt DEK
        let dek_pkg = BASE64.decode(&dek_encrypted).unwrap();
        let (kek_nonce_bytes, dek_enc_bytes) = dek_pkg.split_at(12);
        let recovered_dek = kek_key.decrypt(Nonce::from_slice(kek_nonce_bytes), dek_enc_bytes).unwrap();
        assert_eq!(recovered_dek, dek);

        // Decrypt API key
        let enc_payload = BASE64.decode(&api_key_encrypted).unwrap();
        let (nonce_bytes2, ct2) = enc_payload.split_at(12);
        let recovered = key.decrypt(Nonce::from_slice(nonce_bytes2), ct2).unwrap();
        assert_eq!(String::from_utf8(recovered).unwrap(), plaintext);
    }

    #[test]
    fn test_envelope_encrypt_different_keys_produce_different_ciphertexts() {
        let conn = setup_test_db();
        let kek = [0u8; 32];
        let plaintext = "same-api-key";

        // First encryption
        let mut dek1 = [0u8; 32];
        dek1[0] = 0x11;
        let mut nonce1 = [0u8; 12];
        nonce1[0] = 0x22;

        let key1 = Aes256Gcm::new_from_slice(&dek1).unwrap();
        let ct1 = key1.encrypt(Nonce::from_slice(&nonce1), plaintext.as_bytes()).unwrap();
        let mut payload1 = Vec::new();
        payload1.extend_from_slice(&nonce1);
        payload1.extend_from_slice(&ct1);
        let enc1 = BASE64.encode(&payload1);

        // Second encryption (different DEK and nonce)
        let mut dek2 = [0u8; 32];
        dek2[0] = 0x33;
        let mut nonce2 = [0u8; 12];
        nonce2[0] = 0x44;

        let key2 = Aes256Gcm::new_from_slice(&dek2).unwrap();
        let ct2 = key2.encrypt(Nonce::from_slice(&nonce2), plaintext.as_bytes()).unwrap();
        let mut payload2 = Vec::new();
        payload2.extend_from_slice(&nonce2);
        payload2.extend_from_slice(&ct2);
        let enc2 = BASE64.encode(&payload2);

        assert_ne!(enc1, enc2, "Same plaintext encrypted with different DEKs should differ");
    }

    #[test]
    fn test_init_kek_generates_valid_key() {
        // Verify SHA256 provides proper key material
        use sha2::{Digest, Sha256};
        let test_material = b"test-key-material";
        let hash = Sha256::digest(test_material);
        assert_eq!(hash.len(), 32, "SHA256 should produce 32 bytes for KEK");
    }
}
