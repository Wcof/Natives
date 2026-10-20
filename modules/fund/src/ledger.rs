//! ledger 模块：交易流水（实施 B1）。
//!
//! - 已确认交易才改变持仓；待确认（pending）单独记录，不参与回放。
//! - 手工提交以 `request_id` 幂等：同 ID 同内容返回原结果；同 ID 不同内容
//!   返回冲突；同日同账户同基金允许多笔合法交易（无 (account,fund,date,type) 唯一键）。
//! - 超卖在写入时拒绝（比上游 warning 更严，实施方案 §7.2）。
//! - 金额语义：买入成本 = 金额 + 费用；卖出已实现收益 = 金额 − 费用 − 按移动
//!   平均成本法扣除的成本。精度：份额/净值 4 位、金额/费用 2 位（fixed）。

use crate::fixed::Fixed;
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
        validate_quantity(&self.fund_code, self.quantity)?;
        if !crate::assets::is_valid_calendar_date(&self.trade_date)
            || self.trade_date > crate::finance::today_cn()
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

/// 取或创建账户，返回 id。归档账户不参与自动建档／匹配（v19）：
/// 交易与导入按名命中已归档账户时明确拒绝，用户须先恢复或换名，
/// 避免新流水静默挂到已隐藏账户上。
pub fn ensure_account(conn: &Connection, name: &str) -> Result<i64, LedgerError> {
    if name.trim().is_empty() || name.chars().count() > 64 {
        return Err(LedgerError::Invalid("账户名不能为空".into()));
    }
    let archived: Option<i64> = conn
        .query_row(
            "SELECT id FROM accounts WHERE name = ?1 AND archived_at IS NOT NULL",
            params![name.trim()],
            |r| r.get(0),
        )
        .optional()?;
    if archived.is_some() {
        return Err(LedgerError::Conflict(format!(
            "账户「{}」已归档；请先在账户管理中恢复再记账",
            name.trim()
        )));
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
    let (kind, market, display) = instrument_parts(code)?;
    if name.chars().count() > 64 {
        return Err(LedgerError::Invalid("标的名称过长".into()));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO funds(code, name, updated_at,instrument_kind,market,display_code) VALUES(?1, ?2, ?3,?4,?5,?6)
         ON CONFLICT(code) DO UPDATE SET
           name = CASE WHEN ?2 != '' THEN ?2 ELSE funds.name END,
           updated_at = ?3",
        params![code, name.trim(), now,kind,market,display],
    )?;
    conn.query_row("SELECT id FROM funds WHERE code = ?1", params![code], |r| {
        r.get(0)
    })
    .map_err(Into::into)
}

pub fn instrument_parts(code: &str) -> Result<(&str, &str, &str), LedgerError> {
    let parts: Vec<_> = code.split(':').collect();
    let (kind, market, display) = match parts.as_slice() {
        [display] => ("fund", "CN", *display),
        [kind, market, display]
            if ["stock", "etf"].contains(kind) && ["SSE", "SZSE", "BSE"].contains(market) =>
        {
            (*kind, *market, *display)
        }
        _ => return Err(LedgerError::Invalid("证券身份无效".into())),
    };
    if display.len() != 6 || !display.chars().all(|c| c.is_ascii_digit()) {
        return Err(LedgerError::Invalid("代码须为六位数字".into()));
    }
    Ok((kind, market, display))
}
/// 手动登记的境内场外基金自动确认币种为 CNY：裸 6 位代码即境内公募
/// （前端添加表单 market 固定 CN），净值也以 CNY 公布；已有元数据行
/// （用户经估值对话框显式选择）保持不动。导入／旧基金路径不调用本函数，
/// 维持 assets 模块「旧基金币种须用户确认」的规则。
pub(crate) fn confirm_domestic_fund_currency(
    conn: &Connection,
    code: &str,
    fund_id: i64,
) -> rusqlite::Result<()> {
    let Ok((kind, _, _)) = instrument_parts(code) else {
        return Ok(());
    };
    if kind != "fund" {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO fund_asset_metadata(fund_id, nav_currency, risk_class, updated_at)
         VALUES(?1,'CNY','unknown',?2)
         ON CONFLICT(fund_id) DO NOTHING",
        params![fund_id, now_iso()],
    )?;
    Ok(())
}

pub(crate) fn validate_quantity(code: &str, quantity: Fixed) -> Result<(), LedgerError> {
    let (kind, _, _) = instrument_parts(code)?;
    if kind == "stock" && quantity.raw() % 10_000 != 0 {
        return Err(LedgerError::Invalid("股票数量须为整数股".into()));
    }
    Ok(())
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
            let identity: (String,String,String)=conn.query_row("SELECT a.name,f.code,t.state FROM transactions t JOIN accounts a ON a.id=t.account_id JOIN funds f ON f.id=t.fund_id WHERE t.id=?1",[existing.0],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            let same = identity.0 == input.account_name.trim()
                && identity.1 == input.fund_code
                && identity.2 == input.state
                && existing.1 == input.tx_type
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
    let opening_date:Option<String>=conn.query_row("SELECT as_of FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",params![account_id,fund_id],|r|r.get(0)).optional()?;
    if opening_date.is_some_and(|date| input.trade_date < date) {
        return Err(LedgerError::Invalid("交易不能早于期初日期".into()));
    }

    // 4. 插入 transactions 流水
    let now = now_iso();
    let source = if input.source.is_empty() {
        "manual"
    } else {
        input.source.as_str()
    };
    if source == "manual" {
        confirm_domestic_fund_currency(conn, &input.fund_code, fund_id)?;
    }
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

    crate::trade_cash::record(conn, tx_id, account_id, input, amount.raw())?;
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

// ---------------------------------------------------------------------------
// 期初登记（方案 01 §5.1：登记已有持仓；不虚构当天买入、不再次扣现金；
// 账户＋标的不能重复登记期初）。
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OpeningInput {
    pub account_name: String,
    pub fund_code: String,
    pub fund_name: String,
    pub quantity: Fixed,
    /// 截至日期（期初时点；必须是有效日历日且不晚于今天）。
    pub as_of: String,
    /// 总成本选填；缺省＝未知成本（回放显式 cost_unknown，不当作 0）。
    pub total_cost: Option<Fixed>,
}

pub fn record_opening_tx(
    conn: &Connection,
    input: &OpeningInput,
    today: &str,
) -> Result<i64, LedgerError> {
    if input.quantity.raw() <= 0 {
        return Err(LedgerError::Invalid("期初数量必须大于零".into()));
    }
    if !crate::assets::is_valid_calendar_date(&input.as_of) {
        return Err(LedgerError::Invalid(format!(
            "期初截至日期不是有效日历日：{}",
            input.as_of
        )));
    }
    if input.as_of.as_str() > today {
        return Err(LedgerError::Invalid("期初截至日期不能晚于今天".into()));
    }
    if let Some(cost) = &input.total_cost {
        if cost.raw() < 0 {
            return Err(LedgerError::Invalid("总成本不能为负".into()));
        }
    }
    let account_id = ensure_account(conn, &input.account_name)?;
    validate_quantity(&input.fund_code, input.quantity)?;
    let fund_id = ensure_fund(conn, &input.fund_code, &input.fund_name)?;
    confirm_domestic_fund_currency(conn, &input.fund_code, fund_id)?;
    let existing: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2",
            params![account_id, fund_id],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Err(LedgerError::Conflict(
            "该账户与标的已登记期初；期初不能重复登记，后续变化请用交易或更正流程".into(),
        ));
    }
    // 期初必须先于流水：该账户该标的已有流水时，期初会插在回放时序中间，
    // 破坏「期初＋流水按日期回放」的语义——明确拒绝。
    let tx_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
        params![account_id, fund_id],
        |r| r.get(0),
    )?;
    if tx_count > 0 {
        return Err(LedgerError::Conflict(
            "该账户与标的已有交易流水；期初只能用于存量登记，增量请记录交易".into(),
        ));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO opening_snapshots(account_id, fund_id, as_of, quantity_raw, cost_raw,
                                       derived_cost, nav_raw, nav_date, created_at)
         VALUES(?1,?2,?3,?4,?5,0,NULL,NULL,?6)",
        params![
            account_id,
            fund_id,
            input.as_of,
            input.quantity.raw(),
            input.total_cost.as_ref().map(|c| c.raw()),
            now
        ],
    )?;
    // 期初进入回放后同步持仓投影。
    crate::portfolio::recompute_position(conn, account_id, fund_id)?;
    Ok(fund_id)
}

/// 期初登记（外层封装：单事务；today 为本地日期 YYYY-MM-DD）。
pub fn record_opening(
    store: &Store,
    input: &OpeningInput,
    today: &str,
) -> Result<i64, LedgerError> {
    store.with_write(|conn| record_opening_tx(conn, input, today))
}

// ---------------------------------------------------------------------------
// 待确认（pending）→ 已确认（confirmed）。
// 方案 03 §2：pending→confirmed 只能生效一次；重复确认幂等返回。
// ---------------------------------------------------------------------------

/// 确认待确认交易。返回 created=true 表示本次生效；false 表示已是 confirmed（幂等）。
pub fn confirm_transaction_tx(conn: &Connection, tx_id: i64) -> Result<bool, LedgerError> {
    let row: Option<(i64, i64, String, Option<String>)> = conn
        .query_row(
            "SELECT account_id, fund_id, state, deleted_at FROM transactions WHERE id=?1",
            params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((account_id, fund_id, state, deleted_at)) = row else {
        return Err(LedgerError::NotFound(format!("交易 {tx_id}")));
    };
    if deleted_at.is_some() {
        return Err(LedgerError::Invalid("已删除交易不能确认".into()));
    }
    if state == STATE_CONFIRMED {
        return Ok(false);
    }
    if state != STATE_PENDING {
        return Err(LedgerError::Invalid(format!("未知交易状态：{state}")));
    }
    conn.execute(
        "UPDATE transactions SET state='confirmed',version=version+1 WHERE id=?1 AND state='pending'",
        params![tx_id],
    )?;
    // 确认即参与回放：同事务同步持仓投影并做时序超卖检查。
    crate::portfolio::recompute_position(conn, account_id, fund_id)?;
    let (kind, amount, fee, date, source): (String, i64, i64, String, String) = conn.query_row(
        "SELECT type,amount_raw,fee_raw,trade_date,source FROM transactions WHERE id=?1",
        [tx_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let mut input = TransactionInput::manual(
        String::new(),
        String::new(),
        String::new(),
        kind,
        STATE_CONFIRMED.into(),
        Fixed::zero(4),
        Fixed::zero(4),
        Fixed::amount(fee),
        date,
        String::new(),
    );
    input.source = source;
    crate::trade_cash::record(conn, tx_id, account_id, &input, amount)?;
    Ok(true)
}

pub fn confirm_transaction(store: &Store, tx_id: i64) -> Result<bool, LedgerError> {
    store.with_write(|conn| confirm_transaction_tx(conn, tx_id))
}

pub fn confirm_transaction_versioned(
    store: &Store,
    id: i64,
    version: i64,
) -> Result<bool, LedgerError> {
    store.with_write(|c| {
        let current: i64 =
            c.query_row("SELECT version FROM transactions WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
        if current != version {
            return Err(LedgerError::Conflict("交易版本已变化".into()));
        }
        confirm_transaction_tx(c, id)
    })
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
    pub deleted: bool,
    pub version: i64,
}

pub fn list_transactions(store: &Store, limit: usize) -> Result<Vec<TransactionRow>, LedgerError> {
    store
        .with_read(|conn| -> rusqlite::Result<Vec<TransactionRow>> {
            let mut stmt = conn.prepare(
                "SELECT t.id, a.name, f.code, t.type, t.state, t.quantity_raw, t.price_raw,
                        t.amount_raw, t.fee_raw, t.trade_date, t.source,t.deleted_at,t.version
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
                        deleted: r.get::<_, Option<String>>(11)?.is_some(),
                        version: r.get(12)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| LedgerError::Db(e.to_string()))
}

#[cfg(test)]
mod tests;
