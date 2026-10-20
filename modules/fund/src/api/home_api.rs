//! home_api：投资首页与财务闭环 REST 端点（方案 05 §12 建议接口的落地）。
//!
//! 复用现有 (u16, String) 错误结构与 api_error 风格；金额以分为单位在 JSON
//! 中以 minor 整数传递（展示层转换），写入经 finance 模块定点解析校验。

use crate::finance::{self, FinanceError};
use crate::goals::GoalInput;
use crate::storage::Store;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

pub(crate) fn err(code: u16, e: &FinanceError) -> (u16, String) {
    let message = if matches!(e, FinanceError::Db(_)) {
        "数据操作失败，请重试".to_string()
    } else {
        e.message()
    };
    super::api_error(code, e.code(), &message)
}

pub(crate) fn body_json(body: &str) -> Result<Value, (u16, String)> {
    serde_json::from_str::<Value>(body).map_err(|e| {
        (
            400,
            format!(
                "{{\"error\":\"APP_PARAM_INVALID\",\"message\":\"JSON 无效: {}\"}}",
                e
            ),
        )
    })
}

pub(crate) fn get_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, (u16, String)> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| {
        (
            400,
            format!("{{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 {key}\"}}"),
        )
    })
}

pub(crate) fn get_i64(value: &Value, key: &str) -> Result<i64, (u16, String)> {
    value.get(key).and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            format!("{{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 {key}\"}}"),
        )
    })
}

pub(crate) fn get_opt_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

// ---------------------------------------------------------------------------
// 首页快照
// ---------------------------------------------------------------------------

fn group_plan_error(e: crate::group_plan::GroupPlanError) -> (u16, String) {
    let code = match e.code() {
        "GROUP_PLAN_CONFLICT" => 409,
        "GROUP_PLAN_DB" => 500,
        _ => 400,
    };
    super::api_error(code, e.code(), &e.message())
}

fn plan_to_json(plan: &crate::group_plan::GroupPlan) -> Value {
    serde_json::json!({
        "revision": plan.revision,
        "active": plan.active,
        "pausedLegacyRules": plan.paused_legacy_rules,
        "groups": plan.groups.iter().map(|g| serde_json::json!({
            "groupId": g.group_id,
            "name": g.name,
            "description": g.description,
            "ratioTenths": g.ratio_tenths,
            "sortOrder": g.sort_order,
            "fundBindings": g.fund_bindings.iter()
                .map(|(a, f)| serde_json::json!({"accountId": a, "fundId": f}))
                .collect::<Vec<_>>(),
            "manualBindings": g.manual_bindings.iter()
                .map(|id| serde_json::json!({"assetId": id}))
                .collect::<Vec<_>>(),
            "offlineBindings": g.offline_bindings.iter().map(|id|serde_json::json!({"assetId":id})).collect::<Vec<_>>(),
            "cashBindings": g.cash_bindings.iter().map(|id|serde_json::json!({"accountId":id})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

pub fn api_group_plan_get(store: &Store) -> Result<String, (u16, String)> {
    let plan = store
        .with_read(crate::group_plan::get_plan)
        .map_err(group_plan_error)?;
    let value = match plan {
        Some(p) => plan_to_json(&p),
        None => serde_json::json!({"configured": false}),
    };
    Ok(value.to_string())
}

fn group_inputs_from(value: &Value) -> Result<Vec<crate::group_plan::GroupInput>, (u16, String)> {
    let groups = value
        .get("groups")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            (
                400,
                "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 groups\"}".to_string(),
            )
        })?;
    let mut out = Vec::with_capacity(groups.len());
    for g in groups {
        for key in [
            "fundBindings",
            "manualBindings",
            "offlineBindings",
            "cashBindings",
        ] {
            if g.get(key).is_some_and(|v| !v.is_array()) {
                return Err((400, format!("FIN_INVALID: {key} 须为数组")));
            }
        }
        let fund_bindings = g
            .get("fundBindings")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|r| {
                        Ok((
                            r.get("accountId")
                                .and_then(Value::as_i64)
                                .ok_or_else(|| (400, "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"fundBinding 缺 accountId\"}".to_string()))?,
                            r.get("fundId")
                                .and_then(Value::as_i64)
                                .ok_or_else(|| (400, "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"fundBinding 缺 fundId\"}".to_string()))?,
                        ))
                    })
                    .collect::<Result<Vec<_>, (u16, String)>>()
            })
            .transpose()?
            .unwrap_or_default();
        let manual_bindings = g
            .get("manualBindings")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|r| {
                        r.get("assetId")
                            .and_then(Value::as_i64)
                            .ok_or_else(|| (400, "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"manualBinding 缺 assetId\"}".to_string()))
                    })
                    .collect::<Result<Vec<_>, (u16, String)>>()
            })
            .transpose()?
            .unwrap_or_default();
        out.push(crate::group_plan::GroupInput {
            group_id: g
                .get("groupId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: g
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    (
                        400,
                        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"分组缺少 name\"}"
                            .to_string(),
                    )
                })?
                .to_string(),
            description: g
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            ratio_tenths: g
                .get("ratioTenths")
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    (
                        400,
                        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"分组缺少 ratioTenths\"}"
                            .to_string(),
                    )
                })?,
            sort_order: g.get("sortOrder").and_then(Value::as_i64).unwrap_or(0),
            fund_bindings,
            manual_bindings,
            offline_bindings: binding_ids(g, "offlineBindings", "assetId")?,
            cash_bindings: binding_ids(g, "cashBindings", "accountId")?,
        });
    }
    Ok(out)
}

