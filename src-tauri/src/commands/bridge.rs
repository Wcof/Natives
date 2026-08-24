use crate::Result;
use tauri::State;

use crate::AppState;

/// Get the local HTTP server port (for module asset serving and bridge API).
/// PERF-02: the server binds lazily on the first call here.
#[tauri::command]
pub fn get_http_port(state: State<'_, AppState>) -> Result<u16> {
    Ok(state.http_port.port())
}

/// Generate a session token for a module.
#[tauri::command]
pub fn generate_token(module_id: String, state: State<'_, AppState>) -> Result<String> {
    Ok(state.token_manager.generate(&module_id))
}

/// Validate a session token for a module.
#[tauri::command]
pub fn validate_token(
    token: String,
    module_id: String,
    state: State<'_, AppState>,
) -> Result<bool> {
    Ok(state.token_manager.validate(&token, &module_id))
}

#[cfg(test)]
mod tests {
    // Bridge commands need AppState context — token validation logic
    // is tested in token_manager.rs. No standalone unit tests needed here.
}
