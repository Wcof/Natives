//! 本机独立 Web 服务模式领域请求分发（ADR-0042 / F13）。

use crate::app_dispatch;
use crate::protocol::{safe_error, Request, Response};
use crate::sql_diag;
use crate::workspace_dispatch::workspace_dispatch;

pub(crate) fn is_allowed_web_rpc_method(method: &str) -> bool {
    matches!(
        method,
        "version"
            | "roots"
            | "stat"
            | "list_dir"
            | "disk_usage"
            | "read_file"
            | "image_preview"
            | "pdf_preview"
            | "media_preview"
            | "archive_list"
            | "extract_archive"
            | "create_zip"
            | "write_file"
            | "create_folder"
            | "create_file"
            | "trash"
            | "rename"
            | "copy"
            | "move"
            | "duplicate"
            | "copy_paths"
            | "copy_image"
            | "open"
            | "editor"
            | "reveal"
            | "open_trash"
            | "copy_batch"
            | "move_batch"
            | "trash_batch"
            | "duplicate_batch"
            | "watch_start"
            | "watch_stop"
            | "search"
            | "locate"
            | "search_cancel"
            | "locate_cancel"
            | "preview_cancel"
            | "batch_cancel"
            | "diagnostics_get"
            | "diagnostics_reset"
    ) || method.starts_with("workspace_")
        || method.starts_with("settings_")
        || method.starts_with("preferences_")
        || method.starts_with("apps:")
        || (method.starts_with("app:") && method != "app:release")
        || method.starts_with("model_")
        || method.starts_with("model:")
        || method.starts_with("import_")
}

pub(crate) fn dispatch_service_request<'a>(
    state: &super::server::ServiceState,
    request: &'a Request,
    surface: &str,
) -> Response<'a> {
    state.touch_activity();

    if matches!(
        request.method.as_str(),
        "diagnostics_get" | "diagnostics_reset"
    ) {
        if !sql_diag::enabled() {
            return Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some("ERR_DIAGNOSTICS_DISABLED".into()),
            };
        }
        if request.method == "diagnostics_reset" {
            sql_diag::reset();
        }
        return Response {
            id: &request.id,
            ok: true,
            result: Some(sql_diag::snapshot()),
            error: None,
        };
    }

    if request.method.starts_with("model_") || request.method.starts_with("model:") {
        return state.model_manager.dispatch_request(request, surface);
    }

    if request.method.starts_with("app:") {
        let response = state.runtime_manager.dispatch_request(request);
        if response.ok && request.method == "app:invalidate" {
            if let Some(result) = &response.result {
                state.broadcast_event(&serde_json::json!({"type":"module_changed","domain":"modules","appId":result.get("appId"),"originId":result.get("originId")}).to_string());
            }
        }
        return response;
    }

    if request.method.starts_with("workspace_")
        || request.method.starts_with("settings_")
        || request.method.starts_with("preferences_")
    {
        let store_guard = state.workspace_store.lock().unwrap();
        let resp = match store_guard
            .as_ref()
            .ok_or_else(|| "workspace store is not available".to_string())
            .and_then(|store| workspace_dispatch(store, request))
        {
            Ok(result) => {
                let domain = if request.method.starts_with("preferences_") {
                    "preferences"
                } else if request.method.starts_with("workspace_") {
                    "workspace"
                } else {
                    "settings"
                };
                if !matches!(
                    request.method.as_str(),
                    "preferences_get"
                        | "settings_get"
                        | "workspace_session"
                        | "workspace_snapshot"
                        | "workspace_template_list"
                        | "workspace_export_tabliss"
                        | "workspace_tabliss_preview"
                ) {
                    state.broadcast_event(&serde_json::json!({"type":format!("{domain}_changed"),"domain":domain,"revision":result.get("revision")}).to_string());
                }
                Response {
                    id: &request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                }
            }
            Err(error) => Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some(safe_error(error)),
            },
        };
        return resp;
    }

    if request.method.starts_with("apps:") {
        let store_guard = state.app_store.lock().unwrap();
        let caller_origin = store_guard.as_ref().and_then(|store| store.caller_origin());
        let resp = match store_guard
            .as_ref()
            .ok_or_else(|| "app store is not available".to_string())
            .and_then(|store| {
                app_dispatch::app_dispatch_with_maintenance(
                    store,
                    request,
                    caller_origin.as_deref(),
                    |app_id, close| state.runtime_manager.begin_maintenance(app_id, close),
                )
            }) {
            Ok(result) => {
                if matches!(
                    request.method.as_str(),
                    "apps:set_enabled" | "apps:set_sidebar" | "apps:clear_data" | "apps:handshake"
                ) {
                    state.broadcast_event(&serde_json::json!({"type":"apps_changed","domain":"apps","revision":result.get("revision"),"appId":request.params.get("appId"),"maintenance":request.method=="apps:clear_data" || (request.method=="apps:set_enabled" && request.params.get("enabled").and_then(serde_json::Value::as_bool)==Some(false))}).to_string());
                }
                Response {
                    id: &request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                }
            }
            Err(error) => Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some(safe_error(error)),
            },
        };
        return resp;
    }

    match state.files.handle(
        request,
        surface,
        Some(&|event| state.broadcast_event_to(surface, event)),
    ) {
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
    }
}
