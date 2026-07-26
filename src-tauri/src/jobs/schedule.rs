//! jobs/schedule.rs — 调度语义解析与 next_run 计算（纯函数，无 IO）
//!
//! 契约第 1 节：
//! - `schedule_type`: `once` | `interval` | `cron`
//! - `schedule_value`: once=ISO8601 时刻；interval=秒数（最小 60）；
//!   cron=5 字段表达式（分 时 日 月 周，支持 `*` `N` `*/N` `A-B` `A,B,C`，周日 0/7 别名）
//! - next_run 一律 UTC 语义（与本地时区/DST 无关），存储 ISO8601。

use chrono::{DateTime, Datelike, Duration, SecondsFormat, TimeZone, Timelike, Utc};

/// interval 类型的最小间隔（秒）。
pub const MIN_INTERVAL_SECS: u64 = 60;

/// cron next_run 搜索上限（天）。超过视为表达式不可满足（如 2 月 30 日）。
const CRON_HORIZON_DAYS: i64 = 366 * 5;

#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleSpec {
    /// 一次性任务：目标时刻（UTC）。
    Once(DateTime<Utc>),
    /// 周期任务：间隔秒数（>= MIN_INTERVAL_SECS）。
    IntervalSecs(u64),
    /// 5 字段 cron（UTC 语义）。
    Cron(CronExpr),
}

/// 已解析的 5 字段 cron 表达式（分 时 日 月 周）。
#[derive(Debug, Clone, PartialEq)]
pub struct CronExpr {
    minutes: [bool; 60],
    hours: [bool; 24],
    /// 下标 1..=31 有效。
    days_of_month: [bool; 32],
    /// 下标 1..=12 有效。
    months: [bool; 13],
    /// 下标 0..=6，0=周日（7 在解析时归一化为 0）。
    days_of_week: [bool; 7],
    dom_restricted: bool,
    dow_restricted: bool,
}

impl CronExpr {
    /// 标准 vixie-cron 日期匹配：日/周字段都受限时取「或」，否则取受限的一侧。
    fn day_matches(&self, t: &DateTime<Utc>) -> bool {
        let dom = self.days_of_month[t.day() as usize];
        let dow = self.days_of_week[t.weekday().num_days_from_sunday() as usize];
        match (self.dom_restricted, self.dow_restricted) {
            (false, false) => true,
            (true, false) => dom,
            (false, true) => dow,
            (true, true) => dom || dow,
        }
    }
}

/// 把 UTC 时刻格式化为存储用 ISO8601（`YYYY-MM-DDTHH:MM:SSZ`）。
pub fn format_utc(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// 解析存储的 ISO8601 时刻（兼容 RFC3339 偏移与无时区写法，无时区按 UTC）。
pub fn parse_utc(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Utc));
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|n| Utc.from_utc_datetime(&n))
}

/// 解析 schedule_type + schedule_value。失败返回给上层映射为 JOB_INVALID_SCHEDULE 的描述。
pub fn parse(schedule_type: &str, schedule_value: &str) -> Result<ScheduleSpec, String> {
    match schedule_type {
        "once" => {
            let t = parse_utc(schedule_value.trim())
                .ok_or_else(|| format!("once schedule expects an ISO8601 timestamp, got '{schedule_value}'"))?;
            Ok(ScheduleSpec::Once(t))
        }
        "interval" => {
            let secs: u64 = schedule_value
                .trim()
                .parse()
                .map_err(|_| format!("interval schedule expects seconds, got '{schedule_value}'"))?;
            if secs < MIN_INTERVAL_SECS {
                return Err(format!(
                    "interval must be >= {MIN_INTERVAL_SECS} seconds, got {secs}"
                ));
            }
            Ok(ScheduleSpec::IntervalSecs(secs))
        }
        "cron" => Ok(ScheduleSpec::Cron(parse_cron(schedule_value)?)),
        other => Err(format!(
            "unknown schedule_type '{other}' (expected once|interval|cron)"
        )),
    }
}

