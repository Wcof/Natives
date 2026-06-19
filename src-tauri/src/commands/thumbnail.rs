use crate::{thumbnail, Error, Result};

#[tauri::command]
pub fn thumbnail_generate(file_path: String, width: u32) -> Result<String> {
    match thumbnail::generate_thumbnail(&file_path, width)? {
        Some((jpeg_data, _cached)) => {
            // Return base64-encoded JPEG
            use base64::Engine;
            Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_data))
        }
        None => Err(Error::NotFound(format!(
            "no thumbnail for {file_path}"
        ))),
    }
}
