//! Read-side queries for built-in module state.

use super::types::{App, AppError};
use rusqlite::Connection;

const APP_COLUMNS: &str = "app_id, kind, name, version, enabled, show_in_sidebar, \
    sidebar_order, runtime_spec_json, surface_json, manifest_json, installed_at, \
    updated_at, revision, host_registered, needs_migration";

fn map_app(row: &rusqlite::Row<'_>, apps_root: Option<&std::path::Path>) -> rusqlite::Result<App> {
    let app_id: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let host_registered = row.get::<_, i64>(13)? != 0;
    let runtime_host = (kind == super::types::KIND_MANAGED_LOCAL && host_registered)
        .then(|| crate::app_activation::runtime_host_name(&app_id));
    let activation_generation = apps_root.and_then(|root| {
        crate::app_activation::read_activation_projection(root, &app_id)
            .ok()
            .flatten()
            .and_then(|value| value.get("generation").and_then(serde_json::Value::as_u64))
    });
    Ok(App {
        app_id,
        kind,
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
        host_registered,
        runtime_host,
        needs_migration: row.get::<_, i64>(14)? != 0,
        activation_generation,
    })
}

pub(crate) fn apps(
    conn: &Connection,
    apps_root: Option<&std::path::Path>,
) -> Result<Vec<App>, AppError> {
    let mut statement = conn.prepare(&format!("SELECT {APP_COLUMNS} FROM apps WHERE kind = 'managed_local' ORDER BY sidebar_order, app_id"))?;
    let rows = statement.query_map([], |row| map_app(row, apps_root))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn app(
    conn: &Connection,
    app_id: &str,
    apps_root: Option<&std::path::Path>,
) -> Result<App, AppError> {
    conn.query_row(
        &format!("SELECT {APP_COLUMNS} FROM apps WHERE app_id = ?1 AND kind = 'managed_local'"),
        [app_id],
        |row| map_app(row, apps_root),
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(app_id.into()),
        other => AppError::Sql(other),
    })
}

pub(crate) fn global_revision(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row(
        "SELECT revision FROM app_meta WHERE key = 'app_store'",
        [],
        |row| row.get(0),
    )?)
}
