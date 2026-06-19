use crate::{db, Error, Result};
use tauri::State;

use crate::AppState;

const LOCALE_KEY: &str = "settings:locale";
const DEFAULT_LOCALE: &str = "zh";

#[tauri::command]
pub fn get_locale(state: State<'_, AppState>) -> Result<String> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    Ok(db::get_setting(conn, LOCALE_KEY)?
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string()))
}

#[tauri::command]
pub fn set_locale(locale: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::set_setting(conn, LOCALE_KEY, &locale)?;
    Ok(())
}
