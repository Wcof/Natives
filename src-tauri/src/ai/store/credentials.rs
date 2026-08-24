//! Credential 持久化操作。

use rusqlite::{params, Connection as DbConn};

use crate::ai::model::{Credential, CredentialKind, CredentialStatus};
use crate::{Error, Result};

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn list_credentials(conn: &DbConn, provider_id: Option<&str>) -> Result<Vec<Credential>> {
    let query = match provider_id {
        Some(_) => {
            "SELECT id, provider_id, kind, label, secret_ref, secret_revision, masked_identity, status, priority, concurrency_limit, expires_at, last_refreshed_at, next_refresh_at, identity_fingerprint, metadata_json, created_at, updated_at
             FROM ai_credentials WHERE provider_id = ?1 ORDER BY created_at ASC"
        }
        None => {
            "SELECT id, provider_id, kind, label, secret_ref, secret_revision, masked_identity, status, priority, concurrency_limit, expires_at, last_refreshed_at, next_refresh_at, identity_fingerprint, metadata_json, created_at, updated_at
             FROM ai_credentials ORDER BY created_at ASC"
        }
    };

    let mut stmt = conn.prepare(query).map_err(Error::Database)?;

    let map_row = |row: &rusqlite::Row| {
        let kind_str: String = row.get(2)?;
        let status_str: String = row.get(7)?;
        Ok(Credential {
            id: row.get(0)?,
            provider_id: row.get(1)?,
            kind: CredentialKind::from_str(&kind_str).unwrap_or(CredentialKind::ApiKey),
            label: row.get(3)?,
            secret_ref: row.get(4)?,
            secret_revision: row.get::<_, i64>(5)? as u32,
            masked_identity: row.get(6)?,
            status: CredentialStatus::from_str(&status_str),
            priority: row.get::<_, i64>(8)? as u32,
            concurrency_limit: row.get::<_, i64>(9)? as u32,
            expires_at: row.get(10)?,
            last_refreshed_at: row.get(11)?,
            next_refresh_at: row.get(12)?,
            identity_fingerprint: row.get(13)?,
            metadata_json: row.get(14)?,
            created_at: row.get(15)?,
            updated_at: row.get(16)?,
        })
    };

    let rows = match provider_id {
        Some(pid) => stmt.query_map([pid], map_row).map_err(Error::Database)?,
        None => stmt.query_map([], map_row).map_err(Error::Database)?,
    };

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

pub fn get_credential(conn: &DbConn, id: &str) -> Result<Option<Credential>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, provider_id, kind, label, secret_ref, secret_revision, masked_identity, status, priority, concurrency_limit, expires_at, last_refreshed_at, next_refresh_at, identity_fingerprint, metadata_json, created_at, updated_at
             FROM ai_credentials WHERE id = ?1",
        )
        .map_err(Error::Database)?;

    let mut rows = stmt
        .query_map([id], |row| {
            let kind_str: String = row.get(2)?;
            let status_str: String = row.get(7)?;
            Ok(Credential {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                kind: CredentialKind::from_str(&kind_str).unwrap_or(CredentialKind::ApiKey),
                label: row.get(3)?,
                secret_ref: row.get(4)?,
                secret_revision: row.get::<_, i64>(5)? as u32,
                masked_identity: row.get(6)?,
                status: CredentialStatus::from_str(&status_str),
                priority: row.get::<_, i64>(8)? as u32,
                concurrency_limit: row.get::<_, i64>(9)? as u32,
                expires_at: row.get(10)?,
                last_refreshed_at: row.get(11)?,
                next_refresh_at: row.get(12)?,
                identity_fingerprint: row.get(13)?,
                metadata_json: row.get(14)?,
                created_at: row.get(15)?,
                updated_at: row.get(16)?,
            })
        })
        .map_err(Error::Database)?;

    rows.next().transpose().map_err(Error::Database)
}

