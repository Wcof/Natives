//! HTTP API 业务请求处理（拆分自 api.rs，仅移动，无逻辑变更）。

#[cfg(test)]
use super::positions_detail::positions_detail_with_quotes;
use super::positions_detail::positions_detail_with_sources;
use super::{api_error, json_str, query_param};
use crate::assets::{minus_days, today_iso};
use crate::fixed::{Fixed, SCALE_SHARE};
use crate::ledger;
use crate::portfolio;
use crate::storage::Store;
use rusqlite::OptionalExtension;

fn instrument_key(v: &serde_json::Value) -> Result<String, (u16, String)> {
    let code = v["fundCode"]
        .as_str()
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "缺少代码"))?;
    let kind = v["instrumentKind"].as_str().unwrap_or("fund");
    let key = match kind {
        "fund" => code.to_string(),
        "stock" | "etf" => format!("{kind}:{}:{code}", v["market"].as_str().unwrap_or("")),
        _ => return Err(api_error(400, "LEDGER_INVALID", "标的类型无效")),
    };
    ledger::instrument_parts(&key).map_err(|e| api_error(400, e.code(), &e.message()))?;
    Ok(key)
}

pub(crate) fn api_positions(store: &Store) -> Result<String, (u16, String)> {
    let rows = store
        .with_read(portfolio::list_positions)
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?;
    let items: Vec<String> = rows
        .iter()
        .map(|p| {
            let (kind,market,code)=ledger::instrument_parts(&p.fund_code).unwrap_or(("fund","CN",p.fund_code.as_str()));
            let quantity = Fixed::nav(p.quantity_raw);
            let cost = Fixed::amount(p.cost_raw);
            format!(
                "{{\"account\":{},\"fundCode\":{},\"fundName\":{},\"quantity\":{},\"cost\":{},\"realized\":{},\"asOf\":{},\"accountId\":{},\"fundId\":{},\"instrumentKind\":{},\"market\":{}}}",
                json_str(&p.account_name),
                json_str(code),
                json_str(&p.fund_name),
                json_str(&quantity.to_string_value()),
                json_str(&cost.to_string_value()),
                json_str(&Fixed::amount(p.realized_raw).to_string_value()),
                json_str(&p.as_of),p.account_id,p.fund_id,json_str(kind),json_str(market),
            )
        })
        .collect();
    Ok(format!("{{\"positions\":[{}]}}", items.join(",")))
}

/// 持仓页完整快照（T04 A19~A22）：回放真值 + 身份/场馆 + 净值游标 + 待确认数
/// + 账户元数据与 dataRevision（供编辑/转账 dialog 的 expected revision 使用）。
pub(crate) fn api_positions_detail(store: &Store, path: &str) -> Result<String, (u16, String)> {
    positions_detail_with_sources(
        store,
        path,
        |batch| {
            let request = app_runtime_core::http::HttpRequest::for_internal(
                "GET",
                &format!("/api/market/quotes?codes={}", batch.join(",")),
                vec![],
            );
            crate::market::api_market_quotes(&request)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        },
        |symbol| {
            let request = app_runtime_core::http::HttpRequest::for_internal(
                "GET",
                &format!("/api/market/kline?symbol={symbol}&period=day&fq=bf"),
                vec![],
            );
            crate::market::api_market_kline(&request)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        },
    )
}

