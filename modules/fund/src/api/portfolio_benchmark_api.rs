//! Selected-range benchmark endpoint; market IO stays outside the ledger lock.
use super::home_api::err;
use crate::{
    finance::{self, FinanceError},
    storage::Store,
};
use serde_json::Value;

pub(crate) fn api_portfolio_benchmark(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let revision = crate::market::query_param(path, "dataRevision")
        .filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| super::api_error(400, "APP_PARAM_INVALID", "缺少有效 dataRevision"))?;
    let symbol = match crate::market::query_param(path, "symbol") {
        Some(symbol) => symbol,
        None => crate::settings::get(store).map_err(|e| err(500, &e))?["benchmarkSymbol"]
            .as_str()
            .unwrap_or("sh000300")
            .into(),
    };
    if !crate::portfolio_benchmark::SYMBOLS.contains(&symbol.as_str()) {
        return Err(super::api_error(400, "APP_PARAM_INVALID", "不支持该基准"));
    }
    let today = finance::today_cn();
    let from = crate::market::query_param(path, "from")
        .unwrap_or_else(|| crate::assets::minus_days(&today, 60));
    let to = crate::market::query_param(path, "to").unwrap_or_else(|| today.clone());
    if (!from.is_empty() && !crate::assets::is_valid_calendar_date(&from))
        || !crate::assets::is_valid_calendar_date(&to)
        || from > to
        || to > today
    {
        return Err(super::api_error(
            400,
            "FIN_INVALID",
            "Invalid benchmark range",
        ));
    }

    let read_history = || {
        store
            .with_snapshot(|c| -> Result<Value, FinanceError> {
                if Store::data_revision(c)? != revision {
                    return Err(FinanceError::Conflict("账本已变化，请刷新概览".into()));
                }
                crate::overview::history_range(c, &from, &to)
            })
            .map_err(|e| {
                err(
                    if matches!(e, FinanceError::Conflict(_)) {
                        409
                    } else {
                        500
                    },
                    &e,
                )
            })
    };
    let history = read_history()?;
    if history["points"].as_array().map_or(true, |p| p.len() < 2) {
        return Ok(
            serde_json::json!({"dataRevision":revision,"symbol":symbol,"status":"incomplete",
            "reason":"missing_baseline","points":[],"excessPpm":null})
            .to_string(),
        );
    }
    // 公共行情抓取在锁外，复用已有固定地址和 TTL，不传用户资料。
    let request = app_runtime_core::http::HttpRequest::for_internal(
        "GET",
        &format!("/api/market/kline?symbol={symbol}&period=day&fq=bf"),
        Vec::new(),
    );
    let raw = crate::market::api_market_kline(&request)?;
    let market: Value = serde_json::from_str(&raw)
        .map_err(|_| super::api_error(502, "APP_UPSTREAM", "基准数据格式无效"))?;
    let mut result = store
        .with_snapshot(|c| -> Result<Value, FinanceError> {
            if Store::data_revision(c)? != revision {
                return Err(FinanceError::Conflict("账本已变化，请刷新概览".into()));
            }
            crate::portfolio_benchmark::compare_range(c, &today, &market, &from, &to)
        })
        .map_err(|e| {
            err(
                match e {
                    FinanceError::Conflict(_) => 409,
                    FinanceError::Invalid(_) => 502,
                    _ => 500,
                },
                &e,
            )
        })?;
    result["dataRevision"] = revision.into();
    result["fetchedAt"] = finance::now_secs().into();
    Ok(result.to_string())
}
