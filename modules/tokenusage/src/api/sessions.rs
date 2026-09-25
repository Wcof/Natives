//! 会话相关 API 处理器。

use super::{api_error, json_str, parse_query_param};
use crate::storage::Store;

pub(super) fn api_sessions(store: &Store, _path: &str) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT session_id, source_id, title, project_path, started_at, last_used_at,
                        total_input_tokens + total_output_tokens, total_cost_micros, message_count
                 FROM sessions ORDER BY last_used_at DESC LIMIT 100",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let sid: String = row.get(0)?;
                let source_id: String = row.get(1)?;
                let title: String = row.get(2)?;
                let project: String = row.get(3)?;
                let started: String = row.get(4)?;
                let last_used: String = row.get(5)?;
                let tokens: i64 = row.get(6)?;
                let cost_micros: i64 = row.get(7)?;
                let count: i64 = row.get(8)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"sessionId\":{},\"sourceId\":{},\"title\":{},\"projectPath\":{},\"startedAt\":{},\"lastUsedAt\":{},\"totalTokens\":{},\"costUsd\":{:.4},\"messageCount\":{}}}",
                    json_str(&sid), json_str(&source_id), json_str(&title), json_str(&project),
                    json_str(&started), json_str(&last_used), tokens, cost_usd, count
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"sessions\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_session_detail(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let session_id = parse_query_param(path, "id").unwrap_or_default();
    if session_id.is_empty() {
        return Err(api_error(400, "MISSING_ID", "Missing session id"));
    }

    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, turn_id, model, input_tokens, output_tokens, cache_read_tokens,
                        reasoning_tokens, cost_micros, recorded_at
                 FROM usage_records WHERE session_id = ?1 ORDER BY recorded_at ASC, id ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([&session_id], |row| {
                let id: i64 = row.get(0)?;
                let turn: Option<String> = row.get(1)?;
                let model: String = row.get(2)?;
                let input: i64 = row.get(3)?;
                let output: i64 = row.get(4)?;
                let cache_read: i64 = row.get(5)?;
                let reasoning: i64 = row.get(6)?;
                let cost_micros: i64 = row.get(7)?;
                let recorded_at: String = row.get(8)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"id\":{},\"turnId\":{},\"model\":{},\"inputTokens\":{},\"outputTokens\":{},\"cacheReadTokens\":{},\"reasoningTokens\":{},\"costUsd\":{:.4},\"recordedAt\":{}}}",
                    id, json_str(&turn.unwrap_or_default()), json_str(&model), input, output, cache_read, reasoning, cost_usd, json_str(&recorded_at)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"sessionId\":{},\"records\":[{}]}}", json_str(&session_id), items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}