/// 计算严格晚于 `after` 的下一次运行时刻（UTC）。
/// once 已过期 / cron 不可满足时返回 None。
pub fn next_run(spec: &ScheduleSpec, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match spec {
        ScheduleSpec::Once(t) => {
            if *t > after {
                Some(*t)
            } else {
                None
            }
        }
        ScheduleSpec::IntervalSecs(secs) => Some(after + Duration::seconds(*secs as i64)),
        ScheduleSpec::Cron(expr) => next_cron(expr, after),
    }
}

fn parse_cron(value: &str) -> Result<CronExpr, String> {
    let fields: Vec<&str> = value.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "cron expects 5 fields (minute hour day month weekday), got {}",
            fields.len()
        ));
    }

    let (minute_vals, _) = parse_field(fields[0], 0, 59)?;
    let (hour_vals, _) = parse_field(fields[1], 0, 23)?;
    let (dom_vals, dom_restricted) = parse_field(fields[2], 1, 31)?;
    let (month_vals, _) = parse_field(fields[3], 1, 12)?;
    // 周字段允许 0-7，7 是周日别名，归一化为 0。
    let (dow_raw, dow_restricted) = parse_field(fields[4], 0, 7)?;

    let mut expr = CronExpr {
        minutes: [false; 60],
        hours: [false; 24],
        days_of_month: [false; 32],
        months: [false; 13],
        days_of_week: [false; 7],
        dom_restricted,
        dow_restricted,
    };
    for v in minute_vals {
        expr.minutes[v as usize] = true;
    }
    for v in hour_vals {
        expr.hours[v as usize] = true;
    }
    for v in dom_vals {
        expr.days_of_month[v as usize] = true;
    }
    for v in month_vals {
        expr.months[v as usize] = true;
    }
    for v in dow_raw {
        expr.days_of_week[(v % 7) as usize] = true;
    }
    Ok(expr)
}

/// 解析单个 cron 字段。返回 (允许值集合, 是否受限)。
/// 支持 `*`、`N`、`*/N`、`A-B`、逗号组合（如 `1,5-10`）。
fn parse_field(field: &str, min: u32, max: u32) -> Result<(Vec<u32>, bool), String> {
    let mut values = Vec::new();
    let mut restricted = false;
    for item in field.split(',') {
        let item = item.trim();
        if item.is_empty() {
            return Err(format!("empty item in cron field '{field}'"));
        }
        if item == "*" {
            values.extend(min..=max);
            continue;
        }
        restricted = true;
        if let Some(step) = item.strip_prefix("*/") {
            let n: u32 = step
                .parse()
                .map_err(|_| format!("invalid step '{item}' in cron field"))?;
            if n == 0 {
                return Err("cron step must be > 0".to_string());
            }
            values.extend((min..=max).step_by(n as usize));
        } else if let Some((a, b)) = item.split_once('-') {
            let a: u32 = a
                .trim()
                .parse()
                .map_err(|_| format!("invalid range start in '{item}'"))?;
            let b: u32 = b
                .trim()
                .parse()
                .map_err(|_| format!("invalid range end in '{item}'"))?;
            if a > b {
                return Err(format!("cron range start > end in '{item}'"));
            }
            if a < min || b > max {
                return Err(format!(
                    "cron range '{item}' out of bounds [{min},{max}]"
                ));
            }
            values.extend(a..=b);
        } else {
            let v: u32 = item
                .parse()
                .map_err(|_| format!("invalid number '{item}' in cron field"))?;
            if v < min || v > max {
                return Err(format!("cron value {v} out of bounds [{min},{max}]"));
            }
            values.push(v);
        }
    }
    Ok((values, restricted))
}

