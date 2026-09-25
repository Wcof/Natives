//! HTTP API 业务请求处理（拆分自 api.rs，仅移动，无逻辑变更）。

use super::{api_error, json_str, query_param_from_path};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

pub(crate) fn api_watchlists(store: &Store) -> Result<String, (u16, String)> {
    store
        .with_read(|conn: &rusqlite::Connection| -> rusqlite::Result<String> {
            let mut g_stmt = conn.prepare(
                "SELECT id, name, sort_order, created_at FROM watchlists ORDER BY sort_order ASC, created_at ASC",
            )?;
            let groups: Vec<String> = g_stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    let sort_order: i64 = row.get(2)?;
                    let created_at: String = row.get(3)?;
                    Ok(format!(
                        "{{\"id\":{},\"name\":{},\"sortOrder\":{},\"createdAt\":{}}}",
                        json_str(&id),
                        json_str(&name),
                        sort_order,
                        json_str(&created_at)
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            let mut i_stmt = conn.prepare(
                "SELECT watchlist_id, symbol, name, asset_type, sort_order, created_at FROM watchlist_items ORDER BY sort_order ASC, created_at ASC",
            )?;
            let items: Vec<String> = i_stmt
                .query_map([], |row| {
                    let wid: String = row.get(0)?;
                    let symbol: String = row.get(1)?;
                    let name: String = row.get(2)?;
                    let asset_type: String = row.get(3)?;
                    let sort_order: i64 = row.get(4)?;
                    let created_at: String = row.get(5)?;
                    Ok(format!(
                        "{{\"watchlistId\":{},\"symbol\":{},\"name\":{},\"assetType\":{},\"sortOrder\":{},\"createdAt\":{}}}",
                        json_str(&wid),
                        json_str(&symbol),
                        json_str(&name),
                        json_str(&asset_type),
                        sort_order,
                        json_str(&created_at)
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(format!(
                "{{\"groups\":[{}],\"items\":[{}]}}",
                groups.join(","),
                items.join(",")
            ))
        })
        .map_err(|e| api_error(500, "WATCHLIST_DB", &e.to_string()))
}

pub(crate) fn api_create_watchlist(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let val: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| api_error(400, "APP_PARAM_INVALID", &e.to_string()))?;
    let id = val.get("id").and_then(|v| v.as_str()).unwrap_or("default");
    let name = val
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("默认自选");
    let sort_order = val.get("sortOrder").and_then(|v| v.as_i64()).unwrap_or(0);
    let now = "2026-09-17T00:00:00Z";

    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO watchlists(id, name, sort_order, created_at)
                 VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET name=?2, sort_order=?3",
                [id, name, &sort_order.to_string(), now],
            )?;
            Ok(())
        })
        .map_err(|e| api_error(500, "WATCHLIST_DB", &e.to_string()))?;

    Ok("{\"success\":true}".into())
}

pub(crate) fn api_add_watchlist_item(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let val: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| api_error(400, "APP_PARAM_INVALID", &e.to_string()))?;
    let wid = val
        .get("watchlistId")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let raw_symbol = val
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "symbol required"))?;
    let symbol = crate::market::normalize_symbol(raw_symbol);
    let name = val.get("name").and_then(|v| v.as_str()).unwrap_or(&symbol);
    let asset_type = val
        .get("assetType")
        .and_then(|v| v.as_str())
        .unwrap_or("stock");
    let sort_order = val.get("sortOrder").and_then(|v| v.as_i64()).unwrap_or(0);
    let now = "2026-09-17T00:00:00Z";

    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            // 确保对应 watchlist 存在
            conn.execute(
                "INSERT OR IGNORE INTO watchlists(id, name, sort_order, created_at) VALUES(?1, '默认自选', 0, ?2)",
                [wid, now],
            )?;
            conn.execute(
                "INSERT INTO watchlist_items(watchlist_id, symbol, name, asset_type, sort_order, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(watchlist_id, symbol) DO UPDATE SET name=?3, asset_type=?4, sort_order=?5",
                [wid, &symbol, name, asset_type, &sort_order.to_string(), now],
            )?;
            Ok(())
        })
        .map_err(|e| api_error(500, "WATCHLIST_DB", &e.to_string()))?;

    Ok(format!(
        "{{\"success\":true,\"symbol\":{}}}",
        json_str(&symbol)
    ))
}

pub(crate) fn api_remove_watchlist_item(
    store: &Store,
    request: &HttpRequest,
) -> Result<String, (u16, String)> {
    let wid = query_param_from_path(&request.path, "watchlistId")
        .unwrap_or_else(|| "default".to_string());
    let raw_symbol = query_param_from_path(&request.path, "symbol")
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "symbol required"))?;
    let symbol = crate::market::normalize_symbol(&raw_symbol);

    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "DELETE FROM watchlist_items WHERE watchlist_id=?1 AND symbol=?2",
                [&wid, &symbol],
            )?;
            Ok(())
        })
        .map_err(|e| api_error(500, "WATCHLIST_DB", &e.to_string()))?;

    Ok("{\"success\":true}".into())
}
