use crate::{file_manager, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn fs_list_dir(dir_path: String, options: Option<JsonValue>) -> Result<Vec<JsonValue>> {
    let opts = options
        .and_then(|v| serde_json::from_value::<file_manager::ListDirOptions>(v).ok())
        .unwrap_or_default();
    let entries = file_manager::list_dir(&dir_path, &opts)?;
    serde_json::to_value(entries)
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
pub fn fs_read_file(file_path: String) -> Result<JsonValue> {
    let result = file_manager::read_file(&file_path)?;
    serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn fs_write_file_atomic(
    file_path: String,
    content: String,
    expected_mtime: Option<f64>,
) -> Result<JsonValue> {
    let result = file_manager::write_file_atomic(&file_path, &content, expected_mtime)?;
    serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn fs_create_entry(target_path: String, entry_type: String) -> Result<()> {
    file_manager::create_entry(&target_path, &entry_type)
}

#[tauri::command]
pub fn fs_rename_entry(old_path: String, new_path: String) -> Result<()> {
    file_manager::rename_entry(&old_path, &new_path)
}

#[tauri::command]
pub fn fs_trash_entry(file_path: String) -> Result<()> {
    file_manager::trash_entry(&file_path)
}

#[tauri::command]
pub fn fs_move_entry(from: String, to: String) -> Result<()> {
    file_manager::move_entry(&from, &to)
}

#[tauri::command]
pub fn fs_import_files(source_paths: Vec<String>, dest_dir: String) -> Result<Vec<String>> {
    file_manager::import_files(&source_paths, &dest_dir)
}

#[tauri::command]
pub fn fs_recent_files(root: String) -> Result<Vec<JsonValue>> {
    file_manager::recent_files(&root)
}
