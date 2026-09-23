//! HTTP API 路由与业务请求处理（拆分自 main.rs 以满足 R-B9 与模块边界规范）。

use crate::fixed::{Fixed, SCALE_SHARE};
use crate::import;
use crate::ledger;
use crate::nav;
use crate::portfolio;
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;
use rusqlite::OptionalExtension;
use std::time::Duration;

pub fn handle_api_request(
    store: &Store,
    request: &HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    if route.starts_with("/api/trends") {
        return proxy_trends_engine(request, route);
    }

    let body_text = String::from_utf8_lossy(&request.body).to_string();
    let method = request.method.as_str();
    match (method, route) {
        ("GET", "/api/positions") => api_positions(store),
        ("GET", "/api/transactions") => api_transactions(store),
        ("POST", "/api/transactions") => api_record_transaction(store, &body_text),
        ("GET", "/api/nav") => api_nav_list(store, request.path.as_str()),
        ("POST", "/api/nav/sync") => api_nav_sync(store, &body_text),
        ("GET", "/api/history") => api_history(store, request.path.as_str()),
        ("POST", "/api/import/preview") => api_import_preview(&body_text),
        ("POST", "/api/import/commit") => api_import_commit(store, &body_text),
        ("GET", "/api/accounts") => api_accounts(store),
        ("POST", "/api/accounts") => api_create_account(store, &body_text),
        // 首页「行情」：真实上游代理（东财/腾讯），无 store 依赖。
        ("GET", "/api/market/indices") => crate::market::api_market_indices(request),
        ("GET", "/api/market/stocks") => crate::market::api_market_stocks(request),
        ("GET", "/api/market/etfs") => crate::market::api_market_etfs(request),
        ("GET", "/api/market/sectors") => crate::market::api_market_sectors(request),
        ("GET", "/api/market/sector/stocks") => crate::market::api_market_sector_stocks(request),
        ("GET", "/api/market/themes") => crate::market::api_market_themes(),
        ("GET", "/api/market/theme/etfs") => crate::market::api_market_quotes(request),
        ("GET", "/api/market/quotes") => crate::market::api_market_quotes(request),
        ("GET", "/api/market/detail") => crate::market::api_market_detail(request),
        ("GET", "/api/market/suggest") => crate::market::api_market_suggest(request),
        ("GET", "/api/market/minute") => crate::market::api_market_minute(request),
        ("GET", "/api/market/kline") => crate::market::api_market_kline(request),
        // 自选管理
        ("GET", "/api/watchlists") => api_watchlists(store),
        ("POST", "/api/watchlists") => api_create_watchlist(store, &body_text),
        ("POST", "/api/watchlists/items") => api_add_watchlist_item(store, &body_text),
        ("DELETE", "/api/watchlists/items") => api_remove_watchlist_item(store, request),
        // 原生 Canvas 划线数据持久化
        ("GET", "/api/drawings") => api_drawings_list(store, request),
        ("POST", "/api/drawings") => api_save_drawing(store, &body_text),
        ("DELETE", "/api/drawings") => api_delete_drawing(store, request),
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

fn api_positions(store: &Store) -> Result<String, (u16, String)> {
    let rows = store
        .with_read(portfolio::list_positions)
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?;
    let items: Vec<String> = rows
        .iter()
        .map(|p| {
            let quantity = Fixed::nav(p.quantity_raw);
            let cost = Fixed::amount(p.cost_raw);
            format!(
                "{{\"account\":{},\"fundCode\":{},\"fundName\":{},\"quantity\":{},\"cost\":{},\"realized\":{},\"asOf\":{}}}",
                json_str(&p.account_name),
                json_str(&p.fund_code),
                json_str(&p.fund_name),
                json_str(&quantity.to_string_value()),
                json_str(&cost.to_string_value()),
                json_str(&Fixed::amount(p.realized_raw).to_string_value()),
                json_str(&p.as_of),
            )
        })
        .collect();
    Ok(format!("{{\"positions\":[{}]}}", items.join(",")))
}

fn api_transactions(store: &Store) -> Result<String, (u16, String)> {
    let rows = ledger::list_transactions(store, 200)
        .map_err(|e| api_error(500, e.code(), &e.message()))?;
    let items: Vec<String> = rows
        .iter()
        .map(|t| {
            format!(
                "{{\"id\":{},\"account\":{},\"fundCode\":{},\"type\":{},\"state\":{},\"quantity\":{},\"price\":{},\"amount\":{},\"fee\":{},\"tradeDate\":{},\"source\":{}}}",
                t.id,
                json_str(&t.account_name),
                json_str(&t.fund_code),
                json_str(&t.tx_type),
                json_str(&t.state),
                json_str(&Fixed::nav(t.quantity_raw).to_string_value()),
                json_str(&Fixed::nav(t.price_raw).to_string_value()),
                json_str(&Fixed::amount(t.amount_raw).to_string_value()),
                json_str(&Fixed::amount(t.fee_raw).to_string_value()),
                json_str(&t.trade_date),
                json_str(&t.source),
            )
        })
        .collect();
    Ok(format!("{{\"transactions\":[{}]}}", items.join(",")))
}

fn api_record_transaction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let get_str = |key: &str| -> Result<String, (u16, String)> {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", &format!("{key} 缺失")))
    };
    let parse_fixed = |key: &str, scale: u32| -> Result<Fixed, (u16, String)> {
        Fixed::parse(&get_str(key)?, scale)
            .map_err(|_| api_error(400, "LEDGER_INVALID", &format!("{key} 精度非法")))
    };
    let fee_fixed = value
        .get("fee")
        .and_then(|v| v.as_str())
        .map(|s| Fixed::parse(s, 2))
        .transpose()
        .map_err(|_| api_error(400, "LEDGER_INVALID", "fee 精度非法"))?
        .unwrap_or_else(|| Fixed::zero(2));

    let input = ledger::TransactionInput::manual(
        get_str("account")?,
        get_str("fundCode")?,
        value
            .get("fundName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        get_str("type")?,
        value
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or(ledger::STATE_CONFIRMED)
            .to_string(),
        parse_fixed("quantity", SCALE_SHARE)?,
        parse_fixed("price", SCALE_SHARE)?,
        fee_fixed,
        get_str("tradeDate")?,
        get_str("requestId")?,
    );
    let (id, created) = ledger::record_transaction(store, &input)
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"id\":{id},\"created\":{created}}}"))
}

