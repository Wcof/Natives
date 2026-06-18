use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn disk_usage(dir_path: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 9
    Err(Error::NotImplemented(format!("disk:usage({dir_path})")))
}
