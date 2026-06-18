use crate::{Error, Result};
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn get_theme(state: State<'_, AppState>) -> Result<String> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3 — read from DB settings
    Ok("terminal-volt".to_string())
}

#[tauri::command]
pub fn set_theme(theme: String, state: State<'_, AppState>) -> Result<()> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3 — write to DB + broadcast
    Err(Error::NotImplemented(format!("setTheme({theme})")))
}
