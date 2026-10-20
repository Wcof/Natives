//! Capability migration for the single product's module registry.

use super::types::AppError;
use rusqlite::{Connection, Transaction};

pub fn migrate_apps(conn: &Connection) -> Result<(), AppError> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    migrate_preferences(&tx)?;
    create_tables(&tx)?;
    tx.commit()?;
    Ok(())
}

fn create_tables(conn: &Transaction<'_>) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_meta (
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
        CREATE TABLE IF NOT EXISTS builtin_app_preferences (
            app_id TEXT PRIMARY KEY NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            show_in_sidebar INTEGER NOT NULL DEFAULT 1,
            sidebar_order INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL,
            revision INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    Ok(())
}

fn migrate_preferences(conn: &Transaction<'_>) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS builtin_app_preferences (
            app_id TEXT PRIMARY KEY NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            show_in_sidebar INTEGER NOT NULL DEFAULT 1,
            sidebar_order INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL,
            revision INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    let has_legacy_apps = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'apps')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if has_legacy_apps {
        conn.execute(
            "INSERT OR IGNORE INTO builtin_app_preferences (app_id, enabled, show_in_sidebar, sidebar_order, updated_at, revision)
             SELECT app_id, enabled, show_in_sidebar, sidebar_order, updated_at, revision FROM apps",
            [],
        )?;
        conn.execute("DROP TABLE apps", [])?;
    }
    Ok(())
}
