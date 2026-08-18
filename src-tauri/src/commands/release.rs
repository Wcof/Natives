use crate::{release_wizard, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn release_inspect(project_path: String) -> Result<JsonValue> {
    let inspection = release_wizard::inspect_project(&project_path)?;
    serde_json::to_value(inspection).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn release_prepare(project_path: String, version: String) -> Result<JsonValue> {
    let preparation = release_wizard::prepare_release(&project_path, &version)?;
    serde_json::to_value(preparation).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn release_get_sequence(project_path: String, version: String) -> Result<JsonValue> {
    let plan = release_wizard::get_sequence(&project_path, &version)?;
    serde_json::to_value(plan).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn release_execute(project_path: String, version: String, action: String) -> Result<JsonValue> {
    let execution = release_wizard::execute_action(&project_path, &version, &action)?;
    serde_json::to_value(execution).map_err(|e| Error::Internal(e.to_string()))
}
