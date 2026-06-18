use crate::{update_checker, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn update_check() -> Result<JsonValue> {
    update_checker::check_for_updates()
}

#[tauri::command]
pub fn update_mute(version: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    update_checker::mute_version(conn, &version)
}

#[tauri::command]
pub fn update_get_muted(state: State<'_, AppState>) -> Result<Vec<String>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    update_checker::get_muted_versions(conn)
}
