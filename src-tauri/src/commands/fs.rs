use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn fs_list_dir(dir_path: String, options: Option<JsonValue>) -> Result<JsonValue> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:listDir({dir_path})")))
}

#[tauri::command]
pub fn fs_read_file(file_path: String) -> Result<String> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:readFile({file_path})")))
}

#[tauri::command]
pub fn fs_write_file_atomic(file_path: String, content: String, expected_mtime: Option<f64>) -> Result<()> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:writeFileAtomic({file_path})")))
}

#[tauri::command]
pub fn fs_create_entry(target_path: String, entry_type: String) -> Result<()> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:createEntry({target_path}, {entry_type})")))
}

#[tauri::command]
pub fn fs_rename_entry(old_path: String, new_path: String) -> Result<()> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:renameEntry({old_path} -> {new_path})")))
}

#[tauri::command]
pub fn fs_trash_entry(file_path: String) -> Result<()> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:trashEntry({file_path})")))
}

#[tauri::command]
pub fn fs_move_entry(from: String, to: String) -> Result<()> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:moveEntry({from} -> {to})")))
}

#[tauri::command]
pub fn fs_import_files(source_paths: Vec<String>, dest_dir: String) -> Result<()> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:importFiles({} files -> {dest_dir})", source_paths.len())))
}

#[tauri::command]
pub fn fs_recent_files(root: String) -> Result<Vec<JsonValue>> {
    // TODO: Implement in Loop 8
    Err(Error::NotImplemented(format!("fs:recentFiles({root})")))
}
