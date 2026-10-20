//! 交易现金衔接：当前手工买入原子扣款；历史导入待核对，卖出到账后才可用。
use crate::finance::{self, FinanceError};
use crate::ledger::{LedgerError, TransactionInput};
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) fn record(
    c: &Connection,
    id: i64,
    account: i64,
    input: &TransactionInput,
    amount: i64,
) -> Result<(), LedgerError> {
    if input.state != "confirmed" {
        return Ok(());
    }
    // 账户间转账是成本平移的内部记账，不是市场买卖：金额双边对冲，
    // 家庭现金不变动，不生成 trade_cash 待核对记录（方案 03 费用语义）。
    if input.source == "transfer" {
        return Ok(());
    }
    // 场外基金转换（N09）：转出按赎回结转已实现收益，转入按转入金额入
    // 成本，资金在基金之间直接过渡，不进出家庭现金。
    if input.source == "otc_convert" {
        return Ok(());
    }
    let existing: Option<String> = c
        .query_row(
            "SELECT valued_at FROM manual_assets WHERE account_id=?1 AND product_type='cash'",
            [account],
            |r| r.get(0),
        )
        .optional()?;
    // 未建现金账的旧流水沿用原有持仓语义；首次现金建账作为资金基线。
    let Some(cash_date) = existing else {
        return Ok(());
    };
    let delta = if input.tx_type == "BUY" {
        amount
            .checked_add(input.fee.raw())
            .and_then(i64::checked_neg)
    } else {
        amount.checked_sub(input.fee.raw()).filter(|v| *v >= 0)
    }
    .ok_or_else(|| LedgerError::Invalid("交易金额或费用溢出／卖出费超过收入".into()))?;
    let current = input.source == "manual"
        && input.trade_date == finance::today_cn()
        && cash_date == input.trade_date;
    let status = if !current {
        "unreconciled"
    } else if input.tx_type == "SELL" {
        "receivable"
    } else {
        "settled"
    };
    if status == "settled" {
        let available = finance::cash_position(c)
            .map_err(|e| LedgerError::Invalid(e.message()))?
            .accounts
            .into_iter()
            .find(|a| a.account_id == account)
            .and_then(|a| a.available_minor)
            .ok_or_else(|| LedgerError::Invalid("先核对付款账户预留与现金".into()))?;
        if -delta > available {
            return Err(LedgerError::Invalid(
                "交易金额超过可用现金；已有占用请先取消或按真实成交核对".into(),
            ));
        }
        finance::adjust_account_cash(c, account, delta, &input.trade_date)
            .map_err(|e| LedgerError::Invalid(e.message()))?;
    }
    c.execute("INSERT INTO trade_cash(tx_id,account_id,delta_minor,status,settled_at,changed_revision) VALUES(?1,?2,?3,?4,?5,(SELECT CAST(value AS INTEGER)+1 FROM app_meta WHERE key='data_revision'))",params![id,account,delta,status,if status=="settled"{input.trade_date.as_str()}else{""}])?;
    Ok(())
}

pub fn settle(
    c: &Connection,
    id: i64,
    mode: &str,
    date: &str,
    note: &str,
) -> Result<(), FinanceError> {
    if !crate::assets::is_valid_calendar_date(date)
        || date > finance::today_cn().as_str()
        || note.len() > 4096
    {
        return Err(FinanceError::Invalid("到账日期或说明无效".into()));
    }
    let row:Option<(i64,i64,String,String)>=c.query_row("SELECT x.account_id,x.delta_minor,x.status,t.trade_date FROM trade_cash x JOIN transactions t ON t.id=x.tx_id WHERE x.tx_id=?1 AND t.deleted_at IS NULL",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let (account, delta, status, traded) =
        row.ok_or_else(|| FinanceError::NotFound("交易资金记录".into()))?;
    if status == "settled" {
        return Err(FinanceError::Conflict(
            "交易资金已核对，不能再次调整".into(),
        ));
    }
    if date < traded.as_str() {
        return Err(FinanceError::Invalid("到账日期早于交易日期".into()));
    }
    match mode {
        "post" => {
            if delta < 0 {
                let available = finance::cash_position(c)?
                    .accounts
                    .into_iter()
                    .find(|a| a.account_id == account)
                    .and_then(|a| a.cash_minor)
                    .ok_or_else(|| FinanceError::Invalid("账户现金未知".into()))?;
                if available < -delta {
                    return Err(FinanceError::Invalid("现金不足以核对扣款".into()));
                }
            }
            finance::adjust_account_cash(c, account, delta, date)?;
        }
        "included" if status == "unreconciled" => {}
        _ => {
            return Err(FinanceError::Invalid(
                "请选择实际到账记账，或确认历史现金已包含该笔交易".into(),
            ))
        }
    }
    c.execute("UPDATE trade_cash SET status='settled',settled_at=?2,note=?3,changed_revision=(SELECT CAST(value AS INTEGER)+1 FROM app_meta WHERE key='data_revision') WHERE tx_id=?1",params![id,date,note])?;
    Ok(())
}
