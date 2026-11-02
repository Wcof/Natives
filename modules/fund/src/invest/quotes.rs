//! 行情刷新：只拉**当前持仓**的标的，不拉全市场。
//!
//! 设计约束（技术 04 性能、设计文档 §6）：
//! - 一次批量请求最多 60 个代码（上游限制），超出分批。
//! - 拉取失败**不覆盖旧价**，只把 `price_stale` 置 1，让 UI 明确显示数据陈旧。
//! - 价格按 4 位定点写入 `instruments.latest_price_raw`，同时落一条 `quotes` 日线。
//! - 数字货币（CRYPTO）当前数据源不支持，直接跳过而不是报错。

use super::holdings::list_holdings;
use crate::finance::{now_secs, FinanceError};
use crate::market::fetch_tencent_quotes;
use crate::market::normalize_symbol;
use crate::storage::Store;
use std::collections::HashMap;

const SCALE_PRICE: f64 = 10_000.0;
const BATCH: usize = 60;

#[derive(Debug, Default, serde::Serialize)]
pub struct RefreshResult {
    /// 成功写入新价的标的数。
    pub updated: usize,
    /// 拉到数据但解析不出价格的标的数。
    pub missing: usize,
    /// 数据源不支持的标的数（如数字货币）。
    pub skipped: usize,
    /// 上游失败原因，最多保留 3 条，避免把错误面板刷屏。
    pub errors: Vec<String>,
    /// 当前是否处于交易时段（任一持仓市场）。
    pub trading: bool,
}

/// 标的 → 上游行情代码。返回 None 表示数据源不支持该市场。供 backfill 复用。
pub(crate) fn quote_code_for(market: &str, code: &str) -> Option<String> {
    match market {
        // A 股 / 场内基金：normalize_symbol 会补齐 sh/sz/bj
        "CN" => Some(normalize_symbol(code)),
        "US" => {
            let trimmed = code.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(format!("us{}", trimmed.to_uppercase()))
            }
        }
        // 腾讯港股代码为 hk + 5 位数字
        "HK" => code.trim().parse::<u32>().ok().map(|n| format!("hk{n:05}")),
        _ => None,
    }
}

/// 从腾讯 `asOf`（形如 `20261102161455`）取行情日期。
fn date_from_as_of(raw: &str) -> Option<String> {
    let digits: Vec<char> = raw.chars().take(8).collect();
    if digits.len() < 8 || !digits.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let text: String = digits.into_iter().collect();
    Some(format!("{}-{}-{}", &text[0..4], &text[4..6], &text[6..8]))
}

