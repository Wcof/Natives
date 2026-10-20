//! 真实账本估值与人民币价格指数对齐；不以当前持仓回推历史。
use crate::assets::{civil_from_days, days_from_civil, is_valid_calendar_date};
use crate::finance::FinanceError;
use crate::fixed::Fixed;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) const SYMBOLS: &[&str] = &["sh000300", "sh000016", "sh000905", "sh000001", "sz399006"];

#[cfg(test)]
pub(crate) fn compare(
    conn: &Connection,
    today: &str,
    market: &Value,
) -> Result<Value, FinanceError> {
    let from = civil_from_days(days_from_civil(today).unwrap_or(0) - 60);
    compare_range(conn, today, market, &from, today)
}
pub(crate) fn compare_range(
    conn: &Connection,
    today: &str,
    market: &Value,
    from: &str,
    to: &str,
) -> Result<Value, FinanceError> {
    let history = crate::overview::history_range(conn, from, to)?;
    let missing = |reason| {
        json!({"status":"incomplete","reason":reason,"points":[],
        "source":market["source"],"symbol":market["symbol"],"basis":"CNY-price-index","excessPpm":null})
    };
    let candles = market["candles"]
        .as_array()
        .ok_or_else(|| FinanceError::Invalid("基准数据格式无效".into()))?;
    let mut closes = BTreeMap::new();
    for candle in candles {
        let date = candle["time"]
            .as_str()
            .ok_or_else(|| FinanceError::Invalid("基准日期缺失".into()))?;
        if !date.is_ascii() || !is_valid_calendar_date(date) {
            return Err(FinanceError::Invalid("基准日期无效".into()));
        }
        // 今天的日 K 可能仍在盘中，不能冒充完整收盘回报。
        if date >= today {
            continue;
        }
        let close = Fixed::parse(&candle["close"].to_string(), 4)
            .map_err(|_| FinanceError::Invalid("基准价格精度无效".into()))?
            .raw();
        if close <= 0 || closes.insert(date.to_string(), close).is_some() {
            return Err(FinanceError::Invalid("基准价格无效或日期重复".into()));
        }
    }
    let points = history["points"].as_array().unwrap();
    let aligned: Vec<_> = points
        .iter()
        .filter_map(|point| {
            let date = point["date"].as_str()?;
            closes.get(date).map(|close| (date, *close))
        })
        .collect();
    let (Some((from, start)), Some((to, _))) = (aligned.first(), aligned.last()) else {
        return Ok(missing("missing_benchmark"));
    };
    if from == to {
        return Ok(missing("missing_benchmark"));
    }
    let from_day = days_from_civil(from).unwrap();
    let to_day = days_from_civil(to).unwrap();
    if to_day - from_day > 400 {
        return Ok(missing("missing_daily_history"));
    }
    let mut factor = 1_000_000_i128;
    let mut series =
        vec![json!({"date":from,"portfolioPpm":"0","benchmarkPpm":"0","excessPpm":"0"})];
    let mut excess = 0;
    let mut benchmark = 0;
    for day in from_day + 1..=to_day {
        let date = civil_from_days(day);
        let result = crate::returns::daily_returns(conn, &civil_from_days(day - 1), &date)?;
        if !result.computable {
            return Ok(missing("unverified_history"));
        }
        let begin = result.begin_total_minor.unwrap() as i128;
        let adjusted_end =
            result.end_total_minor.unwrap() as i128 - result.net_external_minor.unwrap() as i128;
        if begin <= 0 || adjusted_end <= 0 {
            return Ok(missing("nonpositive_baseline"));
        }
        factor = factor
            .checked_mul(adjusted_end)
            .ok_or_else(|| FinanceError::Overflow("基准比较收益".into()))?
            / begin;
        if let Some(close) = closes.get(&date) {
            benchmark = (*close as i128 - *start as i128) * 1_000_000 / *start as i128;
            excess = factor - 1_000_000 - benchmark;
            series.push(
                json!({"date":date,"portfolioPpm":(factor-1_000_000).to_string(),
                "benchmarkPpm":benchmark.to_string(),"excessPpm":excess.to_string()}),
            );
        }
    }
    Ok(
        json!({"status":"ready","from":from,"to":to,"requestedFrom":history["from"],"requestedTo":history["to"],
        "shortened":history["from"]!=*from || history["to"]!=*to,"cashFlowTiming":"day-end",
        "source":market["source"],"symbol":market["symbol"],"basis":"CNY-price-index",
        "portfolioPpm":(factor-1_000_000).to_string(),"benchmarkPpm":benchmark.to_string(),
        "excessPpm":excess.to_string(),"points":series}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;

    #[test]
    fn endpoint_rejects_unknown_benchmark_and_stale_revision_without_network() {
        let store = Store::open_in_memory().unwrap();
        let revision = store.with_read(|c| Store::data_revision(c)).unwrap();
        let api =
            |path: &str| crate::api::portfolio_benchmark_api::api_portfolio_benchmark(&store, path);
        assert_eq!(
            api("/api/portfolio/benchmark?dataRevision=999999")
                .unwrap_err()
                .0,
            409
        );
        assert_eq!(
            api(&format!(
                "/api/portfolio/benchmark?dataRevision={revision}&symbol=http://example.com"
            ))
            .unwrap_err()
            .0,
            400
        );
        assert_eq!(api("/api/portfolio/benchmark").unwrap_err().0, 400);
        let empty: Value = serde_json::from_str(
            &api(&format!("/api/portfolio/benchmark?dataRevision={revision}")).unwrap(),
        )
        .unwrap();
        assert_eq!(empty["status"], "incomplete");
        assert_eq!(empty["dataRevision"], revision);
    }

    #[test]
    fn aligned_real_dates_exclude_deposits_and_reject_latest_incomplete_or_duplicate_prices() {
        let store = Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        let start = days_from_civil(&today).unwrap() - 5;
        store.with_write(|c| -> Result<(),FinanceError> {
            for day in 0..5 {
                let meta = json!({"flowsChecked":true,"cashEventThrough":0,"transactionThrough":0,"revision":"0"}).to_string();
                c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?1,?2,0,?2,'ready','','0',?3)",rusqlite::params![civil_from_days(start+day),10000+day*1000,meta])?;
            }
            Ok(())
        }).unwrap();
        let mut market = json!({"symbol":"sh000300","source":"isolated-test","candles":[
            {"time":civil_from_days(start),"close":100.0},
            {"time":civil_from_days(start+2),"close":110.0},
            {"time":civil_from_days(start+4),"close":120.0},
            {"time":today,"close":999.0}]});
        let run = |market: &Value| store.with_read(|c| compare(c, &today, market)).unwrap();
        let out = run(&market);
        assert_eq!(out["status"], "ready");
        assert_eq!(out["points"].as_array().unwrap().len(), 3); // 不填休市指数点。
        assert_eq!(out["benchmarkPpm"], "200000");
        let excess: i64 = out["excessPpm"].as_str().unwrap().parse().unwrap();
        assert!((excess - 200000).abs() < 5); // 逐日整数链的舍入误差。
        assert_eq!(out["to"], civil_from_days(start + 4));
        // 期末新增本金不能被算成超额收益。
        store.with_write(|c| -> Result<(),FinanceError> {
            c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('隔离','0','0')",[])?;
            c.execute("INSERT INTO cash_events(account_id,event_type,amount_minor,occurred_at,created_at) VALUES(1,'external_in',10000,?1,'0')",[civil_from_days(start+4)])?;
            c.execute("UPDATE value_snapshots SET total_minor=total_minor+10000,breakdown_json=json_set(breakdown_json,'$.cashEventThrough',1) WHERE taken_at=?1",[civil_from_days(start+4)])?;
            Ok(())
        }).unwrap();
        assert_eq!(run(&market)["excessPpm"], out["excessPpm"]);
        store.with_write(|c| c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) SELECT taken_at,total_minor,cash_minor,noncash_minor,'incomplete','','0',breakdown_json FROM value_snapshots WHERE taken_at=?1",[civil_from_days(start+2)])).unwrap();
        assert_eq!(run(&market)["status"], "incomplete");
        market["candles"]
            .as_array_mut()
            .unwrap()
            .push(json!({"time":civil_from_days(start),"close":100.0}));
        assert!(store.with_read(|c| compare(c, &today, &market)).is_err());
    }
}
