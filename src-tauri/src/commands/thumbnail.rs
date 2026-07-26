use crate::{image_convert, thumbnail, Error, Result};
use std::sync::OnceLock;
use tokio::sync::Semaphore;

#[tauri::command]
pub async fn thumbnail_generate(file_path: String, width: u32) -> Result<String> {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    let _permit = SLOTS
        .get_or_init(|| Semaphore::new(4))
        .acquire()
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    let requested_path = file_path.clone();
    let result = tokio::task::spawn_blocking(move || thumbnail::generate_thumbnail(&file_path, width))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    match result {
        Some((jpeg_data, _cached)) => {
            // Return base64-encoded JPEG
            use base64::Engine;
            Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_data))
        }
        None => Err(Error::NotFound(format!(
            "no thumbnail for {requested_path}"
        ))),
    }
}

/// HEIC/HEIF 等格式全尺寸透明转码为 jpeg（W8）。
/// 全尺寸 sips 转码比缩略图重，限 2 并发；非 macOS 明确报错。
#[tauri::command]
pub async fn fs_convert_image_preview(
    file_path: String,
) -> Result<image_convert::ConvertImageResult> {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    let _permit = SLOTS
        .get_or_init(|| Semaphore::new(2))
        .acquire()
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    tokio::task::spawn_blocking(move || image_convert::convert_image_preview(&file_path))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
}
