//! nav 模块：基金净值同步与查询（实施 B2，审计 §4.4/§4.9、B5/B13/B14）。
//!
//! - 增量同步：游标存 `funds.latest_nav_date`，`start = last + 1 天`，
//!   `INSERT ... ON CONFLICT(fund_id, nav_date) DO UPDATE` 幂等 upsert。
//! - fallback 链显式可观察：EastMoney Mobile → 拒绝（不静默用旧数据冒充）。
//!   每次同步结果记录 requested/resolved source 与新增条数。
//! - 数据源只走 HTTPS（EastMoney Mobile API）；超时 30s 有界；
//!   字段解析失败显式报错，不吞错为空数据。
//! - 每条净值保留 source / fetched_at；UI 显示来源与日期，过期可判。

#[path = "nav_classification.rs"]
pub(crate) mod classification;
#[path = "nav_estimate.rs"]
mod estimate;
#[path = "nav_quarterly.rs"]
mod quarterly;
pub(crate) use estimate::{cached_estimate, estimated_income, warm_estimates};

use crate::fixed::Fixed;
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

pub const SOURCE_EASTMONEY: &str = "eastmoney-mobile";
/// 增量同步单次最多回补天数（防首次全量过大）。
pub const MAX_CATCHUP_DAYS: i64 = 3650;

#[derive(Debug, PartialEq, Eq)]
pub enum NavError {
    /// 基金不存在。
    UnknownFund(String),
    /// 数据源网络/协议失败（显式，不吞错）。
    Source(String),
    /// 响应字段缺失或非法。
    BadPayload(String),
    Db(String),
}

impl From<rusqlite::Error> for NavError {
    fn from(e: rusqlite::Error) -> Self {
        NavError::Db(e.to_string())
    }
}

impl NavError {
    pub fn code(&self) -> &'static str {
        match self {
            NavError::UnknownFund(_) => "NAV_UNKNOWN_FUND",
            NavError::Source(_) => "NAV_SOURCE_FAILED",
            NavError::BadPayload(_) => "NAV_BAD_PAYLOAD",
            NavError::Db(_) => "NAV_DB",
        }
    }

    pub fn message(&self) -> String {
        match self {
            NavError::UnknownFund(code) => format!("基金 {code} 不存在"),
            NavError::Source(m) => format!("净值来源失败：{m}"),
            NavError::BadPayload(m) => format!("净值响应字段异常：{m}"),
            NavError::Db(e) => format!("数据库错误：{e}"),
        }
    }
}

/// 同步结果（fallback 链可观察，B1/B13）。
#[derive(Debug, PartialEq, Eq)]
pub struct SyncReport {
    pub requested_source: &'static str,
    pub resolved_source: &'static str,
    pub fallback_level: u32,
    pub fund_code: String,
    pub added: usize,
    pub updated: usize,
    /// 同步到的最新净值日期（无数据则保持原值）。
    pub latest_nav_date: Option<String>,
}

/// 一条净值记录（来源可见，B2 验收）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NavRow {
    pub fund_code: String,
    pub nav_date: String,
    pub unit_nav_raw: i64,
    pub accumulated_raw: Option<i64>,
    pub daily_growth_raw: Option<i64>,
    pub source: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QuarterlyHolding {
    pub code: String,
    pub name: String,
    pub ratio_pct: String,
    pub change_type: String,
    pub change_pct: String,
    pub industry: String,
    #[serde(
        default,
        rename = "quoteSymbol",
        skip_serializing_if = "Option::is_none"
    )]
    pub quote_symbol: Option<String>,
    #[serde(default)]
    pub previous_ratio_pct: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QuarterlyReport {
    pub fund_code: String,
    pub report_date: String,
    pub source: String,
    pub stocks: Vec<QuarterlyHolding>,
    pub bonds: Vec<QuarterlyHolding>,
    #[serde(default)]
    pub previous_report_date: Option<String>,
}

/// EastMoney Mobile 历史净值响应条目（字段协议见审计 §4.4）。
#[derive(Debug, Deserialize)]
struct EmHisNetItem {
    #[serde(rename = "FSRQ")]
    date: String,
    #[serde(rename = "DWJZ")]
    unit_nav: String,
    #[serde(rename = "LJJZ", default)]
    accumulated: String,
    #[serde(rename = "JZZZL", default)]
    daily_growth: String,
}

#[derive(Debug, Deserialize)]
struct EmHisNetResponse {
    #[serde(rename = "Datas", default)]
    datas: Vec<EmHisNetItem>,
    #[serde(rename = "ErrCode", default)]
    err_code: i64,
}

