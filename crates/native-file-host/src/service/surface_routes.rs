//! 独立 Web 页面的 Surface 租约路由（ADR-0042）。

use super::http_helpers::{extract_cookie_value, validate_origin, write_http_raw};
use super::server::ServiceState;
use app_runtime_core::http::HttpRequest;
use serde_json::{json, Value};
use std::net::TcpStream;

pub(crate) fn handle_surfaces(
    stream: &mut TcpStream,
    state: &ServiceState,
    req: &HttpRequest,
    route: &str,
) {
    state.touch_activity();
    if !validate_origin(req, state.port) {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"invalid origin",
        );
        return;
    }

    let cookie_name = if cfg!(debug_assertions) {
        "natives_dev_session"
    } else {
        "natives_session"
    };
    let session_id = extract_cookie_value(req, cookie_name).unwrap_or("");
    if !state.auth.lock().unwrap().validate_session(session_id) {
        let _ = write_http_raw(
            stream,
            401,
            "Unauthorized",
            "application/json",
            &[],
            b"{\"error\":\"ERR_UNAUTHENTICATED\"}",
        );
        return;
    }

    let csrf = req.header("x-csrf-token").unwrap_or("");
    if !state.auth.lock().unwrap().validate_csrf(session_id, csrf) {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"invalid CSRF token",
        );
        return;
    }

    if route == "/web/surfaces" && req.method == "POST" {
        let app_id_opt = serde_json::from_slice::<Value>(&req.body)
            .ok()
            .and_then(|v| v.get("appId").and_then(Value::as_str).map(String::from));
        if let Some(ref aid) = app_id_opt {
            if aid != "fund" && aid != "tokenusage" {
                let body = json!({ "ok": false, "error": "ERR_UNKNOWN_APP_ID" });
                let bytes = serde_json::to_vec(&body).unwrap_or_default();
                let _ = write_http_raw(stream, 400, "Bad Request", "application/json", &[], &bytes);
                return;
            }
        }

        match state
            .auth
            .lock()
            .unwrap()
            .register_surface(session_id, app_id_opt)
        {
            Ok(surface_id) => {
                state.files.register(&surface_id);
                let body = json!({ "ok": true, "surfaceId": surface_id });
                let bytes = serde_json::to_vec(&body).unwrap_or_default();
                let _ = write_http_raw(stream, 200, "OK", "application/json", &[], &bytes);
            }
            Err(err) => {
                let body = json!({ "ok": false, "error": err });
                let bytes = serde_json::to_vec(&body).unwrap_or_default();
                let _ = write_http_raw(stream, 400, "Bad Request", "application/json", &[], &bytes);
            }
        }
        return;
    }

    if let Some(surface_id) = route
        .strip_prefix("/web/surfaces/")
        .map(|s| s.trim_matches('/'))
    {
        if req.method == "POST" {
            match state
                .auth
                .lock()
                .unwrap()
                .renew_surface(session_id, surface_id)
            {
                Ok(()) => {
                    let _ = write_http_raw(
                        stream,
                        200,
                        "OK",
                        "application/json",
                        &[],
                        b"{\"ok\":true}",
                    );
                }
                Err(err) => {
                    let body = json!({ "ok": false, "error": err });
                    let bytes = serde_json::to_vec(&body).unwrap_or_default();
                    let _ =
                        write_http_raw(stream, 404, "Not Found", "application/json", &[], &bytes);
                }
            }
        } else if req.method == "DELETE" {
            let released = state
                .auth
                .lock()
                .unwrap()
                .revoke_surface(session_id, surface_id);
            if released == Ok(true) {
                state.runtime_manager.release_surface(surface_id);
                state.model_manager.release_surface(surface_id);
                state.files.release(surface_id);
            }
            let _ = write_http_raw(stream, 200, "OK", "application/json", &[], b"{\"ok\":true}");
        }
    }
}
