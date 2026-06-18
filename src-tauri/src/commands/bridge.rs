use crate::{Error, Result};
use tauri::State;

use crate::AppState;

/// Get the local HTTP server port (for module asset serving and bridge API).
#[tauri::command]
pub fn get_http_port(state: State<'_, AppState>) -> Result<u16> {
    let port = state.http_port.lock().map_err(|e| Error::Internal(e.to_string()))?;
    Ok(*port)
}

/// Generate a session token for a module.
#[tauri::command]
pub fn generate_token(module_id: String, state: State<'_, AppState>) -> Result<String> {
    Ok(state.token_manager.generate(&module_id))
}

/// Validate a session token for a module.
#[tauri::command]
pub fn validate_token(token: String, module_id: String, state: State<'_, AppState>) -> Result<bool> {
    Ok(state.token_manager.validate(&token, &module_id))
}
