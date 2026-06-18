use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn archive_list(archive_path: String) -> Result<Vec<JsonValue>> {
    // TODO: Implement in Loop 9
    Err(Error::NotImplemented(format!("archive:list({archive_path})")))
}
