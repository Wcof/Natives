//! Disclosed-equity contribution scenario, separate from confirmed NAV and ledger valuation.
#[cfg(not(test))]
use super::EastMoneySource;
use super::{QuarterlyReport, SOURCE_EASTMONEY};
use crate::{assets::FundValuationRow, fixed::Fixed};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn key(row: &FundValuationRow, today: &str) -> Option<String> {
    let date = row.nav_date.as_deref()?;
    let nav = row.nav_raw?;
    (row.instrument_kind == "fund"
        && row.nav_currency == "CNY"
        && row.nav_source.as_deref() == Some(SOURCE_EASTMONEY)
        && nav > 0
        && crate::assets::is_valid_calendar_date(date)
        && date < today)
        .then(|| format!("fund-estimate:{}:{date}:{nav}:{today}", row.fund_code))
}

fn unavailable(reason: &str, today: &str) -> Value {
    json!({"status":"unavailable","method":"disclosed-equity-contribution","date":today,"reason":reason})
}

fn symbol(code: &str) -> Option<String> {
    let code = crate::information::public_stock_code(code)?;
    let prefix = if code.starts_with('6') || code.starts_with('5') {
        "sh"
    } else if code.starts_with('0') || code.starts_with('1') || code.starts_with('3') {
        "sz"
    } else {
        "bj"
    };
    Some(format!("{prefix}{code}"))
}

fn calculate(
    report: &QuarterlyReport,
    quotes: &BTreeMap<String, crate::assets::QuoteInput>,
    basis: i64,
    baseline_date: &str,
    today: &str,
) -> Value {
    if basis <= 0
        || !crate::assets::is_valid_calendar_date(baseline_date)
        || !crate::assets::is_valid_calendar_date(&report.report_date)
        || report.report_date.as_str() > today
        || baseline_date >= today
    {
        return unavailable("invalid_basis", today);
    }
    let mut reported = 0i128;
    let mut covered = 0i128;
    let mut contribution = 0i128;
    let mut as_of: Option<&str> = None;
    let mut seen = BTreeSet::new();
    for stock in report.stocks.iter().take(100) {
        let Ok(weight) = Fixed::parse(stock.ratio_pct.trim().trim_end_matches('%'), 4) else {
            return unavailable("invalid_weight", today);
        };
        let weight = weight.raw() as i128;
        if !(0..=1_000_000).contains(&weight) {
            return unavailable("invalid_weight", today);
        }
        reported += weight;
        let Some(symbol) = stock.quote_symbol.clone().or_else(|| symbol(&stock.code)) else {
            continue;
        };
        if !seen.insert(symbol.clone()) {
            return unavailable("duplicate_holding", today);
        }
        let Some(quote) = quotes.get(&symbol) else {
            continue;
        };
        let Some(stamp) = quote
            .as_of
            .as_deref()
            .filter(|stamp| stamp.get(..10) == Some(today))
        else {
            continue;
        };
        // Foreign-currency and older-day quotes are not silently treated as CNY current-day returns.
        if !symbol.starts_with("sh") && !symbol.starts_with("sz") && !symbol.starts_with("bj") {
            continue;
        }
        let Some(change) = quote.change_raw else {
            continue;
        };
        let previous = quote.price_raw as i128 - change as i128;
        if previous <= 0 || quote.price_raw <= 0 {
            continue;
        }
        contribution += weight * (change as i128 * 1_000_000 / previous) / 1_000_000;
        covered += weight;
        as_of = Some(as_of.map_or(stamp, |old| old.min(stamp)));
    }
    if reported > 1_000_000 || covered > 1_000_000 {
        return unavailable("invalid_weight", today);
    }
    if covered == 0 {
        return unavailable("no_current_quotes", today);
    }
    let nav = (basis as i128)
        .checked_mul(1_000_000 + contribution)
        .and_then(|value| i64::try_from(value / 1_000_000).ok());
    let Some(nav) = nav else {
        return unavailable("overflow", today);
    };
    if nav <= 0 {
        return unavailable("invalid_estimate", today);
    }
    json!({"status":"estimated","method":"disclosed-equity-contribution","assumption":"uncovered-unchanged",
        "date":today,"baselineDate":baseline_date,"reportDate":report.report_date,"asOf":as_of,
        "source":report.source,"coverageBps":covered/100,"reportedBps":reported/100,"complete":false,
        "navRaw":nav,"baselineRaw":basis,"unitNav":Fixed::nav(nav).to_string_value(),
        "rateBps":(contribution/100).to_string()})
}

