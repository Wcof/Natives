use crate::{archive, archive_ops, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn archive_list(archive_path: String) -> Result<JsonValue> {
    let listing = archive::list_archive(&archive_path)?;
    serde_json::to_value(listing).map_err(|e| Error::Internal(e.to_string()))
}

/// 解压压缩包（zip 走共享 safe 实现；tar 家族走系统 tar）。
/// dest_dir 缺省为压缩包所在目录下的同名文件夹（防覆盖）。
/// 解压/打包可能耗时较长，走 spawn_blocking 避免占用 IPC 线程。
#[tauri::command]
pub async fn fs_extract_archive(
    archive_path: String,
    dest_dir: Option<String>,
) -> Result<archive_ops::ExtractArchiveResult> {
    tokio::task::spawn_blocking(move || {
        archive_ops::extract_archive(&archive_path, dest_dir.as_deref())
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

/// 把多个文件/目录打包为 zip。dest_zip_path 缺省为第一项同目录下 `<名称>.zip`（防覆盖）。
#[tauri::command]
pub async fn fs_compress_entries(
    paths: Vec<String>,
    dest_zip_path: Option<String>,
) -> Result<archive_ops::CompressEntriesResult> {
    tokio::task::spawn_blocking(move || {
        archive_ops::compress_entries(&paths, dest_zip_path.as_deref())
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}