fn api_nav_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let code =
        query_param(path, "code").ok_or_else(|| api_error(400, "NAV_BAD_PAYLOAD", "code 缺失"))?;
    let rows =
        nav::list_nav(store, &code, 30).map_err(|e| api_error(502, e.code(), &e.message()))?;
    let items: Vec<String> = rows
        .iter()
        .map(|n| {
            format!(
                "{{\"fundCode\":{},\"navDate\":{},\"unitNav\":{},\"source\":{},\"fetchedAt\":{}}}",
                json_str(&n.fund_code),
                json_str(&n.nav_date),
                json_str(&Fixed::nav(n.unit_nav_raw).to_string_value()),
                json_str(&n.source),
                json_str(&n.fetched_at),
            )
        })
        .collect();
    Ok(format!("{{\"nav\":[{}]}}", items.join(",")))
}

fn api_nav_sync(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| api_error(400, "NAV_BAD_PAYLOAD", &e.to_string()))?;
    let code = value
        .get("fundCode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "NAV_BAD_PAYLOAD", "fundCode 缺失"))?;
    let report = nav::sync_fund_nav(store, code, nav::EastMoneySource::fetch_history)
        .map_err(|e| api_error(502, e.code(), &e.message()))?;
    Ok(format!(
        "{{\"fundCode\":{},\"added\":{},\"updated\":{},\"source\":{},\"latestNavDate\":{}}}",
        json_str(&report.fund_code),
        report.added,
        report.updated,
        json_str(report.resolved_source),
        report
            .latest_nav_date
            .map(|d| json_str(&d))
            .unwrap_or_else(|| "null".into()),
    ))
}

fn api_history(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let account = query_param(path, "account")
        .ok_or_else(|| api_error(400, "PORTFOLIO_DB", "account 缺失"))?;
    let days: i64 = query_param(path, "days")
        .and_then(|d| d.parse().ok())
        .unwrap_or(30);
    // 找账户 id。
    let account_id: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id FROM accounts WHERE name = ?1",
                rusqlite::params![account],
                |r| r.get(0),
            )
            .optional()
        })
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?
        .ok_or_else(|| api_error(404, "LEDGER_NOT_FOUND", "账户不存在"))?;
    // 日期窗口（自然日）。
    let today = today_iso();
    let from = minus_days(&today, days);
    let points = store
        .with_read(|conn| portfolio::replay_history(conn, account_id, &from, &today))
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?;
    let items: Vec<String> = points
        .iter()
        .map(|p| {
            let value = p
                .value_raw
                .map(|raw| json_str(&Fixed::amount(raw).to_string_value()))
                .unwrap_or_else(|| "null".into());
            format!(
                "{{\"date\":{},\"value\":{},\"cost\":{}}}",
                json_str(&p.date),
                value,
                json_str(&Fixed::amount(p.cost_raw).to_string_value()),
            )
        })
        .collect();
    Ok(format!("{{\"history\":[{}]}}", items.join(",")))
}

