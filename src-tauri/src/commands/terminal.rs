use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalData {
    pub session_id: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalExit {
    pub session_id: String,
    pub exit_code: i32,
}

#[tauri::command]
pub fn terminal_create(profile_id: Option<String>) -> Result<String> {
    // TODO: Implement in Loop 7 — create PTY session
    Err(Error::NotImplemented(format!("terminal:create({profile_id:?})")))
}

#[tauri::command]
pub fn terminal_write(session_id: String, data: String) -> Result<()> {
    // TODO: Implement in Loop 7 — write to PTY
    Err(Error::NotImplemented(format!("terminal:write({session_id})")))
}

#[tauri::command]
pub fn terminal_resize(session_id: String, cols: u32, rows: u32) -> Result<()> {
    // TODO: Implement in Loop 7 — resize PTY
    Err(Error::NotImplemented(format!("terminal:resize({session_id}, {cols}x{rows})")))
}

#[tauri::command]
pub fn terminal_kill(session_id: String) -> Result<()> {
    // TODO: Implement in Loop 7 — kill PTY
    Err(Error::NotImplemented(format!("terminal:kill({session_id})")))
}

#[tauri::command]
pub fn terminal_cwd(session_id: String) -> Result<String> {
    // TODO: Implement in Loop 7 — get PTY cwd
    Err(Error::NotImplemented(format!("terminal:cwd({session_id})")))
}