pub(crate) fn api_holding_records(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let id = |key| {
        query_param(path, key)
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", "持仓身份或分页无效"))
    };
    let account = id("accountId")?;
    let fund = id("fundId")?;
    let page = if query_param(path, "page").is_some() {
        id("page")?
    } else {
        1
    };
    if page > 1_000_000 {
        return Err(api_error(400, "LEDGER_INVALID", "分页无效"));
    }
    let value=store.with_snapshot(|conn|->rusqlite::Result<serde_json::Value> {
        let identity:Option<(String,String)>=conn.query_row(
            "SELECT a.name,f.code FROM accounts a CROSS JOIN funds f WHERE a.id=?1 AND f.id=?2",
            rusqlite::params![account,fund],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((name,code))=identity else {return Ok(serde_json::Value::Null);};
        let opening:Option<serde_json::Value>=conn.query_row(
            "SELECT quantity_raw,cost_raw,as_of,version FROM opening_snapshots
             WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
            rusqlite::params![account,fund],|r|Ok(serde_json::json!({"quantity":Fixed::share(r.get(0)?).to_string_value(),
                "totalCost":r.get::<_,Option<i64>>(1)?.map(|c|Fixed::amount(c).to_string_value()),
                "asOf":r.get::<_,String>(2)?,"version":r.get::<_,i64>(3)?}))).optional()?;
        let mut stmt=conn.prepare("SELECT id,type,state,quantity_raw,price_raw,fee_raw,trade_date,version,source
            FROM transactions WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL
            ORDER BY trade_date DESC,id DESC LIMIT 21 OFFSET ?3")?;
        let mut tx=stmt.query_map(rusqlite::params![account,fund,(page-1)*20],|r|Ok(serde_json::json!({
            "id":r.get::<_,i64>(0)?,"type":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,
            "quantity":Fixed::share(r.get(3)?).to_string_value(),"price":Fixed::nav(r.get(4)?).to_string_value(),
            "fee":Fixed::amount(r.get(5)?).to_string_value(),"tradeDate":r.get::<_,String>(6)?,
            "version":r.get::<_,i64>(7)?,"source":r.get::<_,String>(8)?,"account":name,"fundCode":code
        })))?.collect::<Result<Vec<_>,_>>()?;
        let has_more=tx.len()>20;tx.truncate(20);
        Ok(serde_json::json!({"opening":opening,"transactions":tx,"page":page,"hasMore":has_more,
            "dataRevision":Store::data_revision(conn)?}))
    }).map_err(|_|api_error(500,"LEDGER_DB","持仓记录读取失败"))?;
    if value.is_null() {
        return Err(api_error(404, "LEDGER_NOT_FOUND", "持仓不存在"));
    }
    Ok(value.to_string())
}

pub(crate) fn api_transactions(store: &Store, path: &str) -> Result<String, (u16, String)> {
    // 可选 state 过滤（GET /api/transactions?state=pending）：待确认列表。
    let state_filter = query_param(path, "state")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let all = ledger::list_transactions(store, 200)
        .map_err(|e| api_error(500, e.code(), &e.message()))?;
    let rows: Vec<_> = all
        .into_iter()
        .filter(|t| {
            (state_filter.is_empty() || t.state == state_filter)
                && (!t.deleted || query_param(path, "includeDeleted").as_deref() == Some("1"))
        })
        .collect();
    let items: Vec<String> = rows
        .iter()
        .map(|t| {
            format!(
                "{{\"id\":{},\"account\":{},\"fundCode\":{},\"type\":{},\"state\":{},\"quantity\":{},\"price\":{},\"amount\":{},\"fee\":{},\"tradeDate\":{},\"source\":{},\"deleted\":{},\"version\":{}}}",
                t.id,
                json_str(&t.account_name),
                json_str(&t.fund_code),
                json_str(&t.tx_type),
                json_str(&t.state),
                json_str(&Fixed::nav(t.quantity_raw).to_string_value()),
                json_str(&Fixed::nav(t.price_raw).to_string_value()),
                json_str(&Fixed::amount(t.amount_raw).to_string_value()),
                json_str(&Fixed::amount(t.fee_raw).to_string_value()),
                json_str(&t.trade_date),
                json_str(&t.source),t.deleted,t.version,
            )
        })
        .collect();
    Ok(format!("{{\"transactions\":[{}]}}", items.join(",")))
}

pub(crate) fn api_record_transaction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let get_str = |key: &str| -> Result<String, (u16, String)> {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", &format!("{key} 缺失")))
    };
    let parse_fixed = |key: &str, scale: u32| -> Result<Fixed, (u16, String)> {
        Fixed::parse(&get_str(key)?, scale)
            .map_err(|_| api_error(400, "LEDGER_INVALID", &format!("{key} 精度非法")))
    };
    let fee_fixed = parse_fixed("fee", 2)?;

    let input = ledger::TransactionInput::manual(
        get_str("account")?,
        instrument_key(&value)?,
        value
            .get("fundName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        get_str("type")?,
        value
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or(ledger::STATE_CONFIRMED)
            .to_string(),
        parse_fixed("quantity", SCALE_SHARE)?,
        parse_fixed("price", SCALE_SHARE)?,
        fee_fixed,
        get_str("tradeDate")?,
        get_str("requestId")?,
    );
    let (id, created) = ledger::record_transaction(store, &input)
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"id\":{id},\"created\":{created}}}"))
}

