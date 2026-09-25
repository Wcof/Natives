//! trends 引擎代理（拆分自 api.rs，仅移动，无逻辑变更）。

use super::api_error;
use app_runtime_core::http::HttpRequest;
use std::time::Duration;

pub(crate) fn proxy_trends_engine(request: &HttpRequest, route: &str) -> Result<String, (u16, String)> {
    let target_subpath = route.strip_prefix("/api/trends").unwrap_or(route);
    let target_subpath = if target_subpath.is_empty() { "/" } else { target_subpath };
    let query_str = request
        .path
        .split_once('?')
        .map(|(_, q)| format!("?{q}"))
        .unwrap_or_default();
    let url = format!("http://127.0.0.1:8795/api{target_subpath}{query_str}");

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();

    let resp = match request.method.as_str() {
        "POST" => {
            let body_text = String::from_utf8_lossy(&request.body);
            agent
                .post(&url)
                .set("Content-Type", "application/json")
                .send_string(&body_text)
        }
        _ => agent.get(&url).call(),
    };

    match resp {
        Ok(r) => {
            let body = r.into_string().map_err(|e| {
                api_error(502, "TRENDS_READ_FAILED", &format!("读取趋势数据失败: {e}"))
            })?;
            Ok(body)
        }
        Err(ureq::Error::Status(code, r)) => {
            let err_body = r
                .into_string()
                .unwrap_or_else(|_| "{\"error\":\"TRENDS_ENGINE_ERROR\"}".into());
            Err((code, err_body))
        }
        Err(e) => Err(api_error(
            502,
            "TRENDS_ENGINE_UNAVAILABLE",
            &format!("趋势分析引擎尚未就绪或未启动: {e}"),
        )),
    }
}
