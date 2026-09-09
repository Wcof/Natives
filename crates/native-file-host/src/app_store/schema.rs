//! App Store schema + capability migrations (ADR-0025 D13/D14).
//!
//! The App Store owns its own tables inside the shared authoritative
//! `natives.db` and migrates them independently with
//! `CREATE TABLE IF NOT EXISTS` + `table_has_column`, following the
//! Workspace Store capability-based pattern. It must NOT touch the shared
//! `PRAGMA user_version` (ADR-0025 D14: no `user_version = N` overwrite of
//! the whole database).

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use super::types::AppError;
use crate::workspace_store::schema::table_has_column;
use crate::workspace_store::WorkspaceError;

use std::path::Path;

/// Clean up legacy child host manifests verified against recorded apps and app root.
/// Operates exclusively within the injected manifest_dir (ADR-0026 D5).
pub fn clean_legacy_child_manifests_in(
    manifest_dir: &Path,
    app_root: &Path,
    conn: &Connection,
) -> Result<(), AppError> {
    if !manifest_dir.is_dir() {
        return Ok(());
    }
    let entries = match std::fs::read_dir(manifest_dir) {
        Ok(entries) => entries,
        Err(error) => {
            let _ = conn.execute(
                "INSERT INTO app_meta (key, value, revision) VALUES ('manifest_cleanup_error', ?1, 0)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [format!("read_dir failed: {error}")],
            );
            return Ok(());
        }
    };

    let mut failed = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.starts_with("com.natives.app.") || !file_name.ends_with(".json") {
            continue;
        }
        if file_name == "com.natives.file_manager.json"
            || file_name == "com.natives.model_host.json"
        {
            continue;
        }

        // Read manifest and verify attribution before removal
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let parsed: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let name = parsed.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name != file_name.trim_end_matches(".json") {
            continue;
        }

        let binary_path = parsed.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let path_obj = Path::new(binary_path);
        let app_id: Option<String> = conn
            .query_row(
                "SELECT app_id FROM apps WHERE json_extract(runtime_spec_json, '$.host') = ?1",
                [name],
                |row| row.get(0),
            )
            .optional()?;

        if app_id.is_some_and(|id| path_obj.starts_with(app_root.join(id))) {
            if let Err(err) = std::fs::remove_file(&path) {
                failed = true;
                let _ = conn.execute(
                    "INSERT INTO app_meta (key, value, revision) VALUES ('manifest_cleanup_retry', ?1, 0)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [format!("failed to remove {}: {err}", path.display())],
                );
            }
        }
    }
    if !failed {
        conn.execute(
            "DELETE FROM app_meta WHERE key = 'manifest_cleanup_retry'",
            [],
        )?;
    }
    Ok(())
}

/// Idempotent App Store migration.
pub fn migrate_apps(conn: &Connection) -> Result<(), AppError> {
    // Acquire the writer before schema reads; concurrent Host connections must wait,
    // not try to upgrade a stale read snapshot into a write transaction.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    create_app_tables(&tx)?;
    migrate_app_columns(&tx)?;
    migrate_v2_apps(&tx)?;
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
    if !table_has_column(conn, "app_install_transactions", "rollback_json")
        .map_err(|error| AppError::InvalidState(error.to_string()))?
    {
        conn.execute("ALTER TABLE app_install_transactions ADD COLUMN rollback_json TEXT NOT NULL DEFAULT ''", [])?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_retained_data (
        app_id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, host TEXT NOT NULL,
        permissions_json TEXT NOT NULL, cleanup_pending INTEGER NOT NULL DEFAULT 0,
        purge_data INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL
    );",
    )?;
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

fn migrate_v2_apps(conn: &Connection) -> Result<(), AppError> {
    let has_needs_migration = match table_has_column(conn, "apps", "needs_migration") {
        Ok(has) => has,
        Err(WorkspaceError::Sql(error)) => return Err(AppError::Sql(error)),
        Err(error) => return Err(AppError::InvalidState(error.to_string())),
    };
    if !has_needs_migration {
        conn.execute(
            "ALTER TABLE apps ADD COLUMN needs_migration INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    let v2_applied: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM app_meta WHERE key = 'v2_migration' AND value = 'completed')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !v2_applied {
        conn.execute(
            "UPDATE apps SET needs_migration = 1 WHERE app_id IN (SELECT DISTINCT app_id FROM app_packages WHERE kind = 'runtime')",
            [],
        )?;
        conn.execute(
            "INSERT INTO app_meta (key, value, revision) VALUES ('v2_migration', 'completed', 0)
             ON CONFLICT(key) DO UPDATE SET value = 'completed'",
            [],
        )?;
    }
    Ok(())
}