fn api_import_preview(body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "IMPORT_INVALID", &e.to_string()))?;
    let template = value
        .get("templateVersion")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "templateVersion 缺失"))?;
    let csv = value
        .get("csv")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "csv 缺失"))?;
    if csv.len() > 5 * 1024 * 1024 {
        return Err(api_error(400, "IMPORT_INVALID", "文件超过 5MiB"));
    }
    let preview =
        import::preview(template, csv).map_err(|e| api_error(400, e.code(), &e.message()))?;
    let json =
        serde_json::to_string(&preview).map_err(|e| api_error(500, "IMPORT_DB", &e.to_string()))?;
    Ok(json)
}

fn api_import_commit(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "IMPORT_INVALID", &e.to_string()))?;
    let template = value
        .get("templateVersion")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "templateVersion 缺失"))?;
    let csv = value
        .get("csv")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "csv 缺失"))?
        .to_string();
    let source_id = value
        .get("sourceId")
        .and_then(|v| v.as_str())
        .unwrap_or("manual-upload")
        .to_string();
    if csv.len() > 5 * 1024 * 1024 {
        return Err(api_error(400, "IMPORT_INVALID", "文件超过 5MiB"));
    }
    let preview =
        import::preview(template, &csv).map_err(|e| api_error(400, e.code(), &e.message()))?;
    let file_hash = app_runtime_core::sha256_hex(csv.as_bytes());
    let params_hash = app_runtime_core::sha256_hex(template.to_string().as_bytes());
    let receipt = import::commit(
        store,
        &source_id,
        template,
        &file_hash,
        &params_hash,
        &preview,
    )
    .map_err(|e| api_error(409, e.code(), &e.message()))?;
    let json =
        serde_json::to_string(&receipt).map_err(|e| api_error(500, "IMPORT_DB", &e.to_string()))?;
    Ok(json)
}

fn api_accounts(store: &Store) -> Result<String, (u16, String)> {
    let names: Vec<String> = store
        .with_read(|conn| -> rusqlite::Result<Vec<String>> {
            let mut stmt = conn.prepare("SELECT name FROM accounts ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| api_error(500, "LEDGER_DB", &e.to_string()))?;
    let items: Vec<String> = names.iter().map(|n| json_str(n)).collect();
    Ok(format!("{{\"accounts\":[{}]}}", items.join(",")))
}

fn api_create_account(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "name 缺失"))?;
    let id = store
        .with_write(|conn| -> Result<i64, ledger::LedgerError> {
            ledger::ensure_account(conn, name)
        })
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"id\":{id}}}"))
}

// ---------- 小工具 ----------

fn query_param<'a>(path: &'a str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(v.to_string());
        }
    }
    None
}

/// 本地日期 YYYY-MM-DD（UTC+8 基准，人民币市场；本地记账以日为粒度无外部依赖）。
fn today_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    let days = secs / 86400;
    civil_from_days(days as i64)
}

fn minus_days(today: &str, days: i64) -> String {
    let y: i64 = today[0..4].parse().unwrap_or(2026);
    let m: i64 = today[5..7].parse().unwrap_or(1);
    let d: i64 = today[8..10].parse().unwrap_or(1);
    let to_days = |y: i64, m: i64, d: i64| -> i64 {
        // days-from-civil（Howard Hinnant 算法）。
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    };
    civil_from_days(to_days(y, m, d) - days)
}

fn civil_from_days(days: i64) -> String {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn api_watchlists(store: &Store) -> Result<String, (u16, String)> {
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

fn api_create_watchlist(store: &Store, body: &str) -> Result<String, (u16, String)> {
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

fn api_add_watchlist_item(store: &Store, body: &str) -> Result<String, (u16, String)> {
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

fn api_remove_watchlist_item(
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

fn api_drawings_list(store: &Store, request: &HttpRequest) -> Result<String, (u16, String)> {
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

fn api_save_drawing(store: &Store, body: &str) -> Result<String, (u16, String)> {
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

fn api_delete_drawing(store: &Store, request: &HttpRequest) -> Result<String, (u16, String)> {
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

fn query_param_from_path<'a>(path: &'a str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

fn proxy_trends_engine(request: &HttpRequest, route: &str) -> Result<String, (u16, String)> {
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
