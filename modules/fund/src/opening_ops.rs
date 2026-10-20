//! 期初记录更正／删除／恢复：版本检查、审计快照和持仓回放共用一个写事务。
use crate::{fixed::Fixed, ledger::LedgerError, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub fn list(store: &Store, account: i64, fund: i64) -> Result<Value, LedgerError> {
    store.with_read(|c|{
  let opening:Option<Value>=c.query_row("SELECT quantity_raw,cost_raw,as_of,version,deleted_at FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2",params![account,fund],|r|Ok(json!({"quantity":Fixed::nav(r.get(0)?).to_string_value(),"totalCost":r.get::<_,Option<i64>>(1)?.map(|v|Fixed::amount(v).to_string_value()),"asOf":r.get::<_,String>(2)?,"version":r.get::<_,i64>(3)?,"deleted":r.get::<_,Option<String>>(4)?.is_some()}))).optional()?;
  let mut s=c.prepare("SELECT version,payload_json,created_at FROM opening_history WHERE account_id=?1 AND fund_id=?2 ORDER BY id DESC LIMIT 50")?;
  let history=s.query_map(params![account,fund],|r|Ok(json!({"version":r.get::<_,i64>(0)?,"record":serde_json::from_str::<Value>(&r.get::<_,String>(1)?).unwrap_or(Value::Null),"recordedAt":r.get::<_,String>(2)?})))?.collect::<Result<Vec<_>,_>>()?;
  Ok(json!({"opening":opening,"history":history}))
 })
}

pub fn change(
    store: &Store,
    account: i64,
    fund: i64,
    expected: i64,
    op: &str,
    input: &Value,
) -> Result<(), LedgerError> {
    store.with_write(|c| change_tx(c, account, fund, expected, op, input))
}

pub(crate) fn change_tx(
    c: &rusqlite::Connection,
    account: i64,
    fund: i64,
    expected: i64,
    op: &str,
    input: &Value,
) -> Result<(), LedgerError> {
    let (qty,cost,date,version,deleted):(i64,Option<i64>,String,i64,Option<String>)=c.query_row("SELECT quantity_raw,cost_raw,as_of,version,deleted_at FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2",params![account,fund],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or_else(||LedgerError::NotFound("期初记录".into()))?;
    if version != expected {
        return Err(LedgerError::Conflict("期初版本已变化，请刷新".into()));
    }
    let now = crate::finance::now_secs().to_string();
    c.execute("INSERT INTO opening_history(account_id,fund_id,version,payload_json,created_at) VALUES(?1,?2,?3,?4,?5)",params![account,fund,version,json!({"quantity":qty.to_string(),"cost":cost.map(|v|v.to_string()),"asOf":date,"deleted":deleted}).to_string(),now])?;
    match op {
        "update" if deleted.is_none() => {
            let quantity = Fixed::parse(input["quantity"].as_str().unwrap_or(""), 4)
                .map_err(|_| LedgerError::Invalid("数量精度无效".into()))?;
            if quantity.raw() <= 0 {
                return Err(LedgerError::Invalid("数量须为正".into()));
            }
            let kind: String = c.query_row(
                "SELECT instrument_kind FROM funds WHERE id=?1",
                [fund],
                |r| r.get(0),
            )?;
            if kind == "stock" && quantity.raw() % 10_000 != 0 {
                return Err(LedgerError::Invalid("股票数量须为整数股".into()));
            }
            let cost = input["totalCost"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| Fixed::parse(s, 2))
                .transpose()
                .map_err(|_| LedgerError::Invalid("成本精度无效".into()))?;
            if cost.is_some_and(|v| v.raw() < 0) {
                return Err(LedgerError::Invalid("成本不能为负".into()));
            }
            let date = input["asOf"].as_str().unwrap_or("");
            if !crate::assets::is_valid_calendar_date(date)
                || date > crate::finance::today_cn().as_str()
            {
                return Err(LedgerError::Invalid("期初日期无效".into()));
            }
            let first:Option<String>=c.query_row("SELECT MIN(trade_date) FROM transactions WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",params![account,fund],|r|r.get(0))?;
            if first.is_some_and(|first| date > first.as_str()) {
                return Err(LedgerError::Invalid("期初不能晚于交易日期".into()));
            }
            c.execute("UPDATE opening_snapshots SET quantity_raw=?3,cost_raw=?4,as_of=?5,version=version+1 WHERE account_id=?1 AND fund_id=?2",params![account,fund,quantity.raw(),cost.map(|v|v.raw()),date])?;
        }
        "delete" if deleted.is_none() => {
            c.execute("UPDATE opening_snapshots SET deleted_at=?3,version=version+1 WHERE account_id=?1 AND fund_id=?2",params![account,fund,now])?;
        }
        "restore" if deleted.is_some() => {
            c.execute("UPDATE opening_snapshots SET deleted_at=NULL,version=version+1 WHERE account_id=?1 AND fund_id=?2",params![account,fund])?;
        }
        _ => return Err(LedgerError::Invalid("期初操作或当前状态无效".into())),
    }
    crate::portfolio::recompute_position(c, account, fund)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger;
    #[test]
    fn stock_opening_versions_and_replay_are_atomic() {
        let s = Store::open_in_memory().unwrap();
        let date = crate::finance::today_cn();
        let input = ledger::OpeningInput {
            account_name: "A".into(),
            fund_code: "stock:SSE:600519".into(),
            fund_name: "Stock".into(),
            quantity: Fixed::nav(10_000),
            as_of: date.clone(),
            total_cost: None,
        };
        let id = ledger::record_opening(&s, &input, &date).unwrap();
        let update = json!({"quantity":"2","totalCost":"100","asOf":date});
        change(&s, 1, id, 1, "update", &update).unwrap();
        assert!(change(&s, 1, id, 1, "update", &update).is_err());
        change(&s, 1, id, 2, "delete", &Value::Null).unwrap();
        assert!(s
            .with_read(crate::portfolio::list_positions)
            .unwrap()
            .is_empty());
        change(&s, 1, id, 3, "restore", &Value::Null).unwrap();
        assert_eq!(
            s.with_read(crate::portfolio::list_positions).unwrap()[0].quantity_raw,
            20_000
        );
        let mut invalid = update.clone();
        invalid["quantity"] = json!("1.5");
        assert!(change(&s, 1, id, 4, "update", &invalid).is_err());
        assert_eq!(
            list(&s, 1, id).unwrap()["history"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}
