use crate::{search, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn search_grep(
    query: String,
    root: String,
    options: Option<JsonValue>,
) -> Result<Vec<JsonValue>> {
    let max_results = options
        .as_ref()
        .and_then(|o| o.get("maxResults"))
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let file_pattern = options
        .as_ref()
        .and_then(|o| o.get("pattern"))
        .and_then(|v| v.as_str());

    let results = search::search_grep(&query, &root, max_results, file_pattern)?;
    serde_json::to_value(results)
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
pub fn search_files(
    query: String,
    root: String,
    options: Option<JsonValue>,
) -> Result<Vec<JsonValue>> {
    let max_results = options
        .as_ref()
        .and_then(|o| o.get("maxResults"))
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let results = search::search_files(&query, &root, max_results)?;
    serde_json::to_value(results)
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
pub fn search_spotlight(query: String, root: String) -> Result<Vec<JsonValue>> {
    let results = search::search_spotlight(&query, &root)?;
    serde_json::to_value(results)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}