fn binding_ids(value: &Value, field: &str, id_field: &str) -> Result<Vec<i64>, (u16, String)> {
    match value.get(field) {
        None => Ok(Vec::new()),
        Some(Value::Array(rows)) if rows.len() <= 200 => rows
            .iter()
            .map(|r| {
                r[id_field]
                    .as_i64()
                    .filter(|id| *id > 0)
                    .ok_or_else(|| super::api_error(400, "GROUP_PLAN_INVALID", "资产引用无效"))
            })
            .collect(),
        _ => Err(super::api_error(
            400,
            "GROUP_PLAN_INVALID",
            "资产引用列表无效或过长",
        )),
    }
}

pub fn api_group_plan_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = crate::group_plan::GroupPlanInput {
        groups: group_inputs_from(&value)?,
        expected_revision: value
            .get("expectedRevision")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    };
    let plan = crate::group_plan::save_plan_tx(store, &input).map_err(group_plan_error)?;
    Ok(plan_to_json(&plan).to_string())
}

pub fn api_group_plan_import_preview(store: &Store) -> Result<String, (u16, String)> {
    let value = store
        .with_read(|c| -> Result<Value, (u16, String)> {
            let today = crate::home::resolve_today(c);
            let summary = crate::assets::assets_summary(c, &today).map_err(|e| {
                (
                    400,
                    format!(
                        "{{\"error\":\"{}\",\"message\":\"{}\"}}",
                        e.code(),
                        e.message().replace('"', "'")
                    ),
                )
            })?;
            crate::group_plan::import_preview(c, &summary).map_err(group_plan_error)
        })
        .map_err(|e: (u16, String)| e)?;
    Ok(value.to_string())
}

/// 首页显示偏好（方案 11 §5 三态）：amount|percent|hidden。
/// 旧布尔 privacyHidden=true→hidden、false→amount；新用户默认 hidden；
/// 非法值按 hidden 处理。旧 caller 读 privacyHidden 时 percent/hidden 投影
/// 为 true（不显示金额），amount 为 false。旧布尔写入仍接受并映射三态。
fn read_display_mode(store: &Store) -> Result<String, (u16, String)> {
    let raw: Option<String> = store
        .with_read(|c| {
            c.query_row(
                "SELECT value FROM app_meta WHERE key='home_display_mode'",
                [],
                |r| r.get(0),
            )
            .optional()
        })
        .map_err(|_| super::api_error(500, "FIN_DB", "偏好读取失败"))?;
    Ok(match raw.as_deref() {
        Some("amount") | Some("percent") | Some("hidden") => raw.unwrap(),
        Some(_) => {
            // 非法值纠正为 hidden，不静默展示金额。
            let _ = store.with_write(|c| {
                c.execute(
                    "INSERT INTO app_meta(key,value) VALUES('home_display_mode','hidden')
                     ON CONFLICT(key) DO UPDATE SET value='hidden'",
                    [],
                )
            });
            "hidden".into()
        }
        // 未设置三态键：按旧布尔迁移语义投影（true→hidden，false→amount），
        // 不写回，保持旧通知/查询 caller 不改显示方式。
        None => {
            let legacy: String = store
                .with_read(|c| {
                    c.query_row(
                        "SELECT COALESCE((SELECT value FROM app_meta WHERE key='home_privacy_hidden'),'true')",
                        [],
                        |r| r.get(0),
                    )
                })
                .map_err(|_| super::api_error(500, "FIN_DB", "偏好读取失败"))?;
            if legacy == "false" {
                "amount".into()
            } else {
                "hidden".into()
            }
        }
    })
}