/// Lock-free network stage. Confirmed fund NAV and bookkeeping quantities remain untouched.
pub(crate) fn warm_estimates(rows: &[FundValuationRow], today: &str) {
    #[cfg(test)]
    {
        let _ = (rows, today);
    }
    #[cfg(not(test))]
    {
        let mut seen = BTreeSet::new();
        for row in rows.iter().filter(|row| key(row, today).is_some()) {
            if !seen.insert(row.fund_code.clone()) {
                continue;
            }
            if seen.len() > 20 {
                break;
            }
            if crate::network::remaining().is_some_and(|remaining| remaining.is_zero()) {
                break;
            }
            let estimate_key = key(row, today).expect("filtered estimate identity");
            let _ = crate::market::cached_public_response(
                estimate_key,
                std::time::Duration::from_secs(60),
                || {
                    let result = match EastMoneySource::fetch_quarterly_positions(&row.fund_code) {
                        Ok(report) => {
                            let symbols = report
                                .stocks
                                .iter()
                                .take(100)
                                .filter_map(|stock| {
                                    stock.quote_symbol.clone().or_else(|| symbol(&stock.code))
                                })
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect::<Vec<_>>();
                            let batch = crate::market::fetch_quote_batch(&symbols);
                            calculate(
                                &report,
                                &batch.quotes,
                                row.nav_raw.unwrap_or(0),
                                row.nav_date.as_deref().unwrap_or(""),
                                today,
                            )
                        }
                        Err(_) => unavailable("disclosure_unavailable", today),
                    };
                    Ok(result.to_string())
                },
            );
        }
    }
}

