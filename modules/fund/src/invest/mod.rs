//! 投资账本 V2：账户 / 标的 / 流水 / 持仓 / 快照。
//!
//! 与既有 `portfolio`／`ledger` 等模块的关系：**并存，互不依赖**。
//! 全部接口挂在 `/api/v2/` 前缀下，由 `api::handle_api_request` 优先分发；
//! 旧路由继续工作，直到 P5 阶段统一下线。
//!
//! 设计文档：`docs/development/invest-v2-design.md`

pub mod ai;
pub mod backfill;
pub mod calendar;
pub mod crud;
pub mod flows;
pub mod fx;
pub mod holdings;
pub mod model;
pub mod overview;
pub mod quotes;
pub mod trading;

#[cfg(test)]
mod tests;

use crate::finance::FinanceError;
use crate::storage::Store;
use crate::wire::api_error;
use app_runtime_core::http::HttpRequest;
use serde::de::DeserializeOwned;
use serde::Serialize;

fn json<T: Serialize>(value: &T) -> Result<String, FinanceError> {
    serde_json::to_string(value).map_err(|e| FinanceError::Db(format!("序列化失败：{e}")))
}

fn body<T: DeserializeOwned>(request: &HttpRequest) -> Result<T, FinanceError> {
    serde_json::from_slice(&request.body)
        .map_err(|e| FinanceError::Invalid(format!("请求体 JSON 无效：{e}")))
}

fn parse_id(text: &str) -> Result<i64, FinanceError> {
    text.parse::<i64>()
        .map_err(|_| FinanceError::Invalid(format!("ID 非法：{text}")))
}

fn map_err(err: FinanceError) -> (u16, String) {
    match err {
        FinanceError::NotFound(m) => api_error(404, "FIN_V2_NOT_FOUND", &m),
        FinanceError::Invalid(m) => api_error(400, "FIN_V2_INVALID", &m),
        FinanceError::Conflict(m) => api_error(409, "FIN_V2_CONFLICT", &m),
        FinanceError::Overflow(m) => api_error(400, "FIN_V2_OVERFLOW", &m),
        FinanceError::Db(m) => api_error(500, "FIN_V2_DB", &m),
    }
}

/// `/api/v2/*` 的入口。返回 JSON 字符串或 `(status, body)` 错误。
pub fn handle(store: &Store, request: &HttpRequest, route: &str) -> Result<String, (u16, String)> {
    let rest = route.trim_start_matches("/api/v2");
    let segs: Vec<&str> = rest
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    dispatch(store, request, &segs).map_err(map_err)
}

