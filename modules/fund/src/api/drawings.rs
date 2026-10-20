//! HTTP API 业务请求处理（拆分自 api.rs）。
//! mode/T04 审计更正：业务与存储操作收口到 crate::chart_drawings 领域模块；
//! 本文件只做传输解析（query/body）与错误 wire 映射，不含业务规则。

use super::{api_error, query_param_from_path};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

fn wire(code: &'static str, message: String) -> (u16, String) {
    let status = match code {
        "DRAWINGS_DB" => 500,
        "DRAWINGS_CONFLICT" => 409,
        _ => 400,
    };
    api_error(status, code, &message)
}

pub(crate) fn api_drawings_list(
    store: &Store,
    request: &HttpRequest,
) -> Result<String, (u16, String)> {
    let raw_symbol = query_param_from_path(&request.path, "symbol")
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "symbol required"))?;
    if raw_symbol.chars().count() > 48 {
        return Err(api_error(
            400,
            "APP_PARAM_INVALID",
            "symbol length must be 1..=48",
        ));
    }
    let period =
        query_param_from_path(&request.path, "period").unwrap_or_else(|| "day".to_string());
    // symbol 归一化与 period 合法性由领域校验（save 与 list 同一规则）。
    let symbol = crate::market::normalize_symbol(&raw_symbol);
    if symbol.is_empty() {
        return Err(api_error(
            400,
            "APP_PARAM_INVALID",
            "symbol length must be 1..=32",
        ));
    }
    crate::chart_drawings::list(store, &symbol, &period).map_err(|e| wire(e.code(), e.message()))
}

pub(crate) fn api_save_drawing(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let val: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| api_error(400, "APP_PARAM_INVALID", "invalid JSON body"))?;
    let draft = crate::chart_drawings::DrawingDraft::from_raw(&val)
        .map_err(|e| wire(e.code(), e.message()))?;
    // 与 migration.rs 相同口径：标准库时间戳，不引入 chrono 依赖。
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now = format!("{now_secs}");
    crate::chart_drawings::save(store, &draft, &now).map_err(|e| wire(e.code(), e.message()))
}

pub(crate) fn api_delete_drawing(
    store: &Store,
    request: &HttpRequest,
) -> Result<String, (u16, String)> {
    let id = query_param_from_path(&request.path, "id")
        .ok_or_else(|| api_error(400, "APP_PARAM_INVALID", "id required"))?;
    if id.is_empty() || id.chars().count() > 64 {
        return Err(api_error(
            400,
            "APP_PARAM_INVALID",
            "id length must be 1..=64",
        ));
    }
    let expected = query_param_from_path(&request.path, "expectedDigest");
    crate::chart_drawings::delete(store, &id, expected.as_deref())
        .map_err(|e| wire(e.code(), e.message()))?;
    Ok("{\"success\":true}".into())
}