/// 拉取所有持仓标的的最新价。只影响 `instruments.latest_price_raw` 与 `quotes`。
pub fn refresh_all(store: &Store) -> Result<RefreshResult, FinanceError> {
    let holdings = list_holdings(store)?;
    // (instrument_id, quote_code, market)
    let mut wanted: Vec<(i64, String, String)> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut result = RefreshResult::default();

    for h in &holdings {
        if !seen.insert(h.instrument_id) {
            continue;
        }
        match quote_code_for(&h.market, &h.code) {
            Some(qc) => wanted.push((h.instrument_id, qc, h.market.clone())),
            None => result.skipped += 1,
        }
    }

    if wanted.is_empty() {
        return Ok(result);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 每个市场评估一次当前时段，避免逐行重复计算
    let mut phase_by_market: HashMap<String, super::trading::Phase> = HashMap::new();
    for (_, _, market) in &wanted {
        phase_by_market
            .entry(market.clone())
            .or_insert_with(|| super::trading::evaluate(store, market, now));
    }
    result.trading = phase_by_market.values().any(|p| p.trading);

    let today = crate::assets::today_iso();
    // instrument_id -> (price_raw, price_date, stale)
    let mut price_by_instrument: HashMap<i64, (i64, String, bool)> = HashMap::new();

    for chunk in wanted.chunks(BATCH) {
        let codes: Vec<&str> = chunk.iter().map(|(_, c, _)| c.as_str()).collect();
        match fetch_tencent_quotes(&codes, None) {
            Ok(quotes) => {
                // 代码 -> (价格, 行情时间戳)
                let mut by_code: HashMap<String, (f64, Option<String>)> = HashMap::new();
                for q in quotes {
                    if let Some(price) = q.price {
                        if price > 0.0 {
                            by_code.insert(q.code.to_lowercase(), (price, q.as_of.clone()));
                        }
                    }
                }
                for (instrument_id, quote_code, market) in chunk {
                    match by_code.get(&quote_code.to_lowercase()) {
                        Some((price, as_of)) => {
                            let raw = (price * SCALE_PRICE).round() as i64;
                            // 行情日期优先取上游时间戳：周末拉到的是周五收盘价，
                            // 不能记成"今天"，否则下一次判断就分不清新鲜度。
                            let price_date = as_of
                                .as_deref()
                                .and_then(date_from_as_of)
                                .unwrap_or_else(|| today.clone());
                            let stale = phase_by_market
                                .get(market)
                                .map(|p| price_date.as_str() < p.last_trading_day.as_str())
                                .unwrap_or(false);
                            price_by_instrument.insert(*instrument_id, (raw, price_date, stale));
                        }
                        None => result.missing += 1,
                    }
                }
            }
            Err(e) => {
                if result.errors.len() < 3 {
                    result.errors.push(e);
                }
            }
        }
    }

    let ts = now_secs();
    store.with_write(|conn| {
        // 先整体标记陈旧；本次成功刷新的标的再按实际新鲜度覆写。
        // 这样「拉取失败」和「从未拉过」在 UI 上都是 stale，不会被误读成最新。
        conn.execute(
            "UPDATE instruments SET price_stale=1 WHERE id IN (
                 SELECT DISTINCT instrument_id FROM flows
             )",
            [],
        )?;
        for (instrument_id, (raw, date, stale)) in &price_by_instrument {
            conn.execute(
                "UPDATE instruments
                 SET latest_price_raw=?1, latest_price_at=?2, price_stale=?3, updated_at=?4
                 WHERE id=?5",
                rusqlite::params![raw, date, if *stale { 1 } else { 0 }, ts, instrument_id],
            )?;
            conn.execute(
                "INSERT INTO quotes(instrument_id, trade_date, close_raw, currency, source, fetched_at)
                 SELECT id, ?1, ?2, currency, 'tencent', ?3 FROM instruments WHERE id=?4
                 ON CONFLICT(instrument_id, trade_date) DO UPDATE SET
                   close_raw=excluded.close_raw, fetched_at=excluded.fetched_at",
                rusqlite::params![date, raw, ts, instrument_id],
            )?;
        }
        Ok::<(), FinanceError>(())
    })?;

    result.updated = price_by_instrument.len();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_codes_map_by_market() {
        assert_eq!(quote_code_for("CN", "600519").as_deref(), Some("sh600519"));
        assert_eq!(quote_code_for("CN", "000001").as_deref(), Some("sh000001"));
        assert_eq!(quote_code_for("CN", "300750").as_deref(), Some("sz300750"));
        assert_eq!(quote_code_for("US", "aapl").as_deref(), Some("usAAPL"));
        assert_eq!(quote_code_for("HK", "700").as_deref(), Some("hk00700"));
        // 数字货币暂无数据源，必须跳过而不是编造价格
        assert!(quote_code_for("CRYPTO", "BTC").is_none());
    }

    /// 手动验证上游可达与解析链路，不进 CI（依赖网络）。
    /// 运行：`cargo test -p fund-module live_ -- --ignored`
    #[test]
    #[ignore = "需要网络：手动验证实时行情上游"]
    fn live_quote_fetch_returns_price() {
        let quotes = fetch_tencent_quotes(&["sh600519"], None).expect("上游应可达");
        assert_eq!(quotes.len(), 1);
        assert!(quotes[0].price.unwrap_or(0.0) > 0.0, "应解析出正价格");
    }

    #[test]
    #[ignore = "需要网络：手动验证 K 线上游"]
    fn live_kline_fetch_returns_bars() {
        let (bars, _) =
            crate::market::fetch_kline_data("sh600519", "day", "bf").expect("K 线应可达");
        assert!(!bars.is_empty());
        assert!(bars.iter().all(|b| b.close > 0.0));
    }
}
