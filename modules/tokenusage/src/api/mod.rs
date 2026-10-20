//! HTTP API 路由与业务请求处理（tokenusage 模块）。

use crate::model_host::ModelUsageQueryClient;
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

mod sessions;
mod settings;
pub(crate) mod stats;

use sessions::{api_session_detail, api_sessions};
use settings::{
    api_collect, api_devices, api_get_settings, api_save_settings, api_sync_pull, api_sync_push,
    api_tray_state,
};
use stats::{api_limits, api_limits_refresh, api_models, api_overview, api_tools, api_trends};

pub fn handle_api_request(
    store: &Store,
    client: &ModelUsageQueryClient,
    request: &HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    let body_text = String::from_utf8_lossy(&request.body).to_string();
    let method = request.method.as_str();
    match (method, route) {
        ("GET", "/api/overview") => api_overview(client),
        ("GET", "/api/tools") => api_tools(client, request.path.as_str()),
        ("GET", "/api/models") => api_models(client, request.path.as_str()),
        ("GET", "/api/sessions") => api_sessions(client, request.path.as_str()),
        ("GET", "/api/sessions/detail") => api_session_detail(client, request.path.as_str()),
        ("GET", "/api/limits") => api_limits(client),
        ("POST", "/api/limits/refresh") => api_limits_refresh(client, &body_text),
        ("GET", "/api/trends") => api_trends(client, request.path.as_str()),
        ("GET", "/api/devices") => api_devices(store),
        ("POST", "/api/sync/push") => api_sync_push(store),
        ("POST", "/api/sync/pull") => api_sync_pull(store, &body_text),
        ("GET", "/api/settings") => api_get_settings(store),
        ("POST", "/api/settings") => api_save_settings(store, &body_text),
        ("POST", "/api/collect") => api_collect(client, &body_text),
        ("GET", "/api/tray/state") => api_tray_state(client),
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
                let mut decoded = Vec::with_capacity(v.len());
                let mut bytes = v.bytes();
                while let Some(byte) = bytes.next() {
                    if byte == b'%' {
                        let hex = [bytes.next()?, bytes.next()?];
                        let text = std::str::from_utf8(&hex).ok()?;
                        decoded.push(u8::from_str_radix(text, 16).ok()?);
                    } else {
                        decoded.push(if byte == b'+' { b' ' } else { byte });
                    }
                }
                return String::from_utf8(decoded).ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_query_decodes_once_and_rejects_broken_escapes() {
        assert_eq!(
            super::parse_query_param("/detail?id=project%3A%E4%BD%A0+ok", "id"),
            Some("project:你 ok".into())
        );
        assert_eq!(super::parse_query_param("/detail?id=bad%2", "id"), None);
    }
}
