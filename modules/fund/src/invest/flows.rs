//! 流水录入与删除。
//!
//! 流水是账本里唯一的人工录入面，因此校验必须在这一层做干净：
//! 类型合法、金额非负、拆股比例完整，并且**录入当时就回放一次**，
//! 确保不会产生任何时点为负的持仓。等展示时才发现数量为负就太晚了。

use super::holdings::{fmt_amount, fmt_qty, replay, FlowRow};
use super::model::{CreateFlow, FlowView, FLOW_TYPES};
use crate::finance::{now_secs, FinanceError};
use crate::fixed::Fixed;
use crate::storage::Store;

const SCALE_QTY: u32 = 4;
const SCALE_AMOUNT: u32 = 2;

fn parse_qty(text: &str, field: &str) -> Result<i64, FinanceError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    Fixed::parse(trimmed, SCALE_QTY)
        .map(|f| f.raw())
        .map_err(|_| FinanceError::Invalid(format!("{field} 最多 4 位小数：{text}")))
}

fn parse_amount(text: &str, field: &str) -> Result<i64, FinanceError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    Fixed::parse(trimmed, SCALE_AMOUNT)
        .map(|f| f.raw())
        .map_err(|_| FinanceError::Invalid(format!("{field} 最多 2 位小数：{text}")))
}

fn validate_trade_date(date: &str) -> Result<String, FinanceError> {
    let trimmed = date.trim();
    let bytes = trimmed.as_bytes();
    let ok = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !ok {
        return Err(FinanceError::Invalid(format!(
            "交易日期需为 YYYY-MM-DD：{date}"
        )));
    }
    Ok(trimmed.to_string())
}

fn to_view(row: &FlowRow) -> FlowView {
    FlowView {
        id: row.id,
        account_id: row.account_id,
        instrument_id: row.instrument_id,
        kind: row.kind.clone(),
        quantity: fmt_qty(row.quantity_raw),
        price: fmt_qty(row.price_raw),
        amount: fmt_amount(row.amount_raw),
        fee: fmt_amount(row.fee_raw),
        split_from: row.split_from,
        split_to: row.split_to,
        trade_date: row.trade_date.clone(),
        note: row.note.clone(),
    }
}

pub fn list_flows(
    store: &Store,
    account_id: Option<i64>,
    instrument_id: Option<i64>,
) -> Result<Vec<FlowView>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, instrument_id, type, quantity_raw, price_raw,
                    amount_raw, fee_raw, split_from, split_to, trade_date, note
             FROM flows
             WHERE (?1 IS NULL OR account_id = ?1)
               AND (?2 IS NULL OR instrument_id = ?2)
             ORDER BY trade_date DESC, id DESC
             LIMIT 500",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, instrument_id], |r| {
                Ok(FlowRow {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    instrument_id: r.get(2)?,
                    kind: r.get(3)?,
                    quantity_raw: r.get(4)?,
                    price_raw: r.get(5)?,
                    amount_raw: r.get(6)?,
                    fee_raw: r.get(7)?,
                    split_from: r.get(8)?,
                    split_to: r.get(9)?,
                    trade_date: r.get(10)?,
                    note: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.iter().map(to_view).collect())
    })
}

