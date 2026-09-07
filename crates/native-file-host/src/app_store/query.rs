//! Read-side queries for the App Store registry.

use rusqlite::Connection;

use super::types::{App, AppError, AppPackage, AppPermission, InstallTransaction};

const APP_COLUMNS: &str = "app_id, kind, name, version, enabled, show_in_sidebar, \
     sidebar_order, runtime_spec_json, surface_json, manifest_json, \
     installed_at, updated_at, revision, host_registered";

fn map_app(row: &rusqlite::Row<'_>) -> rusqlite::Result<App> {
    Ok(App {
        app_id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        version: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
        show_in_sidebar: row.get::<_, i64>(5)? != 0,
        sidebar_order: row.get(6)?,
        runtime_spec_json: row.get(7)?,
        surface_json: row.get(8)?,
        manifest_json: row.get(9)?,
        installed_at: row.get(10)?,
        updated_at: row.get(11)?,
        revision: row.get(12)?,
        host_registered: row.get::<_, i64>(13)? != 0,
    })
}

/// All registered apps, sidebar order first (ADR-0025 D38).
pub(crate) fn apps(conn: &Connection) -> Result<Vec<App>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {APP_COLUMNS} FROM apps ORDER BY sidebar_order, app_id"
    ))?;
    let rows = stmt.query_map([], map_app)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// One app row.
pub(crate) fn app(conn: &Connection, app_id: &str) -> Result<App, AppError> {
    conn.query_row(
        &format!("SELECT {APP_COLUMNS} FROM apps WHERE app_id = ?1"),
        [app_id],
        map_app,
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(app_id.to_string()),
        other => AppError::Sql(other),
    })
}

/// Package receipts for one app.
pub(crate) fn packages(conn: &Connection, app_id: &str) -> Result<Vec<AppPackage>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT app_id, package_id, kind, version, platform, arch, wire_size, payload_size, \
         artifact_sha256, payload_sha256, installed_path, installed_at \
         FROM app_packages WHERE app_id = ?1 ORDER BY package_id",
    )?;
    let rows = stmt.query_map([app_id], |row| {
        Ok(AppPackage {
            app_id: row.get(0)?,
            package_id: row.get(1)?,
            kind: row.get(2)?,
            version: row.get(3)?,
            platform: row.get(4)?,
            arch: row.get(5)?,
            wire_size: row.get(6)?,
            payload_size: row.get(7)?,
            artifact_sha256: row.get(8)?,
            payload_sha256: row.get(9)?,
            installed_path: row.get(10)?,
            installed_at: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Permission grants for one app.
pub(crate) fn permissions(conn: &Connection, app_id: &str) -> Result<Vec<AppPermission>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT app_id, permission, granted, granted_at \
         FROM app_permissions WHERE app_id = ?1 ORDER BY permission",
    )?;
    let rows = stmt.query_map([app_id], |row| {
        Ok(AppPermission {
            app_id: row.get(0)?,
            permission: row.get(1)?,
            granted: row.get::<_, i64>(2)? != 0,
            granted_at: row.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// One install transaction.
pub(crate) fn transaction(
    conn: &Connection,
    install_id: &str,
) -> Result<InstallTransaction, AppError> {
    conn.query_row(
        "SELECT install_id, app_id, from_version, to_version, request_json, state, \
         staging_path, started_at, completed_at, error_code, error_message \
         FROM app_install_transactions WHERE install_id = ?1",
        [install_id],
        |row| {
            Ok(InstallTransaction {
                install_id: row.get(0)?,
                app_id: row.get(1)?,
                from_version: row.get(2)?,
                to_version: row.get(3)?,
                request_json: row.get(4)?,
                state: row.get(5)?,
                staging_path: row.get(6)?,
                started_at: row.get(7)?,
                completed_at: row.get(8)?,
                error_code: row.get(9)?,
                error_message: row.get(10)?,
            })
        },
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => {
            AppError::NotFound(format!("install transaction {install_id}"))
        }
        other => AppError::Sql(other),
    })
}

/// Monotonic global revision for the navigation projection (ADR-0025 D37).
pub(crate) fn global_revision(conn: &Connection) -> Result<i64, AppError> {
    match conn.query_row(
        "SELECT revision FROM app_meta WHERE key = 'app_store'",
        [],
        |row| row.get(0),
    ) {
        Ok(revision) => Ok(revision),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(error) => Err(AppError::Sql(error)),
    }
}