pub fn api_home_preferences(
    store: &Store,
    request: &app_runtime_core::http::HttpRequest,
) -> Result<String, (u16, String)> {
    if request.method == "POST" {
        let value = body_json(&String::from_utf8_lossy(&request.body))?;
        if value.get("privacyHidden").is_none()
            && value.get("notificationsEnabled").is_none()
            && value.get("displayMode").is_none()
        {
            return Err(super::api_error(400, "FIN_INVALID", "缺少偏好字段"));
        }
        for name in ["privacyHidden", "notificationsEnabled"] {
            if value.get(name).is_some_and(|v| !v.is_boolean()) {
                return Err(super::api_error(400, "FIN_INVALID", "偏好必须为布尔值"));
            }
        }
        let display_mode = value.get("displayMode").and_then(Value::as_str);
        if let Some(mode) = display_mode {
            if !matches!(mode, "amount" | "percent" | "hidden") {
                return Err(super::api_error(400, "FIN_INVALID", "displayMode 无效"));
            }
        }
        store.with_write(|c| -> rusqlite::Result<()> {
            for (name,key) in [("privacyHidden","home_privacy_hidden"),("notificationsEnabled","home_notifications_enabled")] {
                if let Some(v)=value.get(name).and_then(Value::as_bool) {c.execute("INSERT INTO app_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[key,if v {"true"}else{"false"}])?;}
            }
            if let Some(mode) = display_mode {
                c.execute(
                    "INSERT INTO app_meta(key,value) VALUES('home_display_mode',?1)
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [mode],
                )?;
            }
            Ok(())
        })
        .map_err(|_| super::api_error(500, "FIN_DB", "偏好保存失败"))?;
    } else if request.method != "GET" {
        return Err(super::api_error(
            405,
            "METHOD_NOT_ALLOWED",
            "请求方法不支持",
        ));
    }
    let display_mode = read_display_mode(store)?;
    // 旧布尔投影：amount→false；percent/hidden→true（避免旧 caller 显露金额）。
    let hidden = display_mode != "amount";
    let notifications=store.with_read(|c|c.query_row("SELECT COALESCE((SELECT value FROM app_meta WHERE key='home_notifications_enabled'),'false')",[],|r|r.get::<_,String>(0))).map_err(|_|super::api_error(500,"FIN_DB","偏好读取失败"))?=="true";
    Ok(
        serde_json::json!({"displayMode":display_mode,"privacyHidden":hidden,"notificationsEnabled":notifications})
            .to_string(),
    )
}

pub fn api_home_snapshot(store: &Store, path: &str) -> Result<String, (u16, String)> {
    home_snapshot(store, path, false)
}
pub(crate) fn api_home_generate(store: &Store, path: &str) -> Result<String, (u16, String)> {
    home_snapshot(store, path, true)
}
fn home_snapshot(store: &Store, path: &str, generate: bool) -> Result<String, (u16, String)> {
    let range = crate::market::query_param(path, "range").unwrap_or_else(|| "all".into());
    let valid = matches!(
        range.as_str(),
        "today" | "month" | "year" | "all" | "custom"
    );
    if !valid {
        return Err(super::api_error(400, "FIN_INVALID", "收益区间无效"));
    }
    let from = crate::market::query_param(path, "from");
    let to = crate::market::query_param(path, "to");
    crate::home::checked_range(
        &range,
        from.as_deref(),
        to.as_deref(),
        &crate::finance::today_cn(),
    )
    .map_err(|e| err(400, &e))?;
    let refresh = crate::market::query_param(path, "refreshEstimates").as_deref() == Some("1");
    let _budget = crate::network::limit_duration(std::time::Duration::from_secs(20));
    if refresh {
        let today = crate::finance::today_cn();
        let rows = store
            .with_read(|conn| crate::assets::collect(conn, &today))
            .map_err(|e| super::api_error(500, e.code(), &e.message()))?;
        crate::nav::warm_estimates(&rows.funds, &today);
        if generate {
            crate::home::build_snapshot_range(store, &range, from.as_deref(), to.as_deref())
                .map_err(|e| err(400, &e))?;
        }
    }
    let snapshot = if generate {
        crate::opportunity_records::generate_home(store, &range, from.as_deref(), to.as_deref())
    } else if !refresh {
        crate::home::cached_snapshot_range(store, &range, from.as_deref(), to.as_deref())
    } else {
        crate::home::build_snapshot_range(store, &range, from.as_deref(), to.as_deref())
    }
    .map_err(|e| {
        err(
            match e {
                FinanceError::Conflict(_) => 409,
                FinanceError::Db(_) => 500,
                _ => 400,
            },
            &e,
        )
    })?;
    Ok(snapshot.to_string())
}

// ---------------------------------------------------------------------------
// 现金与资金事件
// ---------------------------------------------------------------------------

pub fn api_finance_cash(store: &Store) -> Result<String, (u16, String)> {
    let pos = store
        .with_read(finance::cash_position)
        .map_err(|e| err(400, &e))?;
    let accounts = pos
        .accounts
        .iter()
        .map(|a| {
            format!(
                "{{\"accountId\":{},\"accountName\":{},\"cashMinor\":{},\"reservedMinor\":{},\
                 \"reserveState\":{},\"reservedByOrdersMinor\":{},\"availableMinor\":{}}}",
                a.account_id,
                super::json_str(&a.account_name),
                opt_num(a.cash_minor),
                opt_num(a.reserved_minor),
                super::json_str(a.reserve_state),
                a.reserved_by_orders_minor,
                opt_num(a.available_minor),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"cashMinor\":{},\"reservedMinor\":{},\"reserveState\":{},\
         \"reservedByOrdersMinor\":{},\"availableMinor\":{},\"shortfallMinor\":{},\
         \"accounts\":[{accounts}]}}",
        opt_num(pos.cash_minor),
        opt_num(pos.reserved_minor),
        super::json_str(pos.reserve_state),
        pos.reserved_by_orders_minor,
        opt_num(pos.available_minor),
        opt_num(pos.shortfall_minor),
    ))
}

fn opt_num(v: Option<i64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_else(|| "null".into())
}

pub fn api_finance_reserve_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let account_id = get_i64(&value, "accountId")?;
    let purpose = get_opt_str(&value, "purpose").unwrap_or("");
    let store_err = |e: FinanceError| err(if e.code() == "FIN_INVALID" { 400 } else { 409 }, &e);
    store
        .with_write(|conn| {
            finance::save_reserve(conn, account_id, get_opt_str(&value, "amount"), purpose)
        })
        .map_err(store_err)?;
    Ok("{\"ok\":true}".into())
}

pub fn api_finance_event_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = finance::CashEventInput {
        account_id: get_i64(&value, "accountId")?,
        event_type: get_str(&value, "eventType")?.to_string(),
        amount_text: get_str(&value, "amount")?.to_string(),
        occurred_at: get_str(&value, "occurredAt")?.to_string(),
        idempotency_key: get_opt_str(&value, "idempotencyKey").map(str::to_string),
        note: get_opt_str(&value, "note").unwrap_or("").to_string(),
        counterparty_account_id: value.get("counterpartyAccountId").and_then(Value::as_i64),
    };
    let id = store
        .with_write(|conn| {
            if input.event_type == "dividend_reinvest" {
                finance::record_reinvestment(
                    conn,
                    &input,
                    value["assetId"]
                        .as_i64()
                        .ok_or_else(|| FinanceError::Invalid("再投资需人工资产编号".into()))?,
                )
            } else {
                finance::record_cash_event(conn, &input)
            }
        })
        .map_err(|e| err(if e.code() == "FIN_CONFLICT" { 409 } else { 400 }, &e))?;
    Ok(format!("{{\"ok\":true,\"id\":{id}}}"))
}

pub fn api_finance_events(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let limit = crate::market::query_param(path, "limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(50)
        .min(500);
    let rows = store
        .with_read(|conn| finance::list_cash_events(conn, limit))
        .map_err(|e| err(500, &e))?;
    let items = rows
        .iter()
        .map(|r| {
            format!(
                "{{\"id\":{},\"accountId\":{},\"eventType\":{},\"amountMinor\":{},\
                 \"occurredAt\":{},\"note\":{}}}",
                r.id,
                r.account_id,
                super::json_str(&r.event_type),
                r.amount_minor,
                super::json_str(&r.occurred_at),
                super::json_str(&r.note),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!("{{\"events\":[{items}]}}"))
}

/// 保存当前估值快照（收益基线/期末核对）。日期缺省今天；
/// quality 按 taken_at 当日口径评估（回填期初快照记录的是该时刻的状态，
/// 不能用真实今天把历史快照判成过期——02 §3.2 期初建账）。
pub fn api_finance_snapshot_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let today = super::today_iso();
    if get_opt_str(&value, "takenAt").is_some_and(|d| d != today) {
        return Err(super::api_error(
            400,
            "FIN_INVALID",
            "此入口只保存今日真实估值，历史建账须提供当时估值",
        ));
    }
    let checked = value
        .get("flowsChecked")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if get_opt_str(&value, "note").unwrap_or("").len() > 4096 {
        return Err(super::api_error(400, "FIN_INVALID", "说明过长"));
    }
    let id = crate::returns::save_current_snapshot(
        store,
        get_opt_str(&value, "note").unwrap_or(""),
        checked,
    )
    .map_err(|e| err(400, &e))?;
    Ok(serde_json::json!({"ok":true,"id":id}).to_string())
}

pub fn api_finance_returns(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let range = crate::market::query_param(path, "range").unwrap_or_else(|| "all".into());
    let today = super::today_iso();
    let (from, to, _label) = crate::home::resolve_range(
        &range,
        crate::market::query_param(path, "from").as_deref(),
        crate::market::query_param(path, "to").as_deref(),
        &today,
    );
    let result = store
        .with_read(|conn| -> Result<finance::ReturnResult, FinanceError> {
            if from.is_empty() {
                // 跟踪以来：以最早 ready 快照为起点。
                let baseline: Option<String> = conn
                    .query_row(
                        "SELECT MIN(taken_at) FROM value_snapshots WHERE quality='ready'",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?
                    .flatten();
                match baseline {
                    Some(b) => finance::returns_for_range(conn, &b, &to),
                    None => Ok(finance::ReturnResult {
                        computable: false,
                        reason: Some("missing_baseline".into()),
                        from_date: String::new(),
                        to_date: to.clone(),
                        begin_total_minor: None,
                        end_total_minor: None,
                        net_external_minor: None,
                        net_return_minor: None,
                    }),
                }
            } else {
                finance::returns_for_range(conn, &from, &to)
            }
        })
        .map_err(|e| err(400, &e))?;
    Ok(format!(
        "{{\"computable\":{},\"reason\":{},\"from\":{},\"to\":{},\
         \"beginMinor\":{},\"endMinor\":{},\"netExternalMinor\":{},\"netReturnMinor\":{}}}",
        result.computable,
        super::json_str(result.reason.as_deref().unwrap_or("")),
        super::json_str(&result.from_date),
        super::json_str(&result.to_date),
        opt_num(result.begin_total_minor),
        opt_num(result.end_total_minor),
        opt_num(result.net_external_minor),
        opt_num(result.net_return_minor),
    ))
}

// ---------------------------------------------------------------------------
// 占用
// ---------------------------------------------------------------------------

pub fn api_finance_reservation_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = finance::ReservationInput {
        account_id: get_i64(&value, "accountId")?,
        symbol: get_opt_str(&value, "symbol").unwrap_or("").to_string(),
        risk_class: get_str(&value, "riskClass")?.to_string(),
        amount_text: get_str(&value, "amount")?.to_string(),
        expected_date: get_opt_str(&value, "expectedDate")
            .unwrap_or("")
            .to_string(),
        note: get_opt_str(&value, "note").unwrap_or("").to_string(),
    };
    let id = store
        .with_write(|conn| finance::create_reservation(conn, &input))
        .map_err(|e| err(400, &e))?;
    Ok(format!("{{\"ok\":true,\"id\":{id}}}"))
}

pub fn api_finance_reservation_settle(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = get_i64(&value, "id")?;
    let action = get_str(&value, "action")?;
    store
        .with_write(|conn| finance::settle_reservation(conn, id, action))
        .map_err(|e| err(if e.code() == "FIN_CONFLICT" { 409 } else { 400 }, &e))?;
    Ok("{\"ok\":true}".into())
}

// ---------------------------------------------------------------------------
// 多财务目标（G3）
// ---------------------------------------------------------------------------

fn goal_json(conn: &Connection, g: &crate::goals::FinanceGoalRow) -> Result<String, (u16, String)> {
    let today = super::today_iso();
    let (summary, _) = crate::assets::summary_with_cached_quotes(conn, &today).map_err(|e| {
        (
            400,
            format!(
                "{{\"error\":\"ASSET_SUMMARY\",\"message\":\"{}\"}}",
                e.message().replace('"', "'")
            ),
        )
    })?;
    let progress = crate::goals::goal_scope_value(conn, g, &summary, None);
    Ok(format!(
        "{{\"id\":{id},\"name\":{name},\"targetMinor\":{t},\"targetDate\":{date},\
         \"scopeKind\":{scope},\"assetBindings\":{bindings},\"isPrimary\":{primary},\
         \"archived\":{archived},\"monthlyPlanMinor\":{monthly},\"version\":{version},\
         \"note\":{note},\"progress\":{{\"computable\":{pc},\"reasonCodes\":[{reasons}],\
         \"scopeValueMinor\":{sv},\"ratioTenths\":{ratio},\"ratioPpm\":{ppm},\"gapMinor\":{gap},\
         \"surplusMinor\":{surplus},\"remainingInvestments\":{remaining},\
         \"requiredMonthlyMinor\":{required},\"expired\":{expired}}}}}",
        id = g.id,
        name = super::json_str(&g.name),
        t = g.target_minor,
        date = super::json_str(&g.target_date),
        scope = super::json_str(&g.scope_kind),
        bindings = super::json_str(&g.asset_bindings),
        primary = g.is_primary,
        archived = g.archived,
        monthly = opt_num(g.monthly_plan_minor),
        version = g.version,
        note = super::json_str(&g.note),
        pc = progress.computable,
        reasons = progress
            .reason_codes
            .iter()
            .map(|c| super::json_str(c))
            .collect::<Vec<_>>()
            .join(","),
        sv = opt_num(progress.scope_value_minor),
        ratio = opt_num(progress.ratio_tenths),
        ppm = opt_num(progress.ratio_ppm),
        gap = opt_num(progress.gap_minor),
        surplus = opt_num(progress.surplus_minor),
        remaining = opt_num(progress.remaining_investments),
        required = opt_num(progress.required_monthly_minor),
        expired = progress.expired,
    ))
}

pub fn api_goals_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let include_archived = crate::market::query_param(path, "archived")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let rows = store
        .with_read(|conn| -> Result<Vec<String>, (u16, String)> {
            let goals =
                crate::goals::list_goals(conn, include_archived).map_err(|e| err(400, &e))?;
            let mut out = Vec::new();
            for g in &goals {
                out.push(goal_json(conn, g)?);
            }
            Ok(out)
        })
        .map_err(|e| e)?;
    Ok(format!("{{\"goals\":[{}]}}", rows.join(",")))
}

fn goal_input_from(value: &Value) -> Result<GoalInput, (u16, String)> {
    let account_ids = value
        .get("accountIds")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    // P02（方案 01 §3）：唯一当前目标只填金额。name/scopeKind/targetDate 均可缺省
    // （简化首页提交不带 targetDate 也必须成功——审计四-1）；旧 caller 显式字段仍生效。
    Ok(GoalInput {
        name: get_opt_str(value, "name")
            .unwrap_or("我的投资目标")
            .to_string(),
        target_text: get_str(value, "target")?.to_string(),
        target_date: get_opt_str(value, "targetDate").unwrap_or("").to_string(),
        scope_kind: get_opt_str(value, "scopeKind").unwrap_or("A").to_string(),
        account_ids,
        monthly_plan_text: get_opt_str(value, "monthlyPlan").map(str::to_string),
        note: get_opt_str(value, "note").unwrap_or("").to_string(),
    })
}

pub fn api_goal_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = goal_input_from(&value)?;
    let today = super::today_iso();
    let id = store
        .with_write(|conn| {
            // 唯一当前目标（方案 01 §3）：已存在未归档目标时不创建第二个；
            // 重试/双窗口的重复提交转为 409，用户走修改路径。
            let has_current: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM finance_goals WHERE archived=0)",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map(|v| v == 1)?;
            if has_current {
                return Err(FinanceError::Conflict(
                    "已存在当前财务目标；请修改而非新建".into(),
                ));
            }
            crate::goals::create_goal(conn, &input, &today)
        })
        .map_err(|e| err(if e.code() == "FIN_CONFLICT" { 409 } else { 400 }, &e))?;
    Ok(format!("{{\"ok\":true,\"id\":{id}}}"))
}

