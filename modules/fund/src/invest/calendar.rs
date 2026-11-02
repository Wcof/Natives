//! 交易日历与市场开闭市状态。
//!
//! 两个问题，两个数据来源，都不靠硬编码：
//!
//! 1. **哪天是交易日**：拉指数日 K，有行情的就是交易日。
//!    上证指数 → A 股，恒生指数 → 港股，标普 500 → 美股。
//!    调休、临时休市、法定节假日自动包含，零维护。
//! 2. **现在是否开市**：腾讯 K 线接口返回 `qt.market`
//!    （形如 `US_open_交易中`），由数据源处理夏令时，
//!    比自己算「3 月第二个周日」可靠得多。
//!
//! 状态带 60 秒内存缓存：刷新一次行情只发一次状态请求，
//! 短时间内重复判断直接复用。

use crate::finance::FinanceError;
use crate::storage::Store;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 各市场的日历代理指数。
pub fn index_symbol(market: &str) -> Option<&'static str> {
    match market {
        "CN" => Some("sh000001"),
        "HK" => Some("hkHSI"),
        "US" => Some("us.INX"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarketStatus {
    pub cn_open: bool,
    pub hk_open: bool,
    pub us_open: bool,
}

impl MarketStatus {
    pub fn is_open(&self, market: &str) -> bool {
        match market {
            "CN" => self.cn_open,
            "HK" => self.hk_open,
            "US" => self.us_open,
            // 数字货币 7×24
            _ => true,
        }
    }
}

static STATUS_CACHE: Mutex<Option<(Instant, MarketStatus)>> = Mutex::new(None);

/// 读缓存的市场状态；过期或为空时返回 None，由调用方决定是否重新拉取。
pub fn cached_status() -> Option<MarketStatus> {
    let guard = STATUS_CACHE.lock().ok()?;
    let (at, status) = (*guard)?;
    if at.elapsed() < Duration::from_secs(60) {
        Some(status)
    } else {
        None
    }
}

fn store_status(status: MarketStatus) {
    if let Ok(mut guard) = STATUS_CACHE.lock() {
        *guard = Some((Instant::now(), status));
    }
}

/// 解析 `qt.market`：`2026-11-02 22:37:04|HK_close_已收盘|US_open_交易中|...`
///
/// 只认精确的 `SH`/`SZ`/`HK`/`US` 前缀——`NEWSH`、`USA`、`USB` 是同族的其它市场，
/// 混进来会让判断失真。
fn parse_market_field(text: &str) -> MarketStatus {
    let mut status = MarketStatus::default();
    for part in text.split('|').skip(1) {
        let mut fields = part.split('_');
        let key = fields.next().unwrap_or("");
        let state = fields.next().unwrap_or("");
        let open = state == "open";
        match key {
            "SH" | "SZ" => status.cn_open |= open,
            "HK" => status.hk_open |= open,
            "US" => status.us_open |= open,
            _ => {}
        }
    }
    status
}

fn fetch_index_json(market: &str, count: usize) -> Result<serde_json::Value, String> {
    let symbol = index_symbol(market).ok_or_else(|| format!("{market} 无日历指数"))?;
    let url = format!(
        "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={},day,,,{},bf",
        symbol, count
    );
    crate::network::client()
        .get(&url)
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|e| format!("calendar {market}: {e}"))?
        .into_json::<serde_json::Value>()
        .map_err(|e| format!("calendar {market} json: {e}"))
}

/// 从响应里取日 K 的日期列表。bf（不复权）模式下的键是 `day`。
fn extract_dates(value: &serde_json::Value, market: &str) -> Vec<String> {
    let symbol = match index_symbol(market) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let node = value.get("data").and_then(|d| d.get(symbol));
    let bars = node
        .and_then(|n| n.get("day"))
        .or_else(|| node.and_then(|n| n.get("qfqday")))
        .and_then(|v| v.as_array());
    let Some(bars) = bars else { return Vec::new() };
    bars.iter()
        .filter_map(|bar| bar.as_array())
        .filter_map(|row| row.first())
        .filter_map(|v| v.as_str())
        .map(str::to_string)
        .collect()
}

fn extract_status(value: &serde_json::Value, market: &str) -> Option<MarketStatus> {
    let symbol = index_symbol(market)?;
    let field = value
        .get("data")?
        .get(symbol)?
        .get("qt")?
        .get("market")?
        .as_array()?
        .first()?
        .as_str()?;
    Some(parse_market_field(field))
}

#[derive(Debug, Default, serde::Serialize)]
pub struct CalendarResult {
    pub markets: Vec<String>,
    pub days_written: usize,
    pub errors: Vec<String>,
}

/// 刷新交易日历：拉三个市场的指数日 K，写入 `trading_days`，同时更新开闭市缓存。
///
/// `count` 为每个市场回溯的天数（建议 120，够覆盖长假）。
pub fn refresh(store: &Store, count: usize) -> Result<CalendarResult, FinanceError> {
    let mut result = CalendarResult::default();
    let mut merged = MarketStatus::default();
    let mut got_status = false;

    for market in ["CN", "HK", "US"] {
        match fetch_index_json(market, count) {
            Ok(value) => {
                let dates = extract_dates(&value, market);
                if dates.is_empty() {
                    if result.errors.len() < 3 {
                        result.errors.push(format!("{market} 无日历数据"));
                    }
                    continue;
                }
                store.with_write(|conn| {
                    let mut stmt = conn.prepare(
                        "INSERT OR IGNORE INTO trading_days(market, trade_date) VALUES(?1, ?2)",
                    )?;
                    for date in &dates {
                        stmt.execute(rusqlite::params![market, date])?;
                    }
                    Ok::<(), FinanceError>(())
                })?;
                result.days_written += dates.len();
                result.markets.push(market.to_string());

                if let Some(status) = extract_status(&value, market) {
                    merged.cn_open |= status.cn_open;
                    merged.hk_open |= status.hk_open;
                    merged.us_open |= status.us_open;
                    got_status = true;
                }
            }
            Err(e) => {
                if result.errors.len() < 3 {
                    result.errors.push(e);
                }
            }
        }
    }

    if got_status {
        store_status(merged);
    }
    Ok(result)
}

/// 日历里 <= `today` 的最近交易日。
///
/// **只查日历，不做星期兜底**：兜底规则需要知道各市场的开盘时间
/// （周一 08:00 时最近交易日是上周五，10:00 时就是今天），
/// 那是 `trading` 的职责。这里猜错会让「开盘前」被误判成「已开盘」。
pub fn last_trading_day(store: &Store, market: &str, today: &str) -> Option<String> {
    if market == "CRYPTO" {
        return Some(today.to_string());
    }
    store
        .with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT MAX(trade_date) FROM trading_days
                 WHERE market=?1 AND trade_date<=?2",
            )?;
            let mut rows = stmt.query_map(rusqlite::params![market, today], |r| {
                r.get::<_, Option<String>>(0)
            })?;
            Ok::<_, FinanceError>(rows.next().and_then(|v| v.ok()).flatten())
        })
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_symbols_cover_regions() {
        assert_eq!(index_symbol("CN"), Some("sh000001"));
        assert_eq!(index_symbol("HK"), Some("hkHSI"));
        assert_eq!(index_symbol("US"), Some("us.INX"));
        assert_eq!(index_symbol("CRYPTO"), None);
    }

    #[test]
    fn market_field_parses_open_and_closed() {
        let raw = "2026-11-02 22:37:04|HK_close_已收盘|SH_close_已收盘|SZ_close_已收盘|US_open_交易中|NEWSH_close_已收盘|NEWSZ_close_已收盘|NEWHK_close_已收盘|NEWUS_open_交易中|USA_close_未开盘|USB_close_已收盘";
        let s = parse_market_field(raw);
        assert!(!s.cn_open, "SH/SZ 已收盘");
        assert!(!s.hk_open, "HK 已收盘");
        assert!(s.us_open, "US 交易中");
    }

    #[test]
    fn new_prefixes_do_not_leak_into_real_markets() {
        // 只有 NEWUS 开盘、US 收盘时，美股必须判为收盘
        let raw = "2026-11-02 10:00:00|US_close_已收盘|NEWUS_open_交易中";
        let s = parse_market_field(raw);
        assert!(!s.us_open, "NEWUS 不应影响 US");
    }

    #[test]
    fn empty_calendar_returns_none_not_a_guess() {
        let store = Store::open_in_memory().unwrap();
        // 空日历不猜日期：兜底需要开盘时间，那是 trading 的职责。
        // 这里若硬猜一个日期，会把「周一开盘前」误判成「已开盘」。
        assert_eq!(last_trading_day(&store, "CN", "2026-11-01"), None);
    }

    #[test]
    fn calendar_wins_over_weekday_rule_for_holidays() {
        let store = Store::open_in_memory().unwrap();
        // 模拟国庆：10-01..10-07 全休市，日历里最后一天是 09-30
        store
            .with_write(|c| {
                c.execute(
                    "INSERT INTO trading_days(market, trade_date) VALUES
                     ('CN','2026-09-29'),('CN','2026-09-30')",
                    [],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .unwrap();
        // 2026-10-05 是周一，但日历告诉我们最近交易日是 09-30
        assert_eq!(
            last_trading_day(&store, "CN", "2026-10-05").as_deref(),
            Some("2026-09-30")
        );
    }

    #[test]
    fn crypto_returns_today_without_calendar() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(
            last_trading_day(&store, "CRYPTO", "2026-11-01").as_deref(),
            Some("2026-11-01")
        );
    }

    /// 手动验证：`cargo test -p fund-module live_calendar -- --ignored`
    #[test]
    #[ignore = "需要网络：手动验证指数日历上游"]
    fn live_calendar_fetch_returns_dates_and_status() {
        for market in ["CN", "HK", "US"] {
            let value = fetch_index_json(market, 10).expect("指数 K 线应可达");
            let dates = extract_dates(&value, market);
            assert!(!dates.is_empty(), "{market} 应有交易日");
            assert!(
                extract_status(&value, market).is_some(),
                "{market} 应有市场状态"
            );
        }
    }
}