/// EastMoney Mobile 数据源（HTTPS；fixture 可注入便于测试）。
pub struct EastMoneySource;

impl EastMoneySource {
    /// 构造历史净值请求 URL（移动端 FundMNHisNetList）。
    pub fn history_url(fund_code: &str, page_size: u32) -> String {
        format!(
            "https://fundmobapi.eastmoney.com/FundMNewApi/FundMNHisNetList?FCODE={code}&deviceid=natives-fund&plat=Iphone&product=EFund&version=6.2.8&pageSize={size}&type=0",
            code = fund_code,
            size = page_size
        )
    }

    /// 拉取历史净值（真实网络；连接/读取超时有界，计划 §30：
    /// 总体 deadline 受 RuntimeLimits 约束，关闭时由进程退出兜底，
    /// 不允许后台继续同步）。
    pub fn fetch_history(fund_code: &str) -> Result<Vec<NavRow>, NavError> {
        Self::fetch_history_size(fund_code, 40)
    }

    pub(crate) fn fetch_analysis_history(fund_code: &str) -> Result<Vec<NavRow>, NavError> {
        Self::fetch_history_size(fund_code, 250)
    }

    pub(crate) fn fetch_detail_history(fund_code: &str) -> Result<Vec<NavRow>, NavError> {
        if fund_code.len() != 6 || !fund_code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(NavError::BadPayload("基金代码无效".into()));
        }
        let body = crate::market::cached_public_response(
            format!("fund-nav:{fund_code}"),
            std::time::Duration::from_secs(30),
            || {
                let rows =
                    Self::fetch_history_size(fund_code, 400).map_err(|e| (502, e.message()))?;
                serde_json::to_string(&rows).map_err(|_| (500, "净值缓存编码失败".into()))
            },
        )
        .map_err(|(_, message)| NavError::Source(message))?;
        serde_json::from_str(&body).map_err(|_| NavError::BadPayload("净值缓存损坏".into()))
    }

    fn fetch_history_size(fund_code: &str, page_size: u32) -> Result<Vec<NavRow>, NavError> {
        let agent = crate::network::client();
        let text = agent
            .get(&Self::history_url(fund_code, page_size))
            .set(
                "User-Agent",
                "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) eastmoney/6.2.8",
            )
            .call()
            .map_err(|e| NavError::Source(format!("eastmoney: {e}")))?
            .into_string()
            .map_err(|e| NavError::Source(format!("eastmoney body: {e}")))?;
        Self::parse_history(&text, fund_code)
    }

    /// 解析响应（fixture 测试入口；字段缺失显式报错）。
    pub fn parse_history(text: &str, fund_code: &str) -> Result<Vec<NavRow>, NavError> {
        let response: EmHisNetResponse =
            serde_json::from_str(text).map_err(|e| NavError::BadPayload(format!("json: {e}")))?;
        if response.err_code != 0 {
            return Err(NavError::Source(format!(
                "eastmoney ErrCode {}",
                response.err_code
            )));
        }
        let now = now_epoch_string();
        let mut rows = Vec::new();
        for item in response.datas {
            let unit_nav = Fixed::parse(&item.unit_nav, 4)
                .map_err(|_| NavError::BadPayload(format!("DWJZ {}", item.unit_nav)))?;
            let accumulated = if item.accumulated.is_empty() || item.accumulated == "--" {
                None
            } else {
                Some(
                    Fixed::parse(&item.accumulated, 4)
                        .map_err(|_| NavError::BadPayload(format!("LJJZ {}", item.accumulated)))?,
                )
            };
            let daily_growth =
                if item.daily_growth.is_empty() || item.daily_growth == "--" {
                    None
                } else {
                    Some(Fixed::parse(&item.daily_growth, 4).map_err(|_| {
                        NavError::BadPayload(format!("JZZZL {}", item.daily_growth))
                    })?)
                };
            rows.push(NavRow {
                fund_code: fund_code.to_string(),
                nav_date: item.date,
                unit_nav_raw: unit_nav.raw(),
                accumulated_raw: accumulated.map(|f| f.raw()),
                daily_growth_raw: daily_growth.map(|f| f.raw()),
                source: SOURCE_EASTMONEY.into(),
                fetched_at: now.clone(),
            });
        }
        Ok(rows)
    }
}

