//! 场外基金操作 API（N09）：HTTP 入参解析 → otc_holding／otc_ops／otc_plans
//! 域函数 → JSON。错误统一经 wire::api_error 映射（400 参数、409 冲突／
//! 幂等、404 未找到）；写事务与 data_revision 推进由 Store.with_write 承载。

use crate::fixed::{Fixed, SCALE_SHARE};
use crate::otc_holding::{fund_id_by_code, holding_create, holding_edit, holding_edit_preview};
use crate::otc_ops::{
    order_arrival, order_cancel, order_confirm, order_create, orders_list, OrderConfirmInput,
    OrderDraft,
};
use crate::otc_plans::{
    plan_create, plan_run_register, plan_runs, plan_update, plans_list, PlanInput,
};
use crate::storage::Store;
use crate::wire::api_error;
use serde_json::{json, Value};

type ApiResult = Result<String, (u16, String)>;

fn parse_body(body: &str) -> Result<Value, (u16, String)> {
    serde_json::from_str(body).map_err(|e| api_error(400, "OTC_INVALID", &e.to_string()))
}

fn get_str(value: &Value, key: &str) -> Result<String, (u16, String)> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| api_error(400, "OTC_INVALID", &format!("{key} 缺失")))
}

fn get_i64(value: &Value, key: &str) -> Result<i64, (u16, String)> {
    value
        .get(key)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| api_error(400, "OTC_INVALID", &format!("{key} 缺失或非法")))
}

fn opt_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

fn opt_minor(value: &Value, key: &str, label: &str) -> Result<Option<i64>, (u16, String)> {
    match opt_str(value, key) {
        None => Ok(None),
        Some(text) => Fixed::parse(&text, 2)
            .map(|v| Some(v.raw()))
            .map_err(|_| api_error(400, "OTC_INVALID", &format!("{label} 精度非法"))),
    }
}

fn opt_share(value: &Value, key: &str, label: &str) -> Result<Option<i64>, (u16, String)> {
    match opt_str(value, key) {
        None => Ok(None),
        Some(text) => Fixed::parse(&text, SCALE_SHARE)
            .map(|v| Some(v.raw()))
            .map_err(|_| api_error(400, "OTC_INVALID", &format!("{label} 精度非法"))),
    }
}

fn minor_value(text: &str, label: &str) -> Result<i64, (u16, String)> {
    Fixed::parse(text, 2)
        .map(|v| v.raw())
        .map_err(|_| api_error(400, "OTC_INVALID", &format!("{label} 精度非法")))
}

fn map_ledger<T>(result: Result<T, crate::ledger::LedgerError>) -> Result<T, (u16, String)> {
    result.map_err(|e| {
        let code = match e.code() {
            "LEDGER_CONFLICT" | "LEDGER_OVERSELL" => 409,
            "LEDGER_NOT_FOUND" => 404,
            _ => 400,
        };
        api_error(code, e.code(), &e.message())
    })
}

/// 新增持仓预览：{account, fundCode, fundName?, value, gain, asOf, shares?}
pub(crate) fn api_holding_preview(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let value_minor = minor_value(&get_str(&value, "value")?, "持有金额")?;
    let gain_minor = minor_value(&get_str(&value, "gain")?, "持有收益")?;
    let shares = opt_share(&value, "shares", "实际份额")?;
    let code = get_str(&value, "fundCode")?;
    let as_of = get_str(&value, "asOf")?;
    let account = get_str(&value, "account")?;
    if code.len() != 6
        || !code.bytes().all(|byte| byte.is_ascii_digit())
        || !crate::assets::is_valid_calendar_date(&as_of)
        || as_of > super::today_iso()
        || value_minor <= 0
        || gain_minor > value_minor
        || value_minor.checked_sub(gain_minor).is_none()
        || shares.is_some_and(|shares| shares <= 0)
        || account.trim().is_empty()
        || account.chars().count() > 64
    {
        return Err(api_error(
            400,
            "OTC_INVALID",
            "请核对基金代码、估值日期、持有金额及收益",
        ));
    }
    if shares.is_none() {
        let available = map_ledger(store.with_read(|conn| match fund_id_by_code(conn, &code)? {
            Some(id) => crate::otc_holding::nav_as_of(conn, id, &as_of).map(|nav| nav.is_some()),
            None => Ok(false),
        }))?;
        if !available {
            // Market metadata only; the holding is not booked until explicit save.
            map_ledger(store.with_write(|conn| {
                crate::ledger::ensure_fund(conn, &code, value["fundName"].as_str().unwrap_or(""))
            }))?;
            super::nav_api::api_nav_sync(store, &json!({"fundCode":code}).to_string())?;
        }
    }
    let out = map_ledger(holding_edit_preview(
        store,
        &account,
        &code,
        value_minor,
        gain_minor,
        shares,
        &as_of,
    ))?;
    Ok(out.to_string())
}

