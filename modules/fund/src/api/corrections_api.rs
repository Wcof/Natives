//! 交易更正/删除/恢复 API（方案 01 §5.2；corrections.rs 的 HTTP 入口）。
//!
//! 数量/价格沿用现有 4 位定点 raw；金额分服务端重算，不信任页面金额。
//! 预览无副作用；提交 request_id 幂等（重试/双窗口不双写）。

use serde_json::{json, Value};

use crate::corrections::{self, CorrectionInput};
use crate::storage::Store;

fn body_json(body: &str) -> Result<Value, (u16, String)> {
    serde_json::from_str(body).map_err(|e| (400, format!("FIN_INVALID: {e}")))
}

pub fn api_preview_delete(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let tx_id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let preview = corrections::preview_delete(store, tx_id).map_err(api_error)?;
    Ok(json!({
        "txId": preview.tx_id,
        "type": preview.tx_type,
        "quantityRaw": preview.quantity_raw,
        "amountRaw": preview.amount_raw,
        "feeRaw": preview.fee_raw,
        "tradeDate": preview.trade_date,
        "resultingQuantityRaw": preview.resulting_quantity_raw,
        "blockers": preview.blockers,
    })
    .to_string())
}

pub fn api_delete_transaction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let tx_id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let reason = value["reason"].as_str().unwrap_or("").to_string();
    let deleted = corrections::delete_transaction_versioned(
        store,
        tx_id,
        &reason,
        &now_iso(),
        Some(version(&value)?),
    )
    .map_err(api_error)?;
    Ok(json!({"id": tx_id, "deleted": deleted}).to_string())
}

pub fn api_restore_transaction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let tx_id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let restored = corrections::restore_transaction_versioned(store, tx_id, Some(version(&value)?))
        .map_err(api_error)?;
    Ok(json!({"id": tx_id, "restored": restored}).to_string())
}

fn parse_correction(value: &Value) -> Result<CorrectionInput, (u16, String)> {
    Ok(CorrectionInput {
        tx_id: value["id"]
            .as_i64()
            .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?,
        quantity_raw: minor_raw(value, "quantityRaw")?,
        price_raw: minor_raw(value, "priceRaw")?,
        fee_raw: minor_raw(value, "feeRaw")?,
        trade_date: value["tradeDate"].as_str().unwrap_or("").to_string(),
        request_id: value["requestId"].as_str().unwrap_or("").to_string(),
        reason: value["reason"].as_str().unwrap_or("").to_string(),
    })
}

pub fn api_preview_correction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = parse_correction(&value)?;
    let preview = corrections::preview_correction(store, &input).map_err(api_error)?;
    Ok(json!({
        "originalQuantityRaw": preview.original_quantity_raw.to_string(),
        "originalAmountRaw": preview.original_amount_raw.to_string(),
        "newAmountRaw": preview.new_amount_raw.to_string(),
        "resultingQuantityRaw": preview.resulting_quantity_raw.to_string(),
        "cashDifferenceMinor":preview.cash_difference_minor.map(|n|n.to_string()),
        "requiresCashConfirmation":preview.requires_cash_confirmation,
        "blockers": preview.blockers,
    })
    .to_string())
}

pub fn api_submit_correction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let mut input = parse_correction(&value)?;
    if input.request_id.is_empty() {
        return Err((400, "FIN_INVALID: 缺少 requestId（幂等键）".to_string()));
    }
    let tx_id = corrections::submit_correction_with_cash(
        store,
        &input,
        &now_iso(),
        Some(version(&value)?),
        value.get("cashCorrection"),
    )
    .map_err(api_error)?;
    let _ = &mut input;
    Ok(json!({"id": tx_id, "correctionOf": input.tx_id}).to_string())
}

fn api_error(e: crate::ledger::LedgerError) -> (u16, String) {
    match &e {
        crate::ledger::LedgerError::Oversell { .. } => {
            (409, format!("LEDGER_OVERSELL: {}", e.message()))
        }
        crate::ledger::LedgerError::Invalid(_) => (400, format!("FIN_INVALID: {}", e.message())),
        crate::ledger::LedgerError::Conflict(_) => {
            (409, format!("LEDGER_CONFLICT: {}", e.message()))
        }
        crate::ledger::LedgerError::NotFound(_) => {
            (404, format!("LEDGER_NOT_FOUND: {}", e.message()))
        }
        _ => (500, format!("FIN_DB: {}", e.message())),
    }
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days + 719_468);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + i64::from(if m <= 2 { 1 } else { 0 }), m, d)
}

fn version(v: &Value) -> Result<i64, (u16, String)> {
    v["expectedVersion"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or((400, "LEDGER_INVALID: 缺少版本".into()))
}
fn minor_raw(v: &Value, key: &str) -> Result<i64, (u16, String)> {
    v[key]
        .as_i64()
        .or_else(|| v[key].as_str()?.parse().ok())
        .ok_or((400, format!("LEDGER_INVALID: {key} 须为整数定点值")))
}
