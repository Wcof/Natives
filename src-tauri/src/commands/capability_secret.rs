//! Capability-library secret store (ADR-0016 decision 7).
//!
//! Host-owned encrypted storage for MCP env vars, bearer tokens and OAuth
//! refresh tokens in `natives.db` (`capability_secrets`, schema v11).
//!
//! Encryption reuses the provider_api_keys KEK-DEK envelope from
//! `provider_key_manager` — never a second copy of the crypto:
//! - `ciphertext` column = BASE64(nonce || AES-256-GCM ciphertext) under a per-row DEK
//! - `nonce` column      = BASE64(kek_nonce || DEK wrapped by the provider KEK)
//!
//! Plaintext never leaves this module towards the frontend: `list` returns
//! metadata only; decryption happens exclusively in the daemon via
//! `NativesDbBroker::read_capability_secret` (read-only, memory-only).

use crate::{provider_key_manager, AppState, Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

const ALLOWED_KINDS: &[&str] = &["mcp_env", "mcp_bearer", "mcp_oauth_refresh"];

// ── Data types ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySecretSetInput {
    pub kind: String,
    pub owner_ref: String,
    pub key_name: Option<String>,
    pub plaintext: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySecretSetResult {
    pub id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySecretEntry {
    pub id: String,
    pub kind: String,
    pub key_name: Option<String>,
    pub created_at: String,
}

// ── Core (shared with mcp_oauth for refresh-token persistence) ──

/// Encrypt `plaintext` with the provider envelope and upsert it under the
/// natural identity (kind, owner_ref, key_name). Returns the row id.
pub(crate) fn upsert_capability_secret(
    conn: &rusqlite::Connection,
    kind: &str,
    owner_ref: &str,
    key_name: Option<&str>,
    plaintext: &str,
) -> Result<String> {
    if !ALLOWED_KINDS.contains(&kind) {
        return Err(Error::Internal(format!(
            "unsupported capability secret kind '{kind}'"
        )));
    }
    let owner_ref = owner_ref.trim();
    if owner_ref.is_empty() {
        return Err(Error::Internal("ownerRef is required".into()));
    }
    let key_name = key_name.map(str::trim).filter(|s| !s.is_empty());
    if kind == "mcp_env" && key_name.is_none() {
        return Err(Error::Internal(
            "keyName is required for kind 'mcp_env'".into(),
        ));
    }
    if plaintext.is_empty() {
        return Err(Error::Internal("plaintext must not be empty".into()));
    }

    let (ciphertext, nonce) = provider_key_manager::envelope_encrypt(plaintext, conn)?;

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM capability_secrets
             WHERE kind = ?1 AND owner_ref = ?2 AND COALESCE(key_name, '') = COALESCE(?3, '')
             LIMIT 1",
            params![kind, owner_ref, key_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::Database)?;

    match existing {
        Some(id) => {
            conn.execute(
                "UPDATE capability_secrets
                 SET ciphertext = ?1, nonce = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                 WHERE id = ?3",
                params![ciphertext, nonce, id],
            )
            .map_err(Error::Database)?;
            Ok(id)
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO capability_secrets
                    (id, kind, owner_ref, key_name, ciphertext, nonce, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                         strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                params![id, kind, owner_ref, key_name, ciphertext, nonce],
            )
            .map_err(Error::Database)?;
            Ok(id)
        }
    }
}

// ── Tauri commands ──

#[tauri::command]
pub fn capability_secret_set(
    input: CapabilitySecretSetInput,
    state: State<'_, AppState>,
) -> Result<CapabilitySecretSetResult> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let id = upsert_capability_secret(
        &pool_conn,
        &input.kind,
        &input.owner_ref,
        input.key_name.as_deref(),
        &input.plaintext,
    )?;
    Ok(CapabilitySecretSetResult { id })
}

#[tauri::command]
pub fn capability_secret_delete(id: String, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let changed = pool_conn
        .execute("DELETE FROM capability_secrets WHERE id = ?1", params![id])
        .map_err(Error::Database)?;
    if changed == 0 {
        return Err(Error::Internal(format!(
            "capability secret '{id}' not found"
        )));
    }
    Ok(())
}

/// Metadata only — plaintext and ciphertext never cross this boundary.
#[tauri::command]
pub fn capability_secret_list(
    owner_ref: String,
    state: State<'_, AppState>,
) -> Result<Vec<CapabilitySecretEntry>> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let mut stmt = pool_conn
        .prepare(
            "SELECT id, kind, key_name, created_at FROM capability_secrets
             WHERE owner_ref = ?1
             ORDER BY kind ASC, COALESCE(key_name, '') ASC, created_at ASC",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![owner_ref], |row| {
            Ok(CapabilitySecretEntry {
                id: row.get(0)?,
                kind: row.get(1)?,
                key_name: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS provider_api_keys (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                api_key_encrypted TEXT NOT NULL DEFAULT '',
                dek_encrypted TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS capability_secrets (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK(kind IN ('mcp_env','mcp_bearer','mcp_oauth_refresh')),
                owner_ref TEXT NOT NULL,
                key_name TEXT,
                ciphertext TEXT NOT NULL,
                nonce TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_capability_secrets_identity
                ON capability_secrets(kind, owner_ref, COALESCE(key_name, ''));",
        )
        .unwrap();
        conn
    }

    #[test]
    fn capability_secret_encrypt_decrypt_roundtrip() {
        let conn = setup_test_db();
        let id = upsert_capability_secret(
            &conn,
            "mcp_oauth_refresh",
            "server-1",
            None,
            "refresh-token-plaintext",
        )
        .unwrap();

        let (ciphertext, nonce): (String, String) = conn
            .query_row(
                "SELECT ciphertext, nonce FROM capability_secrets WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_ne!(ciphertext, "refresh-token-plaintext");
        assert!(!ciphertext.contains("refresh-token-plaintext"));

        let decrypted = provider_key_manager::envelope_decrypt(&ciphertext, &nonce, &conn).unwrap();
        assert_eq!(decrypted, "refresh-token-plaintext");
    }

    #[test]
    fn capability_secret_upsert_overwrites_same_identity() {
        let conn = setup_test_db();
        let id1 =
            upsert_capability_secret(&conn, "mcp_env", "server-2", Some("API_KEY"), "v1").unwrap();
        let id2 =
            upsert_capability_secret(&conn, "mcp_env", "server-2", Some("API_KEY"), "v2").unwrap();
        assert_eq!(
            id1, id2,
            "same (kind, owner_ref, key_name) must update in place"
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM capability_secrets WHERE owner_ref = 'server-2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let (ciphertext, nonce): (String, String) = conn
            .query_row(
                "SELECT ciphertext, nonce FROM capability_secrets WHERE id = ?1",
                params![id1],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let decrypted = provider_key_manager::envelope_decrypt(&ciphertext, &nonce, &conn).unwrap();
        assert_eq!(decrypted, "v2");
    }

    #[test]
    fn capability_secret_rejects_invalid_input() {
        let conn = setup_test_db();
        assert!(upsert_capability_secret(&conn, "bogus_kind", "s", None, "x").is_err());
        assert!(upsert_capability_secret(&conn, "mcp_bearer", "  ", None, "x").is_err());
        assert!(
            upsert_capability_secret(&conn, "mcp_env", "s", None, "x").is_err(),
            "mcp_env requires keyName"
        );
        assert!(upsert_capability_secret(&conn, "mcp_bearer", "s", None, "").is_err());
    }
}