pub(crate) fn cached_estimate(row: &FundValuationRow, today: &str) -> Option<Value> {
    let text = crate::market::read_public_response(&key(row, today)?)?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn estimated_income(row: &FundValuationRow, estimate: &Value, today: &str) -> Value {
    if estimate["status"] != "estimated" || estimate["date"] != today {
        return json!({"amount":null,"rateBps":null,"date":today,"reason":estimate["reason"]});
    }
    let value = estimate["navRaw"]
        .as_i64()
        .zip(estimate["baselineRaw"].as_i64());
    let amount = value
        .filter(|_| row.nav_currency == "CNY")
        .and_then(|(nav, base)| {
            i64::try_from(row.quantity_raw as i128 * (nav as i128 - base as i128) / 1_000_000).ok()
        });
    let market_value = value
        .filter(|_| row.nav_currency == "CNY")
        .and_then(|(nav, _)| {
            i64::try_from(row.quantity_raw as i128 * nav as i128 / 1_000_000).ok()
        });
    json!({"amount":amount.map(|value|Fixed::amount(value).to_string_value()),"rateBps":estimate["rateBps"],
        "date":today,"baselineDate":estimate["baselineDate"],"estimated":true,"coverageBps":estimate["coverageBps"],
        "asOf":estimate["asOf"],"reportDate":estimate["reportDate"],"marketValue":market_value.map(|value|Fixed::amount(value).to_string_value()),"reason":if amount.is_some(){None}else{Some("currency_or_overflow")}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disclosed_weights_are_not_normalized_to_full_fund_and_old_quotes_are_excluded() {
        let report: QuarterlyReport = serde_json::from_value(json!({"fund_code":"110022","report_date":"2026-06-30","source":"fixture",
            "stocks":[{"code":"600519","name":"stock","ratio_pct":"50","change_type":"","change_pct":"","industry":""}],"bonds":[]})).unwrap();
        let mut quotes = BTreeMap::from([(
            "sh600519".into(),
            crate::assets::QuoteInput {
                price_raw: 1_100_000,
                change_raw: Some(100_000),
                as_of: Some("2026-10-13 15:00:00".into()),
                source: "fixture".into(),
            },
        )]);
        let result = calculate(&report, &quotes, 20000, "2026-10-12", "2026-10-13");
        assert_eq!(result["coverageBps"], 5000);
        assert_eq!(result["rateBps"], "500");
        assert_eq!(result["navRaw"], 21000);
        assert_eq!(result["complete"], false);
        quotes.get_mut("sh600519").unwrap().price_raw = i64::MAX;
        quotes.get_mut("sh600519").unwrap().change_raw = Some(i64::MAX - 1);
        assert_eq!(
            calculate(&report, &quotes, i64::MAX, "2026-10-12", "2026-10-13")["reason"],
            "overflow"
        );
        quotes.get_mut("sh600519").unwrap().as_of = Some("2026-10-12 15:00:00".into());
        assert_eq!(
            calculate(&report, &quotes, 20000, "2026-10-12", "2026-10-13")["status"],
            "unavailable"
        );
    }
    #[test]
    fn estimate_reaches_positions_and_home_without_becoming_confirmed_nav_or_history() {
        let store = crate::storage::Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        let yesterday = crate::assets::minus_days(&today, 1);
        crate::ledger::record_opening(
            &store,
            &crate::ledger::OpeningInput {
                account_name: "estimate".into(),
                fund_code: "903021".into(),
                fund_name: "fixture".into(),
                quantity: Fixed::share(1_000_000),
                total_cost: Some(Fixed::amount(15000)),
                as_of: yesterday.clone(),
            },
            &today,
        )
        .unwrap();
        store.with_write(|conn|{
            conn.execute("INSERT INTO fund_asset_metadata(fund_id,nav_currency,risk_class,updated_at) SELECT id,'CNY','equity','0' FROM funds WHERE code='903021' ON CONFLICT(fund_id) DO UPDATE SET nav_currency='CNY',risk_class='equity'",[])?;
            conn.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) SELECT id,?1,20000,'eastmoney-mobile','0' FROM funds WHERE code='903021'",[&yesterday])?;
            Ok::<_,rusqlite::Error>(())
        }).unwrap();
        let row = store
            .with_read(|conn| crate::assets::collect(conn, &today))
            .unwrap()
            .funds
            .remove(0);
        let report:QuarterlyReport=serde_json::from_value(json!({"fund_code":"903021","report_date":crate::assets::minus_days(&today,45),"source":"fixture","stocks":[{"code":"600519","name":"stock","ratio_pct":"50","change_type":"","change_pct":"","industry":""}],"bonds":[]})).unwrap();
        let quotes = BTreeMap::from([(
            "sh600519".into(),
            crate::assets::QuoteInput {
                price_raw: 1_100_000,
                change_raw: Some(100_000),
                as_of: Some(format!("{today} 15:00:00")),
                source: "fixture".into(),
            },
        )]);
        let estimate = calculate(&report, &quotes, 20000, &yesterday, &today);
        crate::market::store_public_response(
            key(&row, &today).unwrap(),
            std::time::Duration::from_secs(60),
            Ok(estimate.to_string()),
        );
        let request = app_runtime_core::http::HttpRequest::for_internal(
            "GET",
            "/api/positions/detail",
            vec![],
        );
        let value: Value = serde_json::from_str(
            &crate::api::handle_api_request(&store, None, &request, "/api/positions/detail")
                .unwrap(),
        )
        .unwrap();
        let holding = &value["holdings"][0];
        assert!(holding["income"]["day"]["amount"].is_null());
        assert_eq!(holding["income"]["estimatedDay"]["amount"], "10");
        assert_eq!(holding["nav"]["date"], yesterday);
        assert_eq!(holding["marketValue"], "200");
        let home = crate::home::build_snapshot_range(&store, "month", None, None).unwrap();
        assert_eq!(
            home["portfolio"]["ledger"]["byType"]["fund"]["estimatedDay"]["amountMinor"],
            "1000"
        );
        assert_eq!(
            store
                .with_read(|conn| conn
                    .query_row("SELECT COUNT(*) FROM fund_nav", [], |r| r.get::<_, i64>(0)))
                .unwrap(),
            1
        );
        store.with_write(|conn|conn.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,daily_growth_raw,source,fetched_at) SELECT id,?1,21000,50000,'eastmoney-mobile','0' FROM funds WHERE code='903021'",[&today])).unwrap();
        let confirmed: Value = serde_json::from_str(
            &crate::api::handle_api_request(&store, None, &request, "/api/positions/detail")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(confirmed["holdings"][0]["income"]["day"]["amount"], "10");
        assert_eq!(confirmed["holdings"][0]["nav"]["dailyGrowthPct"], "5");
        assert_eq!(confirmed["holdings"][0]["dayPct"], 5.0);
        assert!(confirmed["holdings"][0]["estimate"].is_null());
        let mut unknown = row;
        unknown.nav_currency = "unknown".into();
        assert!(estimated_income(&unknown, &estimate, &today)["amount"].is_null());
        unknown.nav_currency = "CNY".into();
        unknown.quantity_raw = i64::MAX;
        let mut huge = estimate;
        huge["navRaw"] = json!(i64::MAX);
        assert!(estimated_income(&unknown, &huge, &today)["amount"].is_null());
    }
}
