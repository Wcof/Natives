use crate::protocol::Request;
use base64::Engine;
use file_manager_core::file_manager;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) fn run_preview(
    request: &Request,
    cancelled: Option<&AtomicBool>,
) -> Result<Value, String> {
    if cancelled.is_some_and(|token| token.load(Ordering::Relaxed)) {
        return Err("preview cancelled".into());
    }
    let path = request
        .params
        .get("path")
        .and_then(Value::as_str)
        .ok_or("path is required")?;
    let result = if request.method == "archive_list" {
        file_manager::list_archive(path)
            .map(|value| serde_json::to_value(value).unwrap_or_default())
            .map_err(|error| error.to_string())?
    } else if request.method == "pdf_preview" {
        let preview = file_manager::read_pdf_preview(path).map_err(|error| error.to_string())?;
        json!({ "data": base64::engine::general_purpose::STANDARD.encode(preview.bytes), "mimeType": preview.mime_type, "size": preview.size, "mtime": preview.mtime })
    } else if request.method == "media_preview" {
        let preview = file_manager::read_media_preview(path).map_err(|error| error.to_string())?;
        json!({ "data": base64::engine::general_purpose::STANDARD.encode(preview.bytes), "mimeType": preview.mime_type, "size": preview.size, "mtime": preview.mtime })
    } else {
        file_manager::read_file(path)
            .map(|value| serde_json::to_value(value).unwrap_or_default())
            .map_err(|error| error.to_string())?
    };
    if cancelled.is_some_and(|token| token.load(Ordering::Relaxed)) {
        return Err("preview cancelled".into());
    }
    Ok(result)
}
