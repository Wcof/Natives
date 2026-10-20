//! 线下退出事实的查询与应收确认；现金及退出状态在同一事务提交。
use crate::{
    finance::{self, CashEventInput},
    offline::OfflineError,
    storage::Store,
};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub fn exits_page(store: &Store, asset_id: i64, offset: i64) -> Result<Vec<Value>, OfflineError> {
    store.with_read(|c| {
        let mut s=c.prepare("SELECT id,exit_date,amount_minor,fee_minor,settled,remaining_minor,version,settlement_account_id FROM offline_exit_history WHERE asset_id=?1 ORDER BY id DESC LIMIT 50 OFFSET ?2")?;
        let rows=s.query_map([asset_id,offset.max(0)],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"exitDate":r.get::<_,String>(1)?,
            "amountMinor":r.get::<_,i64>(2)?.to_string(),"feeMinor":r.get::<_,i64>(3)?.to_string(),
            "settled":r.get::<_,bool>(4)?,"remainingMinor":r.get::<_,Option<i64>>(5)?.map(|v|v.to_string()),
            "version":r.get::<_,i64>(6)?,"accountId":r.get::<_,Option<i64>>(7)?})))?.collect::<Result<Vec<_>,_>>()?;
        Ok(rows)
    })
}

pub fn settle(
    store: &Store,
    id: i64,
    account: i64,
    expected: i64,
    date: &str,
) -> Result<(), OfflineError> {
    if !crate::assets::is_valid_calendar_date(date) || date > finance::today_cn().as_str() {
        return Err(OfflineError::Invalid("到账日期无效或在未来".into()));
    }
    store.with_write(|c| {
        let (asset,amount,fee,settled,version,old_account):(i64,i64,i64,bool,i64,Option<i64>)=c.query_row(
            "SELECT asset_id,amount_minor,fee_minor,settled,version,settlement_account_id FROM offline_exit_history WHERE id=?1",[id],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?.ok_or(OfflineError::NotFound)?;
        if settled {
            let old_date:Option<String>=c.query_row("SELECT settled_at FROM offline_exit_history WHERE id=?1",[id],|r|r.get(0))?;
            return if old_account==Some(account)&&old_date.as_deref()==Some(date) {Ok(())} else {Err(OfflineError::Conflict{current_version:version})};
        }
        if version!=expected {return Err(OfflineError::Conflict{current_version:version});}
        let net=amount.checked_sub(fee).filter(|v|*v>=0).ok_or_else(||OfflineError::Invalid("回款与费用无效".into()))?;
        let exit_date:String=c.query_row("SELECT exit_date FROM offline_exit_history WHERE id=?1",[id],|r|r.get(0))?;
        if date<exit_date.as_str(){return Err(OfflineError::Invalid("到账日期不能早于退出日期".into()));}
        let event=if net==0 {None}else {Some(finance::record_cash_event(c,&CashEventInput{account_id:account,event_type:"correction".into(),
            amount_text:format!("{}.{:02}",net/100,net%100),occurred_at:date.into(),
            idempotency_key:Some(format!("offline-settlement:{id}")),note:format!("offline:{asset} 退出回款到账"),counterparty_account_id:None})?)};
        c.execute("UPDATE offline_exit_history SET settled=1,settlement_account_id=?2,cash_event_ref=?3,settled_at=?4,version=version+1 WHERE id=?1",params![id,account,event,date])?;
        c.execute("UPDATE offline_assets SET version=version+1,updated_at=?2,exit_settled=CASE WHEN ?3=(SELECT MAX(id) FROM offline_exit_history WHERE asset_id=?1) THEN 1 ELSE exit_settled END,cash_event_ref=CASE WHEN ?3=(SELECT MAX(id) FROM offline_exit_history WHERE asset_id=?1) THEN ?4 ELSE cash_event_ref END WHERE id=?1",params![asset,finance::now_secs(),id,event])?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settlement_is_atomic_and_retries_do_not_duplicate_cash() {
        let s = Store::open_in_memory().unwrap();
        s.with_write(|c|->rusqlite::Result<()> {
            c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('cash',0,0)",[])?;
            c.execute("INSERT INTO offline_assets(product_type,name,value_minor,valued_at,created_at,updated_at) VALUES('deposit','deposit',0,?1,'0','0')",[finance::today_cn()])?;
            c.execute("INSERT INTO offline_exit_history(asset_id,exit_date,amount_minor,fee_minor,created_at) VALUES(1,?1,10000,100,'0')",[finance::today_cn()])?;Ok(())
        }).unwrap();
        assert!(settle(&s, 1, 1, 2, &finance::today_cn()).is_err());
        settle(&s, 1, 1, 1, &finance::today_cn()).unwrap();
        settle(&s, 1, 1, 1, &finance::today_cn()).unwrap();
        assert_eq!(
            s.with_read(
                |c| c.query_row("SELECT COUNT(*) FROM cash_events", [], |r| r
                    .get::<_, i64>(0))
            )
            .unwrap(),
            1
        );
        assert!(exits_page(&s, 1, 0).unwrap()[0]["settled"]
            .as_bool()
            .unwrap());
    }
}
