use crate::protocol::{respond, Request, Response};
use file_manager_core::file_manager;
use serde_json::{json, Value};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) fn emit_batch_progress(
    writer: Option<&Arc<Mutex<io::Stdout>>>,
    request_id: &str,
    completed: usize,
    total: usize,
    cancelled: &AtomicBool,
) {
    if let Some(writer) = writer {
        let progress_id = format!("{request_id}:progress");
        let _ = respond(
            writer,
            Response {
                id: &progress_id,
                ok: true,
                result: Some(json!({
                    "event": "batch_progress",
                    "completed": completed,
                    "total": total,
                    "cancelled": cancelled.load(Ordering::Relaxed),
                })),
                error: None,
            },
        );
    }
}

pub(crate) fn emit_archive_progress(
    writer: &Arc<Mutex<io::Stdout>>,
    request_id: &str,
    phase: &str,
    completed: usize,
    total: usize,
) {
    let progress_id = format!("{request_id}:progress");
    let _ = respond(
        writer,
        Response {
            id: &progress_id,
            ok: true,
            result: Some(json!({
                "event": "archive_progress",
                "phase": phase,
                "completed": completed,
                "total": total,
            })),
            error: None,
        },
    );
}

pub(crate) fn run_batch(
    request: &Request,
    cancelled: &AtomicBool,
    writer: Option<&Arc<Mutex<io::Stdout>>>,
) -> Result<Value, String> {
    let paths = request
        .params
        .get("paths")
        .and_then(Value::as_array)
        .ok_or("paths is required")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let dest = request.params.get("dest").and_then(Value::as_str);
    let destination = if matches!(request.method.as_str(), "copy_batch" | "move_batch") {
        if cancelled.load(Ordering::Relaxed) {
            None
        } else {
            let raw = dest.ok_or("dest is required")?;
            let authorized = file_manager::FileAccessPolicy::authorize_path(
                raw,
                file_manager::OperationPolicy::Write,
            )
            .map_err(|error| error.to_string())?;
            if cancelled.load(Ordering::Relaxed) {
                return Ok(
                    json!({"ok": false, "errors": [], "skipped": [], "count": 0, "cancelled": true, "copied": [], "moved": []}),
                );
            }
            if !authorized.as_path().exists() {
                std::fs::create_dir_all(authorized.as_path()).map_err(|error| error.to_string())?;
            }
            if !authorized.as_path().is_dir() {
                return Err("destination is not a directory".into());
            }
            Some(authorized.as_path().to_path_buf())
        }
    } else {
        None
    };
    let mut completed = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();
    let total = paths.len();
    for (processed, path) in paths.into_iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let result = match request.method.as_str() {
            "copy_batch" => file_manager::copy_entry(
                &path,
                destination
                    .as_ref()
                    .expect("validated destination")
                    .to_string_lossy()
                    .as_ref(),
            ),
            "move_batch" => {
                let target = destination.as_ref().expect("validated destination");
                let source_parent = std::path::Path::new(&path).parent();
                if source_parent
                    .and_then(|parent| std::fs::canonicalize(parent).ok())
                    .is_some_and(|parent| parent == *target)
                {
                    skipped.push(path);
                    emit_batch_progress(writer, &request.id, processed + 1, total, cancelled);
                    continue;
                }
                file_manager::move_entry(&path, target.to_string_lossy().as_ref())
            }
            "trash_batch" => file_manager::trash_entry(&path).map(|_| path.clone()),
            "duplicate_batch" => file_manager::duplicate_entry(&path),
            _ => return Err("unsupported batch method".into()),
        };
        match result {
            Ok(value) => completed.push(value),
            Err(error) => errors.push(json!({"path": path, "error": error.to_string()})),
        }
        emit_batch_progress(writer, &request.id, processed + 1, total, cancelled);
    }
    let result_key = match request.method.as_str() {
        "copy_batch" => "copied",
        "move_batch" => "moved",
        "trash_batch" => "trashed",
        "duplicate_batch" => "duplicated",
        _ => return Err("unsupported batch method".into()),
    };
    let mut response = json!({
        "ok": errors.is_empty() && !cancelled.load(Ordering::Relaxed),
        "errors": errors,
        "skipped": skipped,
        "count": completed.len(),
        "cancelled": cancelled.load(Ordering::Relaxed),
    });
    response[result_key] = json!(completed);
    Ok(response)
}