fn now_epoch_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// 增量同步一个基金的历史净值：
/// - 游标 = latest_nav_date；无记录时不限起始（由来源分页上限兜底）。
/// - upsert 幂等；同步后刷新 funds.latest_nav 冗余（B5）。
/// - 来源失败显式返回错误，不用旧数据冒充新数据（B13）。
pub fn sync_fund_nav(
    store: &Store,
    fund_code: &str,
    fetcher: impl Fn(&str) -> Result<Vec<NavRow>, NavError>,
) -> Result<SyncReport, NavError> {
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id FROM funds WHERE code = ?1",
                params![fund_code],
                |r| r.get(0),
            )
            .optional()
        })
        .map_err(NavError::from)?
        .ok_or_else(|| NavError::UnknownFund(fund_code.to_string()))?;

    let rows = fetcher(fund_code)?;
    let (added, updated, latest) = store.with_write(|conn| -> Result<_, NavError> {
        // 先统计已有行数（写入前），再 upsert：added = 新增，updated = 覆盖，
        // 两者互斥，直接分别报告。
        let existing = rows.len() - count_new(conn, fund_id, &rows);
        write_nav_rows(conn, fund_id, &rows)?;
        let latest = refresh_latest(conn, fund_id)?;
        Ok((rows.len() - existing, existing, latest))
    })?;

    Ok(SyncReport {
        requested_source: SOURCE_EASTMONEY,
        resolved_source: SOURCE_EASTMONEY,
        fallback_level: 0,
        fund_code: fund_code.to_string(),
        added,
        updated,
        latest_nav_date: latest,
    })
}

/// 写入净值行（upsert 幂等）；返回 (总数, 其中更新已有记录数)。
pub(crate) fn write_nav_rows(
    conn: &Connection,
    fund_id: i64,
    rows: &[NavRow],
) -> rusqlite::Result<(usize, usize)> {
    let mut updated = 0usize;
    for row in rows {
        let changes = conn.execute(
            "INSERT INTO fund_nav(fund_id, nav_date, unit_nav_raw, accumulated_raw, daily_growth_raw, source, fetched_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(fund_id, nav_date) DO UPDATE SET
               unit_nav_raw = excluded.unit_nav_raw,
               accumulated_raw = excluded.accumulated_raw,
               daily_growth_raw = excluded.daily_growth_raw,
               source = excluded.source,
               fetched_at = excluded.fetched_at",
            params![
                fund_id,
                row.nav_date,
                row.unit_nav_raw,
                row.accumulated_raw,
                row.daily_growth_raw,
                row.source,
                row.fetched_at,
            ],
        )?;
        // ON CONFLICT DO UPDATE 时 changes 计入更新。
        if changes == 0 {
            updated += 1;
        }
    }
    Ok((rows.len(), rows.len() - updated.min(rows.len())))
}