pub fn api_goal_update(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = get_i64(&value, "id")?;
    let expected_version = get_i64(&value, "expectedVersion")?;
    let input = goal_input_from(&value)?;
    let today = super::today_iso();
    let version = store
        .with_write(|conn| crate::goals::update_goal(conn, id, &input, expected_version, &today))
        .map_err(|e| err(if e.code() == "FIN_CONFLICT" { 409 } else { 400 }, &e))?;
    Ok(format!("{{\"ok\":true,\"version\":{version}}}"))
}

pub fn api_goal_set_primary(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = get_i64(&value, "id")?;
    store
        .with_write(|conn| crate::goals::set_primary_goal(conn, id))
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_goal_archive(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = get_i64(&value, "id")?;
    store
        .with_write(|conn| crate::goals::archive_goal(conn, id))
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_goal_restore(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = get_i64(&value, "id")?;
    store
        .with_write(|conn| crate::goals::restore_goal(conn, id))
        // A11：双窗口恢复冲突返回 409（与 create/update 同口径）。
        .map_err(|e| err(if e.code() == "FIN_CONFLICT" { 409 } else { 400 }, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_finance_reservations(store: &Store) -> Result<String, (u16, String)> {
    let rows=store.with_read(|c| -> rusqlite::Result<Vec<Value>> {
        let mut q=c.prepare("SELECT id,account_id,symbol,amount_minor,status,expected_date,risk_class FROM cash_reservations ORDER BY id DESC LIMIT 100")?;
        let rows=q.query_map([],|r|Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"accountId":r.get::<_,i64>(1)?,"symbol":r.get::<_,String>(2)?,"amountMinor":r.get::<_,i64>(3)?,"status":r.get::<_,String>(4)?,"expectedDate":r.get::<_,String>(5)?,"riskClass":r.get::<_,String>(6)?})))?.collect::<Result<Vec<_>,_>>()?;Ok(rows)
    }).map_err(|_|super::api_error(500,"FIN_DB","占用读取失败"))?;
    Ok(serde_json::json!({"reservations":rows}).to_string())
}

pub fn api_home_evidence(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    let expected = get_str(&v, "dataRevision")?;
    let ids = v["sourceIds"]
        .as_array()
        .ok_or_else(|| super::api_error(400, "FIN_INVALID", "sourceIds 必须为数组"))?
        .iter()
        .map(|v| {
            v.as_i64()
                .ok_or_else(|| super::api_error(400, "FIN_INVALID", "信息源编号无效"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let disclose = v["discloseFinance"].as_bool().unwrap_or(false);
    if disclose {
        let hidden=store.with_read(|c|c.query_row("SELECT COALESCE((SELECT value FROM app_meta WHERE key='home_privacy_hidden'),'true')",[],|r|r.get::<_,String>(0))).map_err(|_|super::api_error(500,"FIN_DB","偏好读取失败"))?;
        if hidden != "false" {
            return Err(super::api_error(
                409,
                "FIN_INVALID",
                "金额隐藏时禁止披露财务数据",
            ));
        }
    }
    // 三态显示（方案 11 §5）：hidden 下不允许私密 AI 预览。
    let display_mode = read_display_mode(store)?;
    if display_mode == "hidden" && disclose {
        return Err(super::api_error(
            409,
            "FIN_INVALID",
            "隐藏模式下不可披露财务数据，请先切换显示方式",
        ));
    }
    let task = get_opt_str(&v, "task").unwrap_or("diagnosis");
    let scope_asset = v
        .get("scope")
        .and_then(|s| s.get("asset"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let information_ids = v
        .get("informationIds")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|x| {
                    x.as_i64()
                        .ok_or_else(|| super::api_error(400, "FIN_INVALID", "资讯编号无效"))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let briefing_market = get_opt_str(&v, "briefingMarket").unwrap_or("");
    let briefing_session = get_opt_str(&v, "briefingSession").unwrap_or("");
    let research = v
        .get("research")
        .map(crate::evidence::research_context)
        .transpose()
        .map_err(|e| err(400, &e))?;
    if scope_asset.starts_with("research:")
        && research.as_ref().is_none_or(|r| {
            r["identity"]["symbol"].as_str() != scope_asset.strip_prefix("research:")
        })
    {
        return Err(super::api_error(400, "FIN_INVALID", "研究查询身份不一致"));
    }
    let mut packet = crate::home::evidence_task(
        store,
        expected,
        &ids,
        disclose,
        task,
        scope_asset,
        &information_ids,
        briefing_market,
        briefing_session,
    )
    .map_err(|e| err(409, &e))?;
    crate::evidence::attach_research(&mut packet, research).map_err(|e| err(400, &e))?;
    Ok(packet.to_string())
}

pub fn api_trade_cash_list(store: &Store) -> Result<String, (u16, String)> {
    let rows=store.with_read(|c| -> rusqlite::Result<Vec<Value>> {let mut q=c.prepare("SELECT x.tx_id,a.name,f.code,t.type,t.trade_date,x.delta_minor,x.status FROM trade_cash x JOIN transactions t ON t.id=x.tx_id JOIN accounts a ON a.id=x.account_id JOIN funds f ON f.id=t.fund_id WHERE x.status!='settled' AND t.deleted_at IS NULL ORDER BY x.tx_id DESC LIMIT 100")?;let rows=q.query_map([],|r|Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"account":r.get::<_,String>(1)?,"symbol":r.get::<_,String>(2)?,"type":r.get::<_,String>(3)?,"date":r.get::<_,String>(4)?,"deltaMinor":r.get::<_,i64>(5)?,"status":r.get::<_,String>(6)?})))?.collect::<Result<Vec<_>,_>>()?;Ok(rows)}).map_err(|_|super::api_error(500,"FIN_DB","交易资金读取失败"))?;
    Ok(serde_json::json!({"trades":rows}).to_string())
}
pub fn api_trade_cash_settle(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    let id = get_i64(&v, "id")?;
    let mode = get_str(&v, "mode")?;
    let date = get_str(&v, "date")?;
    store
        .with_write(|c| {
            crate::trade_cash::settle(c, id, mode, date, get_opt_str(&v, "note").unwrap_or(""))
        })
        .map_err(|e| err(409, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_notification_claim(store: &Store) -> Result<String, (u16, String)> {
    let (ids,information)=store.with_write(|c| -> rusqlite::Result<(Vec<i64>,Vec<i64>)> {
        let enabled:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM app_meta WHERE key='home_notifications_enabled' AND value='true')",[],|r|r.get(0))?;
        if !enabled {return Ok((vec![],vec![]))}
        let mut q=c.prepare("SELECT id FROM watch_events WHERE notify_result='pending' AND status='open' ORDER BY id LIMIT 10")?;
        let ids=q.query_map([],|r|r.get(0))?.collect::<Result<Vec<i64>,_>>()?;
        for id in &ids {c.execute("UPDATE watch_events SET notify_result='claimed' WHERE id=?1",[id])?;}
        let mut q=c.prepare("SELECT id FROM information_alerts WHERE notify_result='pending' AND status='open' ORDER BY id LIMIT ?1")?;
        let information=q.query_map([10-ids.len() as i64],|r|r.get(0))?.collect::<Result<Vec<i64>,_>>()?;
        for id in &information {c.execute("UPDATE information_alerts SET notify_result='claimed' WHERE id=?1",[id])?;}
        Ok((ids,information))
    }).map_err(|_|super::api_error(500,"FIN_DB","通知状态保存失败"))?;
    Ok(serde_json::json!({"eventIds":ids,"informationIds":information}).to_string())
}
pub fn api_notification_result(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    let id = get_i64(&v, "id")?;
    let result = get_str(&v, "result")?;
    if !["delivered", "failed"].contains(&result) {
        return Err(super::api_error(400, "FIN_INVALID", "通知状态无效"));
    }
    let kind = v["kind"].as_str().unwrap_or("event");
    let query = match kind {
        "event" => {
            "UPDATE watch_events SET notify_result=?2 WHERE id=?1 AND notify_result='claimed'"
        }
        "information" => {
            "UPDATE information_alerts SET notify_result=?2 WHERE id=?1 AND notify_result='claimed'"
        }
        _ => return Err(super::api_error(400, "FIN_INVALID", "通知类型无效")),
    };
    let n = store
        .with_write(|c| c.execute(query, rusqlite::params![id, result]))
        .map_err(|_| super::api_error(500, "FIN_DB", "通知状态保存失败"))?;
    if n == 0 {
        return Err(super::api_error(
            409,
            "FIN_CONFLICT",
            "通知未领取或结果已记录",
        ));
    }
    Ok("{\"ok\":true}".into())
}
