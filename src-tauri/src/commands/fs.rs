use crate::{file_manager, Error, Result};
use serde_json::Value as JsonValue;

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

/// Save a base64-encoded blob to disk with path validation and atomic write.
/// Used by URL/image drops (WeChat, browser drags) that need to save binary data.
#[tauri::command]
pub fn fs_save_blob(dir: String, name: String, base64_data: String) -> Result<String> {
    // 1. Clean and validate the file name — reject dangerous characters
    let clean_name = sanitize_filename(&name);
    if clean_name.is_empty() {
        return Err(Error::InvalidInput("empty or invalid file name".into()));
    }
    if clean_name.contains('/') || clean_name.contains("..") || clean_name.contains('\0') {
        return Err(Error::InvalidInput("invalid file name".into()));
    }

    // 2. Resolve and validate the target directory
    let base_dir = std::path::Path::new(&dir);
    let target_path = base_dir.join(&clean_name);

    // 3. Reject path traversal: ensure resolved path is under the intended directory
    let canonical_base = base_dir
        .canonicalize()
        .map_err(|e| Error::Internal(format!("failed to resolve base dir: {e}")))?;
    let canonical_target = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.clone());
    if !canonical_target.starts_with(&canonical_base) {
        return Err(Error::InvalidInput("path traversal detected".into()));
    }

    // 4. Decode base64 data
    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &base64_data,
    )
    .map_err(|e| Error::Internal(format!("base64 decode failed: {e}")))?;

    // 5. Atomic write: write to temp file then rename
    let tmp_name = format!(".tmp-{}-{}", clean_name, std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis());
    let tmp_path = base_dir.join(&tmp_name);

    std::fs::write(&tmp_path, &bytes)?;
    // Sync to ensure data is persisted before rename
    if let Ok(f) = std::fs::File::open(&tmp_path) {
        let _ = f.sync_all();
    }
    // Atomic rename
    std::fs::rename(&tmp_path, &target_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        Error::Io(e)
    })?;

    Ok(target_path.to_string_lossy().to_string())
}

/// Remove dangerous characters from a file name.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|&c| !c.is_control() && c != '/' && c != '\\' && c != '\0')
        .take(255)
        .collect::<String>()
        .trim()
        .to_string()
}

#[tauri::command]
pub fn fs_recent_files(root: String) -> Result<Vec<JsonValue>> {
    file_manager::recent_files(&root)
}

// ── Security Regression Tests for fs_save_blob ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_removes_slashes() {
        // Slashes and backslashes are removed
        assert_eq!(sanitize_filename("evil/file.png"), "evilfile.png");
        assert_eq!(sanitize_filename("subdir\\file.png"), "subdirfile.png");
    }

    #[test]
    fn test_sanitize_filename_removes_null_bytes() {
        assert_eq!(sanitize_filename("evil\x00file.png"), "evilfile.png");
    }

    #[test]
    fn test_sanitize_filename_removes_control_chars() {
        assert_eq!(sanitize_filename("evil\nfile.png"), "evilfile.png");
        assert_eq!(sanitize_filename("evil\rfile.png"), "evilfile.png");
    }

    #[test]
    fn test_sanitize_filename_empty_after_clean() {
        assert_eq!(sanitize_filename(""), "");
        assert_eq!(sanitize_filename("\n\r"), "");
    }

    #[test]
    fn test_sanitize_filename_truncates_to_255() {
        let long = "a".repeat(300);
        let result = sanitize_filename(&long);
        assert_eq!(result.len(), 255);
    }

    #[test]
    fn test_fs_save_blob_rejects_empty_filename() {
        let tmp = std::env::temp_dir().join(format!("natives-test-fs-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let result = fs_save_blob(tmp.to_string_lossy().to_string(), "".to_string(), "dGVzdA==".to_string());
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(result.is_err(), "Expected error for empty filename, got {:?}", result);
        assert!(result.unwrap_err().to_string().contains("empty or invalid"));
    }

    #[test]
    fn test_fs_save_blob_valid_base64() {
        let tmp = std::env::temp_dir().join(format!("natives-test-fs-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&tmp).expect("failed to create temp dir");
        // Use canonical path to avoid symlink issues
        let canonical_tmp = std::fs::canonicalize(&tmp).expect("failed to canonicalize");
        let dir_str = canonical_tmp.to_string_lossy().to_string();
        let result = fs_save_blob(dir_str.clone(), "test.png".to_string(), "dGVzdA==".to_string());
        if result.is_err() {
            eprintln!("fs_save_blob failed: {:?}", result);
            eprintln!("dir: {}, name: test.png", dir_str);
        }
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
        let saved_path = result.unwrap();
        assert!(std::path::Path::new(&saved_path).exists(), "Saved file should exist: {}", saved_path);
        let content = std::fs::read(&saved_path).unwrap();
        assert_eq!(content, b"test");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