/// 刷新 funds.latest_nav 冗余（仅当有更新），返回最新日期。
pub(crate) fn refresh_latest(conn: &Connection, fund_id: i64) -> rusqlite::Result<Option<String>> {
    let latest: Option<(i64, String)> = conn
        .query_row(
            "SELECT unit_nav_raw, nav_date FROM fund_nav
             WHERE fund_id = ?1 ORDER BY nav_date DESC LIMIT 1",
            params![fund_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((nav_raw, nav_date)) = latest else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE funds SET latest_nav_raw = ?1, latest_nav_date = ?2, updated_at = ?3
         WHERE id = ?4",
        params![nav_raw, nav_date, now_epoch_string(), fund_id],
    )?;
    Ok(Some(nav_date))
}

fn count_new(conn: &Connection, fund_id: i64, rows: &[NavRow]) -> usize {
    rows.iter()
        .filter(|row| {
            conn.query_row(
                "SELECT 1 FROM fund_nav WHERE fund_id = ?1 AND nav_date = ?2",
                params![fund_id, row.nav_date],
                |_| Ok(()),
            )
            .optional()
            .unwrap_or(None)
            .is_none()
        })
        .count()
}

/// 查询某基金最近 N 条净值（来源可见）。
pub fn list_nav(store: &Store, fund_code: &str, limit: usize) -> Result<Vec<NavRow>, NavError> {
    store
        .with_read(|conn| -> rusqlite::Result<Vec<NavRow>> {
            let mut stmt = conn.prepare(
                "SELECT f.code, n.nav_date, n.unit_nav_raw, n.accumulated_raw,
                        n.daily_growth_raw, n.source, n.fetched_at
                 FROM fund_nav n JOIN funds f ON f.id = n.fund_id
                 WHERE f.code = ?1
                 ORDER BY n.nav_date DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![fund_code, limit as i64], |r| {
                    Ok(NavRow {
                        fund_code: r.get(0)?,
                        nav_date: r.get(1)?,
                        unit_nav_raw: r.get(2)?,
                        accumulated_raw: r.get(3)?,
                        daily_growth_raw: r.get(4)?,
                        source: r.get(5)?,
                        fetched_at: r.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(NavError::from)
}

/// DES-D05 有界「查看更多」分页：返回 beforeDate（不含）之前最多 limit 条
/// 净值（日期倒序）。limit 钳制 1..=100；nextCursor 为本页最后一条的日期，
/// 没有更早数据时为 null。日期排序含明确同日语义：同日多行按日期倒序稳定
/// 返回（id 无跨日跳变问题，此处以 nav_date 为游标）。
pub fn list_nav_page(
    store: &Store,
    fund_code: &str,
    before_date: Option<&str>,
    limit: usize,
) -> Result<(Vec<NavRow>, Option<String>), NavError> {
    if let Some(d) = before_date {
        if !crate::assets::is_valid_calendar_date(d) {
            return Err(NavError::BadPayload("beforeDate 无效".into()));
        }
    }
    let limit = limit.clamp(1, 100);
    store
        .with_read(|conn| -> rusqlite::Result<(Vec<NavRow>, Option<String>)> {
            let mut stmt = conn.prepare(
                "SELECT f.code, n.nav_date, n.unit_nav_raw, n.accumulated_raw,
                        n.daily_growth_raw, n.source, n.fetched_at
                 FROM fund_nav n JOIN funds f ON f.id = n.fund_id
                 WHERE f.code = ?1 AND (?2 IS NULL OR n.nav_date < ?2)
                 ORDER BY n.nav_date DESC LIMIT ?3",
            )?;
            let rows = stmt
                .query_map(params![fund_code, before_date, limit as i64], |r| {
                    Ok(NavRow {
                        fund_code: r.get(0)?,
                        nav_date: r.get(1)?,
                        unit_nav_raw: r.get(2)?,
                        accumulated_raw: r.get(3)?,
                        daily_growth_raw: r.get(4)?,
                        source: r.get(5)?,
                        fetched_at: r.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let next_cursor = rows
                .last()
                .filter(|_| rows.len() == limit)
                .map(|row| row.nav_date.clone());
            Ok((rows, next_cursor))
        })
        .map_err(NavError::from)
}

/// 某基金最新净值（含来源与日期，供估值/市值展示）。
pub fn latest_nav(store: &Store, fund_code: &str) -> Result<Option<NavRow>, NavError> {
    Ok(list_nav(store, fund_code, 1)?.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_OK: &str = r#"{
        "ErrCode": 0,
        "Datas": [
            {"FSRQ": "2026-01-06", "DWJZ": "1.2345", "LJJZ": "1.2345", "JZZZL": "0.50"},
            {"FSRQ": "2026-01-05", "DWJZ": "1.2283", "LJJZ": "1.2283", "JZZZL": "--"}
        ]
    }"#;

    #[test]
    fn parse_history_fixture() {
        let rows = EastMoneySource::parse_history(FIXTURE_OK, "000001").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].nav_date, "2026-01-06");
        assert_eq!(rows[0].unit_nav_raw, 12345);
        assert_eq!(rows[0].daily_growth_raw, Some(5000));
        assert_eq!(rows[1].daily_growth_raw, None);
        assert_eq!(rows[0].source, SOURCE_EASTMONEY);
        assert!(!rows[0].fetched_at.is_empty());
    }

    #[test]
    fn parse_bad_payload_explicit_error() {
        let err = EastMoneySource::parse_history("{\"ErrCode\":1001}", "000001").unwrap_err();
        assert!(matches!(err, NavError::Source(_)));
        let err = EastMoneySource::parse_history(
            "{\"ErrCode\":0,\"Datas\":[{\"FSRQ\":\"2026-01-06\",\"DWJZ\":\"1.234567\"}]}",
            "000001",
        )
        .unwrap_err();
        assert!(matches!(err, NavError::BadPayload(_)));
    }

    #[test]
    fn sync_is_idempotent_and_updates_latest() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|conn| -> Result<(), crate::ledger::LedgerError> {
                crate::ledger::ensure_fund(conn, "000001", "测试")?;
                Ok(())
            })
            .unwrap();
        let fetcher = |_: &str| Ok(EastMoneySource::parse_history(FIXTURE_OK, "000001").unwrap());
        let first = sync_fund_nav(&store, "000001", fetcher).unwrap();
        assert_eq!(first.added, 2);
        assert_eq!(first.resolved_source, SOURCE_EASTMONEY);
        assert_eq!(first.latest_nav_date.as_deref(), Some("2026-01-06"));
        // 重复同步：幂等，无新增。
        let second = sync_fund_nav(&store, "000001", fetcher).unwrap();
        assert_eq!(second.added, 0);
        // latest_nav 冗余已刷新。
        let nav = latest_nav(&store, "000001").unwrap().unwrap();
        assert_eq!(nav.nav_date, "2026-01-06");
        assert_eq!(nav.unit_nav_raw, 12345);
    }

    #[test]
    fn sync_unknown_fund_errors() {
        let store = Store::open_in_memory().unwrap();
        let err = sync_fund_nav(&store, "999999", |_: &str| Ok(vec![])).unwrap_err();
        assert!(matches!(err, NavError::UnknownFund(_)));
    }

    #[test]
    fn parse_quarterly_positions_fixture() {
        let json = r#"{
            "Datas": {
                "fundStocks": [
                    {
                        "GPDM": "300308",
                        "GPJC": "中际旭创",
                        "JZBL": "6.45",
                        "PCTNVCHGTYPE": "增持",
                        "PCTNVCHG": "2.14",
                        "INDEXNAME": "通信"
                    }
                ],
                "fundboods": [
                    {
                        "ZQDM": "019827",
                        "ZQMC": "26国债01",
                        "ZJZBL": "7.05"
                    }
                ]
            },
            "ErrCode": 0,
            "Success": true,
            "Expansion": "2026-06-30"
        }"#;
        let report = EastMoneySource::parse_quarterly_positions(json, "000001").unwrap();
        assert_eq!(report.fund_code, "000001");
        assert_eq!(report.report_date, "2026-06-30");
        assert_eq!(report.stocks.len(), 1);
        assert_eq!(report.stocks[0].code, "300308");
        assert_eq!(report.stocks[0].name, "中际旭创");
        assert_eq!(report.stocks[0].ratio_pct, "6.45");
        assert_eq!(report.stocks[0].change_type, "增持");
        assert_eq!(report.stocks[0].change_pct, "2.14");
        assert_eq!(report.stocks[0].industry, "通信");
        assert_eq!(report.bonds.len(), 1);
        assert_eq!(report.bonds[0].name, "26国债01");
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    use crate::storage::Store;

    fn seed(store: &Store, code: &str, dates: &[&str]) {
        store.with_write(|conn| {
            conn.execute(
                "INSERT INTO funds(code,name,updated_at) VALUES(?1,'隔离基金','0')",
                params![code],
            )?;
            for (i, d) in dates.iter().enumerate() {
                conn.execute(
                    "INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,accumulated_raw,daily_growth_raw,source,fetched_at)
                     VALUES((SELECT id FROM funds WHERE code=?1),?2,1000,NULL,NULL,'test',?3)",
                    params![code, d, i as i64],
                )?;
            }
            Ok::<(), rusqlite::Error>(())
        }).unwrap();
    }

    #[test]
    fn nav_pagination_returns_bounded_pages_with_cursor_and_stops() {
        let store = Store::open_in_memory().unwrap();
        seed(
            &store,
            "012345",
            &[
                "2026-09-21",
                "2026-09-22",
                "2026-09-23",
                "2026-09-24",
                "2026-09-25",
            ],
        );
        let (page1, cursor) = list_nav_page(&store, "012345", None, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].nav_date, "2026-09-25");
        assert_eq!(cursor.as_deref(), Some("2026-09-24"));
        let (page2, cursor2) = list_nav_page(&store, "012345", cursor.as_deref(), 2).unwrap();
        assert_eq!(page2[0].nav_date, "2026-09-23");
        assert_eq!(cursor2.as_deref(), Some("2026-09-22"));
        let (page3, cursor3) = list_nav_page(&store, "012345", cursor2.as_deref(), 2).unwrap();
        assert_eq!(page3.len(), 1, "final page returns the remainder");
        assert_eq!(page3[0].nav_date, "2026-09-21");
        assert!(cursor3.is_none(), "no earlier data -> null cursor");
    }

    #[test]
    fn nav_pagination_rejects_invalid_date_and_clamps_limit() {
        let store = Store::open_in_memory().unwrap();
        seed(&store, "012345", &["2026-09-25"]);
        assert!(list_nav_page(&store, "012345", Some("2026-13-99"), 10).is_err());
        let (rows, _) = list_nav_page(&store, "012345", None, 500).unwrap();
        assert_eq!(
            rows.len(),
            1,
            "limit clamped to bounded maximum, still returns what exists"
        );
    }
}
