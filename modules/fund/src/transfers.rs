//! 账户间持仓转移（R2 T03 / 方案 03「持仓转移」）。
//!
//! 语义：转移不是市场买卖，按移动平均成本平移份额，双边各记一笔
//! 已确认流水（source=`transfer`），不产生已实现收益，不动家庭现金
//! （trade_cash 对 transfer 直接跳过）。转移日期固定为操作当日：
//! 回放按 (trade_date,id) 时序执行，仅当 asOf 为当天时，预览与提交
//! 基于同一持仓尾部状态，成本扣减完全一致（早于当天的历史转账会
//! 使中间时序成本漂移，明确拒绝，不伪造对账）。
//!
//! 幂等：两腿 request_id 为 `{rid}#out`／`{rid}#in`，由 ledger 流水
//! 幂等检查保证重复提交不重复结算；expectedRevision 与实际 data_revision
//! 不符返回冲突（预览返回当前 revision 供携带）。

use crate::fixed::Fixed;
use crate::ledger::{LedgerError, TransactionInput, TYPE_BUY, TYPE_SELL};
use crate::portfolio::{self, Position};
use rusqlite::{Connection, OptionalExtension};

#[derive(Clone, Debug)]
pub struct TransferInput {
    pub from_id: i64,
    pub to_id: i64,
    pub fund_id: i64,
    pub quantity: Fixed,
    pub request_id: String,
}

#[derive(Debug)]
pub struct TransferPlan {
    pub from_before: Position,
    pub to_before: Option<Position>,
    pub transferred_cost_raw: i64,
}

impl TransferInput {
    fn validate_shape(&self) -> Result<(), LedgerError> {
        if self.from_id == self.to_id {
            return Err(LedgerError::Invalid("转出与转入账户不能相同".into()));
        }
        if self.quantity.raw() <= 0 {
            return Err(LedgerError::Invalid("转移份额必须大于零".into()));
        }
        if self.request_id.is_empty() || self.request_id.chars().count() > 120 {
            return Err(LedgerError::Invalid("requestId 长度须为 1..=120".into()));
        }
        Ok(())
    }
}

