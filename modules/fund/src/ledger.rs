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
mod tests {
    use super::*;
    use crate::fixed::Fixed;
    use crate::storage::Store;

    fn buy(
        store: &Store,
        account: &str,
        code: &str,
        qty: &str,
        price: &str,
        fee: &str,
        date: &str,
        rid: &str,
    ) -> (i64, bool) {
        record_transaction(
            store,
            &TransactionInput::manual(
                account.into(),
                code.into(),
                "测试基金".into(),
                TYPE_BUY.into(),
                STATE_CONFIRMED.into(),
                Fixed::parse(qty, SCALE_SHARE).unwrap(),
                Fixed::parse(price, SCALE_SHARE).unwrap(),
                Fixed::parse(fee, 2).unwrap(),
                date.into(),
                rid.into(),
            ),
        )
        .unwrap()
    }

    fn sell(
        store: &Store,
        account: &str,
        code: &str,
        qty: &str,
        price: &str,
        fee: &str,
        date: &str,
        rid: &str,
    ) -> Result<(i64, bool), LedgerError> {
        record_transaction(
            store,
            &TransactionInput::manual(
                account.into(),
                code.into(),
                "测试基金".into(),
                TYPE_SELL.into(),
                STATE_CONFIRMED.into(),
                Fixed::parse(qty, SCALE_SHARE).unwrap(),
                Fixed::parse(price, SCALE_SHARE).unwrap(),
                Fixed::parse(fee, 2).unwrap(),
                date.into(),
                rid.into(),
            ),
        )
    }

