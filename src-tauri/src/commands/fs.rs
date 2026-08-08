use crate::{file_manager, Error, Result};
use serde_json::Value as JsonValue;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// 共享有界 IO 信号量：所有文件域重命令共享 4 个并发槽位（R-P9/R15）。
/// 模式：async command -> bounded semaphore -> spawn_blocking -> existing file_manager。
fn fs_io_slots() -> &'static Semaphore {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    SLOTS.get_or_init(|| Semaphore::new(4))
}

async fn acquire_io_slot() -> Result<tokio::sync::SemaphorePermit<'static>> {
    fs_io_slots()
        .acquire()
        .await
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub async fn fs_list_dir(dir_path: String, options: Option<JsonValue>) -> Result<Vec<JsonValue>> {
    let _permit = acquire_io_slot().await?;
    let opts = options
        .and_then(|v| serde_json::from_value::<file_manager::ListDirOptions>(v).ok())
        .unwrap_or_default();
    tokio::task::spawn_blocking(move || {
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
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

/// Rich list: entries + parent + current project badge (fanbox-compatible shape).
#[tauri::command]
pub async fn fs_list_dir_detailed(
    dir_path: String,
    options: Option<JsonValue>,
) -> Result<JsonValue> {
    let _permit = acquire_io_slot().await?;
    let opts = options
        .and_then(|v| serde_json::from_value::<file_manager::ListDirOptions>(v).ok())
        .unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        let result = file_manager::list_dir_detailed(&dir_path, &opts)?;
        serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_read_file(file_path: String) -> Result<JsonValue> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || {
        let result = file_manager::read_file(&file_path)?;
        serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_write_file_atomic(
    file_path: String,
    content: String,
    expected_mtime: Option<f64>,
) -> Result<JsonValue> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || {
        let result = file_manager::write_file_atomic(&file_path, &content, expected_mtime)?;
        serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub fn fs_create_entry(target_path: String, entry_type: String) -> Result<()> {
    file_manager::create_entry(&target_path, &entry_type)
}

#[tauri::command]
pub fn fs_rename_entry(old_path: String, new_path: String) -> Result<String> {
    file_manager::rename_entry(&old_path, &new_path)
}

#[tauri::command]
pub fn fs_trash_entry(file_path: String) -> Result<()> {
    file_manager::trash_entry(&file_path)
}

#[tauri::command]
pub fn fs_move_entry(from: String, to: String) -> Result<String> {
    file_manager::move_entry(&from, &to)
}

#[tauri::command]
pub fn fs_copy_entry(from: String, to: String) -> Result<String> {
    file_manager::copy_entry(&from, &to)
}

#[tauri::command]
pub fn fs_duplicate_entry(file_path: String) -> Result<String> {
    file_manager::duplicate_entry(&file_path)
}

#[tauri::command]
pub fn fs_stat(file_path: String) -> Result<file_manager::StatResult> {
    file_manager::stat_path(&file_path)
}

#[tauri::command]
pub async fn fs_import_files(source_paths: Vec<String>, dest_dir: String) -> Result<Vec<String>> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || file_manager::import_files(&source_paths, &dest_dir))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_trash_entries(paths: Vec<String>) -> Result<JsonValue> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || {
        let result = file_manager::trash_entries(&paths)?;
        serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_move_entries(paths: Vec<String>, dest_dir: String) -> Result<JsonValue> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || {
        let result = file_manager::move_entries(&paths, &dest_dir)?;
        serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_copy_entries(paths: Vec<String>, dest_dir: String) -> Result<JsonValue> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || {
        let result = file_manager::copy_entries(&paths, &dest_dir)?;
        serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub fn fs_roots() -> Result<Vec<JsonValue>> {
    file_manager::default_roots()
}

#[tauri::command]
pub fn fs_open_with(path: String, with: Option<String>) -> Result<JsonValue> {
    file_manager::open_with(&path, with.as_deref().unwrap_or("default"))
}

#[tauri::command]
pub fn fs_clipboard_copy_files(paths: Vec<String>) -> Result<JsonValue> {
    file_manager::clipboard_copy_files(&paths)
}

#[tauri::command]
pub fn fs_clipboard_copy_image(file_path: String) -> Result<JsonValue> {
    file_manager::clipboard_copy_image(&file_path)
}

/// Save a base64-encoded blob to disk with path validation and atomic write.
/// Used by URL/image drops (WeChat, browser drags) that need to save binary data.
/// 同步实现保持路径/安全逻辑可单测；异步 command 外包 bounded semaphore + spawn_blocking。
#[tauri::command]
pub async fn fs_save_blob(dir: String, name: String, base64_data: String) -> Result<String> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || save_blob_impl(&dir, &name, &base64_data))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
}

fn save_blob_impl(dir: &str, name: &str, base64_data: &str) -> Result<String> {
    // 1. Clean and validate the file name — reject dangerous characters
    let clean_name = sanitize_filename(name);
    if clean_name.is_empty() {
        return Err(Error::InvalidInput("empty or invalid file name".into()));
    }
    if clean_name.contains('/') || clean_name.contains("..") || clean_name.contains('\0') {
        return Err(Error::InvalidInput("invalid file name".into()));
    }

    // 2. Resolve and validate the target directory
    let base_dir = std::path::Path::new(dir);
    let target_path = base_dir.join(&clean_name);

    // 3. Reject path traversal: ensure resolved path is under the intended directory
    let canonical_base = base_dir
        .canonicalize()
        .map_err(|e| Error::Internal(format!("failed to resolve base dir: {e}")))?;
    // Enforce the shared allowlist/blocklist on the target directory itself.
    // The name-traversal check below only keeps the file inside `dir`; without
    // this, `dir` could be ~/.ssh or any other blocklisted location.
    file_manager::validate_path(&canonical_base)?;
    let canonical_target = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.clone());
    if !canonical_target.starts_with(&canonical_base) {
        return Err(Error::InvalidInput("path traversal detected".into()));
    }

    // 4. Decode base64 data
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, base64_data)
        .map_err(|e| Error::Internal(format!("base64 decode failed: {e}")))?;

    // 5. Atomic write: write to temp file then rename
    let tmp_name = format!(
        ".tmp-{}-{}",
        clean_name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
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
pub async fn fs_recent_files(root: String) -> Result<Vec<file_manager::FileEntry>> {
    let _permit = acquire_io_slot().await?;
    tokio::task::spawn_blocking(move || file_manager::recent_files(&root))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
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
        let result = save_blob_impl(
            tmp.to_string_lossy().to_string().as_str(),
            "",
            "dGVzdA==",
        );
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(
            result.is_err(),
            "Expected error for empty filename, got {:?}",
            result
        );
        assert!(result.unwrap_err().to_string().contains("empty or invalid"));
    }

    #[test]
    fn test_fs_save_blob_valid_base64() {
        let tmp = std::env::temp_dir().join(format!(
            "natives-test-fs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).expect("failed to create temp dir");
        // Use canonical path to avoid symlink issues
        let canonical_tmp = std::fs::canonicalize(&tmp).expect("failed to canonicalize");
        let dir_str = canonical_tmp.to_string_lossy().to_string();
        let result = save_blob_impl(
            dir_str.as_str(),
            "test.png",
            "dGVzdA==",
        );
        if result.is_err() {
            eprintln!("fs_save_blob failed: {:?}", result);
            eprintln!("dir: {}, name: test.png", dir_str);
        }
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
        let saved_path = result.unwrap();
        assert!(
            std::path::Path::new(&saved_path).exists(),
            "Saved file should exist: {}",
            saved_path
        );
        let content = std::fs::read(&saved_path).unwrap();
        assert_eq!(content, b"test");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
