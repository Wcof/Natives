//! HTTP API 业务请求处理（拆分自 api.rs，仅移动，无逻辑变更）。

use super::{api_error, json_str, query_param};
use crate::fixed::Fixed;
use crate::nav;
use crate::storage::Store;

pub(crate) fn api_nav_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let code =
        query_param(path, "code").ok_or_else(|| api_error(400, "NAV_BAD_PAYLOAD", "code 缺失"))?;
    // DES-D05：默认范围最近 30 条；可选 beforeDate+limit(1..100) 有界「查看更多」，
    // 返回 nextCursor；无更多数据时为 null。旧无 cursor 调用保持 30 条语义。
    let rows;
    let next_cursor;
    if query_param(path, "beforeDate").is_some() || query_param(path, "limit").is_some() {
        let before = query_param(path, "beforeDate");
        let limit = query_param(path, "limit")
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or_else(|| api_error(400, "NAV_BAD_PAYLOAD", "limit 无效"))?;
        let (page, cursor) = nav::list_nav_page(store, &code, before.as_deref(), limit)
            .map_err(|e| api_error(502, e.code(), &e.message()))?;
        rows = page;
        next_cursor = cursor;
    } else {
        rows =
            nav::list_nav(store, &code, 30).map_err(|e| api_error(502, e.code(), &e.message()))?;
        next_cursor = None;
    }
    let items:Vec<String>=rows.iter().map(|n|serde_json::json!({"fundCode":n.fund_code,"navDate":n.nav_date,"unitNav":Fixed::nav(n.unit_nav_raw).to_string_value(),"dailyGrowthPct":n.daily_growth_raw.map(|v|Fixed::nav(v).to_string_value()),"source":n.source,"fetchedAt":n.fetched_at}).to_string()).collect();

    Ok(format!(
        "{{\"nav\":[{}],\"nextCursor\":{}}}",
        items.join(","),
        next_cursor
            .map(|d| json_str(&d))
            .unwrap_or_else(|| "null".into()),
    ))
}

pub(crate) fn api_nav_sync(store: &Store, body: &str) -> Result<String, (u16, String)> {
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

pub(crate) fn api_quarterly_positions(path: &str) -> Result<String, (u16, String)> {
    let code =
        query_param(path, "code").ok_or_else(|| api_error(400, "NAV_BAD_PAYLOAD", "code 缺失"))?;
    let report = nav::EastMoneySource::fetch_quarterly_positions(&code)
        .map_err(|e| api_error(502, e.code(), &e.message()))?;
    serde_json::to_string(&report).map_err(|e| api_error(500, "APP_INTERNAL", &e.to_string()))
}

pub(crate) fn api_nav_detail_history(path: &str) -> Result<String, (u16, String)> {
    let code = query_param(path, "code")
        .filter(|s| s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| api_error(400, "NAV_BAD_PAYLOAD", "基金代码无效"))?;
    let rows = nav::EastMoneySource::fetch_detail_history(&code)
        .map_err(|e| api_error(502, e.code(), &e.message()))?;
    let values:Vec<_>=rows.iter().take(400).map(|r|serde_json::json!({"fundCode":r.fund_code,"navDate":r.nav_date,"unitNav":Fixed::nav(r.unit_nav_raw).to_string_value(),"dailyGrowthPct":r.daily_growth_raw.map(|v|Fixed::nav(v).to_string_value()),"source":r.source,"fetchedAt":r.fetched_at})).collect();
    Ok(serde_json::json!({"nav":values,"nextCursor":null}).to_string())
}
