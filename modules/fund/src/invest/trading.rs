//! 交易时段判定（北京时间 UTC+8）。
//!
//! 目的不是做交易日历，而是回答一个具体问题：
//! **「现在拉到的这个价，应该被当成最新价吗？」**
//!
//! 判断依据是「最近一个交易日」：行情日期早于它才标记 stale。
//! 这样周末拉到周五收盘价不算过期，而周一开盘后仍显示周五价才算出问题。
//!
//! 两个判断分别交给更可靠的来源：
//! - **哪天是交易日** → ：指数日 K 反推，节假日与调休自动包含
//! - **此刻是否开市** → 数据源的  字段，夏令时由上游处理
//! 本模块只保留「无网络时的星期规则兜底」，不再自己算夏令时。

use super::backfill::ordinal_to_ymd;
use super::holdings::ymd_to_ordinal;
use crate::storage::Store;

/// 单一时段：起始与结束的「当日秒数」。
struct Session {
    start: u32,
    end: u32,
}

const CN_SESSIONS: &[Session] = &[
    Session {
        start: 9 * 3600 + 30 * 60,
        end: 11 * 3600 + 30 * 60,
    },
    Session {
        start: 13 * 3600,
        end: 15 * 3600,
    },
];

const HK_SESSIONS: &[Session] = &[
    Session {
        start: 9 * 3600 + 30 * 60,
        end: 12 * 3600,
    },
    Session {
        start: 13 * 3600,
        end: 16 * 3600,
    },
];