fn dispatch(store: &Store, request: &HttpRequest, segs: &[&str]) -> Result<String, FinanceError> {
    match (request.method.as_str(), segs) {
        // ---------- 账户 ----------
        ("GET", ["accounts"]) => json(&crud::list_accounts(store)?),
        ("POST", ["accounts"]) => json(&crud::create_account(store, &body(request)?)?),
        ("PATCH", ["accounts", id]) => json(&crud::update_account(
            store,
            parse_id(id)?,
            &body(request)?,
        )?),
        // 账户不做物理删除：流水依赖它，归档即可从选择器里消失
        ("DELETE", ["accounts", id]) => {
            let mut payload: model::UpdateAccount = body(request).unwrap_or(model::UpdateAccount {
                name: None,
                note: None,
                archived: None,
            });
            payload.archived = Some(true);
            json(&crud::update_account(store, parse_id(id)?, &payload)?)
        }

        // ---------- 标的 ----------
        ("GET", ["instruments"]) => json(&crud::list_instruments(store)?),
        ("POST", ["instruments"]) => json(&crud::create_instrument(store, &body(request)?)?),
        ("PATCH", ["instruments", id]) => json(&crud::update_instrument(
            store,
            parse_id(id)?,
            &body(request)?,
        )?),
        ("DELETE", ["instruments", id]) => {
            let mut payload: model::UpdateInstrument =
                body(request).unwrap_or(model::UpdateInstrument {
                    name: None,
                    group_code: None,
                    share_class: None,
                    archived: None,
                });
            payload.archived = Some(true);
            json(&crud::update_instrument(store, parse_id(id)?, &payload)?)
        }

        // ---------- 流水 ----------
        ("GET", ["flows"]) => {
            let account = crate::market::query_param(&request.path, "account")
                .and_then(|v| v.parse::<i64>().ok());
            let instrument = crate::market::query_param(&request.path, "instrument")
                .and_then(|v| v.parse::<i64>().ok());
            json(&flows::list_flows(store, account, instrument)?)
        }
        ("POST", ["flows"]) => json(&flows::create_flow(store, &body(request)?)?),
        ("DELETE", ["flows", id]) => {
            flows::delete_flow(store, parse_id(id)?)?;
            Ok("{\"ok\":true}".to_string())
        }

        // ---------- 持仓与总览 ----------
        ("GET", ["holdings"]) => json(&holdings::list_holdings(store)?),
        ("GET", ["overview"]) => {
            // 打开总览时顺手沉淀当天快照：模块无常驻进程，不能依赖 cron。
            let _ = overview::write_today_snapshot(store);
            json(&overview::build(store)?)
        }
        ("GET", ["snapshots"]) => {
            let limit = crate::market::query_param(&request.path, "limit")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(365);
            json(&holdings::list_snapshots(store, limit)?)
        }

        // ---------- 行情 ----------
        // 只刷当前持仓标的，不拉全市场；失败不覆盖旧价。
        // 汇率作为附带动作：失败不阻断行情结果，最多让外币持仓继续标「未折算」。
        ("POST", ["quotes", "refresh"]) => {
            // 先刷交易日历与开闭市状态：后面的 stale 判断依赖它，
            // 否则刚拉到的周末收盘价会被误判成过期。
            let calendar_result = calendar::refresh(store, 120).ok();
            let quote_result = quotes::refresh_all(store)?;
            // 汇率是附带动作：失败不阻断行情结果，最多让外币持仓继续标「未折算」
            let fx_result = fx::refresh_all(store).ok();
            let payload = serde_json::json!({
                "updated": quote_result.updated,
                "missing": quote_result.missing,
                "skipped": quote_result.skipped,
                "errors": quote_result.errors,
                "trading": quote_result.trading,
                "fx": fx_result,
                "calendar": calendar_result,
            });
            Ok(payload.to_string())
        }
        // 单独刷汇率，便于只关心折算的场景
        ("POST", ["fx", "refresh"]) => json(&fx::refresh_all(store)?),
        // 补算缺失的历史快照；不覆盖已有日期。
        ("POST", ["snapshots", "backfill"]) => {
            let days = crate::market::query_param(&request.path, "days")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(backfill::MAX_BACKFILL_DAYS);
            json(&backfill::backfill(store, days)?)
        }

        // ---------- AI ----------
        // 模块不发起推理（ADR-0037/0044）：只组装输入、保存结果。
        ("GET", ["ai", "settings"]) => json(&ai::read_settings(store)?),
        ("PUT", ["ai", "settings"]) => json(&ai::save_settings(store, &body(request)?)?),
        ("GET", ["ai", "context"]) => {
            let account = crate::market::query_param(&request.path, "account")
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or_else(|| FinanceError::Invalid("缺少 account 参数".into()))?;
            let instrument = crate::market::query_param(&request.path, "instrument")
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or_else(|| FinanceError::Invalid("缺少 instrument 参数".into()))?;
            json(&ai::build_context(store, account, instrument)?)
        }
        ("GET", ["ai", "evaluations"]) => {
            let account = crate::market::query_param(&request.path, "account")
                .and_then(|v| v.parse::<i64>().ok());
            let instrument = crate::market::query_param(&request.path, "instrument")
                .and_then(|v| v.parse::<i64>().ok());
            json(&ai::list_evaluations(store, account, instrument)?)
        }
        ("POST", ["ai", "evaluations"]) => json(&ai::save_evaluation(store, &body(request)?)?),
        ("GET", ["ai", "trend"]) => {
            let account = crate::market::query_param(&request.path, "account")
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or_else(|| FinanceError::Invalid("缺少 account 参数".into()))?;
            let instrument = crate::market::query_param(&request.path, "instrument")
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or_else(|| FinanceError::Invalid("缺少 instrument 参数".into()))?;
            let limit = crate::market::query_param(&request.path, "limit")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(30);
            json(&ai::trend(store, account, instrument, limit)?)
        }

        _ => Err(FinanceError::NotFound(format!(
            "未知接口 {} /api/v2/{}",
            request.method,
            segs.join("/")
        ))),
    }
}
