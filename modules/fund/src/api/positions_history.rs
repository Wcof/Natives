//! Account-scoped private income history; prices never become ledger authority.
use super::{api_error, positions_income, query_param};
use crate::{assets, ledger, storage::Store};
use rusqlite::params;
use serde_json::{json, Value};

pub(super) fn income_history(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let id = |key| {
        query_param(path, key)
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", "持仓身份无效"))
    };
    let account = id("accountId")?;
    let fund = id("fundId")?;
    let from = query_param(path, "from")
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "缺少收益起始日期"))?;
    let to = query_param(path, "to")
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "缺少收益结束日期"))?;
    let today = crate::finance::today_cn();
    let days = assets::days_from_civil(&from).zip(assets::days_from_civil(&to));
    if !assets::is_valid_calendar_date(&from)
        || !assets::is_valid_calendar_date(&to)
        || from > to
        || to > today
        || !days.is_some_and(|(a, b)| b - a < 370)
    {
        return Err(api_error(
            400,
            "LEDGER_INVALID",
            "收益日期区间无效或超过 370 天",
        ));
    }
    let (code,currency,exists,initial_revision)=store.with_read(|c| -> rusqlite::Result<_> {
        let row=c.query_row("SELECT f.code,COALESCE(m.nav_currency,'unknown') FROM funds f JOIN accounts a ON a.id=?1 LEFT JOIN fund_asset_metadata m ON m.fund_id=f.id WHERE f.id=?2 AND a.archived_at IS NULL",params![account,fund],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?;
        Ok((row.0,row.1,positions_income::load(c,account,fund)?.has_records(),Store::data_revision(c)?))
    }).map_err(|e|if matches!(e,rusqlite::Error::QueryReturnedNoRows){api_error(404,"LEDGER_NOT_FOUND","持仓不存在")}else{api_error(500,"LEDGER_DB","持仓身份读取失败")})?;
    if !exists {
        return Err(api_error(404, "LEDGER_NOT_FOUND", "持仓不存在"));
    }
    if query_param(path, "expectedRevision").is_some_and(|r| r != initial_revision) {
        return Err(api_error(409, "LEDGER_CONFLICT", "账本已变化，请刷新持仓"));
    }
    let (kind, market, raw_code) = ledger::instrument_parts(&code)
        .map_err(|_| api_error(400, "LEDGER_INVALID", "标的身份无效"))?;
    if kind == "fund" && currency != "CNY" {
        return Ok(json!({"dataRevision":initial_revision,"from":from,"to":to,"rows":[],"status":"unavailable","missingCount":0,"reason":if currency=="unknown"{"currency_unknown"}else{"currency_unsupported"}}).to_string());
    }
    let prefix = match market {
        "SSE" => "sh",
        "SZSE" => "sz",
        "BSE" => "bj",
        _ => "",
    };
    let (public_prices, market_failed) = if kind == "fund" {
        (None, false)
    } else {
        match crate::market::unadjusted_history(&format!("{prefix}{raw_code}")) {
            Ok(prices) => (Some(prices), false),
            Err(_) => (None, true),
        }
    };
    let local = if kind == "fund" {
        store
            .with_read(|c| positions_income::nav_prices(c, fund, &to))
            .map_err(|_| api_error(500, "LEDGER_DB", "净值历史读取失败"))?
    } else {
        std::collections::BTreeMap::new()
    };
    let needs_public = kind == "fund"
        && (local
            .first_key_value()
            .is_none_or(|(date, _)| date.as_str() > from.as_str())
            || local
                .last_key_value()
                .is_none_or(|(date, _)| date.as_str() < to.as_str()));
    let (source_rows, source_failed) = if needs_public {
        match crate::nav::EastMoneySource::fetch_detail_history(raw_code) {
            Ok(rows) => (rows, false),
            Err(_) => (Vec::new(), true),
        }
    } else {
        (Vec::new(), false)
    };
    let result = store.with_snapshot(|c| -> rusqlite::Result<Value> {
        let revision = Store::data_revision(c)?;
        if revision != initial_revision {return Ok(json!({"conflict":true}));}
        let book = positions_income::load(c,account,fund)?;
        if !book.has_records() {return Ok(json!({"notFound":true}));}
        let prices = if kind == "fund" {
            let mut merged=source_rows.iter().filter(|r|r.nav_date<=to && r.unit_nav_raw>0).map(|r|(r.nav_date.clone(),r.unit_nav_raw)).collect::<std::collections::BTreeMap<_,_>>();
            merged.extend(positions_income::nav_prices(c,fund,&to)?);merged
        } else {public_prices.as_ref().map(|p|positions_income::market_prices(p,&to)).unwrap_or_default()};
        let rows=book.history(&from,&to,&prices);
        let missing=rows.iter().filter(|r|r["amount"].is_null()).count();
        Ok(json!({"dataRevision":revision,"from":from,"to":to,"rows":rows,"missingCount":missing,
            "sourceUnavailable":source_failed || market_failed,"status":if missing>0 || source_failed || market_failed {"partial"} else if rows.is_empty() {"empty"} else {"ready"},
            "priceFrom":prices.keys().next(),"priceTo":prices.keys().next_back()}))
    }).map_err(|_|api_error(500,"LEDGER_DB","收益历史读取失败"))?;
    if result["conflict"] == true {
        return Err(api_error(409, "LEDGER_CONFLICT", "账本已变化，请刷新持仓"));
    }
    if result["notFound"] == true {
        return Err(api_error(404, "LEDGER_NOT_FOUND", "没有该账户的持仓记录"));
    }
    Ok(result.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::Fixed;

    #[test]
    fn history_is_account_scoped_read_only_and_rejects_stale_or_invalid_queries() {
        let store = Store::open_in_memory().unwrap();
        let mut input = ledger::OpeningInput {
            account_name: "A".into(),
            fund_code: "110022".into(),
            fund_name: "Fixture".into(),
            quantity: Fixed::share(100000),
            total_cost: Some(Fixed::amount(1000)),
            as_of: "2026-01-05".into(),
        };
        let fund = ledger::record_opening(&store, &input, "2026-01-07").unwrap();
        input.account_name = "B".into();
        input.quantity = Fixed::share(200000);
        ledger::record_opening(&store, &input, "2026-01-07").unwrap();
        store.with_write(|c|->rusqlite::Result<()> {
            c.execute("INSERT INTO fund_asset_metadata(fund_id,nav_currency,risk_class,updated_at) VALUES(?1,'CNY','equity','0') ON CONFLICT(fund_id) DO UPDATE SET nav_currency='CNY'",[fund])?;
            for (date,price) in [("2026-01-04",10000),("2026-01-05",11000),("2026-01-06",12000),("2026-01-07",13000)] {
                c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(?1,?2,?3,'user_confirmed','0')",params![fund,date,price])?;
            }
            Ok(())
        }).unwrap();
        let revision = store.with_read(Store::data_revision).unwrap();
        let path = |account| {
            format!("/api/holding/income-history?accountId={account}&fundId={fund}&from=2026-01-06&to=2026-01-07&expectedRevision={revision}")
        };
        let a: Value = serde_json::from_str(&income_history(&store, &path(1)).unwrap()).unwrap();
        let b: Value = serde_json::from_str(&income_history(&store, &path(2)).unwrap()).unwrap();
        assert_eq!(a["rows"][1]["amount"], "1");
        assert_eq!(b["rows"][1]["amount"], "2");
        assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
        assert_eq!(income_history(&store, &path(999)).unwrap_err().0, 404);
        assert_eq!(
            income_history(
                &store,
                &path(1).replace(
                    &format!("expectedRevision={revision}"),
                    "expectedRevision=stale"
                )
            )
            .unwrap_err()
            .0,
            409
        );
        for replacement in ["from=2026-02-30", "from=2027-01-01", "from=2020-01-01"] {
            assert_eq!(
                income_history(&store, &path(1).replace("from=2026-01-06", replacement))
                    .unwrap_err()
                    .0,
                400
            );
        }
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE fund_asset_metadata SET nav_currency='unknown' WHERE fund_id=?1",
                    [fund],
                )
            })
            .unwrap();
        let value: Value = serde_json::from_str(
            &income_history(
                &store,
                &path(1).replace(&format!("&expectedRevision={revision}"), ""),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["reason"], "currency_unknown");
    }
}
