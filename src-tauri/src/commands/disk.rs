use crate::{disk_usage, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn disk_usage(dir_path: String) -> Result<Vec<JsonValue>> {
    let items = disk_usage::get_disk_usage(&dir_path)?;
    serde_json::to_value(items)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}
