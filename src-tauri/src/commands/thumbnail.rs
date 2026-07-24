use crate::{thumbnail, Error, Result};
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
