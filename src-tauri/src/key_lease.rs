use crate::{Error, Result};
use rusqlite::params;

/// Acquire a non-primary key for a sub-agent run.
/// Uses BEGIN IMMEDIATE to prevent concurrent selection of the same key.
/// Returns (key_id, provider_id) or an error if no key is available.
pub fn acquire_secondary_key(
    conn: &rusqlite::Connection,
    provider_id: &str,
    run_id: &str,
) -> Result<(String, String)> {
    // Use immediate transaction to prevent concurrent races
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| Error::Internal(format!("failed to begin transaction: {e}")))?;

    let result = (|| -> Result<(String, String)> {
        // Find an eligible non-primary key: active, valid, no active lease
        let keys: Vec<(String, String)> = conn
            .prepare(
                "SELECT pak.id, pak.provider_id
                 FROM provider_api_keys pak
                 WHERE pak.provider_id = ?1
                   AND pak.is_primary = 0
                   AND pak.is_active = 1
                   AND pak.test_status = 'valid'
                   AND pak.id NOT IN (
                       SELECT key_id FROM provider_key_leases
                       WHERE released_at IS NULL
                   )
                 ORDER BY pak.last_leased_at ASC, pak.created_at ASC
                 LIMIT 1",
            )
            .map_err(|e| Error::Internal(e.to_string()))?
            .query_map(params![provider_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Internal(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let (key_id, prov_id) = keys
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound("NO_AVAILABLE_SECONDARY_KEY".to_string()))?;

        let now = chrono::Utc::now().to_rfc3339();

        // Record the lease
        conn.execute(
            "INSERT INTO provider_key_leases (run_id, provider_id, key_id, acquired_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![run_id, prov_id, key_id, now],
        )
        .map_err(|e| Error::Internal(format!("failed to insert lease: {e}")))?;

        // Update last_leased_at on the key
        conn.execute(
            "UPDATE provider_api_keys SET last_leased_at = ?1 WHERE id = ?2",
            params![now, key_id],
        )
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok((key_id, prov_id))
    })();

    conn.execute_batch("COMMIT")
        .map_err(|e| Error::Internal(format!("failed to commit: {e}")))?;

    result
}

/// Release a key lease when a run completes, fails, or is cancelled.
pub fn release_key_lease(conn: &rusqlite::Connection, run_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE provider_key_leases SET released_at = ?1 WHERE run_id = ?2 AND released_at IS NULL",
        params![now, run_id],
    )
    .map_err(|e| Error::Internal(format!("failed to release lease: {e}")))?;
    Ok(())
}

/// Check if a run has an active lease and return the key_id.
pub fn get_leased_key(conn: &rusqlite::Connection, run_id: &str) -> Result<Option<String>> {
    let key_id: Option<String> = conn
        .query_row(
            "SELECT key_id FROM provider_key_leases
             WHERE run_id = ?1 AND released_at IS NULL",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(key_id)
}

/// Get the primary key for a provider.
/// Returns (key_id, provider_id) or an error if no valid primary key exists.
pub fn get_primary_key(conn: &rusqlite::Connection, provider_id: &str) -> Result<(String, String)> {
    conn.query_row(
        "SELECT id, provider_id FROM provider_api_keys
         WHERE provider_id = ?1 AND is_primary = 1 AND is_active = 1 AND test_status = 'valid'",
        params![provider_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .map_err(|_| Error::NotFound("NO_VALID_PRIMARY_KEY".to_string()))
}

/// Mark a key as having failed (for fallback tracking).
pub fn mark_key_failed(conn: &rusqlite::Connection, key_id: &str, error_code: &str, error_message: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE provider_api_keys
         SET test_status = ?1, last_test_at = ?2, last_error_code = ?3, last_error_message = ?4, updated_at = ?5
         WHERE id = ?6",
        params!["unavailable", now, error_code, error_message, now, key_id],
    )
    .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Check if a run has already used the primary key fallback.
pub fn has_fallback_used(conn: &rusqlite::Connection, run_id: &str) -> Result<bool> {
    let fallback: bool = conn
        .query_row(
            "SELECT fallback_used FROM provider_key_leases WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    Ok(fallback)
}

/// Mark that a run has used the primary key fallback.
pub fn mark_fallback_used(conn: &rusqlite::Connection, run_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE provider_key_leases SET fallback_used = 1 WHERE run_id = ?1",
        params![run_id],
    )
    .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Ensure the key leases table exists.
pub fn ensure_lease_table(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_key_leases (
            run_id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            key_id TEXT NOT NULL,
            acquired_at TEXT NOT NULL,
            released_at TEXT,
            fallback_used INTEGER NOT NULL DEFAULT 0
        );"
    ).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

use rusqlite::OptionalExtension;
