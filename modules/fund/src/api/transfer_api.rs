//! 持仓转移接口（方案 03：POST /api/positions/transfer/preview、/transfer）。

use crate::fixed::Fixed;
use crate::storage::Store;
use crate::transfers::{self, TransferInput};

fn ledger_err(e: crate::ledger::LedgerError) -> (u16, String) {
    let code = match e.code() {
        "LEDGER_CONFLICT" => 409,
        "LEDGER_NOT_FOUND" => 404,
        "LEDGER_OVERSELL" => 400,
        "LEDGER_DB" => 500,
        _ => 400,
    };
    super::api_error(code, e.code(), &e.message())
}

fn parse_input(body: &str) -> Result<(TransferInput, Option<String>, Option<i64>), (u16, String)> {
    let value = super::home_api::body_json(body)?;
    let from_id = super::home_api::get_i64(&value, "fromId")?;
    let to_id = super::home_api::get_i64(&value, "toId")?;
    let fund_id = super::home_api::get_i64(&value, "securityId")?;
    let quantity_text = super::home_api::get_str(&value, "quantity")?;
    let quantity = Fixed::parse(quantity_text, 4)
        .map_err(|_| super::api_error(400, "TRANSFER_INVALID", "quantity 最多 4 位小数"))?;
    let request_id = super::home_api::get_str(&value, "requestId")?.to_string();
    // asOf 可选：缺省为当天；显式提供必须等于当天（成本平移只在持仓尾部时序
    // 确定，见 crate::transfers 模块注释），不静默忽略用户意图。
    let as_of = super::home_api::get_opt_str(&value, "asOf").map(str::to_string);
    let expected_revision = match value.get("expectedRevision") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(v.as_i64().ok_or_else(|| {
            super::api_error(400, "TRANSFER_INVALID", "expectedRevision 须为整数")
        })?),
    };
    Ok((
        TransferInput {
            from_id,
            to_id,
            fund_id,
            quantity,
            request_id,
        },
        as_of,
        expected_revision,
    ))
}

pub(crate) fn api_transfer_preview(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let (input, as_of, _) = parse_input(body)?;
    let today = super::today_iso();
    if as_of.as_deref().is_some_and(|d| d != today) {
        return Err(super::api_error(
            400,
            "TRANSFER_INVALID",
            "转账按操作当日入账；asOf 必须为当天或省略",
        ));
    }
    store
        .with_read(|conn| transfers::preview(conn, &input))
        .map(|preview| preview_wire(&preview))
        .map_err(ledger_err)
}

/// 业务预览结果 → 既有 wire JSON（字段与字节形态保持不变）。
fn preview_wire(p: &transfers::TransferPreview) -> String {
    let qty_text = |raw: i64| Fixed::share(raw).to_string_value();
    let cost_text = |raw: i64| Fixed::amount(raw).to_string_value();
    let from_qty = super::json_str(&qty_text(p.from_quantity_raw));
    let from_cost = super::json_str(&cost_text(p.from_cost_raw));
    let to_qty = super::json_str(&qty_text(p.to_quantity_raw));
    let to_cost = super::json_str(&cost_text(p.to_cost_raw));
    let transferred = super::json_str(&cost_text(p.transferred_cost_raw));
    let quantity = super::json_str(&qty_text(p.quantity_raw));
    format!(
        "{{\"from\":{{\"quantity\":{from_qty},\"cost\":{from_cost}}},\
         \"to\":{{\"quantity\":{to_qty},\"cost\":{to_cost}}},\
         \"transferredCost\":{transferred},\"quantity\":{quantity},\
         \"cashImpact\":\"none\",\
         \"fee\":\"转账不产生费用；如需真实买卖费用请分别记录卖出与买入\",\
         \"dataRevision\":{revision},\"targetExisting\":{target_existing}}}",
        revision = p.revision,
        target_existing = p.target_existing,
    )
}

pub(crate) fn api_transfer_commit(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let (input, as_of, expected_revision) = parse_input(body)?;
    let today = super::today_iso();
    if as_of.as_deref().is_some_and(|d| d != today) {
        return Err(super::api_error(
            400,
            "TRANSFER_INVALID",
            "转账按操作当日入账；asOf 必须为当天或省略",
        ));
    }
    // 幂等重试（两腿已存在）先于 revision 守卫，域层处理；首次提交必须携带
    // expectedRevision，防止基于陈旧预览的静默覆盖。
    let expected_revision = expected_revision.ok_or_else(|| {
        super::api_error(
            400,
            "TRANSFER_INVALID",
            "缺少 expectedRevision（请携预览返回的 dataRevision）",
        )
    })?;
    let (out_id, in_id, created) = store
        .with_write(|conn| transfers::commit(conn, &input, expected_revision, &today))
        .map_err(ledger_err)?;
    Ok(format!(
        "{{\"ok\":true,\"outTxId\":{out_id},\"inTxId\":{in_id},\"created\":{created}}}"
    ))
}
