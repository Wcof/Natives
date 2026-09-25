//! HTTP API 路由与业务请求处理（tokenusage 模块）。

use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

mod sessions;
mod settings;
mod stats;

use sessions::{api_session_detail, api_sessions};
use settings::{
    api_collect, api_devices, api_get_settings, api_save_settings, api_sync_pull, api_sync_push,
    api_tray_state,
};
use stats::{
    api_limits, api_limits_refresh, api_models, api_overview, api_tools, api_trends,
};

pub fn handle_api_request(
    store: &Store,
    request: &HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    let body_text = String::from_utf8_lossy(&request.body).to_string();
    let method = request.method.as_str();
    match (method, route) {
        ("GET", "/api/overview") => api_overview(store),
        ("GET", "/api/tools") => api_tools(store),
        ("GET", "/api/models") => api_models(store),
        ("GET", "/api/sessions") => api_sessions(store, request.path.as_str()),
        ("GET", "/api/sessions/detail") => api_session_detail(store, request.path.as_str()),
        ("GET", "/api/limits") => api_limits(store),
        ("POST", "/api/limits/refresh") => api_limits_refresh(store, &body_text),
        ("GET", "/api/trends") => api_trends(store, request.path.as_str()),
        ("GET", "/api/devices") => api_devices(store),
        ("POST", "/api/sync/push") => api_sync_push(store),
        ("POST", "/api/sync/pull") => api_sync_pull(store, &body_text),
        ("GET", "/api/settings") => api_get_settings(store),
        ("POST", "/api/settings") => api_save_settings(store, &body_text),
        ("POST", "/api/collect") => api_collect(store, &body_text),
        ("GET", "/api/tray/state") => api_tray_state(store),
        _ => Err((404, "{\"error\":\"not found\"}".into())),
    }
}

pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn api_error(code: u16, error_code: &str, message: &str) -> (u16, String) {
    (
        code,
        format!(
            "{{\"error\":{},\"message\":{}}}",
            json_str(error_code),
            json_str(message)
        ),
    )
}

fn parse_query_param(path: &str, key: &str) -> Option<String> {
    let q = path.split_once('?')?;
    for pair in q.1.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}
