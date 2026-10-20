//! Read-side queries for built-in module state.

use super::types::{App, AppError};
use rusqlite::{Connection, OptionalExtension};

fn product_activation(apps_root: Option<&std::path::Path>) -> (Option<u64>, String) {
    apps_root
        .and_then(|root| {
            crate::app_activation::read_product_activation_receipt(root)
                .ok()
                .flatten()
        })
        .map(|value| {
            (
                value
                    .get("productGeneration")
                    .and_then(serde_json::Value::as_u64),
                value
                    .get("productVersion")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .unwrap_or_default()
}

fn project_app(
    conn: &Connection,
    module: &crate::app_product::FixedModule,
    generation: Option<u64>,
    product_version: &str,
) -> Result<App, AppError> {
    let preference = conn
        .query_row(
            "SELECT enabled, show_in_sidebar, sidebar_order, updated_at, revision FROM builtin_app_preferences WHERE app_id = ?1",
            [module.app_id],
            |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, i64>(1)? != 0, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()?;
    let (enabled, show_in_sidebar, sidebar_order, updated_at, revision) =
        preference.unwrap_or((true, true, 0, 0, 0));
    Ok(App {
        app_id: module.app_id.into(),
        name: module.name_zh.into(),
        version: product_version.to_string(),
        enabled,
        show_in_sidebar,
        sidebar_order,
        updated_at,
        revision,
        product_generation: generation,
        capabilities: serde_json::from_str(module.capabilities_json).unwrap_or_default(),
        requires: serde_json::from_str(module.requires_json).unwrap_or_default(),
        views: serde_json::from_str(module.views_json).unwrap_or_default(),
    })
}

pub(crate) fn apps(
    conn: &Connection,
    apps_root: Option<&std::path::Path>,
) -> Result<Vec<App>, AppError> {
    let (generation, product_version) = product_activation(apps_root);
    let mut apps = crate::app_product::FIXED_MODULES
        .iter()
        .map(|module| project_app(conn, module, generation, &product_version))
        .collect::<Result<Vec<_>, _>>()?;
    apps.sort_by(|left, right| {
        (left.sidebar_order, &left.app_id).cmp(&(right.sidebar_order, &right.app_id))
    });
    Ok(apps)
}

pub(crate) fn app(
    conn: &Connection,
    app_id: &str,
    apps_root: Option<&std::path::Path>,
) -> Result<App, AppError> {
    let module = crate::app_product::FIXED_MODULES
        .iter()
        .find(|module| module.app_id == app_id)
        .ok_or_else(|| AppError::NotFound(app_id.into()))?;
    let (generation, product_version) = product_activation(apps_root);
    project_app(conn, module, generation, &product_version)
}

pub(crate) fn global_revision(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row(
        "SELECT revision FROM app_meta WHERE key = 'app_store'",
        [],
        |row| row.get(0),
    )?)
}
