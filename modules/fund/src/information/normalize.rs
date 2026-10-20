//! 资讯规范化纯函数（PanWatch M04 迁移：`src/modules/market/api/news.py` 的时间解析、
//! `vendors/events.py` 的事件分类词表与重要度、`news_ranker.py` 的去重排序语义）。
//!
//! 改写（方案 04 §3/§4 明确要求）：
//! - 时间解析：东财公开来源的时间字符串不带时区，按来源归属的北京时间（UTC+8）解析后
//!   存 UTC；无法解析时返回 unknown，**不复刻**供体 `datetime.now()` 填充（未知不伪装今天）。
//! - 事件分类保留词表但仅作为启发式标签（`eventType`），不参与买卖判断；分类冲突保留
//!   候选列表，默认展示普通公告。
//! - 去重键为 (source_id, external_id)；标题相似不作为跨来源唯一身份（跨来源疑似相同
//!   事件由聚合展示层处理，不合并正文）。
//! - 排序不含情绪分数（供体 POSITIVE_HINTS/NEGATIVE_HINTS 计数不迁移）；重要度与
//!   关联层级是可解释规则。

/// 发布时间精度（方案 04 §3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePrecision {
    Second,
    Minute,
    Day,
    Unknown,
}

impl TimePrecision {
    pub fn as_str(self) -> &'static str {
        match self {
            TimePrecision::Second => "second",
            TimePrecision::Minute => "minute",
            TimePrecision::Day => "day",
            TimePrecision::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> TimePrecision {
        match s {
            "second" => TimePrecision::Second,
            "minute" => TimePrecision::Minute,
            "day" => TimePrecision::Day,
            _ => TimePrecision::Unknown,
        }
    }
}

/// 东财公开来源时间字符串 → (UTC ISO 秒, 精度)。
///
/// 东财（含 F10 基金公告的 `2026-08-29T00:00:00`）不带时区，按北京时间 UTC+8 解释。
/// 解析失败返回 (None, Unknown)；调用方不得用当前时间填充。
pub fn parse_publish_time(raw: &str) -> (Option<String>, TimePrecision) {
    let text = raw.trim();
    if text.is_empty() {
        return (None, TimePrecision::Unknown);
    }
    let normalized = text.replace('T', " ");
    let trimmed = normalized.trim_end_matches(".000");
    // 完整时间：YYYY-MM-DD HH:MM[:SS]
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() == 2 {
        if let Some((y, mo, d)) = parse_date(parts[0]) {
            let hms: Vec<&str> = parts[1].split(':').collect();
            if hms.len() == 2 || hms.len() == 3 {
                let h: u32 = hms[0].parse().unwrap_or(99);
                let mi: u32 = hms[1].parse().unwrap_or(99);
                let (s, precision) = if hms.len() == 3 {
                    (hms[2].parse::<u32>().unwrap_or(99), TimePrecision::Second)
                } else {
                    (0, TimePrecision::Minute)
                };
                if h < 24 && mi < 60 && s < 60 {
                    // 日期型来源字段以 00:00:00 填充（T01 公告 notice_date 实测）：
                    // 恰为午夜按 day 精度，不冒称有时刻证据。
                    let precision = if h == 0 && mi == 0 && s == 0 {
                        TimePrecision::Day
                    } else {
                        precision
                    };
                    return (Some(format_iso_utc(y, mo, d, h, mi, s)), precision);
                }
            }
            // 时间部分非法但日期合法：降级为 day 精度。
            return (Some(format_iso_utc(y, mo, d, 0, 0, 0)), TimePrecision::Day);
        }
        return (None, TimePrecision::Unknown);
    }
    if parts.len() == 1 {
        if let Some((y, mo, d)) = parse_date(parts[0]) {
            return (Some(format_iso_utc(y, mo, d, 0, 0, 0)), TimePrecision::Day);
        }
    }
    (None, TimePrecision::Unknown)
}

fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    if !crate::assets::is_valid_calendar_date(s) {
        return None;
    }
    let bits: Vec<&str> = s.split('-').collect();
    if bits.len() != 3 {
        return None;
    }
    let y: i32 = bits[0].parse().ok()?;
    let mo: u32 = bits[1].parse().ok()?;
    let d: u32 = bits[2].parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, mo, d))
}