pub fn insert_credential(
    conn: &DbConn,
    id: &str,
    provider_id: &str,
    kind: CredentialKind,
    label: &str,
    secret_ref: &str,
    secret_revision: u32,
    masked_identity: &str,
    status: CredentialStatus,
    priority: u32,
    concurrency_limit: u32,
    expires_at: Option<&str>,
    last_refreshed_at: Option<&str>,
    next_refresh_at: Option<&str>,
    identity_fingerprint: Option<&str>,
    metadata_json: Option<&str>,
) -> Result<Credential> {
    let now = now_rfc3339();

    conn.execute(
        "INSERT INTO ai_credentials
            (id, provider_id, kind, label, secret_ref, secret_revision, masked_identity,
             status, priority, concurrency_limit, expires_at, last_refreshed_at,
             next_refresh_at, identity_fingerprint, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)
         ON CONFLICT(provider_id, identity_fingerprint) WHERE identity_fingerprint IS NOT NULL
         DO UPDATE SET
             label = excluded.label,
             secret_ref = excluded.secret_ref,
             secret_revision = excluded.secret_revision,
             masked_identity = excluded.masked_identity,
             status = excluded.status,
             priority = excluded.priority,
             concurrency_limit = excluded.concurrency_limit,
             expires_at = excluded.expires_at,
             last_refreshed_at = excluded.last_refreshed_at,
             next_refresh_at = excluded.next_refresh_at,
             metadata_json = excluded.metadata_json,
             updated_at = excluded.updated_at",
        params![
            id,
            provider_id,
            kind.as_str(),
            label,
            secret_ref,
            secret_revision as i64,
            masked_identity,
            status.as_str(),
            priority as i64,
            concurrency_limit as i64,
            expires_at,
            last_refreshed_at,
            next_refresh_at,
            identity_fingerprint,
            metadata_json,
            now,
        ],
    )
    .map_err(Error::Database)?;

    get_credential(conn, id)?
        .ok_or_else(|| Error::Internal("Failed to read back inserted credential".into()))
}

pub fn update_credential_status(
    conn: &DbConn,
    id: &str,
    status: CredentialStatus,
    expires_at: Option<Option<&str>>,
    last_refreshed_at: Option<Option<&str>>,
    next_refresh_at: Option<Option<&str>>,
) -> Result<()> {
    let now = now_rfc3339();
    let existing = get_credential(conn, id)?
        .ok_or_else(|| Error::NotFound(format!("Credential {id} not found")))?;

    let new_expires = expires_at.unwrap_or(existing.expires_at.as_deref());
    let new_refreshed = last_refreshed_at.unwrap_or(existing.last_refreshed_at.as_deref());
    let new_next = next_refresh_at.unwrap_or(existing.next_refresh_at.as_deref());

    conn.execute(
        "UPDATE ai_credentials SET status = ?1, expires_at = ?2, last_refreshed_at = ?3, next_refresh_at = ?4, updated_at = ?5
         WHERE id = ?6",
        params![
            status.as_str(),
            new_expires,
            new_refreshed,
            new_next,
            now,
            id,
        ],
    )
    .map_err(Error::Database)?;

    Ok(())
}

pub fn update_credential_policy(
    conn: &DbConn,
    id: &str,
    label: Option<&str>,
    priority: Option<u32>,
    concurrency_limit: Option<u32>,
    status: Option<CredentialStatus>,
) -> Result<Credential> {
    let existing = get_credential(conn, id)?
        .ok_or_else(|| Error::NotFound(format!("Credential {id} not found")))?;
    let now = now_rfc3339();

    let new_label = label.unwrap_or(&existing.label);
    let new_priority = priority.unwrap_or(existing.priority);
    let new_concurrency = concurrency_limit.unwrap_or(existing.concurrency_limit);
    let new_status = status.unwrap_or(existing.status);

    conn.execute(
        "UPDATE ai_credentials SET label = ?1, priority = ?2, concurrency_limit = ?3, status = ?4, updated_at = ?5
         WHERE id = ?6",
        params![
            new_label,
            new_priority as i64,
            new_concurrency as i64,
            new_status.as_str(),
            now,
            id,
        ],
    )
    .map_err(Error::Database)?;

    get_credential(conn, id)?
        .ok_or_else(|| Error::Internal("Failed to read back updated credential".into()))
}

pub fn delete_credential(conn: &DbConn, id: &str) -> Result<bool> {
    let count = conn
        .execute("DELETE FROM ai_credentials WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(count > 0)
}

pub fn bind_credential_connection(
    conn: &DbConn,
    credential_id: &str,
    connection_id: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO ai_credential_connections (credential_id, connection_id) VALUES (?1, ?2)",
        params![credential_id, connection_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn list_credential_connections(conn: &DbConn, credential_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT connection_id FROM ai_credential_connections WHERE credential_id = ?1")
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([credential_id], |row| row.get(0))
        .map_err(Error::Database)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}
