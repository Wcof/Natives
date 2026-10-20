//! 线下投资资产 API（方案 03 §6：在 assets API 风格上扩展；O01～O05 HTTP 入口）。
//!
//! 金额跨 JS 用十进制字符串（amountText→minor 由 Fixed 换算）；版本冲突 409。

use serde_json::{json, Value};

use crate::offline::{self, OfflineInput, OfflineRow};
use crate::storage::Store;

fn body_json(body: &str) -> Result<Value, (u16, String)> {
    serde_json::from_str(body).map_err(|e| (400, format!("FIN_INVALID: {e}")))
}

/// 金额字段契约（方案 03 §2）：一律为最小货币单位（分）的整数；
/// JSON number 与纯整数字符串等价，杜绝「数字=分、字符串=元」的 100 倍歧义。
/// 小数、超大、非数字与缺失必填金额显式拒绝。
fn get_minor(value: &Value, key: &str) -> Result<Option<i64>, (u16, String)> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(Some(
            n.as_i64()
                .ok_or((400, format!("FIN_INVALID: {key} 溢出")))?,
        )),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => s.trim().parse::<i64>().map(Some).map_err(|_| {
            (
                400,
                format!("FIN_INVALID: {key} 须为整数分（不得带小数或单位）"),
            )
        }),
        Some(_) => Err((400, format!("FIN_INVALID: {key} 类型无效"))),
    }
}

fn parse_input(value: &Value) -> Result<OfflineInput, (u16, String)> {
    let product_type = value["productType"].as_str().unwrap_or("").to_string();
    Ok(OfflineInput {
        product_type,
        name: value["name"].as_str().unwrap_or("").to_string(),
        location: value["location"].as_str().unwrap_or("").to_string(),
        value_minor: get_minor(value, "valueMinor")?
            .ok_or((400, "FIN_INVALID: 缺少 valueMinor".to_string()))?,
        valued_at: value["valuedAt"].as_str().unwrap_or("").to_string(),
        cost_minor: get_minor(value, "costMinor")?,
        risk_class: value["riskClass"].as_str().unwrap_or("unknown").to_string(),
        note: value["note"].as_str().unwrap_or("").to_string(),
        attrs_json: value
            .get("attrs")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "{}".into()),
    })
}

fn row_json(r: &OfflineRow) -> Value {
    json!({
        "id": r.id,
        "productType": r.product_type,
        "name": r.name,
        "location": r.location,
        "valueMinor": r.value_minor.to_string(),
        "valuedAt": r.valued_at,
        "costMinor": r.cost_minor.map(|v|v.to_string()),
        "riskClass": r.risk_class,
        "note": r.note,
        "attrs": serde_json::from_str::<Value>(&r.attrs_json).unwrap_or(Value::Null),
        "exitDate": r.exit_date,
        "exitAmountMinor": r.exit_amount_minor.map(|v|v.to_string()),
        "exitFeeMinor": r.exit_fee_minor.map(|v|v.to_string()),
        "exitSettled": r.exit_settled,
        "deleted": r.deleted,
        "version": r.version,
    })
}

pub fn api_offline_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let query = path.split('?').nth(1).unwrap_or("");
    let include_deleted = query.split('&').any(|p| p == "includeDeleted=1");
    let page = crate::market::query_param(path, "page")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 1_000_000);
    let product_type = crate::market::query_param(path, "productType").unwrap_or_default();
    let rows = offline::list_assets_filtered(
        store,
        include_deleted,
        50,
        page.saturating_sub(1).saturating_mul(50),
        &product_type,
    )
    .map_err(api_error)?;
    Ok(json!({ "assets": rows.iter().map(row_json).collect::<Vec<_>>(),"page":page,"pageSize":50 }).to_string())
}

pub fn api_offline_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = parse_input(&value)?;
    let id = offline::create_asset(store, &input).map_err(api_error)?;
    Ok(json!({"id": id}).to_string())
}