/// 校验并生成计划（在读快照内执行；commit 在写事务内重算同一状态）。
fn build_plan(conn: &Connection, input: &TransferInput) -> Result<TransferPlan, LedgerError> {
    input.validate_shape()?;
    for id in [input.from_id, input.to_id] {
        let row: Option<(Option<String>,)> = conn
            .query_row(
                "SELECT archived_at FROM accounts WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?,)),
            )
            .optional()?;
        let (archived,) = row.ok_or_else(|| LedgerError::NotFound(format!("账户 {id}")))?;
        if archived.is_some() {
            return Err(LedgerError::Invalid(
                "归档账户不能作为转入／转出账户".into(),
            ));
        }
    }
    let (fund_code, _fund_name): (String, String) = conn
        .query_row(
            "SELECT code, name FROM funds WHERE id = ?1",
            [input.fund_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| LedgerError::NotFound(format!("标的 {}", input.fund_id)))?;
    crate::ledger::validate_quantity(&fund_code, input.quantity)?;

    let from_before = portfolio::replay_position(conn, input.from_id, input.fund_id)?
        .ok_or_else(|| LedgerError::NotFound("转出账户无该标的持仓".into()))?;
    if from_before.cost_unknown {
        return Err(LedgerError::Invalid(
            "转出持仓成本未知，无法按成本平移转移；请先更正期初／流水".into(),
        ));
    }
    if input.quantity.raw() > from_before.quantity_raw {
        return Err(LedgerError::Oversell {
            available_raw: from_before.quantity_raw,
        });
    }
    let to_before = portfolio::replay_position(conn, input.to_id, input.fund_id)?;
    if to_before.as_ref().is_some_and(|p| p.cost_unknown) {
        return Err(LedgerError::Invalid(
            "转入持仓成本未知，转移会混淆成本口径；请先更正".into(),
        ));
    }
    // 与 replay SELL 同一算式：cost × q / qty（i128 整除 ROUND_DOWN）。
    let transferred_cost_raw = (from_before.cost_raw as i128 * input.quantity.raw() as i128
        / from_before.quantity_raw as i128) as i64;
    Ok(TransferPlan {
        from_before,
        to_before,
        transferred_cost_raw,
    })
}

/// 单腿构造：SELL 出／BUY 入同额同量零费，amount=转移成本。
fn leg(
    conn: &Connection,
    input: &TransferInput,
    account_id: i64,
    tx_type: &str,
    transferred_cost_raw: i64,
    trade_date: &str,
    suffix: &str,
) -> Result<TransactionInput, LedgerError> {
    let account_name: String = conn.query_row(
        "SELECT name FROM accounts WHERE id = ?1",
        [account_id],
        |r| r.get(0),
    )?;
    let (fund_code, fund_name): (String, String) = conn.query_row(
        "SELECT code, name FROM funds WHERE id = ?1",
        [input.fund_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // 单价（4 位）= 金额(2 位) ÷ 份额(4 位)：raw4 = amount_raw × 10^6 ÷ qty_raw。
    let price_raw =
        (transferred_cost_raw as i128 * 1_000_000 / input.quantity.raw() as i128) as i64;
    Ok(TransactionInput {
        account_name,
        fund_code,
        fund_name,
        tx_type: tx_type.to_string(),
        state: crate::ledger::STATE_CONFIRMED.to_string(),
        quantity: input.quantity,
        price: Fixed::share(price_raw),
        fee: Fixed::zero(2),
        amount: Some(Fixed::amount(transferred_cost_raw)),
        trade_date: trade_date.to_string(),
        source: "transfer".to_string(),
        request_id: Some(format!("{}#{}", input.request_id, suffix)),
        external_id: None,
        batch_id: None,
        line_no: None,
        payload_hash: None,
    })
}

/// 查找已存在的转账两腿（幂等重试命中时返回原 tx id）。
fn existing_legs(conn: &Connection, request_id: &str) -> Result<Option<(i64, i64)>, LedgerError> {
    let out: Option<i64> = conn
        .query_row(
            "SELECT id FROM transactions WHERE request_id = ?1",
            [format!("{request_id}#out")],
            |r| r.get(0),
        )
        .optional()?;
    let Some(out_id) = out else {
        return Ok(None);
    };
    let in_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM transactions WHERE request_id = ?1",
            [format!("{request_id}#in")],
            |r| r.get(0),
        )
        .optional()?;
    let in_id = in_id.ok_or_else(|| {
        LedgerError::Conflict("转账请求状态不完整（仅存在一腿），请核对流水".into())
    })?;
    Ok(Some((out_id, in_id)))
}

/// 提交转账：单事务内 revision 守卫 + 双边流水 + 持仓重算；任一失败整体回滚。
/// 返回 (outTxId, inTxId, created)。
pub fn commit(
    conn: &Connection,
    input: &TransferInput,
    expected_revision: i64,
    today: &str,
) -> Result<(i64, i64, bool), LedgerError> {
    if let Some((out_id, in_id)) = existing_legs(conn, &input.request_id)? {
        return Ok((out_id, in_id, false));
    }
    let current_revision: i64 = crate::storage::Store::data_revision(conn)?
        .parse()
        .map_err(|_| LedgerError::Db("data_revision 损坏".into()))?;
    if current_revision != expected_revision {
        return Err(LedgerError::Conflict(format!(
            "数据已变化（当前修订 {current_revision}，请求修订 {expected_revision}），请重新预览"
        )));
    }
    if input.quantity.raw() <= 0 {
        return Err(LedgerError::Invalid("转移份额必须大于零".into()));
    }
    let plan = build_plan(conn, input)?;
    let out_input = leg(
        conn,
        input,
        input.from_id,
        TYPE_SELL,
        plan.transferred_cost_raw,
        today,
        "out",
    )?;
    let (out_id, _) = crate::ledger::record_transaction_tx(conn, &out_input)?;
    let in_input = leg(
        conn,
        input,
        input.to_id,
        TYPE_BUY,
        plan.transferred_cost_raw,
        today,
        "in",
    )?;
    let (in_id, _) = crate::ledger::record_transaction_tx(conn, &in_input)?;
    Ok((out_id, in_id, true))
}

/// 转账预览业务结果（mode/T04）：领域返回业务类型，HTTP 适配层负责
/// 序列化为既有 wire 形态（含 dataRevision 供 expectedRevision 携带）。
#[derive(Debug, Clone, PartialEq)]
pub struct TransferPreview {
    pub from_quantity_raw: i64,
    pub from_cost_raw: i64,
    pub to_quantity_raw: i64,
    pub to_cost_raw: i64,
    pub transferred_cost_raw: i64,
    pub quantity_raw: i64,
    pub revision: i64,
    pub target_existing: bool,
}

/// 预览：读快照内校验并计算双边结果；不写任何状态。
pub fn preview(conn: &Connection, input: &TransferInput) -> Result<TransferPreview, LedgerError> {
    let plan = build_plan(conn, input)?;
    let revision: i64 = crate::storage::Store::data_revision(conn)?
        .parse()
        .map_err(|_| LedgerError::Db("data_revision 损坏".into()))?;
    let from_after_qty = plan.from_before.quantity_raw - input.quantity.raw();
    let from_after_cost = plan.from_before.cost_raw - plan.transferred_cost_raw;
    let (to_after_qty, to_after_cost) = match &plan.to_before {
        Some(p) => (
            p.quantity_raw + input.quantity.raw(),
            p.cost_raw + plan.transferred_cost_raw,
        ),
        None => (input.quantity.raw(), plan.transferred_cost_raw),
    };
    Ok(TransferPreview {
        from_quantity_raw: from_after_qty,
        from_cost_raw: from_after_cost,
        to_quantity_raw: to_after_qty,
        to_cost_raw: to_after_cost,
        transferred_cost_raw: plan.transferred_cost_raw,
        quantity_raw: input.quantity.raw(),
        revision,
        target_existing: plan.to_before.is_some(),
    })
}

#[cfg(test)]
#[path = "transfers_tests.rs"]
mod tests;
