use crate::{Error, Result};
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn get_locale(state: State<'_, AppState>) -> Result<String> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3 — read from DB settings
    Ok("zh".to_string())
}

#[tauri::command]
pub fn set_locale(locale: String, state: State<'_, AppState>) -> Result<()> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3 — write to DB + broadcast
    Err(Error::NotImplemented(format!("setLocale({locale})")))
}
