use crate::Result;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalData {
    pub session_id: String,
    pub data: String,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalExit {
    pub session_id: String,
    pub exit_code: i32,
}

#[tauri::command]
pub fn terminal_create(
    app: tauri::AppHandle,
    profile_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String> {
    let env_overrides = None; // TODO: inject env from profile
    let (session_id, _pid) = state
        .terminal_manager
        .create_session(app, profile_id.as_deref(), env_overrides)?;
    Ok(session_id)
}

#[tauri::command]
pub fn terminal_write(session_id: String, data: String, state: State<'_, AppState>) -> Result<()> {
    state.terminal_manager.write(&session_id, &data)
}

#[tauri::command]
pub fn terminal_resize(
    session_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> Result<()> {
    state.terminal_manager.resize(&session_id, cols, rows)
}

#[tauri::command]
pub fn terminal_kill(session_id: String, state: State<'_, AppState>) -> Result<()> {
    state.terminal_manager.kill(&session_id)
}

#[tauri::command]
pub fn terminal_cwd(session_id: String, state: State<'_, AppState>) -> Result<String> {
    state.terminal_manager.cwd(&session_id)
}