fn read_flows_for_pair(
    store: &Store,
    account_id: i64,
    instrument_id: i64,
) -> Result<Vec<FlowRow>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, instrument_id, type, quantity_raw, price_raw,
                    amount_raw, fee_raw, split_from, split_to, trade_date, note
             FROM flows
             WHERE account_id=?1 AND instrument_id=?2
             ORDER BY trade_date, id",
        )?;
        let rows = stmt
            .query_map([account_id, instrument_id], |r| {
                Ok(FlowRow {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    instrument_id: r.get(2)?,
                    kind: r.get(3)?,
                    quantity_raw: r.get(4)?,
                    price_raw: r.get(5)?,
                    amount_raw: r.get(6)?,
                    fee_raw: r.get(7)?,
                    split_from: r.get(8)?,
                    split_to: r.get(9)?,
                    trade_date: r.get(10)?,
                    note: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// 录入一笔流水。校验通过后写入，并返回该笔记录。
pub fn create_flow(store: &Store, payload: &CreateFlow) -> Result<FlowView, FinanceError> {
    if !FLOW_TYPES.contains(&payload.kind.as_str()) {
        return Err(FinanceError::Invalid(format!(
            "流水类型非法：{}（允许 {}）",
            payload.kind,
            FLOW_TYPES.join(" / ")
        )));
    }
    let trade_date = validate_trade_date(&payload.trade_date)?;
    let quantity = parse_qty(&payload.quantity, "数量")?;
    let price = parse_qty(&payload.price, "价格")?;
    let amount = parse_amount(&payload.amount, "金额")?;
    let fee = parse_amount(&payload.fee, "费用")?;

    if quantity < 0 || price < 0 || amount < 0 || fee < 0 {
        return Err(FinanceError::Invalid(
            "数量、价格、金额、费用不得为负".into(),
        ));
    }

    // 分类型校验：避免出现「记账上说得通、回放时算不出」的组合
    match payload.kind.as_str() {
        "BUY" | "SELL" => {
            if quantity == 0 {
                return Err(FinanceError::Invalid("买入/卖出必须给出数量".into()));
            }
        }
        "DIVIDEND" => {
            if amount == 0 {
                return Err(FinanceError::Invalid("分红必须给出金额".into()));
            }
            if quantity != 0 {
                return Err(FinanceError::Invalid("分红不改变数量，请勿填数量".into()));
            }
        }
        "SPLIT" => {
            let from = payload.split_from.unwrap_or(0);
            let to = payload.split_to.unwrap_or(0);
            if from <= 0 || to <= 0 {
                return Err(FinanceError::Invalid(
                    "拆股/送转必须给出正整数比例，如 1 拆 2 填 1 和 2".into(),
                ));
            }
            if quantity != 0 || amount != 0 {
                return Err(FinanceError::Invalid(
                    "拆股只改数量比例，不填数量与金额".into(),
                ));
            }
        }
        "FEE" => {
            if amount == 0 {
                return Err(FinanceError::Invalid("费用必须给出金额".into()));
            }
        }
        _ => {}
    }

    // 幂等：同一 request_id 重复提交返回既有记录
    if let Some(rid) = payload.request_id.as_deref() {
        let rid = rid.trim();
        if !rid.is_empty() {
            if let Some(existing) = find_by_request_id(store, rid)? {
                return Ok(existing);
            }
        }
    }

    // 回放校验：把新流水并进去重算，任何时点数量为负都拒绝
    let mut rows = read_flows_for_pair(store, payload.account_id, payload.instrument_id)?;
    let mut candidate = FlowRow {
        id: i64::MAX,
        account_id: payload.account_id,
        instrument_id: payload.instrument_id,
        kind: payload.kind.clone(),
        quantity_raw: quantity,
        price_raw: price,
        amount_raw: amount,
        fee_raw: fee,
        split_from: payload.split_from,
        split_to: payload.split_to,
        trade_date: trade_date.clone(),
        note: payload.note.trim().to_string(),
    };
    rows.push(candidate.clone());
    rows.sort_by(|a, b| (a.trade_date.as_str(), a.id).cmp(&(b.trade_date.as_str(), b.id)));
    replay(&rows).map_err(|e| FinanceError::Invalid(format!("这笔流水会让持仓不成立：{e}")))?;

    let ts = now_secs();
    let request_id = payload
        .request_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    store.with_write(|conn| {
        // 账户与标的必须存在且未归档
        let account_ok: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND archived_at IS NULL)",
            [payload.account_id],
            |r| r.get(0),
        )?;
        if !account_ok {
            return Err(FinanceError::NotFound(format!(
                "账户 {} 不存在或已归档",
                payload.account_id
            )));
        }
        let instrument_ok: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM instruments WHERE id=?1 AND archived_at IS NULL)",
            [payload.instrument_id],
            |r| r.get(0),
        )?;
        if !instrument_ok {
            return Err(FinanceError::NotFound(format!(
                "标的 {} 不存在或已归档",
                payload.instrument_id
            )));
        }

        conn.execute(
            "INSERT INTO flows(account_id, instrument_id, type, quantity_raw, price_raw,
                               amount_raw, fee_raw, split_from, split_to, trade_date,
                               note, request_id, created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![
                payload.account_id,
                payload.instrument_id,
                payload.kind,
                quantity,
                price,
                amount,
                fee,
                payload.split_from,
                payload.split_to,
                trade_date,
                payload.note.trim(),
                request_id,
                ts
            ],
        )?;
        candidate.id = conn.last_insert_rowid();
        Ok(to_view(&candidate))
    })
}

fn find_by_request_id(store: &Store, rid: &str) -> Result<Option<FlowView>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, instrument_id, type, quantity_raw, price_raw,
                    amount_raw, fee_raw, split_from, split_to, trade_date, note
             FROM flows WHERE request_id=?1",
        )?;
        let mut rows = stmt.query_map([rid], |r| {
            Ok(FlowRow {
                id: r.get(0)?,
                account_id: r.get(1)?,
                instrument_id: r.get(2)?,
                kind: r.get(3)?,
                quantity_raw: r.get(4)?,
                price_raw: r.get(5)?,
                amount_raw: r.get(6)?,
                fee_raw: r.get(7)?,
                split_from: r.get(8)?,
                split_to: r.get(9)?,
                trade_date: r.get(10)?,
                note: r.get(11)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(to_view(&row?))),
            None => Ok(None),
        }
    })
}

/// 删除一笔流水。删除后重放必须仍然成立，否则拒绝（防止把后续卖出的依据删掉）。
pub fn delete_flow(store: &Store, id: i64) -> Result<(), FinanceError> {
    let account_instrument = store.with_read(|conn| {
        conn.query_row(
            "SELECT account_id, instrument_id FROM flows WHERE id=?1",
            [id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                FinanceError::NotFound(format!("流水 {id} 不存在"))
            }
            other => FinanceError::Db(other.to_string()),
        })
    })?;

    let remaining: Vec<FlowRow> =
        read_flows_for_pair(store, account_instrument.0, account_instrument.1)?
            .into_iter()
            .filter(|f| f.id != id)
            .collect();
    replay(&remaining)
        .map_err(|e| FinanceError::Conflict(format!("删除这笔流水会让后续记录不成立：{e}")))?;

    store.with_write(|conn| {
        conn.execute("DELETE FROM flows WHERE id=?1", [id])?;
        Ok(())
    })
}