/// 期初登记（方案 01 §5.1：登记已有持仓；不扣现金、不虚构当天买入）。
pub(crate) fn api_record_opening(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let get_str = |key: &str| -> Result<String, (u16, String)> {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", &format!("{key} 缺失")))
    };
    let parse_fixed = |key: &str, scale: u32| -> Result<Fixed, (u16, String)> {
        Fixed::parse(&get_str(key)?, scale)
            .map_err(|_| api_error(400, "LEDGER_INVALID", &format!("{key} 精度非法")))
    };
    let total_cost = value
        .get("totalCost")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| Fixed::parse(s, 2))
        .transpose()
        .map_err(|_| api_error(400, "LEDGER_INVALID", "totalCost 精度非法"))?;
    let input = ledger::OpeningInput {
        account_name: get_str("account")?,
        fund_code: instrument_key(&value)?,
        fund_name: value
            .get("fundName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        quantity: parse_fixed("quantity", crate::fixed::SCALE_SHARE)?,
        as_of: get_str("asOf")?,
        total_cost,
    };
    let today = super::today_iso();
    let fund_id = ledger::record_opening(store, &input, &today)
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"ok\":true,\"fundId\":{fund_id}}}"))
}

/// 待确认交易确认（pending→confirmed 一次生效；重复确认幂等）。
pub(crate) fn api_confirm_transaction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let id = value
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "id 缺失"))?;
    let version = value["expectedVersion"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "缺少版本"))?;
    let confirmed = ledger::confirm_transaction_versioned(store, id, version)
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"id\":{id},\"confirmed\":{confirmed}}}"))
}

pub(crate) fn api_opening_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let page = crate::market::query_param(path, "page")
        .unwrap_or_else(|| "1".into())
        .parse::<i64>()
        .ok()
        .filter(|p| (1..=1_000_000).contains(p))
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "分页无效"))?;
    let rows=store.with_read(|c|->rusqlite::Result<Vec<serde_json::Value>>{
  let mut stmt=c.prepare("SELECT o.account_id,o.fund_id,a.name,f.code,f.name,o.deleted_at FROM opening_snapshots o JOIN accounts a ON a.id=o.account_id JOIN funds f ON f.id=o.fund_id ORDER BY o.deleted_at,o.account_id,o.fund_id LIMIT 50 OFFSET ?1")?;
  let rows=stmt.query_map([(page-1)*50],|r|{let key:String=r.get(3)?;let(kind,market,code)=ledger::instrument_parts(&key).unwrap_or(("fund","CN",key.as_str()));Ok(serde_json::json!({"accountId":r.get::<_,i64>(0)?,"fundId":r.get::<_,i64>(1)?,"account":r.get::<_,String>(2)?,"fundCode":code,"fundName":r.get::<_,String>(4)?,"instrumentKind":kind,"market":market,"deleted":r.get::<_,Option<String>>(5)?.is_some()}))})?.collect::<Result<Vec<_>,_>>()?;Ok(rows)
 }).map_err(|_|api_error(500,"LEDGER_DB","期初列表读取失败"))?;
    Ok(serde_json::json!({"openings":rows,"page":page,"pageSize":50}).to_string())
}
pub(crate) fn api_holding_valuation(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| api_error(400, "LEDGER_INVALID", "JSON 无效"))?;
    let fund = v["fundId"]
        .as_i64()
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "缺少标的"))?;
    let price = Fixed::parse(v["price"].as_str().unwrap_or(""), 4)
        .map_err(|_| api_error(400, "LEDGER_INVALID", "价格精度无效"))?;
    let date = v["asOf"].as_str().unwrap_or("");
    if price.raw() < 0
        || !crate::assets::is_valid_calendar_date(date)
        || date > today_iso().as_str()
    {
        return Err(api_error(400, "LEDGER_INVALID", "价格或估值日期无效"));
    }
    store.with_write(|c|->Result<(),crate::assets::AssetError>{
  let revision=Store::data_revision(c)?;
  if v["dataRevision"].as_str()!=Some(revision.as_str()){return Err(crate::assets::AssetError::Invalid("数据已变化，请刷新后估值".into()));}
  crate::assets::set_fund_metadata(c,fund,v["navCurrency"].as_str().unwrap_or("unknown"),v["riskClass"].as_str().unwrap_or("unknown"))?;
  c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(?1,?2,?3,'user_confirmed',?4) ON CONFLICT(fund_id,nav_date) DO UPDATE SET unit_nav_raw=excluded.unit_nav_raw,source=excluded.source,fetched_at=excluded.fetched_at",rusqlite::params![fund,date,price.raw(),crate::finance::now_secs().to_string()])?;
  c.execute("UPDATE funds SET latest_nav_raw=(SELECT unit_nav_raw FROM fund_nav WHERE fund_id=?1 ORDER BY nav_date DESC LIMIT 1),latest_nav_date=(SELECT nav_date FROM fund_nav WHERE fund_id=?1 ORDER BY nav_date DESC LIMIT 1) WHERE id=?1",[fund])?;Ok(())
 }).map_err(|e|api_error(409,e.code(),&e.message()))?;
    Ok("{\"ok\":true}".into())
}

