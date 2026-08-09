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
    // Self-sufficient against pre-existing tables missing the NE-P0-02 columns.
    ensure_lease_table(conn)?;
    migrate_lease_columns(conn)?;
    // Use immediate transaction to prevent concurrent races
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| Error::Internal(format!("failed to begin transaction: {e}")))?;

    let result = (|| -> Result<(String, String)> {
        // Find an eligible non-primary key: active, valid, no active lease.
        // A lease is "active" only while unreleased AND not TTL-expired, so a
        // short-TTL broker lease frees the key once it lapses (NE-P0-02).
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
                         AND (expires_at IS NULL OR expires_at = '' OR expires_at > datetime('now'))
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

/// Release a key lease by run id through the main Host DB connection.
/// Convenience for the Credential Broker's revocation path.
pub fn release_key_lease_for_run(run_id: &str) -> Result<()> {
    let db = crate::db::get_main_conn()
        .map_err(|e| Error::Internal(format!("DB connection failed: {e}")))?;
    release_key_lease(&db, run_id)
}

/// Check if a run has an active (unreleased, unexpired) lease and return the key_id.
pub fn get_leased_key(conn: &rusqlite::Connection, run_id: &str) -> Result<Option<String>> {
    ensure_lease_table(conn)?;
    migrate_lease_columns(conn)?;
    let key_id: Option<String> = conn
        .query_row(
            "SELECT key_id FROM provider_key_leases
             WHERE run_id = ?1 AND released_at IS NULL
               AND (expires_at IS NULL OR expires_at = '' OR expires_at > datetime('now'))",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(key_id)
}

/// True when `run_id` currently holds an unreleased, unexpired lease.
pub fn lease_is_active(conn: &rusqlite::Connection, run_id: &str) -> Result<bool> {
    ensure_lease_table(conn)?;
    migrate_lease_columns(conn)?;
    let active: bool = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM provider_key_leases
                WHERE run_id = ?1 AND released_at IS NULL
                  AND (expires_at IS NULL OR expires_at = '' OR expires_at > datetime('now'))
             )",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(active)
}

/// Persist a broker-issued credential lease in the Host-authoritative
/// `provider_key_leases` table. Records lease **metadata only** — never key
/// material. The row is keyed by `run_id` (one active lease per run) and made
/// TTL-aware via `expires_at` so an abandoned lease frees the key.
pub fn record_lease(
    conn: &rusqlite::Connection,
    run_id: &str,
    provider_id: &str,
    key_id: &str,
    lease_id: &str,
    expires_at: &str,
) -> Result<()> {
    ensure_lease_table(conn)?;
    migrate_lease_columns(conn)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO provider_key_leases
            (run_id, provider_id, key_id, lease_id, acquired_at, expires_at, released_at, fallback_used)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 0)",
        params![run_id, provider_id, key_id, lease_id, now, expires_at],
    )
    .map_err(|e| Error::Internal(format!("failed to record lease: {e}")))?;
    Ok(())
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
pub fn mark_key_failed(
    conn: &rusqlite::Connection,
    key_id: &str,
    error_code: &str,
    error_message: &str,
) -> Result<()> {
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

/// Ensure the key leases table exists. The `lease_id` / `expires_at` columns
/// are the NE-P0-02 lease metadata (short TTL + durable revocation); existing
/// installs are migrated by [`migrate_lease_columns`].
pub fn ensure_lease_table(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_key_leases (
            run_id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            key_id TEXT NOT NULL,
            lease_id TEXT,
            acquired_at TEXT NOT NULL,
            expires_at TEXT,
            released_at TEXT,
            fallback_used INTEGER NOT NULL DEFAULT 0
        );",
    )
    .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Add `lease_id` / `expires_at` columns to a pre-existing
/// `provider_key_leases` table (idempotent). No data is touched.
pub fn migrate_lease_columns(conn: &rusqlite::Connection) -> Result<()> {
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(provider_key_leases)")
        .map_err(|e| Error::Internal(e.to_string()))?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(Result::ok)
        .collect();
    if !columns.iter().any(|c| c == "lease_id") {
        conn.execute_batch("ALTER TABLE provider_key_leases ADD COLUMN lease_id TEXT")
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    if !columns.iter().any(|c| c == "expires_at") {
        conn.execute_batch("ALTER TABLE provider_key_leases ADD COLUMN expires_at TEXT")
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> rusqlite::Connection {
        rusqlite::Connection::open_in_memory().unwrap()
    }

    #[test]
    fn records_and_releases_lease_with_ttl() {
        let conn = mem();
        record_lease(
            &conn,
            "run-1",
            "openai",
            "k1",
            "lease-1",
            "2999-01-01T00:00:00Z",
        )
        .unwrap();
        assert!(lease_is_active(&conn, "run-1").unwrap());
        assert_eq!(
            get_leased_key(&conn, "run-1").unwrap().as_deref(),
            Some("k1")
        );
        release_key_lease(&conn, "run-1").unwrap();
        assert!(!lease_is_active(&conn, "run-1").unwrap());
        assert_eq!(get_leased_key(&conn, "run-1").unwrap(), None);
    }

    #[test]
    fn ttl_expired_lease_is_not_active() {
        let conn = mem();
        record_lease(
            &conn,
            "run-1",
            "openai",
            "k1",
            "lease-1",
            "2000-01-01T00:00:00Z",
        )
        .unwrap();
        assert!(!lease_is_active(&conn, "run-1").unwrap());
        assert_eq!(get_leased_key(&conn, "run-1").unwrap(), None);
    }

    #[test]
    fn migrate_is_idempotent_and_preserves_rows() {
        let conn = mem();
        ensure_lease_table(&conn).unwrap();
        // Legacy row without the new columns.
        conn.execute(
            "INSERT INTO provider_key_leases (run_id, provider_id, key_id, acquired_at)
             VALUES ('legacy-run', 'openai', 'k9', '2020-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        migrate_lease_columns(&conn).unwrap();
        migrate_lease_columns(&conn).unwrap(); // second call is a no-op
                                               // Existing row survives and is still tracked as an active (legacy) lease.
        assert!(lease_is_active(&conn, "legacy-run").unwrap());
        record_lease(
            &conn,
            "legacy-run",
            "openai",
            "k2",
            "lease-2",
            "2999-01-01T00:00:00Z",
        )
        .unwrap();
        assert_eq!(
            get_leased_key(&conn, "legacy-run").unwrap().as_deref(),
            Some("k2")
        );
    }
}
