//! 会话相关 API 处理器（经 Model Host 8766 窄接口获取）。

use super::stats::known_cost;
use super::{api_error, json_str, parse_query_param};
use crate::model_host::ModelUsageQueryClient;

pub(super) fn api_sessions(
    client: &ModelUsageQueryClient,
    path: &str,
) -> Result<String, (u16, String)> {
    let query = path.split_once('?').map(|(_, q)| q);
    let result = client
        .get_sessions(query)
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;

    let mut items = Vec::new();
    for s in result.sessions {
        let sid = if s.key.is_empty() {
            s.tool_id.clone()
        } else {
            s.key.clone()
        };
        items.push(format!(
            "{{\"sessionId\":{},\"sourceId\":{},\"title\":{},\"projectPath\":{},\"startedAt\":{},\"lastUsedAt\":{},\"totalTokens\":{},\"costUsd\":{},\"messageCount\":{}}}",
            json_str(&sid),
            json_str(&s.tool_id),
            json_str(if s.title.is_empty() { &s.source } else { &s.title }),
            json_str(""),
            json_str(&s.first_at),
            json_str(&s.last_at),
            s.tokens,
            known_cost(s.cost_usd, s.priced),
            s.requests
        ));
    }

    Ok(format!("{{\"sessions\":[{}]}}", items.join(",")))
}

pub(super) fn api_session_detail(
    client: &ModelUsageQueryClient,
    path: &str,
) -> Result<String, (u16, String)> {
    let session_id = parse_query_param(path, "id").unwrap_or_default();
    if session_id.is_empty() {
        return Err(api_error(400, "MISSING_ID", "Missing session id"));
    }

    let offset = parse_query_param(path, "offset")
        .unwrap_or_else(|| "0".into())
        .parse::<u32>()
        .map_err(|_| api_error(400, "INVALID_OFFSET", "Invalid offset"))?;
    let q = format!(
        "sessionKey={}&range=all&limit=50&offset={offset}",
        urlencoding(&session_id)
    );
    let result = client
        .get_events(Some(&q))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;

    let mut items = Vec::new();
    for ev in result.events {
        let cost_usd = ev.cost_micro as f64 / 1_000_000.0;
        items.push(format!(
            "{{\"id\":{},\"turnId\":{},\"model\":{},\"inputTokens\":{},\"outputTokens\":{},\"cacheReadTokens\":{},\"reasoningTokens\":{},\"costUsd\":{},\"recordedAt\":{}}}",
            json_str(&ev.id),
            json_str(""),
            json_str(&ev.model),
            ev.input_tokens,
            ev.output_tokens,
            ev.cache_read_tokens,
            ev.reasoning_tokens,
            known_cost(cost_usd, Some(ev.cost_status.as_deref() == Some("priced"))),
            json_str(&ev.requested_at)
        ));
    }

    Ok(format!(
        "{{\"sessionId\":{},\"records\":[{}],\"hasMore\":{},\"nextOffset\":{}}}",
        json_str(&session_id),
        items.join(","),
        result.has_more,
        u64::from(offset) + items.len() as u64
    ))
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}