pub(crate) fn api_opening_history(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let id = |key| {
        query_param(path, key)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", "缺少持仓身份"))
    };
    crate::opening_ops::list(store, id("accountId")?, id("fundId")?)
        .map(|v| v.to_string())
        .map_err(|e| api_error(409, e.code(), &e.message()))
}
pub(crate) fn api_opening_change(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| api_error(400, "LEDGER_INVALID", "JSON 无效"))?;
    let id = |key| {
        v[key]
            .as_i64()
            .filter(|n| *n > 0)
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", "缺少持仓身份或版本"))
    };
    crate::opening_ops::change(
        store,
        id("accountId")?,
        id("fundId")?,
        id("expectedVersion")?,
        v["op"].as_str().unwrap_or(""),
        &v,
    )
    .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok("{\"ok\":true}".into())
}

pub(crate) fn api_history(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let account = query_param(path, "account")
        .ok_or_else(|| api_error(400, "PORTFOLIO_DB", "account 缺失"))?;
    let days: i64 = query_param(path, "days")
        .and_then(|d| d.parse().ok())
        .unwrap_or(30);
    // 找账户 id。
    let account_id: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id FROM accounts WHERE name = ?1",
                rusqlite::params![account],
                |r| r.get(0),
            )
            .optional()
        })
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?
        .ok_or_else(|| api_error(404, "LEDGER_NOT_FOUND", "账户不存在"))?;
    // 日期窗口（自然日）。
    let today = today_iso();
    let from = minus_days(&today, days);
    let points = store
        .with_read(|conn| portfolio::replay_history(conn, account_id, &from, &today))
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?;
    let items: Vec<String> = points
        .iter()
        .map(|p| {
            let value = p
                .value_raw
                .map(|raw| json_str(&Fixed::amount(raw).to_string_value()))
                .unwrap_or_else(|| "null".into());
            format!(
                "{{\"date\":{},\"value\":{},\"cost\":{}}}",
                json_str(&p.date),
                value,
                json_str(&Fixed::amount(p.cost_raw).to_string_value()),
            )
        })
        .collect();
    Ok(format!("{{\"history\":[{}]}}", items.join(",")))
}

pub(crate) fn api_accounts(store: &Store) -> Result<String, (u16, String)> {
    // 旧名称数组兼容（方案 03）：仅活跃账户，(sort_order,id) 与旧 ORDER BY id
    // 在未重排时等价；归档账户不出现在交易选择与旧导入链。
    let names: Vec<String> = store
        .with_read(|conn| crate::accounts::active_names(conn))
        .map_err(|e| api_error(500, "LEDGER_DB", &e.message()))?;
    let items: Vec<String> = names.iter().map(|n| json_str(n)).collect();
    Ok(format!("{{\"accounts\":[{}]}}", items.join(",")))
}

