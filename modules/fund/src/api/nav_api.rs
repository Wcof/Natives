//! HTTP API 业务请求处理（拆分自 api.rs，仅移动，无逻辑变更）。

use super::{api_error, json_str, query_param};
use crate::fixed::Fixed;
use crate::nav;
use crate::storage::Store;

pub(crate) fn api_nav_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
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
