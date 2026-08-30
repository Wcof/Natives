use crate::protocol::Request;
use base64::Engine;
use file_manager_core::file_manager;
use serde_json::{json, Value};
use std::collections::HashMap;

fn import_parent_and_name(request: &Request) -> Result<(&str, &str), String> {
    let parent = request
        .params
        .get("parent")
        .and_then(Value::as_str)
        .ok_or("parent is required")?;
    let name = request
        .params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("name is required")?;
    if !file_manager::valid_name(name) {
        return Err("invalid import name".into());
    }
    Ok((parent, name))
}

fn import_probe(request: &Request) -> Result<Value, String> {
    let (parent, name) = import_parent_and_name(request)?;
    let authorized = file_manager::FileAccessPolicy::authorize_path(
        parent,
        file_manager::OperationPolicy::Write,
    )
    .map_err(|error| error.to_string())?;
    if !authorized.as_path().is_dir() {
        return Err("import parent is not a directory".into());
    }
    let target = authorized.as_path().join(name);
    Ok(json!({
        "exists": std::fs::symlink_metadata(target).is_ok(),
    }))
}

pub(crate) fn handle_import(
    request: &Request,
    imports: &mut HashMap<String, file_manager::ImportWriter>,
) -> Result<Value, String> {
    match request.method.as_str() {
        "import_probe" => import_probe(request),
        "import_begin" => {
            let upload_id = request
                .params
                .get("uploadId")
                .and_then(Value::as_str)
                .ok_or("uploadId is required")?;
            if upload_id.is_empty() || imports.contains_key(upload_id) {
                return Err("invalid or duplicate upload id".into());
            }
            let (parent, name) = import_parent_and_name(request)?;
            let size = request
                .params
                .get("size")
                .and_then(Value::as_u64)
                .ok_or("size is required")?;
            let conflict = request
                .params
                .get("conflict")
                .and_then(Value::as_str)
                .ok_or("conflict is required")?;
            let writer = file_manager::begin_import(parent, name, size, conflict)
                .map_err(|error| error.to_string())?;
            let path = writer.target_path().to_string_lossy().to_string();
            let renamed = writer
                .target_path()
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value != name);
            imports.insert(upload_id.to_string(), writer);
            Ok(json!({
                "uploadId": upload_id,
                "path": path,
                "name": std::path::Path::new(&path).file_name().and_then(|value| value.to_str()).unwrap_or(name),
                "size": size,
                "renamed": renamed,
            }))
        }
        "import_chunk" => {
            let upload_id = request
                .params
                .get("uploadId")
                .and_then(Value::as_str)
                .ok_or("uploadId is required")?;
            let offset = request
                .params
                .get("offset")
                .and_then(Value::as_u64)
                .ok_or("offset is required")?;
            let data = request
                .params
                .get("data")
                .and_then(Value::as_str)
                .ok_or("data is required")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|_| "invalid base64 data".to_string())?;
            if bytes.len() > file_manager::MAX_IMPORT_CHUNK_BYTES {
                return Err("import chunk is too large".into());
            }
            let writer = imports.get_mut(upload_id).ok_or("unknown upload id")?;
            writer
                .write_chunk(offset, &bytes)
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "uploadId": upload_id,
                "received": writer.received(),
                "size": writer.expected_size(),
            }))
        }
        "import_end" => {
            let upload_id = request
                .params
                .get("uploadId")
                .and_then(Value::as_str)
                .ok_or("uploadId is required")?;
            let writer = imports.get(upload_id).ok_or("unknown upload id")?;
            if writer.received() != writer.expected_size() {
                return Err("import is incomplete".into());
            }
            let writer = imports.remove(upload_id).ok_or("unknown upload id")?;
            let result = writer.finish().map_err(|error| error.to_string())?;
            Ok(json!({"path": result.path, "size": result.size, "mtime": result.mtime}))
        }
        "import_cancel" => {
            let upload_id = request
                .params
                .get("uploadId")
                .and_then(Value::as_str)
                .ok_or("uploadId is required")?;
            let cancelled = imports.remove(upload_id).is_some();
            Ok(json!({"uploadId": upload_id, "cancelled": cancelled}))
        }
        _ => Err("unsupported import method".into()),
    }
}
