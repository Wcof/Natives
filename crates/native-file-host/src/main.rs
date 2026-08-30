//! Chrome Native Messaging bridge for the Files domain.
//! The browser extension is the UI; this process owns all filesystem access.

use file_manager_core::file_manager;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

mod batch;
mod dispatch;
mod import;
mod preview;
mod protocol;
mod search;
mod session;
mod watch;
mod workspace_store;

#[cfg(test)]
mod tests;

use batch::{emit_archive_progress, run_batch};
use dispatch::handle;
use import::handle_import;
use preview::run_preview;
use protocol::{read_frame, respond, safe_error, validate_request, Request, Response};
use search::run_search;
use session::{reap_finished_jobs, shutdown_jobs, ActiveJobs};
use watch::WatchManager;
use workspace_store::WorkspaceStore;

/// Typed workspace/settings method family (ADR-0024). The store is opened per
/// request: the Host stays stateless across connections and SQLite WAL handles
/// short-lived handles cheaply at newtab traffic levels.
fn workspace_dispatch(store: &WorkspaceStore, request: &Request) -> Result<Value, String> {
    let params = &request.params;
    let expected = |params: &Value| params.get("expectedRevision").and_then(Value::as_i64);
    macro_rules! handle_result {
        ($result:expr) => {
            match $result {
                Ok(val) => serde_json::to_value(val).map_err(|e| e.to_string()),
                Err(error) => Err(error.to_string()),
            }
        };
    }
    match request.method.as_str() {
        "workspace_session" => handle_result!(store.session()),
        "workspace_snapshot" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.snapshot(id))
        }
        "workspace_create" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            let template = params.get("template").and_then(Value::as_str);
            handle_result!(store.create(name, template))
        }
        "workspace_rename" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            handle_result!(store.rename(id, name, expected(params)))
        }
        "workspace_reorder" => {
            let ids = parse_id_list(params.get("orderedIds").ok_or("orderedIds is required")?)?;
            handle_result!(store.reorder(&ids, expected(params)))
        }
        "workspace_pin" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let pinned = params
                .get("pinned")
                .and_then(Value::as_bool)
                .ok_or("pinned is required")?;
            handle_result!(store.pin(id, pinned, expected(params)))
        }
        "workspace_duplicate" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.duplicate(id, expected(params)))
        }
        "workspace_delete" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.delete(id, expected(params)))
        }
        "workspace_open_tab" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.open_tab(id))
        }
        "workspace_close_tab" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.close_tab(id))
        }
        "workspace_reorder_tabs" => {
            let ids = parse_id_list(params.get("orderedIds").ok_or("orderedIds is required")?)?;
            handle_result!(store.reorder_tabs(&ids, expected(params)))
        }
        "workspace_widget_upsert" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let widget: workspace_store::WidgetRecord =
                serde_json::from_value(params.get("widget").cloned().ok_or("widget is required")?)
                    .map_err(|e| e.to_string())?;
            handle_result!(store.widget_upsert(id, &widget, expected(params)))
        }
        "workspace_widget_remove" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let widget_id = params
                .get("widgetId")
                .and_then(Value::as_str)
                .ok_or("widgetId is required")?;
            handle_result!(store.widget_remove(id, widget_id, expected(params)))
        }
        "workspace_widget_reorder" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let ids = parse_id_list(params.get("orderedIds").ok_or("orderedIds is required")?)?;
            handle_result!(store.widget_reorder(id, &ids, expected(params)))
        }
        "workspace_background_save" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let background = params
                .get("background")
                .cloned()
                .ok_or("background is required")?;
            handle_result!(store.background_save(id, &background, expected(params)))
        }
        "workspace_instantiate_template" => {
            let template = params
                .get("template")
                .and_then(Value::as_str)
                .ok_or("template is required")?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            handle_result!(store.instantiate_template(template, name))
        }
        "workspace_template_list" => handle_result!(store.template_list()),
        "workspace_template_save" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            let workspace_id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.template_save(name, workspace_id))
        }
        "workspace_template_delete" => {
            let template_id = params
                .get("templateId")
                .and_then(Value::as_str)
                .ok_or("templateId is required")?;
            handle_result!(store.template_delete(template_id))
        }
        "workspace_tabliss_preview" => {
            let tabliss = params
                .get("tabliss")
                .cloned()
                .ok_or("tabliss is required")?;
            handle_result!(store.tabliss_preview(&tabliss))
        }
        "workspace_export_tabliss" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.export_tabliss(id))
        }
        "workspace_save_from_tabliss" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let tabliss = params
                .get("tabliss")
                .cloned()
                .ok_or("tabliss is required")?;
            handle_result!(store.save_from_tabliss(id, &tabliss, expected(params)))
        }
        "workspace_reset" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let template = params
                .get("template")
                .and_then(Value::as_str)
                .ok_or("template is required")?;
            handle_result!(store.reset(id, template, expected(params)))
        }
        "settings_get" => {
            let keys = params
                .get("keys")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            store.settings_get(&keys).map_err(|e| e.to_string())
        }
        "settings_set" => {
            let entries = params
                .get("entries")
                .cloned()
                .ok_or("entries is required")?;
            store.settings_set(&entries).map_err(|e| e.to_string())
        }
        other => Err(format!("unsupported workspace method: {other}")),
    }
}

fn parse_id_list(value: &Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| "orderedIds must be an array of strings".into())
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
