//! HTTP API 业务请求处理（拆分自 api.rs）。
//! mode/T04 审计更正：业务与数据操作收口到 crate::watchlists 领域模块；
//! 本文件只做传输解析（JSON body）与错误 wire 映射，不含业务规则。

use super::{api_error, query_param_from_path};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

fn wire(code: &'static str, message: String) -> (u16, String) {
    let status = match code {
        "WATCHLIST_DB" => 500,
        "WATCHLIST_CONFLICT" => 409,
        _ => 400,
    };
    api_error(status, code, &message)
}

pub(crate) fn api_watchlists(store: &Store) -> Result<String, (u16, String)> {
    crate::watchlists::list(store).map_err(|e| wire(e.code(), e.message()))
}

pub(crate) fn api_create_watchlist(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let val: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| api_error(400, "APP_PARAM_INVALID", "invalid JSON body"))?;
    let draft = crate::watchlists::WatchlistDraft::from_raw(&val)
        .map_err(|e| wire(e.code(), e.message()))?;
    let now = crate::watchlists::now_rfc3339();
    crate::watchlists::create(store, &draft, &now).map_err(|e| wire(e.code(), e.message()))?;
    Ok("{\"success\":true}".into())
}

pub(crate) fn api_add_watchlist_item(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let val: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| api_error(400, "APP_PARAM_INVALID", "invalid JSON body"))?;
    let draft = crate::watchlists::WatchlistItemDraft::from_raw(&val)
        .map_err(|e| wire(e.code(), e.message()))?;
    let now = crate::watchlists::now_rfc3339();
    crate::watchlists::add_item(store, &draft, &now).map_err(|e| wire(e.code(), e.message()))?;
    Ok(format!(
        "{{\"success\":true,\"symbol\":{}}}",
        crate::wire::json_str(&draft.symbol)
    ))
}

pub(crate) fn api_remove_watchlist_item(
    store: &Store,
    request: &HttpRequest,
) -> Result<String, (u16, String)> {
    let wid = query_param_from_path(&request.path, "watchlistId")
        .unwrap_or_else(|| "default".to_string());
    if wid.is_empty() || wid.chars().count() > 64 {
        return Err(api_error(
            400,
            "APP_PARAM_INVALID",
            "watchlistId length must be 1..=64",
        ));
    }
    let raw_symbol = query_param_from_path(&request.path, "symbol")
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "symbol required"))?;
    if raw_symbol.chars().count() > 48 {
        return Err(api_error(
            400,
            "APP_PARAM_INVALID",
            "symbol length must be 1..=48",
        ));
    }
    let symbol = crate::watchlists::WatchlistItemDraft::from_raw(&serde_json::json!({
        "watchlistId": wid,
        "symbol": raw_symbol,
    }))
    .map_err(|e| wire(e.code(), e.message()))?;
    crate::watchlists::remove_item(store, &symbol.watchlist_id, &symbol.symbol)
        .map_err(|e| wire(e.code(), e.message()))?;
    Ok("{\"success\":true}".into())
}

pub(crate) fn api_manage_watchlist(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = serde_json::from_str(body)
        .map_err(|_| api_error(400, "APP_PARAM_INVALID", "invalid JSON body"))?;
    crate::watchlists::manage(store, &value)
        .map_err(|error| wire(error.code(), error.message()))?;
    Ok("{\"success\":true}".into())
}
