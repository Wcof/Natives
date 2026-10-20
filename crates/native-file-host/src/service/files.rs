//! Surface-scoped Files tasks reuse the existing domain operations and cancellation.
use crate::{
    batch::run_batch,
    dispatch,
    import::handle_import,
    preview::run_preview,
    protocol::Request,
    search::{run_locate, run_search},
    watch::WatchManager,
};
use file_manager_core::file_manager::{self, ImportWriter};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(super) struct FilesTasks {
    owners: Mutex<HashSet<String>>,
    watchers: Mutex<HashMap<String, WatchManager>>,
    imports: Mutex<HashMap<String, ImportWriter>>,
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
    changed: Arc<AtomicBool>,
}
impl FilesTasks {
    pub fn register(&self, id: &str) {
        if let Ok(mut owners) = self.owners.lock() {
            owners.insert(id.into());
        }
    }
    pub fn release(&self, id: &str) {
        if let Ok(mut owners) = self.owners.lock() {
            owners.remove(id);
        }
        let prefix = format!("{id}:");
        if let Ok(mut watchers) = self.watchers.lock() {
            if let Some(mut watcher) = watchers.remove(id) {
                watcher.shutdown();
            }
        }
        if let Ok(mut imports) = self.imports.lock() {
            imports.retain(|key, _| !key.starts_with(&prefix));
        }
        if let Ok(active) = self.active.lock() {
            for (_, token) in active.iter().filter(|(key, _)| key.starts_with(&prefix)) {
                token.store(true, Ordering::Release);
            }
        }
    }
    pub fn release_missing(&self, active: &HashSet<String>) {
        let missing = self
            .owners
            .lock()
            .map(|owners| owners.difference(active).cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for id in missing {
            self.release(&id);
        }
    }
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }
    pub fn handle(
        &self,
        request: &Request,
        owner: &str,
        events: Option<&dyn Fn(&str)>,
    ) -> Result<Value, String> {
        // Serializes release with task admission, so EOF cannot leave a late task behind.
        let owners = self.owners.lock().map_err(|_| "FILES_UNAVAILABLE")?;
        if !owners.contains(owner) {
            return Err("ERR_SURFACE_NOT_OWNED".into());
        }
        let key = format!("{owner}:{}", request.id);
        if request.method.ends_with("_cancel") && !request.method.starts_with("import_") {
            let target = format!(
                "{owner}:{}",
                request.params["requestId"].as_str().unwrap_or("")
            );
            let active = self.active.lock().map_err(|_| "FILES_UNAVAILABLE")?;
            if let Some(token) = active.get(&target) {
                token.store(true, Ordering::Release);
                return Ok(json!({"cancelled":true}));
            }
            return Err("FILES_TASK_NOT_OWNED".into());
        }
        if matches!(request.method.as_str(), "watch_start" | "watch_stop") {
            let changed = self.changed.clone();
            let mut watchers = self.watchers.lock().map_err(|_| "FILES_UNAVAILABLE")?;
            let watcher = watchers.entry(owner.into()).or_default();
            if request.method == "watch_start" {
                watcher.shutdown();
            }
            return watcher.handle_with_callback(
                request,
                Arc::new(move |_| {
                    changed.store(true, Ordering::Release);
                }),
            );
        }
        if request.method.starts_with("import_") {
            let mut owned = request.clone();
            if let Some(id) = request.params["uploadId"].as_str() {
                owned.params["uploadId"] = json!(format!("{owner}:{id}"));
            }
            let mut imports = self.imports.lock().map_err(|_| "FILES_UNAVAILABLE")?;
            if request.method == "import_begin"
                && (imports.len() >= 64
                    || imports
                        .keys()
                        .filter(|key| key.starts_with(&format!("{owner}:")))
                        .count()
                        >= 4)
            {
                return Err("FILES_TASK_LIMIT".into());
            }
            let mut result = handle_import(&owned, &mut imports)?;
            if let Some(id) = request.params.get("uploadId") {
                result["uploadId"] = id.clone();
            }
            return Ok(result);
        }
        let token = Arc::new(AtomicBool::new(false));
        {
            let mut active = self.active.lock().map_err(|_| "FILES_UNAVAILABLE")?;
            if active.len() >= 64
                || active
                    .keys()
                    .filter(|key| key.starts_with(&format!("{owner}:")))
                    .count()
                    >= 4
                || active.contains_key(&key)
            {
                return Err("FILES_TASK_LIMIT".into());
            }
            active.insert(key.clone(), token.clone());
        }
        drop(owners);
        let result = run(request, &token, events);
        if let Ok(mut active) = self.active.lock() {
            active.remove(&key);
        }
        if result.is_ok()
            && matches!(
                request.method.as_str(),
                "write_file"
                    | "create_file"
                    | "create_folder"
                    | "rename"
                    | "trash"
                    | "copy"
                    | "move"
                    | "duplicate"
                    | "duplicate_batch"
                    | "copy_batch"
                    | "move_batch"
                    | "trash_batch"
                    | "create_zip"
                    | "extract_archive"
            )
        {
            self.changed.store(true, Ordering::Release);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_cancel_and_release_are_owned_and_do_not_revive() {
        let files = FilesTasks::default();
        files.register("one");
        files.register("two");
        let token = Arc::new(AtomicBool::new(false));
        files
            .active
            .lock()
            .unwrap()
            .insert("one:task".into(), token.clone());
        let request = Request {
            id: "cancel".into(),
            method: "search_cancel".into(),
            params: json!({"requestId":"task"}),
        };
        assert!(files.handle(&request, "two", None).is_err());
        assert!(!token.load(Ordering::Acquire));
        assert!(files.handle(&request, "one", None).is_ok());
        token.store(false, Ordering::Release);
        files.release("one");
        assert!(token.load(Ordering::Acquire));
        assert!(files.handle(&request, "one", None).is_err());
        assert!(files.owners.lock().unwrap().contains("two"));
    }
}
fn emit_archive_event(
    events: Option<&dyn Fn(&str)>,
    request_id: &str,
    phase: &str,
    completed: usize,
    total: usize,
) {
    if let Some(emit) = events {
        let body = json!({
            "type": "task_progress",
            "domain": "files",
            "id": format!("{request_id}:progress"),
            "result": {"event": "archive_progress", "phase": phase, "completed": completed, "total": total},
        });
        emit(&body.to_string());
    }
}

fn run(
    request: &Request,
    token: &AtomicBool,
    events: Option<&dyn Fn(&str)>,
) -> Result<Value, String> {
    match request.method.as_str() {
        "search" => run_search(request, Some(token)),
        "locate" => run_locate(request, Some(token)),
        "read_file" | "archive_list" => run_preview(request, Some(token)),
        "copy_batch" | "move_batch" | "trash_batch" | "duplicate_batch" => {
            let progress = |completed: usize, total: usize| {
                if let Some(emit) = events {
                    let body = json!({
                        "type": "task_progress",
                        "domain": "files",
                        "id": format!("{}:progress", request.id),
                        "result": {
                            "event": "batch_progress",
                            "completed": completed,
                            "total": total,
                            "cancelled": token.load(Ordering::Acquire),
                        }
                    });
                    emit(&body.to_string());
                }
            };
            run_batch(request, token, None, Some(&progress))
        }
        "create_zip" => {
            let paths = request.params["paths"]
                .as_array()
                .ok_or("paths is required")?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            let dest = request.params["dest"].as_str().ok_or("dest is required")?;
            let name = request.params["name"].as_str().ok_or("name is required")?;
            let total = paths.len().max(1);
            emit_archive_event(events, &request.id, "validating", 0, total);
            let value = file_manager::create_zip_with_cancel(&paths, dest, name, token)
                .map_err(|e| e.to_string())?;
            emit_archive_event(events, &request.id, "compressing", total, total);
            serde_json::to_value(value).map_err(|e| e.to_string())
        }
        "extract_archive" => {
            if token.load(Ordering::Acquire) {
                return Err("operation cancelled".into());
            }
            emit_archive_event(events, &request.id, "validating", 0, 1);
            let value = file_manager::extract_archive_with_cancel(
                request.params["path"].as_str().ok_or("path is required")?,
                request.params["dest"].as_str().ok_or("dest is required")?,
                token,
            )
            .map_err(|e| e.to_string())?;
            emit_archive_event(events, &request.id, "extracting", 1, 1);
            serde_json::to_value(value).map_err(|e| e.to_string())
        }
        _ => dispatch::handle(request),
    }
}
