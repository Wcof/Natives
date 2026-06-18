use crate::{db, Error, Result};
use tauri::{Emitter, State};

use crate::AppState;

const THEME_KEY: &str = "settings:theme";
const DEFAULT_THEME: &str = "terminal-volt";

#[tauri::command]
pub fn get_theme(state: State<'_, AppState>) -> Result<String> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    Ok(db::get_setting(conn, THEME_KEY)?
        .unwrap_or_else(|| DEFAULT_THEME.to_string()))
}

#[tauri::command]
pub fn set_theme(theme: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::set_setting(conn, THEME_KEY, &theme)?;
    Ok(())
}
