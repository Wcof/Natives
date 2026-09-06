//! Chrome Native Messaging bridge for the Files domain.
//! The browser extension is the UI; this process owns all filesystem access.

use file_manager_core::file_manager;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

mod app_dispatch;
mod app_store;
mod batch;
mod dispatch;
mod import;
mod preview;
mod protocol;
mod search;
mod session;
mod watch;
mod workspace_dispatch;
mod workspace_store;

#[cfg(test)]
mod tests;

use app_store::AppStore;
use batch::{emit_archive_progress, run_batch};
use dispatch::handle;
use import::handle_import;
use preview::run_preview;
use protocol::{read_frame, respond, safe_error, validate_request, Request, Response};
use search::run_search;
use session::{reap_finished_jobs, shutdown_jobs, ActiveJobs};
use watch::WatchManager;
use workspace_store::WorkspaceStore;

const MAX_CONCURRENT_JOBS: usize = 8;
const MAX_CONCURRENT_IMPORTS: usize = 4;

fn workspace_dispatch(store: &WorkspaceStore, request: &Request) -> Result<Value, String> {
    workspace_dispatch::workspace_dispatch(store, request)
}

fn main() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let writer = Arc::new(Mutex::new(io::stdout()));
    let mut watchers = WatchManager::default();
    let mut imports: HashMap<String, file_manager::ImportWriter> = HashMap::new();
    let searches: ActiveJobs = Arc::new(Mutex::new(HashMap::new()));
    let mut jobs = Vec::new();
    let workspace_store_singleton = match WorkspaceStore::open(&workspace_store::default_db_path())
    {
        Ok(store) => Some(store),
        Err(error) => {
            eprintln!("workspace store initialization failed: {error}");
            None
        }
    };
    let app_store_singleton = match AppStore::open(&workspace_store::default_db_path()) {
        Ok(store) => Some(store),
        Err(error) => {
            eprintln!("app store initialization failed: {error}");
            None
        }
    };
    loop {
        reap_finished_jobs(&mut jobs);
        let Some(body) = read_frame(&mut input) else {
            break;
        };
        let request: Request = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(error) => {
                respond(
                    &writer,
                    Response {
                        id: "",
                        ok: false,
                        result: None,
                        error: Some(safe_error(error.to_string())),
                    },
                )?;
                continue;
            }
        };
        if let Err(error) = validate_request(&request) {
            respond(
                &writer,
                Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some(safe_error(error)),
                },
            )?;
            continue;
        }
        if request.method == "search_cancel"
            || request.method == "preview_cancel"
            || request.method == "batch_cancel"
        {
            let target = request
                .params
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if let Ok(active) = searches.lock() {
                if let Some(token) = active.get(target) {
                    token.store(true, Ordering::Relaxed);
                }
            }
            respond(
                &writer,
                Response {
                    id: &request.id,
                    ok: true,
                    result: Some(json!({"cancelled": true})),
                    error: None,
                },
            )?;
            continue;
        }
        if request.method.starts_with("import_") {
            if request.method == "import_begin" && imports.len() >= MAX_CONCURRENT_IMPORTS {
                let response = Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some("host_busy: maximum concurrent imports reached".into()),
                };
                respond(&writer, response)?;
                continue;
            }
            let response = match handle_import(&request, &mut imports) {
                Ok(result) => Response {
                    id: &request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some(safe_error(error)),
                },
            };
            respond(&writer, response)?;
            continue;
        }
        if request.method.starts_with("workspace_") || request.method.starts_with("settings_") {
            let response = match workspace_store_singleton
                .as_ref()
                .ok_or_else(|| "workspace store is not available".to_string())
                .and_then(|store| workspace_dispatch(store, &request))
            {
                Ok(result) => Response {
                    id: &request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some(safe_error(error)),
                },
            };
            respond(&writer, response)?;
            continue;
        }
        if request.method.starts_with("apps:") {
            let response = match app_store_singleton
                .as_ref()
                .ok_or_else(|| "app store is not available".to_string())
                .and_then(|store| app_dispatch::app_dispatch(store, &request))
            {
                Ok(result) => Response {
                    id: &request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some(safe_error(error)),
                },
            };
            respond(&writer, response)?;
            continue;
        }
        if request.method == "search"
            || request.method == "read_file"
            || request.method == "archive_list"
            || request.method == "extract_archive"
            || request.method == "create_zip"
            || matches!(
                request.method.as_str(),
                "copy_batch" | "move_batch" | "trash_batch" | "duplicate_batch"
            )
        {
            reap_finished_jobs(&mut jobs);
            if jobs.len() >= MAX_CONCURRENT_JOBS {
                let response = Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some("host_busy: maximum concurrent jobs reached".into()),
                };
                respond(&writer, response)?;
                continue;
            }
            let token = Arc::new(AtomicBool::new(false));
            if let Ok(mut active) = searches.lock() {
                active.insert(request.id.clone(), Arc::clone(&token));
            }
            let writer = Arc::clone(&writer);
            let searches = Arc::clone(&searches);
            jobs.push(thread::spawn(move || {
                let result = if matches!(
                    request.method.as_str(),
                    "copy_batch" | "move_batch" | "trash_batch" | "duplicate_batch"
                ) {
                    run_batch(&request, &token, Some(&writer))
                } else if request.method == "search" {
                    run_search(&request, Some(&token))
                } else if request.method == "extract_archive" {
                    match (
                        request.params.get("path").and_then(Value::as_str),
                        request.params.get("dest").and_then(Value::as_str),
                    ) {
                        (Some(path), Some(dest)) => file_manager::extract_archive(path, dest)
                            .map(|value| serde_json::to_value(value).unwrap_or_default())
                            .map_err(|e| e.to_string()),
                        (None, _) => Err("path is required".into()),
                        (_, None) => Err("dest is required".into()),
                    }
                } else if request.method == "create_zip" {
                    let paths = request.params.get("paths").and_then(Value::as_array);
                    let dest = request.params.get("dest").and_then(Value::as_str);
                    let name = request.params.get("name").and_then(Value::as_str);
                    match (paths, dest, name) {
                        (Some(paths), Some(dest), Some(name)) => {
                            let paths = paths
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>();
                            emit_archive_progress(&writer, &request.id, "validating", 0, 0);
                            let result =
                                file_manager::create_zip_with_cancel(&paths, dest, name, &token);
                            if result.is_ok() {
                                emit_archive_progress(&writer, &request.id, "compressing", 1, 1);
                            }
                            result
                                .map(|value| serde_json::to_value(value).unwrap_or_default())
                                .map_err(|e| e.to_string())
                        }
                        (None, _, _) => Err("paths is required".into()),
                        (_, None, _) => Err("dest is required".into()),
                        (_, _, None) => Err("name is required".into()),
                    }
                } else {
                    run_preview(&request, Some(&token))
                };
                if let Ok(mut active) = searches.lock() {
                    active.remove(&request.id);
                }
                let response = match result {
                    Ok(result) => Response {
                        id: &request.id,
                        ok: true,
                        result: Some(result),
                        error: None,
                    },
                    Err(error) => Response {
                        id: &request.id,
                        ok: false,
                        result: None,
                        error: Some(safe_error(error)),
                    },
                };
                let _ = respond(&writer, response);
            }));
            continue;
        }
        if request.method == "watch_start" || request.method == "watch_stop" {
            match watchers.handle(&request, &writer) {
                Ok(result) => respond(
                    &writer,
                    Response {
                        id: &request.id,
                        ok: true,
                        result: Some(result),
                        error: None,
                    },
                )?,
                Err(error) => respond(
                    &writer,
                    Response {
                        id: &request.id,
                        ok: false,
                        result: None,
                        error: Some(safe_error(error)),
                    },
                )?,
            }
            continue;
        }
        match handle(&request) {
            Ok(result) => respond(
                &writer,
                Response {
                    id: &request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                },
            )?,
            Err(error) => respond(
                &writer,
                Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some(safe_error(error)),
                },
            )?,
        }
    }
    watchers.shutdown();
    imports.clear();
    shutdown_jobs(&searches, &mut jobs);
    Ok(())
}
