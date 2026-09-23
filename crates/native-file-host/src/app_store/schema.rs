//! Capability migration for the single product's module registry.

use super::types::AppError;
use crate::workspace_store::schema::table_has_column;
use rusqlite::{Connection, Transaction};

pub fn migrate_apps(conn: &Connection) -> Result<(), AppError> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    create_tables(&tx)?;
    ensure_columns(&tx)?;
    tx.commit()?;
    Ok(())
}

fn create_tables(conn: &Transaction<'_>) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS apps (
            app_id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            show_in_sidebar INTEGER NOT NULL DEFAULT 1,
            sidebar_order INTEGER NOT NULL DEFAULT 0,
            runtime_spec_json TEXT NOT NULL,
            surface_json TEXT NOT NULL,
            manifest_json TEXT NOT NULL,
            installed_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            revision INTEGER NOT NULL DEFAULT 0,
            host_registered INTEGER NOT NULL DEFAULT 0,
            needs_migration INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS app_meta (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL DEFAULT '',
            revision INTEGER NOT NULL DEFAULT 0
        );
        INSERT OR IGNORE INTO app_meta (key, value, revision) VALUES ('app_store', '', 0);
        CREATE TABLE IF NOT EXISTS app_data_reset_receipts (
            request_id TEXT PRIMARY KEY NOT NULL,
            app_id TEXT NOT NULL,
            scope_json TEXT NOT NULL,
            state TEXT NOT NULL,
            cleared_json TEXT NOT NULL DEFAULT '[]',
            error_code TEXT,
            error_message TEXT,
            created_at INTEGER NOT NULL,
            completed_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_apps_revision ON apps (revision);",
    )?;
    Ok(())
}

fn ensure_columns(conn: &Connection) -> Result<(), AppError> {
    for (name, ddl) in [
        (
            "sidebar_order",
            "ALTER TABLE apps ADD COLUMN sidebar_order INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "host_registered",
            "ALTER TABLE apps ADD COLUMN host_registered INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "needs_migration",
            "ALTER TABLE apps ADD COLUMN needs_migration INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if !table_has_column(conn, "apps", name)
            .map_err(|e| AppError::InvalidState(e.to_string()))?
        {
            conn.execute(ddl, [])?;
        }
    }
    // Existing pre-convergence rows remain as inert history. They are never
    // projected or mutated by the current module API.
    conn.execute(
        "UPDATE apps SET needs_migration = 1, host_registered = 0 WHERE kind <> 'managed_local'",
        [],
    )?;
    Ok(())
}
