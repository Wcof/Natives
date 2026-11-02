//! 历史快照补算。
//!
//! 模块无常驻进程，无法用 cron 每天落快照。改用 lazy 策略：打开时发现缺哪天就补哪天。
//!
//! 两个关键决定：
//! 1. **按日期截断回放**：历史某天的持仓数量 = 只回放到那天为止的流水。
//!    这样拆股、加仓、清仓都能正确还原，而不是用今天的持仓套历史价。
//! 2. **用不复权价（bf）**：复权价会改写历史价格，用来算历史市值会与当时真值偏离。
//!    不复权价在除权日有跳空，那正是真实市值变化。
//!
//! 补算只写**缺失的日期**，绝不覆盖已有快照——已有的是用实时价落的，更准。

use super::holdings::{replay, ymd_to_ordinal, FlowRow};
use super::quotes::quote_code_for;
use crate::finance::FinanceError;
use crate::storage::Store;
use std::collections::{BTreeMap, HashMap};

/// 单次补算的最大天数，防止首次打开时因历史过长而卡住界面。
pub const MAX_BACKFILL_DAYS: i64 = 90;

#[derive(Debug, Default, serde::Serialize)]
pub struct BackfillResult {
    /// 实际写入的快照天数。
    pub days: usize,
    /// 因缺历史价而标记为不完整的天数。
    pub stale_days: usize,
    /// 成功取到历史价的标的数。
    pub priced_instruments: usize,
    /// 取价失败的原因，最多 3 条。
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedSnapshot {
    pub date: String,
    pub net_worth: i64,
    pub cost: i64,
    pub stale: bool,
}

pub(crate) fn ordinal_to_ymd(ordinal: i64) -> String {
    // 与 ymd_to_ordinal 互逆的常用展开（Fliegel–Van Flandern）
    let mut l = ordinal + 68569;
    let n = 4 * l / 146097;
    l -= (146097 * n + 3) / 4;
    let i = 4000 * (l + 1) / 1461001;
    l = l - 1461 * i / 4 + 31;
    let j = 80 * l / 2447;
    let day = l - 2447 * j / 80;
    l = j / 11;
    let month = j + 2 - 12 * l;
    let year = 100 * (n - 49) + i + l;
    format!("{year:04}-{month:02}-{day:02}")
}

/// 生成 [from, to] 闭区间的日期列表。
pub fn dates_between(from: &str, to: &str) -> Vec<String> {
    match (ymd_to_ordinal(from), ymd_to_ordinal(to)) {
        (Some(a), Some(b)) if b >= a => (a..=b).map(ordinal_to_ymd).collect(),
        _ => Vec::new(),
    }
}

/// 纯计算：给定流水分组、历史价、日期列表，算出每天的快照。
///
/// 无历史价的标的**不计入**当天市值（与实时口径一致），并把这天标记为 stale。
pub fn compute_series(
    grouped: &BTreeMap<(i64, i64), Vec<FlowRow>>,
    prices: &HashMap<i64, HashMap<String, i64>>,
    dates: &[String],
) -> Result<Vec<ComputedSnapshot>, String> {
    let mut out = Vec::with_capacity(dates.len());
    for date in dates {
        let mut net_worth: i64 = 0;
        let mut cost: i64 = 0;
        let mut stale = false;
        for ((_account_id, instrument_id), rows) in grouped {
            let upto: Vec<FlowRow> = rows
                .iter()
                .filter(|f| f.trade_date.as_str() <= date.as_str())
                .cloned()
                .collect();
            if upto.is_empty() {
                continue;
            }
            let state = replay(&upto)?;
            if state.quantity == 0 {
                continue;
            }
            cost += state.cost;
            match prices.get(instrument_id).and_then(|m| m.get(date)) {
                Some(price) => {
                    net_worth += ((state.quantity as i128) * (*price as i128) / 1_000_000) as i64;
                }
                None => stale = true,
            }
        }
        out.push(ComputedSnapshot {
            date: date.clone(),
            net_worth,
            cost,
            stale,
        });
    }
    Ok(out)
}

fn latest_snapshot_date(store: &Store) -> Result<Option<String>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare("SELECT MAX(snapshot_date) FROM snapshots")?;
        let mut rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
        match rows.next() {
            Some(v) => Ok(v?),
            None => Ok(None),
        }
    })
}

/// 拉一只标的的全部日线收盘价（不复权）。
fn fetch_close_prices(market: &str, code: &str) -> Result<HashMap<String, i64>, String> {
    let symbol = quote_code_for(market, code).ok_or_else(|| "数据源不支持该市场".to_string())?;
    let (bars, _source) =
        crate::market::fetch_kline_data(&symbol, "day", "bf").map_err(|(_, msg)| msg)?;
    let mut map = HashMap::with_capacity(bars.len());
    for bar in bars {
        if bar.close > 0.0 {
            map.insert(bar.date, (bar.close * 10_000.0).round() as i64);
        }
    }
    Ok(map)
}