    /// 实施方案 §7.2 黄金用例（B-G2）。
    #[test]
    fn golden_case_moving_average_cost() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), LedgerError> {
                ensure_account(conn, "默认账户")?;
                ensure_fund(conn, "000001", "测试基金")?;
                Ok(())
            })
            .unwrap();

        buy(
            &store,
            "默认账户",
            "000001",
            "100",
            "1.00",
            "1.00",
            "2026-01-05",
            "r1",
        );
        buy(
            &store,
            "默认账户",
            "000001",
            "50",
            "1.20",
            "0.50",
            "2026-01-06",
            "r2",
        );
        sell(
            &store,
            "默认账户",
            "000001",
            "60",
            "1.30",
            "0.50",
            "2026-01-07",
            "r3",
        )
        .unwrap();

        let position = store
            .with_read(|conn| crate::portfolio::replay_position(conn, 1, 1))
            .unwrap()
            .unwrap();
        // 剩余份额 90、成本 96.90
        assert_eq!(position.quantity_raw, 900_000);
        assert_eq!(position.cost_raw, 96_90);
        assert_eq!(position.realized_raw, 12_90);
    }

    #[test]
    fn oversell_rejected_at_write_time() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), LedgerError> {
                ensure_account(conn, "A")?;
                ensure_fund(conn, "000002", "X")?;
                Ok(())
            })
            .unwrap();
        buy(&store, "A", "000002", "10", "1.00", "0", "2026-01-05", "b1");
        let err = sell(&store, "A", "000002", "11", "1.00", "0", "2026-01-06", "s1").unwrap_err();
        assert!(matches!(
            err,
            LedgerError::Oversell {
                available_raw: 100_000
            }
        ));
        // 数据未被污染：持仓仍是 10。
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(position.quantity_raw, 100_000);
    }

    #[test]
    fn same_request_id_same_payload_is_idempotent() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), LedgerError> {
                ensure_account(conn, "A")?;
                ensure_fund(conn, "000003", "X")?;
                Ok(())
            })
            .unwrap();
        let (first, created) = buy(
            &store,
            "A",
            "000003",
            "10",
            "1.00",
            "0",
            "2026-01-05",
            "same",
        );
        assert!(created);
        let (second, created_again) = buy(
            &store,
            "A",
            "000003",
            "10",
            "1.00",
            "0",
            "2026-01-05",
            "same",
        );
        assert!(!created_again);
        assert_eq!(first, second);
        // 重复提交不倍增持仓。
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(position.quantity_raw, 100_000);
    }

    #[test]
    fn same_request_id_different_payload_conflicts() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), LedgerError> {
                ensure_account(conn, "A")?;
                ensure_fund(conn, "000004", "X")?;
                Ok(())
            })
            .unwrap();
        buy(
            &store,
            "A",
            "000004",
            "10",
            "1.00",
            "0",
            "2026-01-05",
            "dup",
        );
        // buy 助手内部 unwrap，直接用 record_transaction 验证冲突路径。
        let err = record_transaction(
            &store,
            &TransactionInput::manual(
                "A".into(),
                "000004".into(),
                "X".into(),
                TYPE_BUY.into(),
                STATE_CONFIRMED.into(),
                Fixed::parse("20", SCALE_SHARE).unwrap(),
                Fixed::parse("1.00", SCALE_SHARE).unwrap(),
                Fixed::zero(2),
                "2026-01-05".into(),
                "dup".into(),
            ),
        )
        .map(|_| ())
        .unwrap_err();
        assert!(matches!(err, LedgerError::Conflict(_)));
    }

    #[test]
    fn same_day_multiple_buys_allowed() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), LedgerError> {
                ensure_account(conn, "A")?;
                ensure_fund(conn, "000005", "X")?;
                Ok(())
            })
            .unwrap();
        buy(&store, "A", "000005", "10", "1.00", "0", "2026-01-05", "d1");
        buy(&store, "A", "000005", "10", "1.00", "0", "2026-01-05", "d2");
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(position.quantity_raw, 200_000);
    }

    #[test]
    fn pending_transaction_does_not_change_position() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), LedgerError> {
                ensure_account(conn, "A")?;
                ensure_fund(conn, "000006", "X")?;
                Ok(())
            })
            .unwrap();
        record_transaction(
            &store,
            &TransactionInput::manual(
                "A".into(),
                "000006".into(),
                "X".into(),
                TYPE_BUY.into(),
                STATE_PENDING.into(),
                Fixed::parse("10", SCALE_SHARE).unwrap(),
                Fixed::parse("1.00", SCALE_SHARE).unwrap(),
                Fixed::zero(2),
                "2026-01-05".into(),
                "p1".into(),
            ),
        )
        .unwrap();
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap();
        assert!(position.is_none(), "待确认交易不得改变持仓");
    }

    #[test]
    fn invalid_input_rejected() {
        let store = Store::open_in_memory().unwrap();
        // 负费用在写入校验时拒绝（精度超限由 Fixed::parse 在调用方拒绝，
        // 已在 fixed 测试覆盖；这里覆盖 ledger 自身校验）。
        let err = record_transaction(
            &store,
            &TransactionInput::manual(
                "A".into(),
                "000007".into(),
                "X".into(),
                TYPE_BUY.into(),
                STATE_CONFIRMED.into(),
                Fixed::parse("10", SCALE_SHARE).unwrap(),
                Fixed::parse("1.00", SCALE_SHARE).unwrap(),
                Fixed::parse("-1.00", 2).unwrap(),
                "2026-01-05".into(),
                "p2".into(),
            ),
        )
        .map(|_| ())
        .unwrap_err();
        assert!(matches!(err, LedgerError::Invalid(_)));
    }

    #[test]
    fn timeline_oversell_rejected_at_write_time() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), LedgerError> {
                ensure_account(conn, "A")?;
                ensure_fund(conn, "000008", "X")?;
                Ok(())
            })
            .unwrap();
        // 1. 2026-01-05 买入 10 份
        buy(&store, "A", "000008", "10", "1.00", "0", "2026-01-05", "b1");
        // 2. 2026-01-10 卖出 10 份（此时剩余 0 份）
        sell(&store, "A", "000008", "10", "1.00", "0", "2026-01-10", "s1").unwrap();

        // 3. 倒签在 2026-01-08 卖出 5 份：如果允许插入，会导致 2026-01-10 那笔卖出超卖（时序回放出现负份额）
        let err = sell(&store, "A", "000008", "5", "1.00", "0", "2026-01-08", "s2").unwrap_err();
        assert!(matches!(err, LedgerError::Oversell { .. }));

        // 验证原子回滚，流水库中没有这笔 s2，持仓保持正确
        let tx_count: i64 = store
            .with_read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM transactions WHERE request_id = 's2'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(tx_count, 0, "时序超卖事务必须完整回滚");
    }
}