/// 美股按北京时间近似：夏令时 21:30–04:00，冬令时 22:30–05:00。
/// 这里取一个覆盖两者的宽区间，宁可多算「可能开盘中」，也不漏刷。
const US_SESSIONS: &[Session] = &[
    Session {
        start: 21 * 3600 + 30 * 60,
        end: 24 * 3600,
    },
    Session {
        start: 0,
        end: 5 * 3600,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    /// 当前是否处于（或可能处于）交易时段。
    pub trading: bool,
    /// 最近一个交易日 YYYY-MM-DD；行情早于这天即为过期。
    pub last_trading_day: String,
    /// 市场标识，回传给前端做文案。
    pub market: String,
}

fn local_secs_of_day(now_secs: u64) -> u32 {
    ((now_secs + 8 * 3600) % 86_400) as u32
}

/// Unix 日序号 → 儒略日序号。
/// `ordinal_to_ymd` 吃的是儒略日，直接喂 Unix 天数会差 2440588 天（公元前 4713 年）。
const UNIX_EPOCH_ORDINAL: i64 = 2_440_588;

fn unix_days_to_ordinal(days: i64) -> i64 {
    days + UNIX_EPOCH_ORDINAL
}

/// 0=周一 … 5=周六 6=周日（与 `ymd_to_ordinal` 的儒略日对齐）。
fn weekday(ordinal: i64) -> i64 {
    ordinal.rem_euclid(7)
}

fn is_weekend(ordinal: i64) -> bool {
    let w = weekday(ordinal);
    w == 5 || w == 6
}

fn previous_weekday(mut ordinal: i64) -> i64 {
    while is_weekend(ordinal) {
        ordinal -= 1;
    }
    ordinal
}

fn in_sessions(secs: u32, sessions: &[Session]) -> bool {
    sessions.iter().any(|s| secs >= s.start && secs < s.end)
}

/// 评估某市场当前状态。`now_secs` 为 Unix 秒，便于测试注入。
///
/// 优先级：数据源状态 > 本地时段规则；最近交易日以日历为准，日历缺失才回退星期规则。
pub fn evaluate(store: &Store, market: &str, now_secs: u64) -> Phase {
    let today = ordinal_to_ymd(unix_days_to_ordinal(
        ((now_secs + 8 * 3600) / 86_400) as i64,
    ));
    let today_ord = ymd_to_ordinal(&today).unwrap_or(0);
    let secs = local_secs_of_day(now_secs);

    let (sessions, open_at): (&[Session], u32) = match market {
        "CN" => (CN_SESSIONS, CN_SESSIONS[0].start),
        "HK" => (HK_SESSIONS, HK_SESSIONS[0].start),
        "US" => (US_SESSIONS, US_SESSIONS[0].start),
        // 数字货币 7×24，永远有最新价
        _ => {
            return Phase {
                trading: true,
                last_trading_day: today,
                market: market.to_string(),
            }
        }
    };

    let weekday_today = !is_weekend(today_ord);
    // 开闭市优先问数据源：它处理夏令时、午休、临时休市，比本地规则可靠。
    // 拿不到状态（首次启动、断网）才退回本地时段判断。
    let trading = match super::calendar::cached_status() {
        Some(status) => status.is_open(market),
        None => weekday_today && in_sessions(secs, sessions),
    };

    // 最近交易日以交易日历为准（含节假日与调休）；日历为空时回退星期规则。
    let fallback_ord = if weekday_today && secs >= open_at {
        today_ord
    } else {
        previous_weekday(today_ord - 1)
    };
    let last_trading_day = super::calendar::last_trading_day(store, market, &today)
        .unwrap_or_else(|| ordinal_to_ymd(fallback_ord));

    Phase {
        trading,
        last_trading_day,
        market: market.to_string(),
    }
}

/// 供测试与调试：某市场最近 N 个交易日（不含节假日，仅排除周末）。
pub fn recent_trading_days(store: &Store, market: &str, now_secs: u64, n: usize) -> Vec<String> {
    let phase = evaluate(store, market, now_secs);
    let mut out = Vec::with_capacity(n);
    let mut cursor = phase.last_trading_day.clone();
    while out.len() < n {
        out.push(cursor.clone());
        let ord = ymd_to_ordinal(&cursor).unwrap_or(0);
        cursor = ordinal_to_ymd(previous_weekday(ord - 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invest::backfill::dates_between;

    /// 构造 UTC 秒：北京时间 YYYY-MM-DD HH:MM。
    /// 空库（无交易日历）时的评估：验证星期规则兜底路径。
    fn eval(market: &str, now: u64) -> Phase {
        evaluate(&Store::open_in_memory().unwrap(), market, now)
    }

    fn bj(day: &str, hour: u32, minute: u32) -> u64 {
        let ord = ymd_to_ordinal(day).unwrap();
        // 儒略日 → Unix 日：1970-01-01 的 ordinal 为 2440588
        let days = ord - 2_440_588;
        (days as u64) * 86_400 + (hour as u64) * 3600 + (minute as u64) * 60 - 8 * 3600
    }

    #[test]
    fn weekday_mapping_matches_known_dates() {
        // 2000-01-01 是周六；2000-01-03 是周一
        assert_eq!(ymd_to_ordinal("2000-01-01").unwrap() % 7, 5);
        assert_eq!(ymd_to_ordinal("2000-01-03").unwrap() % 7, 0);
    }

    #[test]
    fn cn_open_on_weekday_morning() {
        // 2026-11-02 是周一，10:00 开盘中
        let p = eval("CN", bj("2026-11-02", 10, 0));
        assert!(p.trading);
        assert_eq!(p.last_trading_day, "2026-11-02");
    }

    #[test]
    fn cn_closed_during_lunch_break() {
        let p = eval("CN", bj("2026-11-02", 12, 0));
        assert!(!p.trading, "午休不应算交易中");
        // 但今天仍是交易日，行情日期应为今天
        assert_eq!(p.last_trading_day, "2026-11-02");
    }

    #[test]
    fn cn_before_open_points_to_previous_weekday() {
        // 周一 08:00 还没开盘，最近交易日是上周五
        let p = eval("CN", bj("2026-11-02", 8, 0));
        assert!(!p.trading);
        assert_eq!(p.last_trading_day, "2026-10-30");
    }

    #[test]
    fn cn_weekend_points_to_friday() {
        // 2026-11-01 是周日
        let p = eval("CN", bj("2026-11-01", 12, 0));
        assert!(!p.trading);
        assert_eq!(p.last_trading_day, "2026-10-30");
    }

    #[test]
    fn crypto_always_trading() {
        let p = eval("CRYPTO", bj("2026-11-01", 3, 0));
        assert!(p.trading);
        assert_eq!(p.last_trading_day, "2026-11-01");
    }

    #[test]
    fn recent_trading_days_skip_weekend() {
        let days = recent_trading_days(
            &Store::open_in_memory().unwrap(),
            "CN",
            bj("2026-11-02", 10, 0),
            3,
        );
        assert_eq!(days, vec!["2026-11-02", "2026-10-30", "2026-10-29"]);
    }

    #[test]
    fn dates_between_is_used_by_backfill_not_here() {
        // 保留一个弱断言，确认跨模块日期工具口径一致
        assert_eq!(dates_between("2026-11-01", "2026-11-02").len(), 2);
    }
}