pub(crate) fn api_create_account(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "name 缺失"))?;
    let id = store
        .with_write(|conn| -> Result<i64, ledger::LedgerError> {
            ledger::ensure_account(conn, name)
        })
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"id\":{id}}}"))
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn quote_refresh_rejects_a_ledger_changed_during_network_io() {
        let store = Store::open_in_memory().unwrap();
        api_record_opening(&store, r#"{"account":"isolated","fundCode":"600519","instrumentKind":"stock","market":"SSE","quantity":"100","totalCost":"100","asOf":"2026-01-01"}"#).unwrap();
        let result = positions_detail_with_quotes(
            &store,
            "/api/positions/detail?refresh=1&kind=stock",
            |_| {
                store
                    .with_write(|conn| conn.execute("UPDATE accounts SET name='changed'", []))
                    .unwrap();
                Some(serde_json::json!({"quotes":[{"code":"sh600519","price":2}]}))
            },
        );
        assert_eq!(result.unwrap_err().0, 409);
        let result = positions_detail_with_quotes(
            &store,
            "/api/positions/detail?refresh=1&kind=stock",
            |_| Some(serde_json::json!({"quotes":[{"code":"sh600519","price":2}]})),
        )
        .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["accounts"][0]["name"], "changed");
        assert_eq!(result["holdings"][0]["quantity"], "100");
        assert_eq!(result["holdings"][0]["marketValue"], "200");
    }

    #[test]
    fn quote_refresh_kind_all_covers_exchange_holdings_and_rejects_unknown_kind() {
        let store = Store::open_in_memory().unwrap();
        for body in [
            r#"{"account":"isolated","fundCode":"600519","instrumentKind":"stock","market":"SSE","quantity":"100","totalCost":"100","asOf":"2026-01-01"}"#,
            r#"{"account":"isolated","fundCode":"510300","instrumentKind":"etf","market":"SSE","quantity":"200","totalCost":"200","asOf":"2026-01-01"}"#,
        ] {
            api_record_opening(&store, body).unwrap();
        }
        let mut requested: Vec<String> = Vec::new();
        let result = positions_detail_with_quotes(&store, "/api/positions/detail?refresh=1&kind=all", |batch| {
            requested.extend(batch.iter().map(|s| s.to_string()));
            Some(serde_json::json!({"quotes":[{"code":"sh600519","price":2.05,"changePct":2.5,"change":0.05,"asOf":crate::finance::today_cn()},{"code":"sh510300","price":3}]}))
        })
        .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(requested.contains(&"sh600519".to_string()));
        assert!(requested.contains(&"sh510300".to_string()));
        let find = |code: &str| {
            result["holdings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|h| h["fundCode"] == code)
                .cloned()
                .unwrap()
        };
        assert_eq!(find("600519")["marketValue"], "205");
        assert_eq!(find("600519")["dayPct"].as_f64(), Some(2.5));
        assert_eq!(find("600519")["dayPnl"], "5");
        assert_eq!(
            find("600519")["holdingDays"],
            crate::assets::days_from_civil(&crate::finance::today_cn()).unwrap()
                - crate::assets::days_from_civil("2026-01-01").unwrap()
        );
        assert_eq!(find("510300")["marketValue"], "600");
        let err = positions_detail_with_quotes(
            &store,
            "/api/positions/detail?refresh=1&kind=bogus",
            |_| None,
        )
        .unwrap_err();
        assert_eq!(err.0, 400);
    }

    #[test]
    fn manual_otc_fund_values_automatically_once_nav_synced() {
        let store = Store::open_in_memory().unwrap();
        api_record_opening(
            &store,
            r#"{"account":"isolated","fundCode":"110022","instrumentKind":"fund","quantity":"100","totalCost":"1000","asOf":"2026-01-01"}"#,
        )
        .unwrap();
        // 手动添加的场外基金币种随建档确认为 CNY，无需用户再走估值对话框。
        let currency: String = store
            .with_read(|c| {
                c.query_row(
                    "SELECT nav_currency FROM fund_asset_metadata WHERE fund_id=1",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(currency, "CNY");
        // 净值未同步前市值显式缺位，不用成本冒充。
        let detail: serde_json::Value = serde_json::from_str(
            &positions_detail_with_quotes(&store, "/api/positions/detail", |_| None).unwrap(),
        )
        .unwrap();
        assert_eq!(detail["holdings"][0]["venue"], "otc");
        assert_eq!(
            detail["holdings"][0]["marketValue"],
            serde_json::Value::Null
        );
        // 上游净值同步（等价 nav::sync_fund_nav 的写入效果）：当日、昨日与
        // 周/月/年起点前各一行，全部为动态真实日期。
        let today = crate::finance::today_cn();
        let today_days = crate::assets::days_from_civil(&today).unwrap();
        let week_start = today_days - (((today_days + 4) % 7 + 6) % 7);
        let month_start = format!("{}-01", &today[..7]);
        let year_start = format!("{}-01-01", &today[..4]);
        let mut nav_rows: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        nav_rows.insert(
            crate::assets::civil_from_days(
                crate::assets::days_from_civil(&year_start).unwrap() - 1,
            ),
            10_000,
        );
        nav_rows.insert(
            crate::assets::civil_from_days(
                crate::assets::days_from_civil(&month_start).unwrap() - 1,
            ),
            11_000,
        );
        nav_rows.insert(crate::assets::civil_from_days(week_start - 1), 12_000);
        nav_rows.insert(crate::assets::civil_from_days(today_days - 1), 14_000);
        nav_rows.insert(today.clone(), 15_000);
        store
            .with_write(|c| {
                for (d, raw) in &nav_rows {
                    c.execute(
                        "INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,?2,'eastmoney','0')
                         ON CONFLICT(fund_id,nav_date) DO UPDATE SET unit_nav_raw=excluded.unit_nav_raw",
                        rusqlite::params![d, raw],
                    )?;
                }
                Ok::<(), rusqlite::Error>(())
            })
            .unwrap();
        let detail: serde_json::Value = serde_json::from_str(
            &positions_detail_with_quotes(&store, "/api/positions/detail", |_| None).unwrap(),
        )
        .unwrap();
        let h = &detail["holdings"][0];
        assert_eq!(h["nav"]["nav"], "1.5");
        assert_eq!(h["marketValue"], "150");
        // 与生产同规则独立推导基线：期间起点前最后净值。
        let baseline = |cut: &str| {
            nav_rows
                .iter()
                .filter(|(d, _)| d.as_str() < cut)
                .max_by(|a, b| a.0.cmp(b.0))
                .map(|(_, v)| *v)
                .unwrap()
        };
        let pnl = |base: i64| Fixed::amount(15_000 - base).to_string_value();
        assert_eq!(h["dayPct"].as_f64(), Some(7.14));
        assert_eq!(h["dayPnl"], "10");
        let home = crate::home::build_snapshot(&store, "all").unwrap();
        let ledger = &home["portfolio"]["ledger"];
        assert_eq!(ledger["marketValueMinor"], "15000");
        assert_eq!(ledger["holdingProfitMinor"], "-85000");
        assert_eq!(ledger["day"]["amountMinor"], "1000");
        assert_eq!(ledger["day"]["date"], h["income"]["day"]["date"]);

        assert_eq!(
            h["weekPnl"],
            pnl(baseline(&crate::assets::civil_from_days(week_start)))
        );
        assert_eq!(h["monthPnl"], pnl(baseline(&month_start)));
        assert_eq!(h["yearPct"].as_f64(), Some(50.0));
        assert_eq!(
            h["holdingDays"].as_i64(),
            Some(today_days - crate::assets::days_from_civil("2026-01-01").unwrap())
        );
        store
            .with_write(|c| {
                c.execute(
                    "DELETE FROM fund_nav WHERE fund_id=1 AND nav_date=?1",
                    [&today],
                )
            })
            .unwrap();
        let delayed: serde_json::Value = serde_json::from_str(
            &positions_detail_with_quotes(&store, "/api/positions/detail", |_| None).unwrap(),
        )
        .unwrap();
        let h = &delayed["holdings"][0];
        assert!(h["dayPnl"].is_null());
        assert!(h["dayPct"].is_null());
        assert_eq!(h["income"]["day"]["date"], today);
        assert!(h["income"]["day"]["amount"].is_null());
        assert_eq!(h["income"]["day"]["reason"], "missing_daily_price");
        let home = crate::home::build_snapshot(&store, "all").unwrap();
        assert!(home["portfolio"]["ledger"]["day"]["amountMinor"].is_null());
        assert_eq!(home["portfolio"]["ledger"]["day"]["missingCount"], 1);

        assert_eq!(
            h["income"]["yesterday"]["date"],
            crate::assets::civil_from_days(today_days - 1)
        );
    }
}