/// 补齐缺失的历史快照。返回本次实际写入的情况。
pub fn backfill(store: &Store, max_days: i64) -> Result<BackfillResult, FinanceError> {
    let grouped = super::holdings::load_flows(store)?;
    if grouped.is_empty() {
        return Ok(BackfillResult::default());
    }
    let instruments = super::holdings::load_instrument_meta_map(store)?;
    let today = crate::assets::today_iso();
    let mut result = BackfillResult::default();

    // 起点：已有快照则从次日开始；否则从最早流水日（不超过 max_days）
    let earliest = grouped
        .values()
        .filter_map(|rows| rows.first().map(|f| f.trade_date.clone()))
        .min()
        .unwrap_or_else(|| today.clone());

    let start = match latest_snapshot_date(store)? {
        Some(last) => match (ymd_to_ordinal(&last), ymd_to_ordinal(&today)) {
            (Some(a), Some(b)) if b > a => ordinal_to_ymd(a + 1),
            _ => return Ok(result), // 已是最新
        },
        None => earliest,
    };

    let mut dates = dates_between(&start, &today);
    if dates.is_empty() {
        return Ok(result);
    }
    // 只补最近 max_days 天
    let limit = max_days.clamp(1, MAX_BACKFILL_DAYS) as usize;
    if dates.len() > limit {
        dates = dates.split_off(dates.len() - limit);
    }

    // 拉历史价：每只标的只请求一次
    let mut prices: HashMap<i64, HashMap<String, i64>> = HashMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for (_k, rows) in grouped.iter() {
        let Some(first) = rows.first() else { continue };
        let instrument_id = first.instrument_id;
        if !seen.insert(instrument_id) {
            continue;
        }
        let Some(meta) = instruments.get(&instrument_id) else {
            continue;
        };
        match fetch_close_prices(&meta.market, &meta.code) {
            Ok(map) if !map.is_empty() => {
                prices.insert(instrument_id, map);
                result.priced_instruments += 1;
            }
            Ok(_) => {
                if result.errors.len() < 3 {
                    result.errors.push(format!("{} 无历史行情", meta.code));
                }
            }
            Err(e) => {
                if result.errors.len() < 3 {
                    result.errors.push(format!("{}: {e}", meta.code));
                }
            }
        }
    }

    let series = compute_series(&grouped, &prices, &dates).map_err(FinanceError::Invalid)?;
    for point in series {
        if point.stale {
            result.stale_days += 1;
        }
        super::holdings::upsert_snapshot(
            store,
            &point.date,
            point.net_worth,
            point.cost,
            0,
            point.stale,
        )?;
        result.days += 1;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(id: i64, kind: &str, qty: i64, amount: i64, date: &str) -> FlowRow {
        FlowRow {
            id,
            account_id: 1,
            instrument_id: 1,
            kind: kind.to_string(),
            quantity_raw: qty,
            price_raw: 0,
            amount_raw: amount,
            fee_raw: 0,
            split_from: None,
            split_to: None,
            trade_date: date.to_string(),
            note: String::new(),
        }
    }

    #[test]
    fn ordinal_roundtrip_covers_leap_day() {
        for date in ["2024-02-29", "2026-01-01", "2026-11-02", "2000-12-31"] {
            let o = ymd_to_ordinal(date).unwrap();
            assert_eq!(ordinal_to_ymd(o), date);
        }
    }

    #[test]
    fn dates_between_is_inclusive() {
        let d = dates_between("2026-01-30", "2026-02-02");
        assert_eq!(
            d,
            vec!["2026-01-30", "2026-01-31", "2026-02-01", "2026-02-02"]
        );
        assert!(dates_between("2026-02-02", "2026-01-01").is_empty());
    }

    #[test]
    fn historical_quantity_uses_flow_cutoff_not_today() {
        // 1/10 买 100，3/01 再买 100；2 月的快照只能是 100 股
        let mut grouped = BTreeMap::new();
        grouped.insert(
            (1i64, 1i64),
            vec![
                flow(1, "BUY", 1_000_000, 1_000_00, "2026-01-10"),
                flow(2, "BUY", 1_000_000, 2_000_00, "2026-03-01"),
            ],
        );
        let mut prices = HashMap::new();
        let mut m = HashMap::new();
        m.insert("2026-02-01".to_string(), 150_000i64); // 15.00
        m.insert("2026-03-05".to_string(), 150_000i64);
        prices.insert(1i64, m);

        let series = compute_series(
            &grouped,
            &prices,
            &["2026-02-01".to_string(), "2026-03-05".to_string()],
        )
        .unwrap();
        // 2/01：100 股 × 15 = 1500
        assert_eq!(series[0].net_worth, 1_500_00);
        // 3/05：200 股 × 15 = 3000
        assert_eq!(series[1].net_worth, 3_000_00);
    }

    #[test]
    fn missing_price_marks_day_stale_and_excludes_value() {
        let mut grouped = BTreeMap::new();
        grouped.insert(
            (1i64, 1i64),
            vec![flow(1, "BUY", 1_000_000, 1_000_00, "2026-01-10")],
        );
        let prices: HashMap<i64, HashMap<String, i64>> = HashMap::new();
        let series = compute_series(&grouped, &prices, &["2026-02-01".to_string()]).unwrap();
        assert_eq!(series[0].net_worth, 0);
        assert!(series[0].stale, "无价格必须标记 stale，不能给 0 市值冒充");
        // 成本仍然记入，便于 UI 显示投入
        assert_eq!(series[0].cost, 1_000_00);
    }

    #[test]
    fn cleared_position_contributes_nothing() {
        let mut grouped = BTreeMap::new();
        grouped.insert(
            (1i64, 1i64),
            vec![
                flow(1, "BUY", 1_000_000, 1_000_00, "2026-01-10"),
                flow(2, "SELL", 1_000_000, 1_200_00, "2026-01-20"),
            ],
        );
        let mut prices = HashMap::new();
        let mut m = HashMap::new();
        m.insert("2026-02-01".to_string(), 120_000i64);
        prices.insert(1i64, m);
        let series = compute_series(&grouped, &prices, &["2026-02-01".to_string()]).unwrap();
        assert_eq!(series[0].net_worth, 0, "已清仓标的不应计入历史净值");
        assert!(!series[0].stale, "清仓不算缺数据");
    }

    #[test]
    fn fmt_amount_used_for_snapshot_output_is_normalized() {
        assert_eq!(super::super::holdings::fmt_amount(1_500_00), "1500");
    }
}
