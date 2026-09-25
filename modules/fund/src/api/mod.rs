//! HTTP API 路由与业务请求处理（拆分自 main.rs 以满足 R-B9 与模块边界规范）。

mod drawings;
mod import_api;
mod nav_api;
mod positions;
mod proxy;
mod watchlists;

pub(crate) use proxy::proxy_trends_engine;

use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

use drawings::{api_delete_drawing, api_drawings_list, api_save_drawing};
use import_api::{api_import_commit, api_import_preview};
use nav_api::{api_nav_list, api_nav_sync};
use positions::{
    api_accounts, api_create_account, api_history, api_positions, api_record_transaction,
    api_transactions,
};
use watchlists::{api_add_watchlist_item, api_create_watchlist, api_remove_watchlist_item, api_watchlists};

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

fn query_param_from_path<'a>(path: &'a str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}