/// Calendar-day UTC collection window; unknown publication times remain unknown.
pub(crate) fn window_start(now: &str, days: u32) -> Option<String> {
    let (y, m, d) = parse_date(now.get(..10)?)?;
    let (y, m, d) = civil_from_days(days_from_civil(y, m, d) - i64::from(days.clamp(1, 30) - 1));
    Some(format!("{y:04}-{m:02}-{d:02}T00:00:00Z"))
}

/// UTC+8 的本地时间字段 → UTC ISO 字符串（考虑跨日）。
fn format_iso_utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> String {
    let epoch_days = days_from_civil(y, mo, d);
    // 当日 8 点对应 UTC 0 点，因此北京时间减 8 小时得到 UTC。
    let local_secs = epoch_days * 86400 + h as i64 * 3600 + mi as i64 * 60 + s as i64;
    let utc_secs = local_secs - 8 * 3600;
    let (yy, mm, dd) = civil_from_days(utc_secs.div_euclid(86400));
    let rem = utc_secs.rem_euclid(86400);
    format!(
        "{yy:04}-{mm:02}-{dd:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant 的 days_from_civil（公历天数，现有 assets.rs 同族算法）。
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(if m <= 2 { 1 } else { 0 });
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 天数（1970-01-01 起算）→ 公历日期。
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    ((y + i64::from(if m <= 2 { 1 } else { 0 })) as i32, m, d)
}

/// 公告事件类型词表（M02 `_guess_event_type` 顺序迁移；纯启发式标签）。
pub fn classify_event(title: &str, column_names: &[String]) -> Vec<&'static str> {
    let mut hits: Vec<&'static str> = Vec::new();
    let mut push = |t: &'static str| {
        if !hits.contains(&t) {
            hits.push(t);
        }
    };
    let t = title;
    if [
        "业绩预告",
        "业绩快报",
        "年报",
        "半年报",
        "季报",
        "三季报",
        "一季报",
        // T01 实网样本：东财/F10 财报标题实际用「半年度报告／中期报告／年度报告」，
        // 供体词表按短词匹配不到，按真实标题补全（方案 04 §2 实网核对要求）。
        "半年度报告",
        "中期报告",
        "年度报告",
    ]
    .iter()
    .any(|k| t.contains(k))
    {
        push("earnings");
    }
    if ["分红", "派息", "除权", "除息", "送转", "股权登记"]
        .iter()
        .any(|k| t.contains(k))
    {
        push("dividend");
    }
    if ["停牌", "复牌"].iter().any(|k| t.contains(k)) {
        push("suspension");
    }
    if ["回购", "股份回购"].iter().any(|k| t.contains(k)) {
        push("repurchase");
    }
    if ["增发", "配股", "定向增发", "发行"]
        .iter()
        .any(|k| t.contains(k))
    {
        push("financing");
    }
    if ["减持", "增持", "股东", "董监高", "持股变动"]
        .iter()
        .any(|k| t.contains(k))
    {
        push("insider");
    }
    if ["诉讼", "仲裁", "立案", "处罚", "监管", "问询函"]
        .iter()
        .any(|k| t.contains(k))
    {
        push("regulatory");
    }
    if ["重组", "并购", "收购", "出售资产", "重大资产"]
        .iter()
        .any(|k| t.contains(k))
    {
        push("restructuring");
    }
    if column_names
        .iter()
        .any(|c| c.contains("临时公告") || c.contains("重大事项"))
    {
        push("major");
    }
    if hits.is_empty() {
        hits.push("notice");
    }
    hits
}

/// 公告重要度（M02 `_guess_importance` 顺序迁移；展示排序用，不做投资结论）。
pub fn announcement_importance(title: &str, column_names: &[String]) -> i32 {
    let t = title;
    if [
        "重大",
        "业绩预告",
        "业绩快报",
        "年报",
        "半年报",
        "重组",
        "停牌",
        "复牌",
    ]
    .iter()
    .any(|k| t.contains(k))
    {
        return 3;
    }
    if ["季报", "分红", "回购", "增持", "减持", "问询函", "处罚"]
        .iter()
        .any(|k| t.contains(k))
    {
        return 2;
    }
    if column_names.iter().any(|c| c.contains("临时")) {
        return 1;
    }
    0
}

/// 新闻标题重要度（M01 `_eastmoney_news_importance` 迁移）。
pub fn news_importance(title: &str) -> i32 {
    if ["重磅", "突发", "紧急", "重大", "独家"]
        .iter()
        .any(|k| title.contains(k))
    {
        return 2;
    }
    if ["快讯", "消息", "公告", "研报"]
        .iter()
        .any(|k| title.contains(k))
    {
        return 1;
    }
    0
}

