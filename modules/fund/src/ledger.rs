//! ledger 模块：交易流水（实施 B1）。
//!
//! - 已确认交易才改变持仓；待确认（pending）单独记录，不参与回放。
//! - 手工提交以 `request_id` 幂等：同 ID 同内容返回原结果；同 ID 不同内容
//!   返回冲突；同日同账户同基金允许多笔合法交易（无 (account,fund,date,type) 唯一键）。
//! - 超卖在写入时拒绝（比上游 warning 更严，实施方案 §7.2）。
//! - 金额语义：买入成本 = 金额 + 费用；卖出已实现收益 = 金额 − 费用 − 按移动
//!   平均成本法扣除的成本。精度：份额/净值 4 位、金额/费用 2 位（fixed）。

use crate::fixed::Fixed;
#[cfg(test)]
use crate::fixed::SCALE_SHARE;
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};

pub const TYPE_BUY: &str = "BUY";
pub const TYPE_SELL: &str = "SELL";
pub const STATE_CONFIRMED: &str = "confirmed";
pub const STATE_PENDING: &str = "pending";

#[derive(Debug, PartialEq, Eq)]
pub enum LedgerError {
    /// 超卖：当前可卖份额不足（写入时拒绝）。
    Oversell {
        available_raw: i64,
    },
    /// request_id 已存在但内容不同。
    Conflict(String),
    /// 账户或基金不存在。
    NotFound(String),
    /// 精度或字段非法。
    Invalid(String),
    Db(String),
}

impl From<rusqlite::Error> for LedgerError {
    fn from(e: rusqlite::Error) -> Self {
        LedgerError::Db(e.to_string())
    }
}

impl From<crate::portfolio::PortfolioError> for LedgerError {
    fn from(e: crate::portfolio::PortfolioError) -> Self {
        match e {
            crate::portfolio::PortfolioError::Oversell { available_raw, .. } => {
                LedgerError::Oversell { available_raw }
            }
            crate::portfolio::PortfolioError::Db(e) => LedgerError::Db(e),
        }
    }
}

impl LedgerError {
    pub fn code(&self) -> &'static str {
        match self {
            LedgerError::Oversell { .. } => "LEDGER_OVERSELL",
            LedgerError::Conflict(_) => "LEDGER_CONFLICT",
            LedgerError::NotFound(_) => "LEDGER_NOT_FOUND",
            LedgerError::Invalid(_) => "LEDGER_INVALID",
            LedgerError::Db(_) => "LEDGER_DB",
        }
    }

    pub fn message(&self) -> String {
        match self {
            LedgerError::Oversell { available_raw } => {
                format!("可卖份额不足：当前 {}", Fixed::nav(*available_raw))
            }
            LedgerError::Conflict(id) => format!("requestId {id} 已用于不同内容"),
            LedgerError::NotFound(what) => format!("不存在：{what}"),
            LedgerError::Invalid(what) => format!("非法输入：{what}"),
            LedgerError::Db(e) => format!("数据库错误：{e}"),
        }
    }
}

/// 一笔待记录交易（统一入口输入：手工记账与 CSV 导入共用）。
#[derive(Clone, Debug)]
pub struct TransactionInput {
    pub account_name: String,
    pub fund_code: String,
    pub fund_name: String,
    pub tx_type: String,       // BUY / SELL
    pub state: String,         // confirmed / pending
    pub quantity: Fixed,       // 4 位
    pub price: Fixed,          // 4 位
    pub fee: Fixed,            // 2 位
    pub amount: Option<Fixed>, // 2 位，可选（缺省由 quantity × price 推导）
    pub trade_date: String,    // YYYY-MM-DD
    pub source: String,        // manual / import
    pub request_id: Option<String>,
    pub external_id: Option<String>,
    pub batch_id: Option<String>,
    pub line_no: Option<i64>,
    pub payload_hash: Option<String>,
}

impl TransactionInput {
    pub fn manual(
        account_name: String,
        fund_code: String,
        fund_name: String,
        tx_type: String,
        state: String,
        quantity: Fixed,
        price: Fixed,
        fee: Fixed,
        trade_date: String,
        request_id: String,
    ) -> Self {
        Self {
            account_name,
            fund_code,
            fund_name,
            tx_type,
            state,
            quantity,
            price,
            fee,
            amount: None,
            trade_date,
            source: "manual".into(),
            request_id: Some(request_id),
            external_id: None,
            batch_id: None,
            line_no: None,
            payload_hash: None,
        }
    }