/// 新增持仓提交：{account, fundCode, fundName, value, gain, asOf, shares?,
/// acceptEarlierNav?, requestId}
pub(crate) fn api_holding_create(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let value_minor = minor_value(&get_str(&value, "value")?, "持有金额")?;
    let gain_minor = minor_value(&get_str(&value, "gain")?, "持有收益")?;
    let shares = opt_share(&value, "shares", "实际份额")?;
    let out = map_ledger(holding_create(
        store,
        &get_str(&value, "account")?,
        &get_str(&value, "fundCode")?,
        &get_str(&value, "fundName")?,
        value_minor,
        gain_minor,
        shares,
        &get_str(&value, "asOf")?,
        value
            .get("acceptEarlierNav")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        &get_str(&value, "requestId")?,
        &super::today_iso(),
    ))?;
    Ok(out.to_string())
}

/// 编辑预览（与新增同参）。
pub(crate) fn api_holding_edit_preview(store: &Store, body: &str) -> ApiResult {
    api_holding_preview(store, body)
}

/// 编辑提交：{mode, account, fundCode, value, gain, asOf, shares?, reason,
/// requestId, acceptEarlierNav?, openingVersion?}
pub(crate) fn api_holding_edit(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let value_minor = minor_value(&get_str(&value, "value")?, "持有金额")?;
    let gain_minor = minor_value(&get_str(&value, "gain")?, "持有收益")?;
    let shares = opt_share(&value, "shares", "实际份额")?;
    let out = map_ledger(holding_edit(
        store,
        &get_str(&value, "mode")?,
        &get_str(&value, "account")?,
        &get_str(&value, "fundCode")?,
        value_minor,
        gain_minor,
        shares,
        &get_str(&value, "asOf")?,
        &get_str(&value, "reason")?,
        &get_str(&value, "requestId")?,
        value
            .get("acceptEarlierNav")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        value.get("openingVersion").and_then(|v| v.as_i64()),
        &super::today_iso(),
    ))?;
    Ok(out.to_string())
}

/// 订单列表：GET /api/otc/orders?account=<name>&fund=<code>
pub(crate) fn api_orders_list(store: &Store, path: &str) -> ApiResult {
    let account = crate::market::query_param(path, "account")
        .ok_or_else(|| api_error(400, "OTC_INVALID", "account 缺失"))?;
    let fund = crate::market::query_param(path, "fund")
        .ok_or_else(|| api_error(400, "OTC_INVALID", "fund 缺失"))?;
    let out = map_ledger(orders_list(store, &account, &fund))?;
    Ok(out.to_string())
}

