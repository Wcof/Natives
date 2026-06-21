use crate::{agent, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn agent_scan_projects() -> Result<Vec<JsonValue>> {
    let projects = agent::scan_projects()?;
    serde_json::to_value(projects)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn agent_get_sessions(project_path: String) -> Result<Vec<JsonValue>> {
    let sessions = agent::scan_sessions(&project_path)?;
    serde_json::to_value(sessions)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn agent_scan_skills() -> Result<JsonValue> {
    let skills_data = agent::scan_skills()?;
    serde_json::to_value(skills_data).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn agent_detect_status(output: String, exit_code: Option<i32>) -> Result<JsonValue> {
    agent::detect_status(&output, exit_code)
}