    fn validate(&self) -> Result<(), LedgerError> {
        if self.tx_type != TYPE_BUY && self.tx_type != TYPE_SELL {
            return Err(LedgerError::Invalid(format!("type {}", self.tx_type)));
        }
        if self.state != STATE_CONFIRMED && self.state != STATE_PENDING {
            return Err(LedgerError::Invalid(format!("state {}", self.state)));
        }
        if self.quantity.is_negative() || self.quantity.is_zero() {
            return Err(LedgerError::Invalid("数量必须为正".into()));
        }
        if self.price.is_negative() {
            return Err(LedgerError::Invalid("价格不能为负".into()));
        }
        if self.fee.is_negative() {
            return Err(LedgerError::Invalid("费用不能为负".into()));
        }
        if let Some(amt) = &self.amount {
            if amt.is_negative() {
                return Err(LedgerError::Invalid("金额不能为负".into()));
            }
        }
        if self.trade_date.len() != 10
            || !self
                .trade_date
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-')
        {
            return Err(LedgerError::Invalid(format!(
                "trade_date {}",
                self.trade_date
            )));
        }
        if let Some(ref rid) = self.request_id {
            if rid.is_empty() || rid.len() > 128 {
                return Err(LedgerError::Invalid("requestId 长度须为 1..=128".into()));
            }
        }
        Ok(())
    }

    /// 金额 = 指定金额 或 数量 × 价格（ROUND_DOWN 到 2 位）。
    pub fn amount(&self) -> Result<Fixed, LedgerError> {
        if let Some(amt) = self.amount {
            Ok(amt)
        } else {
            self.quantity
                .checked_mul_to(&self.price, 2)
                .ok_or_else(|| LedgerError::Invalid("金额溢出".into()))
        }
    }
}