/// 订单创建：{op, account, fundCode, fundName, appliedDate, cutoff,
/// amount?, fee?, feeKnown?, shares?, targetCode?, targetName?,
/// planId?, periodKey?, requestId, note?}
pub(crate) fn api_order_create(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let draft = OrderDraft {
        op: match get_str(&value, "op")?.as_str() {
            "buy" => "buy",
            "sell" => "sell",
            "convert" => "convert",
            other => {
                return Err(api_error(
                    400,
                    "OTC_INVALID",
                    &format!("未知订单类型 {other}"),
                ))
            }
        },
        account_name: get_str(&value, "account")?,
        fund_code: get_str(&value, "fundCode")?,
        fund_name: value
            .get("fundName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        applied_date: get_str(&value, "appliedDate")?,
        cutoff: if get_str(&value, "cutoff")?.as_str() == "after_1500" {
            "after_1500".into()
        } else {
            "before_1500".into()
        },
        amount_minor: opt_minor(&value, "amount", "加仓金额")?,
        fee_minor: opt_minor(&value, "fee", "手续费")?,
        fee_known: value
            .get("feeKnown")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        shares_raw: opt_share(&value, "shares", "减仓份额")?,
        target_code: opt_str(&value, "targetCode"),
        target_name: opt_str(&value, "targetName"),
        plan_id: value.get("planId").and_then(|v| v.as_i64()),
        period_key: opt_str(&value, "periodKey").unwrap_or_default(),
        request_id: get_str(&value, "requestId")?,
        note: opt_str(&value, "note").unwrap_or_default(),
    };
    let out = map_ledger(order_create(store, &draft, &super::today_iso()))?;
    Ok(out.to_string())
}

/// 订单确认：{id, expectedVersion, confirmedShares?, confirmedAmount?,
/// fee?, feeKnown?, confirmedNavDate?, confirmedNav?, note?}
pub(crate) fn api_order_confirm(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let input = OrderConfirmInput {
        id: get_i64(&value, "id")?,
        expected_version: get_i64(&value, "expectedVersion")?,
        confirmed_shares_text: opt_str(&value, "confirmedShares"),
        confirmed_amount_text: opt_str(&value, "confirmedAmount"),
        fee_text: opt_str(&value, "fee"),
        fee_known: value
            .get("feeKnown")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        confirmed_nav_date: opt_str(&value, "confirmedNavDate"),
        confirmed_nav_text: opt_str(&value, "confirmedNav"),
    };
    let out = map_ledger(order_confirm(store, &input, &super::today_iso()))?;
    Ok(out.to_string())
}

/// 订单取消／失败：{id, expectedVersion, outcome:'cancelled'|'failed', reason?}
pub(crate) fn api_order_cancel(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let out = map_ledger(order_cancel(
        store,
        get_i64(&value, "id")?,
        get_i64(&value, "expectedVersion")?,
        &get_str(&value, "outcome")?,
        &opt_str(&value, "reason").unwrap_or_default(),
    ))?;
    Ok(out.to_string())
}

/// 到账登记：{id, date, mode:'post'|'included', note?}
pub(crate) fn api_order_arrival(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let out = map_ledger(order_arrival(
        store,
        get_i64(&value, "id")?,
        &get_str(&value, "date")?,
        &opt_str(&value, "mode").unwrap_or_else(|| "post".into()),
        &opt_str(&value, "note").unwrap_or_default(),
    ))?;
    Ok(out.to_string())
}

fn plan_input(value: &Value) -> Result<PlanInput, (u16, String)> {
    Ok(PlanInput {
        account_name: get_str(value, "account")?,
        fund_code: get_str(value, "fundCode")?,
        fund_name: value
            .get("fundName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        amount_minor: minor_value(&get_str(value, "amount")?, "定投金额")?,
        freq: get_str(value, "freq")?,
        weekday: value.get("weekday").and_then(|v| v.as_i64()),
        monthday: value.get("monthday").and_then(|v| v.as_i64()),
        start_date: get_str(value, "startDate")?,
        end_date: opt_str(value, "endDate"),
        non_trading: value
            .get("nonTrading")
            .and_then(|v| v.as_str())
            .unwrap_or("next")
            .to_string(),
        note: opt_str(value, "note").unwrap_or_default(),
    })
}

pub(crate) fn api_plan_create(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let input = plan_input(&value)?;
    let out = map_ledger(plan_create(store, &input, &super::today_iso()))?;
    Ok(out.to_string())
}

/// 计划更新：{id, expectedVersion, action:'pause'|'resume'|'end'|'edit', ...计划字段}
pub(crate) fn api_plan_update(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let action = get_str(&value, "action")?;
    let input = if action == "edit" {
        Some(plan_input(&value)?)
    } else {
        None
    };
    let out = map_ledger(plan_update(
        store,
        get_i64(&value, "id")?,
        get_i64(&value, "expectedVersion")?,
        &action,
        input.as_ref(),
        &super::today_iso(),
    ))?;
    Ok(out.to_string())
}

#[cfg(test)]
mod plan_action_tests {
    use super::*;
    #[test]
    fn pause_resume_end_require_version_but_not_editor_fields() {
        let store = Store::open_in_memory().unwrap();
        let input = serde_json::json!({"account":"plan-account","fundCode":"110022","amount":"10.00","freq":"daily","startDate":super::super::today_iso()});
        let created: Value =
            serde_json::from_str(&api_plan_create(&store, &input.to_string()).unwrap()).unwrap();
        let id = created["id"].as_i64().unwrap();
        for (action, version) in [("pause", 1), ("resume", 2), ("end", 3)] {
            let body = serde_json::json!({"id":id,"expectedVersion":version,"action":action});
            api_plan_update(&store, &body.to_string()).unwrap();
            assert_eq!(
                api_plan_update(&store, &body.to_string()).unwrap_err().0,
                409
            );
        }
        let edit = serde_json::json!({"id":id,"expectedVersion":4,"action":"edit"});
        assert_eq!(
            api_plan_update(&store, &edit.to_string()).unwrap_err().0,
            400
        );
    }
}

/// 计划列表：GET /api/otc/plans?account=<name>&fund=<code>
pub(crate) fn api_plans_list(store: &Store, path: &str) -> ApiResult {
    let account = crate::market::query_param(path, "account")
        .ok_or_else(|| api_error(400, "OTC_INVALID", "account 缺失"))?;
    let fund = crate::market::query_param(path, "fund")
        .ok_or_else(|| api_error(400, "OTC_INVALID", "fund 缺失"))?;
    let out = map_ledger(plans_list(store, &account, &fund, &super::today_iso()))?;
    Ok(out.to_string())
}

/// 期次记录：GET /api/otc/plans/runs?planId=<id>
pub(crate) fn api_plan_runs(store: &Store, path: &str) -> ApiResult {
    let plan_id = crate::market::query_param(path, "planId")
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or_else(|| api_error(400, "OTC_INVALID", "planId 缺失或非法"))?;
    let out = map_ledger(plan_runs(store, plan_id))?;
    Ok(out.to_string())
}

/// 期次登记：{planId, periodKey, mode:'buy'|'skip', cutoff?, requestId}
pub(crate) fn api_plan_run_register(store: &Store, body: &str) -> ApiResult {
    let value = parse_body(body)?;
    let out = map_ledger(plan_run_register(
        store,
        get_i64(&value, "planId")?,
        &get_str(&value, "periodKey")?,
        &get_str(&value, "mode")?,
        &opt_str(&value, "cutoff").unwrap_or_else(|| "before_1500".into()),
        &get_str(&value, "requestId")?,
        &super::today_iso(),
    ))?;
    Ok(out.to_string())
}

/// 基金身份解析（预览用）：GET /api/otc/fund?code=<code> → {exists}
pub(crate) fn api_fund_exists(store: &Store, path: &str) -> ApiResult {
    let code = crate::market::query_param(path, "code")
        .ok_or_else(|| api_error(400, "OTC_INVALID", "code 缺失"))?;
    let exists = store
        .with_read(|conn| fund_id_by_code(conn, &code))
        .map_err(|e| api_error(500, e.code(), &e.message()))?;
    let out = map_ledger(Ok(json!({"exists": exists.is_some()})))?;
    Ok(out.to_string())
}
