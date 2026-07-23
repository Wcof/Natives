//! Internal Workshop adapter — web-module via module_manager + Unique Origin iframe.
//!
//! Does NOT expose Workshop Bridge to external/local sources. Lifecycle maps:
//! start → enable, stop → disable, delete → uninstall, open → workshop_module.

use crate::creative_app::install;
use crate::creative_app::model::*;
use crate::module_manager;
use crate::{Error, Result};
use rusqlite::Connection;
use std::path::Path;
use tauri::AppHandle;

pub fn list(conn: &Connection) -> Result<Vec<CreativeAppSummary>> {
    let modules = module_manager::list_modules(conn)?;
    let mut out = Vec::with_capacity(modules.len());
    for m in modules {
        let (description, icon) = load_module_meta(conn, &m.id);
        out.push(install::summary_from_internal(
            &m.id,
            &m.name,
            &m.version,
            m.enabled,
            description,
            icon,
        ));
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
    let modules = module_manager::list_modules(conn)?;
    let m = modules
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| Error::NotFound(id.into()))?;
    let (description, icon) = load_module_meta(conn, &m.id);
    Ok(install::summary_from_internal(
        &m.id,
        &m.name,
        &m.version,
        m.enabled,
        description,
        icon,
    ))
}

pub fn start(conn: &Connection, app: &AppHandle, id: &str) -> Result<CreativeAppSummary> {
    module_manager::enable_module(conn, id)?;
    crate::emit_db_state_changed(
        app,
        "module",
        serde_json::json!({ "action": "enable", "moduleId": id }),
    );
    crate::emit_db_state_changed(
        app,
        "creative-app",
        serde_json::json!({ "action": "start", "id": id }),
    );
    get(conn, id)
}

pub fn stop(conn: &Connection, app: &AppHandle, id: &str) -> Result<CreativeAppSummary> {
    module_manager::disable_module(conn, id)?;
    crate::emit_db_state_changed(
        app,
        "module",
        serde_json::json!({ "action": "disable", "moduleId": id }),
    );
    crate::emit_db_state_changed(
        app,
        "creative-app",
        serde_json::json!({ "action": "stop", "id": id }),
    );
    get(conn, id)
}

pub fn delete(
    conn: &Connection,
    app: &AppHandle,
    modules_dir: &Path,
    id: &str,
) -> Result<DeleteResult> {
    module_manager::uninstall_module(conn, modules_dir, id)?;
    crate::emit_db_state_changed(
        app,
        "module",
        serde_json::json!({ "action": "uninstall", "moduleId": id }),
    );
    crate::emit_db_state_changed(
        app,
        "creative-app",
        serde_json::json!({ "action": "deleted", "id": id }),
    );
    Ok(DeleteResult {
        ok: true,
        warnings: vec![],
    })
}

pub fn open_target(conn: &Connection, id: &str) -> Result<OpenTarget> {
    let modules = module_manager::list_modules(conn)?;
    let m = modules
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| Error::NotFound(id.into()))?;
    if m.enabled == 0 {
        return Err(Error::InvalidInput("module is disabled".into()));
    }
    Ok(OpenTarget::WorkshopModule {
        module_id: id.to_string(),
    })
}

fn load_module_meta(conn: &Connection, id: &str) -> (Option<String>, Option<String>) {
    let mut stmt = match conn.prepare("SELECT description, icon FROM modules WHERE id = ?1") {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    stmt.query_row(rusqlite::params![id], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
        ))
    })
    .unwrap_or((None, None))
}