/// 取或创建账户，返回 id。
pub fn ensure_account(conn: &Connection, name: &str) -> Result<i64, LedgerError> {
    if name.trim().is_empty() {
        return Err(LedgerError::Invalid("账户名不能为空".into()));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts(name, created_at, updated_at) VALUES(?1, ?2, ?2)
         ON CONFLICT(name) DO UPDATE SET updated_at = ?2",
        params![name.trim(), now],
    )?;
    conn.query_row(
        "SELECT id FROM accounts WHERE name = ?1",
        params![name.trim()],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

/// 取或创建基金主档，返回 id。
pub fn ensure_fund(conn: &Connection, code: &str, name: &str) -> Result<i64, LedgerError> {
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err(LedgerError::Invalid(format!(
            "基金代码 {code} 须为 6 位数字"
        )));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO funds(code, name, updated_at) VALUES(?1, ?2, ?3)
         ON CONFLICT(code) DO UPDATE SET
           name = CASE WHEN ?2 != '' THEN ?2 ELSE funds.name END,
           updated_at = ?3",
        params![code, name.trim(), now],
    )?;
    conn.query_row("SELECT id FROM funds WHERE code = ?1", params![code], |r| {
        r.get(0)
    })
    .map_err(Into::into)
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// 统一写入入口（单事务内执行，供手工记账与 CSV 导入共用）。
///
/// 流程：
/// 1. 输入校验与金额确定；
/// 2. requestId / externalId 幂等命中检查；
/// 3. 账户与基金自动建档/匹配；
/// 4. 插入流水；
/// 5. 若为已确认流水，通过 `recompute_position`（全时序回放）进行超卖检查与持仓重算；
///    若存在任何时序超卖，底层回放立即报错，促使外层事务完整回滚。
pub fn record_transaction_tx(
    conn: &Connection,
    input: &TransactionInput,
) -> Result<(i64, bool), LedgerError> {
    input.validate()?;
    let amount = input.amount()?;

    // 1. requestId 幂等检查
    if let Some(ref rid) = input.request_id {
        let row: Option<(i64, String, i64, i64, i64, i64, String)> = conn
            .query_row(
                "SELECT id, type, quantity_raw, price_raw, fee_raw, amount_raw, trade_date
                 FROM transactions WHERE request_id = ?1",
                params![rid],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional()?;
        if let Some(existing) = row {
            let same = existing.1 == input.tx_type
                && existing.2 == input.quantity.raw()
                && existing.3 == input.price.raw()
                && existing.4 == input.fee.raw()
                && existing.5 == amount.raw()
                && existing.6 == input.trade_date;
            if same {
                return Ok((existing.0, false));
            } else {
                return Err(LedgerError::Conflict(rid.clone()));
            }
        }
    }

    // 2. externalId 幂等检查（若账户已存在）
    if let Some(ref ext) = input.external_id {
        let existing_acc_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM accounts WHERE name = ?1",
                params![input.account_name.trim()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(acc_id) = existing_acc_id {
            let existing_tx_id: Option<i64> = conn
                .query_row(
                    "SELECT id FROM transactions WHERE account_id = ?1 AND external_id = ?2",
                    params![acc_id, ext],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(tx_id) = existing_tx_id {
                return Ok((tx_id, false));
            }
        }
    }

    // 3. 账户与基金自动建档/匹配
    let account_id = ensure_account(conn, &input.account_name)?;
    let fund_id = ensure_fund(conn, &input.fund_code, &input.fund_name)?;

    // 4. 插入 transactions 流水
    let now = now_iso();
    let source = if input.source.is_empty() {
        "manual"
    } else {
        input.source.as_str()
    };
    conn.execute(
        "INSERT INTO transactions(account_id, fund_id, type, state, quantity_raw, price_raw,
             amount_raw, fee_raw, trade_date, source, request_id, external_id,
             batch_id, line_no, payload_hash, created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            account_id,
            fund_id,
            input.tx_type,
            input.state,
            input.quantity.raw(),
            input.price.raw(),
            amount.raw(),
            input.fee.raw(),
            input.trade_date,
            source,
            input.request_id,
            input.external_id,
            input.batch_id,
            input.line_no,
            input.payload_hash,
            now,
        ],
    )?;
    let tx_id = conn.last_insert_rowid();

    // 5. 若已确认，时序超卖检查与持仓重算
    // recompute_position 会按 (trade_date, id) 全量回放。
    // 若在任何时刻发生超卖，返回 PortfolioError::Oversell，促使当前事务回滚。
    if input.state == STATE_CONFIRMED {
        crate::portfolio::recompute_position(conn, account_id, fund_id)?;
    }

    Ok((tx_id, true))
}

/// 记录一笔手工交易（幂等）；已确认交易在事务内同步重算持仓投影。
/// 返回 (transaction_id, created)：created=false 表示重复提交命中原结果。
pub fn record_transaction(
    store: &Store,
    input: &TransactionInput,
) -> Result<(i64, bool), LedgerError> {
    store.with_write(|conn| record_transaction_tx(conn, input))
}

/// 交易列表（分页由调用方限制），按日期倒序。
pub struct TransactionRow {
    pub id: i64,
    pub account_name: String,
    pub fund_code: String,
    pub tx_type: String,
    pub state: String,
    pub quantity_raw: i64,
    pub price_raw: i64,
    pub amount_raw: i64,
    pub fee_raw: i64,
    pub trade_date: String,
    pub source: String,
}

pub fn list_transactions(store: &Store, limit: usize) -> Result<Vec<TransactionRow>, LedgerError> {
    store
        .with_read(|conn| -> rusqlite::Result<Vec<TransactionRow>> {
            let mut stmt = conn.prepare(
                "SELECT t.id, a.name, f.code, t.type, t.state, t.quantity_raw, t.price_raw,
                        t.amount_raw, t.fee_raw, t.trade_date, t.source
                 FROM transactions t
                 JOIN accounts a ON a.id = t.account_id
                 JOIN funds f ON f.id = t.fund_id
                 ORDER BY t.trade_date DESC, t.id DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit as i64], |r| {
                    Ok(TransactionRow {
                        id: r.get(0)?,
                        account_name: r.get(1)?,
                        fund_code: r.get(2)?,
                        tx_type: r.get(3)?,
                        state: r.get(4)?,
                        quantity_raw: r.get(5)?,
                        price_raw: r.get(6)?,
                        amount_raw: r.get(7)?,
                        fee_raw: r.get(8)?,
                        trade_date: r.get(9)?,
                        source: r.get(10)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| LedgerError::Db(e.to_string()))
}

#[cfg(test)]
mod tests;