pub fn api_offline_update(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let expected_version = value["expectedVersion"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 expectedVersion".to_string()))?;
    let input = parse_input(&value)?;
    offline::update_asset(store, id, expected_version, &input).map_err(api_error)?;
    Ok(json!({"ok": true}).to_string())
}

pub fn api_offline_exit(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let input = offline::ExitInput {
        exit_date: value["exitDate"].as_str().unwrap_or("").to_string(),
        exit_amount_minor: get_minor(&value, "exitAmountMinor")?.ok_or((
            400,
            "FIN_INVALID: 缺少 exitAmountMinor（回收金额必填）".to_string(),
        ))?,
        exit_fee_minor: get_minor(&value, "exitFeeMinor")?
            .ok_or_else(|| (400, "FIN_INVALID: 请确认退出费用".into()))?,
        settle_account_id: value["settleAccountId"].as_i64(),
        remaining_value_minor: get_minor(&value, "remainingValueMinor")?,
        expected_version: value["expectedVersion"]
            .as_i64()
            .ok_or((400, "FIN_INVALID: 缺少 expectedVersion".to_string()))?,
        idempotency_key: value["requestId"].as_str().unwrap_or("").to_string(),
    };
    if input.idempotency_key.is_empty() {
        return Err((400, "FIN_INVALID: 缺少 requestId".into()));
    }
    offline::exit_asset(store, id, &input).map_err(api_error)?;
    Ok(json!({"ok": true}).to_string())
}

pub fn api_offline_exits(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let id = crate::market::query_param(path, "id")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| (400, "FIN_INVALID: 缺少 id".into()))?;
    Ok(json!({"exits":crate::offline_settlement::exits_page(store,id,history_offset(path)?).map_err(api_error)?}).to_string())
}

pub fn api_offline_settle(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    let id = v["id"]
        .as_i64()
        .ok_or_else(|| (400, "FIN_INVALID: 缺少退出 id".into()))?;
    let account = v["accountId"]
        .as_i64()
        .ok_or_else(|| (400, "FIN_INVALID: 缺少账户".into()))?;
    let version = v["expectedVersion"]
        .as_i64()
        .ok_or_else(|| (400, "FIN_INVALID: 缺少版本".into()))?;
    crate::offline_settlement::settle(
        store,
        id,
        account,
        version,
        v["settledAt"].as_str().unwrap_or(""),
    )
    .map_err(api_error)?;
    Ok(json!({"ok":true}).to_string())
}

pub fn api_offline_delete(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let expected_version = value["expectedVersion"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 expectedVersion".to_string()))?;
    let reason = value["reason"].as_str().unwrap_or("").to_string();
    offline::delete_asset(store, id, &reason, expected_version).map_err(api_error)?;
    Ok(json!({"ok": true}).to_string())
}

pub fn api_offline_restore(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let version = value["expectedVersion"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少版本".into()))?;
    offline::restore_asset_versioned(store, id, version).map_err(api_error)?;
    Ok(json!({"ok": true}).to_string())
}

pub fn api_offline_history(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let query = path.split('?').nth(1).unwrap_or("");
    let mut id: Option<i64> = None;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if let (Some("id"), Some(v)) = (kv.next(), kv.next()) {
            id = v.parse().ok();
        }
    }
    let id = id.ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let history =
        offline::value_history_page(store, id, history_offset(path)?).map_err(api_error)?;
    Ok(json!(history
        .iter()
        .map(|(v, at, created)| json!({"valueMinor": v.to_string(), "valuedAt": at, "recordedAt": created}))
        .collect::<Vec<_>>())
    .to_string())
}

fn history_offset(path: &str) -> Result<i64, (u16, String)> {
    let page = crate::market::query_param(path, "page").unwrap_or_else(|| "1".into());
    let page = page
        .parse::<i64>()
        .ok()
        .filter(|p| (1..=1_000_000).contains(p))
        .ok_or_else(|| (400, "FIN_INVALID: page".into()))?;
    Ok((page - 1) * 50)
}

fn api_error(e: offline::OfflineError) -> (u16, String) {
    match &e {
        offline::OfflineError::Conflict { .. } => (409, format!("{}: {}", e.code(), e.message())),
        offline::OfflineError::NotFound => (404, format!("{}: {}", e.code(), e.message())),
        offline::OfflineError::Invalid(_) => (400, format!("{}: {}", e.code(), e.message())),
        offline::OfflineError::Db(_) => (500, format!("{}: {}", e.code(), e.message())),
    }
}
