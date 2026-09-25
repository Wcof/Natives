//! HTTP API 业务请求处理（拆分自 api.rs，仅移动，无逻辑变更）。

use super::{api_error, json_str, query_param_from_path};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

pub(crate) fn api_drawings_list(store: &Store, request: &HttpRequest) -> Result<String, (u16, String)> {
    let raw_symbol = query_param_from_path(&request.path, "symbol")
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "symbol required"))?;
    let symbol = crate::market::normalize_symbol(&raw_symbol);
    let period =
        query_param_from_path(&request.path, "period").unwrap_or_else(|| "day".to_string());

    store
        .with_read(|conn: &rusqlite::Connection| -> rusqlite::Result<String> {
            let mut stmt = conn.prepare(
                "SELECT id, tool_type, points_json, options_json, updated_at FROM chart_drawings WHERE symbol=?1 AND period=?2 ORDER BY created_at ASC",
            )?;
            let drawings: Vec<String> = stmt
                .query_map([&symbol, &period], |row| {
                    let id: String = row.get(0)?;
                    let tool_type: String = row.get(1)?;
                    let points: String = row.get(2)?;
                    let options: String = row.get(3)?;
                    let updated_at: String = row.get(4)?;
                    Ok(format!(
                        "{{\"id\":{},\"symbol\":{},\"period\":{},\"toolType\":{},\"points\":{},\"options\":{},\"updatedAt\":{}}}",
                        json_str(&id),
                        json_str(&symbol),
                        json_str(&period),
                        json_str(&tool_type),
                        points,
                        options,
                        json_str(&updated_at)
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(format!("{{\"drawings\":[{}]}}", drawings.join(",")))
        })
        .map_err(|e| api_error(500, "DRAWINGS_DB", &e.to_string()))
}

pub(crate) fn api_save_drawing(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let val: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| api_error(400, "APP_PARAM_INVALID", &e.to_string()))?;
    let id = val
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "id required"))?;
    let raw_symbol = val
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "symbol required"))?;
    let symbol = crate::market::normalize_symbol(raw_symbol);
    let period = val.get("period").and_then(|v| v.as_str()).unwrap_or("day");
    let tool_type = val
        .get("toolType")
        .and_then(|v| v.as_str())
        .unwrap_or("trendline");
    let points = val
        .get("points")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "[]".into());
    let options = val
        .get("options")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".into());
    let now = "2026-09-17T00:00:00Z";

    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO chart_drawings(id, symbol, period, tool_type, points_json, options_json, created_at, updated_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(id) DO UPDATE SET points_json=?5, options_json=?6, updated_at=?7",
                [id, &symbol, period, tool_type, &points, &options, now],
            )?;
            Ok(())
        })
        .map_err(|e| api_error(500, "DRAWINGS_DB", &e.to_string()))?;

    Ok("{\"success\":true}".into())
}

pub(crate) fn api_delete_drawing(store: &Store, request: &HttpRequest) -> Result<String, (u16, String)> {
    let id = query_param_from_path(&request.path, "id")
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "id required"))?;

    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute("DELETE FROM chart_drawings WHERE id=?1", [&id])?;
            Ok(())
        })
        .map_err(|e| api_error(500, "DRAWINGS_DB", &e.to_string()))?;

    Ok("{\"success\":true}".into())
}
