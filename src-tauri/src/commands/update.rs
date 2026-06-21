use crate::{update_checker, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn update_check(state: State<'_, AppState>) -> Result<JsonValue> {
    update_checker::check_for_updates(&state)
}

#[tauri::command]
pub fn update_mute(version: String, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    update_checker::mute_version(conn, &version)
}

#[tauri::command]
pub fn update_dismiss(version: String, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    update_checker::dismiss_version(conn, &version)
}

#[tauri::command]
pub fn update_get_muted(state: State<'_, AppState>) -> Result<Vec<String>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    update_checker::get_muted_versions(conn)
}

#[tauri::command]
pub fn update_get_dismissed(state: State<'_, AppState>) -> Result<Vec<String>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    update_checker::get_dismissed_versions(conn)
}
