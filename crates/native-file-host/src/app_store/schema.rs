//! App Store schema + capability migrations (ADR-0025 D13/D14).
//!
//! The App Store owns its own tables inside the shared authoritative
//! `natives.db` and migrates them independently with
//! `CREATE TABLE IF NOT EXISTS` + `table_has_column`, following the
//! Workspace Store capability-based pattern. It must NOT touch the shared
//! `PRAGMA user_version` (ADR-0025 D14: no `user_version = N` overwrite of
//! the whole database).

use rusqlite::Connection;

use super::types::AppError;
use crate::workspace_store::schema::table_has_column;
use crate::workspace_store::WorkspaceError;

/// Idempotent App Store migration.
pub fn migrate_apps(conn: &Connection) -> Result<(), AppError> {
    let tx = conn.unchecked_transaction()?;
    create_app_tables(&tx)?;
    migrate_app_columns(&tx)?;
    tx.commit()?;
    Ok(())
}

fn create_app_tables(conn: &Connection) -> Result<(), AppError> {
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
            host_registered INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS app_packages (
            app_id TEXT NOT NULL,
            package_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            version TEXT NOT NULL,
            platform TEXT NOT NULL,
            arch TEXT NOT NULL,
            wire_size INTEGER NOT NULL,
            payload_size INTEGER NOT NULL,
            artifact_sha256 TEXT NOT NULL,
            payload_sha256 TEXT NOT NULL,
            installed_path TEXT NOT NULL,
            installed_at INTEGER NOT NULL,
            PRIMARY KEY (app_id, package_id)
        );
        CREATE TABLE IF NOT EXISTS app_permissions (
            app_id TEXT NOT NULL,
            permission TEXT NOT NULL,
            granted INTEGER NOT NULL DEFAULT 0,
            granted_at INTEGER NOT NULL,
            PRIMARY KEY (app_id, permission)
        );
        CREATE TABLE IF NOT EXISTS app_install_transactions (
            install_id TEXT PRIMARY KEY NOT NULL,
            app_id TEXT NOT NULL,
            from_version TEXT NOT NULL,
            to_version TEXT NOT NULL,
            request_json TEXT NOT NULL,
            state TEXT NOT NULL,
            staging_path TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            completed_at INTEGER,
            error_code TEXT,
            error_message TEXT
        );
        -- Per-package staging state (ADR-0025 D11): the app-level chain
        -- lives on the transaction row; download/verify/stage progress is
        -- tracked per package so a retry resumes at the right step.
        CREATE TABLE IF NOT EXISTS app_package_stages (
            install_id TEXT NOT NULL,
            package_id TEXT NOT NULL,
            state TEXT NOT NULL,
            staged_path TEXT NOT NULL DEFAULT '',
            payload_size INTEGER NOT NULL DEFAULT 0,
            payload_sha256 TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (install_id, package_id)
        );
        -- Monotonic global revision for the navigation projection
        -- (ADR-0025 D35/D37): per-row apps.revision decreases when the last
        -- app uninstalls, so the projection counter lives in its own row.
        CREATE TABLE IF NOT EXISTS app_meta (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL DEFAULT '',
            revision INTEGER NOT NULL DEFAULT 0
        );
        INSERT OR IGNORE INTO app_meta (key, value, revision) VALUES ('app_store', '', 0);
        CREATE INDEX IF NOT EXISTS idx_apps_revision ON apps (revision);
        CREATE INDEX IF NOT EXISTS idx_app_install_transactions_app
            ON app_install_transactions (app_id, started_at);
        ",
    )?;
    Ok(())
}

fn migrate_app_columns(conn: &Connection) -> Result<(), AppError> {
    // ADR-0025 D13 ships the full V1 column set; this hook keeps later App
    // Store schema growth capability-based without touching user_version.
    let has_sidebar_order = match table_has_column(conn, "apps", "sidebar_order") {
        Ok(has) => has,
        Err(WorkspaceError::Sql(error)) => return Err(AppError::Sql(error)),
        Err(error) => return Err(AppError::InvalidState(error.to_string())),
    };
    if !has_sidebar_order {
        conn.execute(
            "ALTER TABLE apps ADD COLUMN sidebar_order INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // Phase A5: explicit host-registration state (1 = manifest written with
    // a real caller origin; 0 = skipped, e.g. no origin). A2 rows predate
    // the column and stay 0 — an explicit "not registered", not a guess.
    let has_host_registered = match table_has_column(conn, "apps", "host_registered") {
        Ok(has) => has,
        Err(WorkspaceError::Sql(error)) => return Err(AppError::Sql(error)),
        Err(error) => return Err(AppError::InvalidState(error.to_string())),
    };
    if !has_host_registered {
        conn.execute(
            "ALTER TABLE apps ADD COLUMN host_registered INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}
