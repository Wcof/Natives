//! Preserve old manual IDs while users confirm conversion to the shared holdings ledger.
use crate::{
    fixed::Fixed,
    ledger::{self, LedgerError},
    storage::Store,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
fn row(c: &Connection, id: i64) -> Result<Value, LedgerError> {
    c.query_row("SELECT m.id,m.account_id,a.name,m.product_type,m.instrument_key,m.name,m.risk_class,m.market_value_minor,m.valued_at,m.version,m.deleted_at IS NOT NULL,m.converted_ref FROM manual_assets m JOIN accounts a ON a.id=m.account_id WHERE m.id=?1 AND m.product_type IN ('gold','stock','etf')",[id],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"accountId":r.get::<_,i64>(1)?,"account":r.get::<_,String>(2)?,"productType":r.get::<_,String>(3)?,"instrumentKey":r.get::<_,String>(4)?,"name":r.get::<_,String>(5)?,"riskClass":r.get::<_,String>(6)?,"marketValue":Fixed::amount(r.get::<_,i64>(7)?).to_string_value(),"valuedAt":r.get::<_,String>(8)?,"version":r.get::<_,i64>(9)?,"deleted":r.get::<_,bool>(10)?,"convertedRef":r.get::<_,Option<String>>(11)?}))).optional()?.ok_or_else(||LedgerError::NotFound("旧人工资产".into()))
}
pub fn list_page(store: &Store, page: i64, kind: &str) -> Result<Value, LedgerError> {
    if !(1..=1_000_000).contains(&page) || !matches!(kind, "" | "gold" | "holdings") {
        return Err(LedgerError::Invalid("分页或类型无效".into()));
    }
    store.with_read(|c|{
 let mut q=c.prepare("SELECT id FROM manual_assets WHERE product_type IN ('gold','stock','etf') AND converted_ref IS NULL AND (?1='' OR (?1='gold' AND product_type='gold') OR (?1='holdings' AND product_type IN ('stock','etf'))) ORDER BY id DESC LIMIT 50 OFFSET ?2")?;
 let ids=q.query_map(params![kind,(page-1)*50],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;drop(q);
 Ok(json!({"assets":ids.into_iter().map(|id|row(c,id)).collect::<Result<Vec<_>,_>>()?,"page":page,"pageSize":50}))
})
}
pub fn change(store: &Store, v: &Value) -> Result<Value, LedgerError> {
    let id = v["id"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| LedgerError::Invalid("缺少资产 ID".into()))?;
    let version = v["expectedVersion"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| LedgerError::Invalid("缺少版本".into()))?;
    let op = v["op"].as_str().unwrap_or("");
    let today = crate::finance::today_cn();
    store.with_write(|c|{
  let old=row(c,id)?;
  if old["version"].as_i64()!=Some(version)||!old["convertedRef"].is_null(){return Err(LedgerError::Conflict("记录已变化或已转换，请刷新".into()));}
  if old["deleted"]==true&&op!="restore"{return Err(LedgerError::Invalid("归档记录请先恢复".into()));}
  let mut reference=Value::Null;
  match op{
   "delete"=>{c.execute("UPDATE manual_assets SET deleted_at=?2,version=version+1 WHERE id=?1",params![id,today])?;}
   "restore"=>{if old["deleted"]!=true{return Err(LedgerError::Conflict("记录未归档".into()));}c.execute("UPDATE manual_assets SET deleted_at=NULL,version=version+1 WHERE id=?1",[id])?;}
   "update"=>{
    if old["productType"]!="gold"{return Err(LedgerError::Invalid("人工证券请先确认转换为持仓".into()));}
    let input=crate::assets::ManualAssetInput{account_name:old["account"].as_str().unwrap().into(),product_type:"gold".into(),market:None,code:None,name:v["name"].as_str().unwrap_or("").into(),risk_class:v["riskClass"].as_str().unwrap_or("gold").into(),market_value_text:v["marketValue"].as_str().unwrap_or("").into(),valued_at:v["valuedAt"].as_str().unwrap_or("").into(),confirm_distinct:false};
    crate::assets::update_manual_asset(c,id,&input,&today).map_err(|e|LedgerError::Invalid(e.message()))?;c.execute("UPDATE manual_assets SET version=version+1 WHERE id=?1",[id])?;
   }
   "convert"=>{
    if !matches!(old["productType"].as_str(),Some("stock"|"etf"))||v["confirmed"]!=true{return Err(LedgerError::Invalid("需要确认人工证券转换预览".into()));}
    let key=old["instrumentKey"].as_str().unwrap();let mut parts=key.split(':');let _=parts.next();let market=parts.next().unwrap_or("");let code=parts.next().unwrap_or("");
    let fixed=|name:&str,scale|Fixed::parse(v[name].as_str().unwrap_or(""),scale).map_err(|_|LedgerError::Invalid(format!("{name} 精度非法")));
    let cost=if v["totalCost"].as_str().unwrap_or("").is_empty(){None}else{Some(fixed("totalCost",2)?)};
    let quantity=fixed("quantity",4)?;let price=fixed("price",4)?;if price.is_negative(){return Err(LedgerError::Invalid("价格不能为负".into()));}
    let date=v["asOf"].as_str().unwrap_or("");
    let input=ledger::OpeningInput{account_name:old["account"].as_str().unwrap().into(),fund_code:format!("{}:{market}:{code}",old["productType"].as_str().unwrap()),fund_name:old["name"].as_str().unwrap().into(),quantity,as_of:date.into(),total_cost:cost};
    let fund=ledger::record_opening_tx(c,&input,&today)?;
    crate::assets::set_fund_metadata(c,fund,"CNY",old["riskClass"].as_str().unwrap()).map_err(|e|LedgerError::Invalid(e.message()))?;
    c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(?1,?2,?3,'user_confirmed',?4)",params![fund,date,price.raw(),crate::finance::now_secs()])?;
    let account=old["accountId"].as_i64().unwrap();reference=json!(format!("fund:{account}:{fund}"));
    c.execute("UPDATE manual_assets SET converted_ref=?2,version=version+1 WHERE id=?1",params![id,reference.as_str()])?;
    c.execute("UPDATE group_plan_bindings SET source_kind='fund',account_id=?2,fund_id=?3,asset_id=NULL WHERE source_kind='manual' AND asset_id=?1",params![id,account,fund])?;
    c.execute("UPDATE group_plan SET revision=revision+1,updated_at=?1 WHERE id=1",[crate::finance::now_secs()])?;
   }
   _=>return Err(LedgerError::Invalid("未知操作".into())),
  }
  c.execute("INSERT INTO legacy_asset_history(asset_id,payload_json,created_at) VALUES(?1,?2,?3)",params![id,json!({"previous":old,"operation":op,"input":v,"convertedRef":reference}).to_string(),crate::finance::now_secs()])?;
  Ok(json!({"ok":true,"convertedRef":reference}))
 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_is_atomic_and_gold_archive_restores() {
        let store = Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        let input = crate::assets::ManualAssetInput {
            account_name: "A".into(),
            product_type: "stock".into(),
            market: Some("SSE".into()),
            code: Some("600519".into()),
            name: "Stock".into(),
            risk_class: "equity".into(),
            market_value_text: "100".into(),
            valued_at: today.clone(),
            confirm_distinct: false,
        };
        store
            .with_write::<_, rusqlite::Error>(|c| {
                c.execute(
                    "INSERT INTO accounts(name,created_at,updated_at) VALUES('A','0','0')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let id = crate::assets::create_asset(&store, &input, &today).unwrap();
        let request = json!({"id":id,"expectedVersion":1,"op":"convert","confirmed":true,"quantity":"10","price":"10","totalCost":"80","asOf":today});
        let result = change(&store, &request).unwrap();
        assert_eq!(result["convertedRef"], "fund:1:1");
        assert!(change(&store, &request).is_err());
        let summary = crate::assets::summary(&store, &today).unwrap();
        assert_eq!(summary.known_value_minor, Some(10000));
        assert_eq!(summary.manual.len(), 0);
        assert_eq!(summary.funds.len(), 1);
        let history: i64 = store
            .with_read(|c| {
                c.query_row("SELECT COUNT(*) FROM legacy_asset_history", [], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(history, 1);
        let mut gold = input;
        gold.product_type = "gold".into();
        gold.market = None;
        gold.code = None;
        gold.name = "Gold".into();
        gold.risk_class = "gold".into();
        let gold = crate::assets::create_asset(&store, &gold, &today).unwrap();
        change(
            &store,
            &json!({"id":gold,"expectedVersion":1,"op":"delete"}),
        )
        .unwrap();
        assert_eq!(
            crate::assets::summary(&store, &today).unwrap().manual.len(),
            0
        );
        change(
            &store,
            &json!({"id":gold,"expectedVersion":2,"op":"restore"}),
        )
        .unwrap();
        assert_eq!(
            crate::assets::summary(&store, &today).unwrap().manual.len(),
            1
        );
    }
}
