//! Validated RPC entry points. Business handlers never derive a page's identity.
use super::dispatch::{dispatch_service_request, is_allowed_web_rpc_method};
use super::http_helpers::{extract_cookie_value, validate_origin, write_http_raw};
use super::server::ServiceState;
use crate::app_store::AppStore;
use crate::protocol::{validate_request, Request, Response};
use app_runtime_core::http::HttpRequest;
use app_runtime_core::session::constant_time_eq;
use serde_json::json;
use std::net::TcpStream;

pub(super) fn handle_relay(stream: &mut TcpStream, state: &ServiceState, req: &HttpRequest) {
    state.touch_activity();
    // F01 P0 修复：
    // 1. 任何带有 Origin 头的请求直接拒绝（浏览器请求必带或可含 Origin，Native 进程不带 Origin）
    if req.header("origin").is_some() {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"relay does not accept origin",
        );
        return;
    }
    // 2. 必须具备合法的 X-Natives-Relay-Secret 报头且与服务内私密密钥一致
    let provided_secret = req.header("x-natives-relay-secret").unwrap_or("");
    if provided_secret.is_empty() || !constant_time_eq(provided_secret, &state.relay_secret) {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"unauthorized relay access",
        );
        return;
    }
    if req.header("content-type") != Some("application/json") {
        let _ = write_http_raw(
            stream,
            415,
            "Unsupported Media Type",
            "text/plain",
            &[],
            b"JSON required",
        );
        return;
    }

    let Ok(request) = serde_json::from_slice::<Request>(&req.body) else {
        let _ = write_http_raw(stream, 400, "Bad Request", "text/plain", &[], b"bad JSON");
        return;
    };
    if let Some(result) = super::native_lease::handle(state, &request) {
        let response = match result {
            Ok(result) => json!({"id": request.id, "ok": true, "result": result}),
            Err(error) => json!({"id": request.id, "ok": false, "error": error}),
        };
        let body = serde_json::to_vec(&response).unwrap_or_default();
        let _ = write_http_raw(stream, 200, "OK", "application/json", &[], &body);
        return;
    }
    if !request.method.starts_with("app:")
        && !request.method.starts_with("model_")
        && validate_request(&request).is_err()
    {
        let _ = write_http_raw(
            stream,
            400,
            "Bad Request",
            "text/plain",
            &[],
            b"invalid native request",
        );
        return;
    }
    let surface = req.header("x-natives-surface-id").unwrap_or("");
    if !state
        .native_surfaces
        .lock()
        .is_ok_and(|leases| leases.contains_key(surface))
    {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"Native owner required",
        );
        return;
    }
    let Ok(_request_lease) = super::surface_requests::admit(state, surface) else {
        let _ = write_http_raw(
            stream,
            429,
            "Too Many Requests",
            "application/json",
            &[],
            b"{\"error\":\"ERR_REQUEST_LIMIT\"}",
        );
        return;
    };
    let response = dispatch_service_request(state, &request, surface);
    let (_, resp_bytes) = encode_response(&response);
    let _ = write_http_raw(stream, 200, "OK", "application/json", &[], &resp_bytes);
}

pub(super) fn handle_rpc(stream: &mut TcpStream, state: &ServiceState, req: &HttpRequest) {
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
    if req.header("content-type") != Some("application/json") {
        let _ = write_http_raw(
            stream,
            415,
            "Unsupported Media Type",
            "text/plain",
            &[],
            b"JSON required",
        );
        return;
    }

    let Ok(mut request) = serde_json::from_slice::<Request>(&req.body) else {
        let _ = write_http_raw(stream, 400, "Bad Request", "text/plain", &[], b"bad JSON");
        return;
    };

    if !is_allowed_web_rpc_method(&request.method) {
        let body = json!({ "id": request.id, "ok": false, "error": "ERR_METHOD_NOT_ALLOWED" });
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let _ = write_http_raw(stream, 400, "Bad Request", "application/json", &[], &bytes);
        return;
    }
    let surface = req.header("x-natives-surface-id").unwrap_or("");
    if !state.auth.lock().unwrap().owns_surface(session_id, surface) {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"Surface owner required",
        );
        return;
    }
    if request.method.starts_with("app:") {
        let surface_id = req.header("x-natives-surface-id").unwrap_or("");
        let app_id = request
            .params
            .get("appId")
            .and_then(serde_json::Value::as_str);
        if !state
            .auth
            .lock()
            .unwrap()
            .authorize_app_surface(session_id, surface_id, app_id)
        {
            let _ = write_http_raw(
                stream,
                403,
                "Forbidden",
                "application/json",
                &[],
                b"{\"error\":\"ERR_SURFACE_NOT_OWNED\"}",
            );
            return;
        }
        if let Some(params) = request.params.as_object_mut() {
            params.insert("surfaceId".into(), surface_id.into());
        }
    }
    if request.method == "apps:handshake" {
        let trusted_origin = state
            .app_store
            .lock()
            .ok()
            .and_then(|store| store.as_ref().and_then(AppStore::caller_origin));
        let Some(trusted_origin) = trusted_origin else {
            let _ = write_http_raw(
                stream,
                503,
                "Service Unavailable",
                "text/plain",
                &[],
                b"product origin unavailable",
            );
            return;
        };
        request.params = json!({ "origin": trusted_origin });
    }
    if !request.method.starts_with("app:")
        && !request.method.starts_with("model_")
        && !request.method.starts_with("model:")
        && validate_request(&request).is_err()
    {
        eprintln!(
            "natives-file-host: invalid web RPC: {}",
            validate_request(&request).unwrap_err()
        );
        let _ = write_http_raw(
            stream,
            400,
            "Bad Request",
            "text/plain",
            &[],
            b"invalid request",
        );
        return;
    }

    let Ok(_request_lease) = super::surface_requests::admit(state, surface) else {
        let _ = write_http_raw(
            stream,
            429,
            "Too Many Requests",
            "application/json",
            &[],
            b"{\"error\":\"ERR_REQUEST_LIMIT\"}",
        );
        return;
    };
    let response = dispatch_service_request(state, &request, surface);
    let (conflict, resp_bytes) = encode_response(&response);
    let _ = write_http_raw(
        stream,
        if conflict { 409 } else { 200 },
        if conflict { "Conflict" } else { "OK" },
        "application/json",
        &[],
        &resp_bytes,
    );
}

fn encode_response(response: &Response<'_>) -> (bool, Vec<u8>) {
    let mut value = serde_json::to_value(response)
        .unwrap_or_else(|_| json!({"id":response.id,"ok":false,"error":"ERR_RPC_SERIALIZATION"}));
    let error = response.error.as_deref().unwrap_or("");
    let conflict = error.starts_with("revision conflict:") || error == "revision_conflict";
    if conflict {
        value["errorCode"] = json!("revision_conflict");
    } else if !response.ok
        && !error.is_empty()
        && error
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        value["errorCode"] = json!(error);
    }
    (conflict, value.to_string().into_bytes())
}
