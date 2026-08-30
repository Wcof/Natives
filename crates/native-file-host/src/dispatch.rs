use crate::protocol::Request;
use crate::search::{run_locate, run_search};
use base64::Engine;
use file_manager_core::file_manager;
use serde_json::{json, Value};

pub(crate) fn handle(request: &Request) -> Result<Value, String> {
    match request.method.as_str() {
        "version" => Ok(json!({
            "protocolVersion": 1,
            "hostVersion": env!("CARGO_PKG_VERSION"),
        })),
        "roots" => file_manager::default_roots()
            .map(Value::Array)
            .map_err(|e| e.to_string()),
        "list_dir" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            let offset = request
                .params
                .get("offset")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let limit = request
                .params
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(500)
                .min(2_000) as usize;
            let options = file_manager::ListDirOptions {
                sort_by: request
                    .params
                    .get("sortBy")
                    .and_then(Value::as_str)
                    .unwrap_or("name")
                    .to_string(),
                sort_dir: request
                    .params
                    .get("sortDir")
                    .and_then(Value::as_str)
                    .unwrap_or("asc")
                    .to_string(),
                show_hidden: request
                    .params
                    .get("showHidden")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                probe_projects: request
                    .params
                    .get("probeProjects")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            file_manager::list_dir_page_with_options(path, offset, limit, &options)
                .map(|(v, has_more)| {
                    let mut value = serde_json::to_value(v).unwrap_or_default();
                    value["hasMore"] = json!(has_more);
                    value
                })
                .map_err(|e| e.to_string())
        }
        "disk_usage" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            serde_json::to_value(file_manager::disk_usage(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        }
        "search" => run_search(request, None),
        "locate" => run_locate(request),
        "read_file" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            file_manager::read_file(path)
                .map(|v| serde_json::to_value(v).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        "image_preview" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            let preview = file_manager::read_image_preview(path).map_err(|e| e.to_string())?;
            Ok(
                json!({ "data": base64::engine::general_purpose::STANDARD.encode(preview.bytes), "mimeType": preview.mime_type, "size": preview.size, "mtime": preview.mtime }),
            )
        }
        "pdf_preview" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            let preview = file_manager::read_pdf_preview(path).map_err(|e| e.to_string())?;
            Ok(
                json!({ "data": base64::engine::general_purpose::STANDARD.encode(preview.bytes), "mimeType": preview.mime_type, "size": preview.size, "mtime": preview.mtime }),
            )
        }
        "media_preview" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            let preview = file_manager::read_media_preview(path).map_err(|e| e.to_string())?;
            Ok(
                json!({ "data": base64::engine::general_purpose::STANDARD.encode(preview.bytes), "mimeType": preview.mime_type, "size": preview.size, "mtime": preview.mtime }),
            )
        }
        "archive_list" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            file_manager::list_archive(path)
                .map(|value| serde_json::to_value(value).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        "stat" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            file_manager::stat_path(path)
                .map(|v| serde_json::to_value(v).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        "create_folder" => {
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
                return Err("invalid folder name".into());
            }
            file_manager::create_entry(&format!("{parent}/{name}"), "folder")
                .map(|_| json!({"ok": true}))
                .map_err(|e| e.to_string())
        }
        "create_file" => {
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
                return Err("invalid file name".into());
            }
            file_manager::create_entry(&format!("{parent}/{name}"), "file")
                .map(|_| json!({"ok": true}))
                .map_err(|e| e.to_string())
        }
        "write_file" => {
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
            let encoded = request
                .params
                .get("data")
                .and_then(Value::as_str)
                .ok_or("data is required")?;
            if !file_manager::valid_name(name) {
                return Err("invalid name".into());
            }
            file_manager::FileAccessPolicy::authorize_path(
                parent,
                file_manager::OperationPolicy::Write,
            )
            .map_err(|e| e.to_string())?;
            let target = std::path::Path::new(parent).join(name);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| "invalid base64 data".to_string())?;
            let expected_mtime = request.params.get("expectedMtime").and_then(Value::as_f64);
            let result =
                file_manager::write_bytes_atomic(&target.to_string_lossy(), &bytes, expected_mtime)
                    .map_err(|e| e.to_string())?;
            Ok(
                json!({"path": target.to_string_lossy(), "mtime": result.mtime, "conflict": result.conflict}),
            )
        }
        "rename" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            let name = request
                .params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            if !file_manager::valid_name(name) {
                return Err("invalid name".into());
            }
            let parent = std::path::Path::new(path)
                .parent()
                .ok_or("path has no parent")?;
            file_manager::rename_entry(path, &parent.join(name).to_string_lossy())
                .map(|value| json!({"path": value}))
                .map_err(|e| e.to_string())
        }
        "trash" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            file_manager::trash_entry(path)
                .map(|_| json!({"ok": true}))
                .map_err(|e| e.to_string())
        }
        "copy" | "move" => {
            let from = request
                .params
                .get("from")
                .or_else(|| request.params.get("source"))
                .and_then(Value::as_str)
                .ok_or("from is required")?;
            let to = request
                .params
                .get("to")
                .or_else(|| request.params.get("target"))
                .and_then(Value::as_str)
                .ok_or("to is required")?;
            let result = if request.method == "copy" {
                file_manager::copy_entry(from, to)
            } else {
                file_manager::move_entry(from, to)
            };
            result
                .map(|value| json!({"path": value}))
                .map_err(|e| e.to_string())
        }
        "duplicate" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            file_manager::duplicate_entry(path)
                .map(|value| json!({"path": value}))
                .map_err(|e| e.to_string())
        }
        "copy_batch" | "move_batch" => {
            let paths = request
                .params
                .get("paths")
                .and_then(Value::as_array)
                .ok_or("paths is required")?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            let dest = request
                .params
                .get("dest")
                .and_then(Value::as_str)
                .ok_or("dest is required")?;
            let result = if request.method == "copy_batch" {
                file_manager::copy_entries(&paths, dest)
            } else {
                file_manager::move_entries(&paths, dest)
            };
            result.map_err(|e| e.to_string())
        }
        "trash_batch" => {
            let paths = request
                .params
                .get("paths")
                .and_then(Value::as_array)
                .ok_or("paths is required")?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            file_manager::trash_entries(&paths).map_err(|e| e.to_string())
        }
        "copy_paths" => {
            let paths = request
                .params
                .get("paths")
                .and_then(Value::as_array)
                .ok_or("paths is required")?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            file_manager::clipboard_copy_files(&paths).map_err(|e| e.to_string())
        }
        "copy_image" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            file_manager::clipboard_copy_image(path).map_err(|e| e.to_string())
        }
        "open" | "editor" | "reveal" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("path is required")?;
            let mode = if request.method == "reveal" {
                "reveal"
            } else if request.method == "editor" {
                "editor"
            } else {
                "default"
            };
            file_manager::open_with(path, mode).map_err(|e| e.to_string())
        }
        "open_trash" => file_manager::open_trash().map_err(|e| e.to_string()),
        _ => Err("unsupported method".into()),
    }
}