/// 找严格晚于 after 的下一个匹配分钟（UTC）。逐段跳跃（月→日→时→分），
/// 超过搜索上限（约 5 年）返回 None。
fn next_cron(expr: &CronExpr, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut t = after.with_second(0)?.with_nanosecond(0)? + Duration::minutes(1);
    let horizon = after + Duration::days(CRON_HORIZON_DAYS);
    while t <= horizon {
        if !expr.months[t.month() as usize] {
            let (y, m) = if t.month() == 12 {
                (t.year() + 1, 1)
            } else {
                (t.year(), t.month() + 1)
            };
            t = Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0).single()?;
            continue;
        }
        if !expr.day_matches(&t) {
            let next_day = t.date_naive().succ_opt()?;
            t = Utc.from_utc_datetime(&next_day.and_hms_opt(0, 0, 0)?);
            continue;
        }
        if !expr.hours[t.hour() as usize] {
            t = t.with_minute(0)? + Duration::hours(1);
            continue;
        }
        if !expr.minutes[t.minute() as usize] {
            t += Duration::minutes(1);
            continue;
        }
        return Some(t);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    fn cron_next(expr: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let spec = parse("cron", expr).unwrap();
        next_run(&spec, after)
    }

    // ── interval ──

    #[test]
    fn interval_parses_and_advances() {
        let spec = parse("interval", "60").unwrap();
        assert_eq!(spec, ScheduleSpec::IntervalSecs(60));
        let after = utc(2026, 7, 26, 12, 0, 30);
        assert_eq!(next_run(&spec, after), Some(utc(2026, 7, 26, 12, 1, 30)));
    }

    #[test]
    fn interval_below_minimum_rejected() {
        assert!(parse("interval", "59").is_err());
        assert!(parse("interval", "0").is_err());
        assert!(parse("interval", "abc").is_err());
        assert!(parse("interval", "-60").is_err());
    }

    // ── once ──

    #[test]
    fn once_future_returns_target() {
        let spec = parse("once", "2026-08-01T00:00:00Z").unwrap();
        let after = utc(2026, 7, 26, 0, 0, 0);
        assert_eq!(next_run(&spec, after), Some(utc(2026, 8, 1, 0, 0, 0)));
    }

    #[test]
    fn once_expired_returns_none() {
        let spec = parse("once", "2026-07-01T00:00:00Z").unwrap();
        let after = utc(2026, 7, 26, 0, 0, 0);
        assert_eq!(next_run(&spec, after), None);
        // 恰好等于 after 也算过期（next 必须严格晚于 after）
        let spec = parse("once", "2026-07-26T00:00:00Z").unwrap();
        assert_eq!(next_run(&spec, after), None);
    }

    #[test]
    fn once_accepts_offset_and_naive_as_utc() {
        // +08:00 偏移换算到 UTC
        let spec = parse("once", "2026-08-01T08:00:00+08:00").unwrap();
        assert_eq!(spec, ScheduleSpec::Once(utc(2026, 8, 1, 0, 0, 0)));
        // 无时区按 UTC
        let spec = parse("once", "2026-08-01T00:00:00").unwrap();
        assert_eq!(spec, ScheduleSpec::Once(utc(2026, 8, 1, 0, 0, 0)));
        assert!(parse("once", "not-a-date").is_err());
    }

    // ── cron 语法形态 ──

    #[test]
    fn cron_every_minute() {
        let after = utc(2026, 7, 26, 12, 0, 30);
        // 严格晚于 after：秒数截断后进位到下一分钟
        assert_eq!(cron_next("* * * * *", after), Some(utc(2026, 7, 26, 12, 1, 0)));
    }

    #[test]
    fn cron_fixed_minute() {
        let after = utc(2026, 7, 26, 12, 10, 0);
        assert_eq!(cron_next("5 * * * *", after), Some(utc(2026, 7, 26, 13, 5, 0)));
    }

    #[test]
    fn cron_step_minutes() {
        let after = utc(2026, 7, 26, 12, 16, 0);
        assert_eq!(
            cron_next("*/15 * * * *", after),
            Some(utc(2026, 7, 26, 12, 30, 0))
        );
    }

    #[test]
    fn cron_hour_range() {
        // 工作时段 9-17 点整点；18 点之后跳到次日 9 点
        let after = utc(2026, 7, 26, 18, 0, 0);
        assert_eq!(
            cron_next("0 9-17 * * *", after),
            Some(utc(2026, 7, 27, 9, 0, 0))
        );
    }

    #[test]
    fn cron_comma_list_days() {
        // 每月 1、15 日 0 点。7/26 之后是 8/1。
        let after = utc(2026, 7, 26, 0, 0, 0);
        assert_eq!(
            cron_next("0 0 1,15 * *", after),
            Some(utc(2026, 8, 1, 0, 0, 0))
        );
    }

    #[test]
    fn cron_sunday_zero_and_seven_alias() {
        let after = utc(2026, 7, 22, 0, 0, 0); // 2026-07-22 是周三
        let with_zero = cron_next("0 0 * * 0", after);
        let with_seven = cron_next("0 0 * * 7", after);
        assert_eq!(with_zero, with_seven);
        assert_eq!(with_zero, Some(utc(2026, 7, 26, 0, 0, 0))); // 下个周日
    }

    #[test]
    fn cron_dom_dow_or_semantics() {
        // 标准 vixie：日与周都受限时取「或」。
        // 13 号或周五。2026-08-01 之后：最近的周五是 8/7，早于 8/13。
        let after = utc(2026, 8, 1, 0, 0, 0); // 周六
        assert_eq!(
            cron_next("0 0 13 * 5", after),
            Some(utc(2026, 8, 7, 0, 0, 0))
        );
        // 从 8/7 之后：8/13（周四）先于下个周五 8/14。
        let after = utc(2026, 8, 7, 0, 0, 0);
        assert_eq!(
            cron_next("0 0 13 * 5", after),
            Some(utc(2026, 8, 13, 0, 0, 0))
        );
    }

    #[test]
    fn cron_boundary_exact_match_advances() {
        // after 恰好命中表达式时，next 必须是下一个匹配点（严格晚于 after）
        let after = utc(2026, 1, 1, 0, 0, 0);
        assert_eq!(cron_next("0 0 * * *", after), Some(utc(2026, 1, 2, 0, 0, 0)));
    }

    #[test]
    fn cron_year_rollover() {
        // 每年 1 月 1 日 0 点，从 2 月出发跨年
        let after = utc(2026, 2, 10, 0, 0, 0);
        assert_eq!(cron_next("0 0 1 1 *", after), Some(utc(2027, 1, 1, 0, 0, 0)));
    }

    #[test]
    fn cron_impossible_date_returns_none() {
        // 2 月 30 日不存在
        let after = utc(2026, 7, 26, 0, 0, 0);
        assert_eq!(cron_next("0 0 30 2 *", after), None);
    }

    #[test]
    fn cron_leap_day() {
        // 2 月 29 日：2026/2027 非闰年，下一次在 2028
        let after = utc(2026, 3, 1, 0, 0, 0);
        assert_eq!(
            cron_next("0 0 29 2 *", after),
            Some(utc(2028, 2, 29, 0, 0, 0))
        );
    }

    #[test]
    fn cron_invalid_forms_rejected() {
        assert!(parse("cron", "60 * * * *").is_err()); // 分钟越界
        assert!(parse("cron", "* 24 * * *").is_err()); // 小时越界
        assert!(parse("cron", "* * 0 * *").is_err()); // 日下界
        assert!(parse("cron", "* * * 13 *").is_err()); // 月越界
        assert!(parse("cron", "* * * * 8").is_err()); // 周越界
        assert!(parse("cron", "* * * *").is_err()); // 4 字段
        assert!(parse("cron", "* * * * * *").is_err()); // 6 字段
        assert!(parse("cron", "*/0 * * * *").is_err()); // 步长 0
        assert!(parse("cron", "10-5 * * * *").is_err()); // 逆序区间
        assert!(parse("cron", "a * * * *").is_err()); // 非数字
        assert!(parse("cron", "1,,2 * * * *").is_err()); // 空项
    }

    #[test]
    fn unknown_schedule_type_rejected() {
        assert!(parse("daily", "whatever").is_err());
    }

    #[test]
    fn format_and_parse_roundtrip() {
        let t = utc(2026, 7, 26, 12, 34, 56);
        let s = format_utc(&t);
        assert_eq!(s, "2026-07-26T12:34:56Z");
        assert_eq!(parse_utc(&s), Some(t));
        // 兼容旧 to_rfc3339() 写入的 +00:00 形式
        assert_eq!(parse_utc("2026-07-26T12:34:56+00:00"), Some(t));
    }
}