/// 剥离 HTML 标签（M01 `_strip_html` 迁移）。
pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// 剥 JSONP 外壳（M01 `_parse_jsonp` 语义；非预期结构返回 None，不猜测）。
pub fn parse_jsonp(text: &str) -> Option<&str> {
    let s = text.trim();
    let suffix = s.strip_prefix("jQuery(")?;
    let inner = suffix.strip_suffix(')')?;
    Some(inner)
}

/// 列表排序（方案 04 §4：直接关联重要公告 → 其他直接 → 间接 → 候选；
/// 同层按发布时间降序与稳定 externalId）。`layer` 越小越靠前。
pub fn rank_items<T, F>(items: &mut [T], key: F)
where
    F: Fn(&T) -> (u8, i64, String),
{
    items.sort_by(|a, b| {
        let (la, ta, ia) = key(a);
        let (lb, tb, ib) = key(b);
        la.cmp(&lb)
            .then(tb.cmp(&ta))
            .then(ia.cmp(&ib))
            .then(std::cmp::Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_publish_time_bijing_to_utc() {
        // 东财新闻 date 无时区，按北京时间解释：2026-09-28 15:13:39 CST → 07:13:39Z。
        let (iso, p) = parse_publish_time("2026-09-28 15:13:39");
        assert_eq!(iso.as_deref(), Some("2026-09-28T07:13:39Z"));
        assert_eq!(p, TimePrecision::Second);
        let (iso, p) = parse_publish_time("2026-08-15 00:00:00");
        assert_eq!(iso.as_deref(), Some("2026-08-14T16:00:00Z"));
        assert_eq!(p, TimePrecision::Day, "midnight-only公告日期按 day 精度");
        let (iso, p) = parse_publish_time("2026-08-29T00:00:00");
        assert_eq!(iso.as_deref(), Some("2026-08-28T16:00:00Z"));
        assert_eq!(p, TimePrecision::Day);
        let (iso, p) = parse_publish_time("2026-08-15 08:30");
        assert_eq!(iso.as_deref(), Some("2026-08-15T00:30:00Z"));
        assert_eq!(p, TimePrecision::Minute);
    }

    #[test]
    fn parse_publish_time_unknown_never_fakes_now() {
        for bad in ["", "无法解析", "2026-13-01 99:99", "not-a-date"] {
            let (iso, p) = parse_publish_time(bad);
            assert_eq!(iso, None, "input {bad:?} must not fake a timestamp");
            assert_eq!(p, TimePrecision::Unknown);
        }
    }

    #[test]
    fn classify_event_vocabulary() {
        assert_eq!(classify_event("2026年半年度报告", &[]), vec!["earnings"]);
        assert_eq!(
            classify_event("关于回购股份的通知", &[]),
            vec!["repurchase"]
        );
        assert_eq!(classify_event("日常公告", &[]), vec!["notice"]);
        let cols = vec!["临时公告".to_string()];
        assert_eq!(classify_event("董事决议", &cols), vec!["major"]);
        // 多类型命中保留候选列表（方案 04 §5）。
        let both = classify_event("分红除息暨停牌公告", &[]);
        assert!(both.contains(&"dividend") && both.contains(&"suspension"));
    }

    #[test]
    fn strip_html_and_jsonp() {
        assert_eq!(strip_html(" <b>标题</b> 正文 "), "标题 正文");
        assert_eq!(parse_jsonp("jQuery({\"code\":0})"), Some("{\"code\":0}"));
        assert_eq!(parse_jsonp("<html>blocked</html>"), None);
        assert_eq!(parse_jsonp("jQuery(unclosed"), None);
    }

    #[test]
    fn rank_items_stable_ordering() {
        #[derive(Debug, PartialEq)]
        struct Item {
            id: &'static str,
            layer: u8,
            ts: i64,
        }
        let mut items = vec![
            Item {
                id: "b",
                layer: 1,
                ts: 200,
            },
            Item {
                id: "a",
                layer: 0,
                ts: 100,
            },
            Item {
                id: "c",
                layer: 1,
                ts: 300,
            },
        ];
        rank_items(&mut items, |i| (i.layer, i.ts, i.id.to_string()));
        assert_eq!(
            items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec!["a", "c", "b"]
        );
    }
}
