use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn agent_scan_projects() -> Result<Vec<JsonValue>> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented("agent:scanProjects".into()))
}

#[tauri::command]
pub fn agent_get_sessions(project_path: String) -> Result<Vec<JsonValue>> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented(format!("agent:getSessions({project_path})")))
}

#[tauri::command]
pub fn agent_scan_skills() -> Result<Vec<JsonValue>> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented("agent:scanSkills".into()))
}

#[tauri::command]
pub fn agent_detect_status(output: String, exit_code: Option<i32>) -> Result<JsonValue> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented("agent:detectStatus".into()))
}
