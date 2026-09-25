//! 统计类 API 处理器（overview / tools / models / limits / trends / 面板聚合）。

use super::{api_error, json_str};
use crate::storage::Store;

pub(super) fn api_overview(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        // 今日统计 (UTC / 本地日期)
        let today = chrono_today();
        let week_start = chrono_days_ago(7);
        let month_start = format!("{}-01", &today[..7.min(today.len())]);

        let (today_tokens, today_cost_micros, today_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates WHERE date = ?1",
                [&today],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let (week_tokens, week_cost_micros, week_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates WHERE date >= ?1",
                [&week_start],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let (month_tokens, month_cost_micros, month_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates WHERE date >= ?1",
                [&month_start],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let (all_tokens, all_cost_micros, all_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let today_cost = today_cost_micros as f64 / 1_000_000.0;
        let week_cost = week_cost_micros as f64 / 1_000_000.0;
        let month_cost = month_cost_micros as f64 / 1_000_000.0;
        let all_cost = all_cost_micros as f64 / 1_000_000.0;

        Ok(format!(
            "{{\"currency\":\"USD\",\"periods\":{{\"today\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}},\"thisWeek\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}},\"thisMonth\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}},\"allTime\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}}}}}}",
            today_tokens, today_cost, today_sessions,
            week_tokens, week_cost, week_sessions,
            month_tokens, month_cost, month_sessions,
            all_tokens, all_cost, all_sessions
        ))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_tools(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.display_name, s.category, s.enabled,
                        COALESCE(SUM(d.total_tokens), 0),
                        COALESCE(SUM(d.cost_micros), 0),
                        COALESCE(SUM(d.session_count), 0),
                        COALESCE(s.last_scanned_at, '')
                 FROM usage_sources s
                 LEFT JOIN daily_aggregates d ON s.id = d.source_id
                 GROUP BY s.id ORDER BY SUM(d.total_tokens) DESC, s.display_name ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let category: String = row.get(2)?;
                let enabled: bool = row.get::<_, i64>(3)? != 0;
                let tokens: i64 = row.get(4)?;
                let cost_micros: i64 = row.get(5)?;
                let sessions: i64 = row.get(6)?;
                let last_scanned: String = row.get(7)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"id\":{},\"name\":{},\"category\":{},\"enabled\":{},\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{},\"lastScannedAt\":{}}}",
                    json_str(&id), json_str(&name), json_str(&category), enabled, tokens, cost_usd, sessions, json_str(&last_scanned)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"tools\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_models(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT model,
                        COALESCE(SUM(total_tokens), 0),
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cost_micros), 0)
                 FROM daily_aggregates
                 GROUP BY model ORDER BY SUM(total_tokens) DESC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let model: String = row.get(0)?;
                let total: i64 = row.get(1)?;
                let input: i64 = row.get(2)?;
                let output: i64 = row.get(3)?;
                let cost_micros: i64 = row.get(4)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"model\":{},\"totalTokens\":{},\"inputTokens\":{},\"outputTokens\":{},\"costUsd\":{:.4}}}",
                    json_str(&model), total, input, output, cost_usd
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"models\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_limits(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, account_id, window_kind, label, used_percent,
                        remaining_percent, used_units, total_units, unit_type, resets_at,
                        fetched_at, status
                 FROM limits_cache ORDER BY provider_id ASC, window_kind ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let provider: String = row.get(0)?;
                let account: String = row.get(1)?;
                let window: String = row.get(2)?;
                let label: String = row.get(3)?;
                let used_pct: Option<f64> = row.get(4)?;
                let remaining_pct: Option<f64> = row.get(5)?;
                let used_units: Option<f64> = row.get(6)?;
                let total_units: Option<f64> = row.get(7)?;
                let unit_type: String = row.get(8)?;
                let resets_at: Option<String> = row.get(9)?;
                let fetched_at: String = row.get(10)?;
                let status: String = row.get(11)?;

                Ok(format!(
                    "{{\"providerId\":{},\"accountId\":{},\"windowKind\":{},\"label\":{},\"usedPercent\":{},\"remainingPercent\":{},\"usedUnits\":{},\"totalUnits\":{},\"unitType\":{},\"resetsAt\":{},\"fetchedAt\":{},\"status\":{}}}",
                    json_str(&provider), json_str(&account), json_str(&window), json_str(&label),
                    used_pct.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "null".into()),
                    remaining_pct.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "null".into()),
                    used_units.map(|v| format!("{:.2}", v)).unwrap_or_else(|| "null".into()),
                    total_units.map(|v| format!("{:.2}", v)).unwrap_or_else(|| "null".into()),
                    json_str(&unit_type),
                    json_str(&resets_at.unwrap_or_default()),
                    json_str(&fetched_at),
                    json_str(&status)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"limits\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_limits_refresh(store: &Store, _body: &str) -> Result<String, (u16, String)> {
    match crate::limits::refresh_limits(store) {
        Ok(count) => Ok(format!("{{\"ok\":true,\"refreshed\":{}}}", count)),
        Err(e) => Err(api_error(500, "LIMITS_REFRESH_FAILED", &e)),
    }
}

pub(super) fn api_trends(store: &Store, _path: &str) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT date, source_id, SUM(total_tokens), SUM(cost_micros), SUM(session_count)
                 FROM daily_aggregates GROUP BY date, source_id ORDER BY date ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let date: String = row.get(0)?;
                let source_id: String = row.get(1)?;
                let tokens: i64 = row.get(2)?;
                let cost_micros: i64 = row.get(3)?;
                let sessions: i64 = row.get(4)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"date\":{},\"sourceId\":{},\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}}",
                    json_str(&date), json_str(&source_id), tokens, cost_usd, sessions
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"trends\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

// 面板聚合数据（ADR-0032 顶栏只读接口）：总览、工具 Top、最近会话、趋势。
// 注意：调用方必须已持有 conn（with_read 闭包内），严禁再走 with_read——
// conn 是非重入 Mutex，二次加锁会自锁死锁。
pub(super) fn panel_json(conn: &rusqlite::Connection) -> String {
    {
        let today = chrono_today();

        let week_start = chrono_days_ago(7);
        let month_start = format!("{}-01", &today[..7.min(today.len())]);

        // 总览：今日 + 本周 + 本月 + 累计
        let (today_tokens, today_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date = ?1",
                [&today],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let (week_tokens, week_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1",
                [&week_start],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let (month_tokens, month_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1",
                [&month_start],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let (all_tokens, all_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        // 按周期的工具分解：today / thisWeek / thisMonth / allTime（allTime 兼容原 tools 字段）
        let fmt_tools = |rows: Vec<(String, i64, f64)>| -> String {
            rows.iter()
                .map(|(name, tokens, cost)| {
                    format!(
                        "{{\"name\":{},\"tokens\":{},\"costUsd\":{:.2}}}",
                        json_str(name),
                        tokens,
                        cost / 1_000_000.0
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        let today_tools = fmt_tools(tools_decomposed(conn, "d.date >= ", &today));
        let week_tools = fmt_tools(tools_decomposed(conn, "d.date >= ", &week_start));
        let month_tools = fmt_tools(tools_decomposed(conn, "d.date >= ", &month_start));
        let all_tools = fmt_tools(tools_decomposed(conn, "", ""));

        // 最近会话 Top5
        let mut stmt = conn
            .prepare(
                "SELECT session_id, source_id, total_input_tokens + total_output_tokens + total_cache_read_tokens + total_cache_write_tokens + total_reasoning_tokens, total_cost_micros, last_used_at
                 FROM sessions ORDER BY last_used_at DESC LIMIT 5",
            )
            .unwrap_or_else(|_| unreachable!());
        let sessions_json: Vec<String> = stmt
            .query_map([], |row| {
                let sid: String = row.get(0)?;
                let src: String = row.get(1)?;
                let tokens: i64 = row.get(2)?;
                let cost: f64 = row.get::<_, i64>(3)? as f64 / 1_000_000.0;
                let active: String = row.get(4)?;
                Ok(format!(
                    "{{\"id\":{},\"source\":{},\"totalTokens\":{},\"costUsd\":{:.2},\"lastActive\":{}}}",
                    json_str(&sid), json_str(&src), tokens, cost, json_str(&active)
                ))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        drop(stmt);

        // 近 7 天趋势
        let mut stmt = conn
            .prepare(
                "SELECT date, SUM(total_tokens), SUM(cost_micros) FROM daily_aggregates
                 GROUP BY date ORDER BY date DESC LIMIT 7",
            )
            .unwrap_or_else(|_| unreachable!());
        let trends: Vec<String> = stmt
            .query_map([], |row| {
                let date: String = row.get(0)?;
                let tokens: i64 = row.get(1)?;
                let cost: f64 = row.get::<_, i64>(2)? as f64 / 1_000_000.0;
                Ok(format!(
                    "{{\"date\":{},\"totalTokens\":{},\"costUsd\":{:.2}}}",
                    json_str(&date),
                    tokens,
                    cost
                ))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        drop(stmt);

        format!(
            "{{\"today\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisWeek\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisMonth\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"allTime\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"overview\":{{\"periods\":{{\"today\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisWeek\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisMonth\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"allTime\":{{\"totalTokens\":{},\"costUsd\":{:.2}}}}}}},\"tools\":[{}],\"toolsByPeriod\":{{\"today\":[{}],\"thisWeek\":[{}],\"thisMonth\":[{}],\"allTime\":[{}]}},\"sessions\":[{}],\"trends\":[{}]}}",
            today_tokens,
            today_cost_micros as f64 / 1_000_000.0,
            week_tokens,
            week_cost_micros as f64 / 1_000_000.0,
            month_tokens,
            month_cost_micros as f64 / 1_000_000.0,
            all_tokens,
            all_cost_micros as f64 / 1_000_000.0,
            today_tokens,
            today_cost_micros as f64 / 1_000_000.0,
            week_tokens,
            week_cost_micros as f64 / 1_000_000.0,
            month_tokens,
            month_cost_micros as f64 / 1_000_000.0,
            all_tokens,
            all_cost_micros as f64 / 1_000_000.0,
            all_tools,
            today_tools,
            week_tools,
            month_tools,
            all_tools,
            sessions_json.join(","),
            trends.join(",")
        )
    }
}

// 按周期的工具分解（date_filter 为空串表示全量），按 token 降序，最多 8 条。
// 调用方必须已持有 conn（panel_json 的 with_read 闭包内），严禁二次加锁。
fn tools_decomposed(
    conn: &rusqlite::Connection,
    date_filter: &str,
    date_value: &str,
) -> Vec<(String, i64, f64)> {
    let sql: String = if date_filter.is_empty() {
        "SELECT s.display_name, COALESCE(SUM(d.total_tokens), 0), COALESCE(SUM(d.cost_micros), 0)
         FROM usage_sources s LEFT JOIN daily_aggregates d ON s.id = d.source_id
         GROUP BY s.id ORDER BY SUM(d.total_tokens) DESC LIMIT 8"
        .to_string()
    } else {
        format!(
            "SELECT s.display_name, COALESCE(SUM(d.total_tokens), 0), COALESCE(SUM(d.cost_micros), 0)
             FROM usage_sources s LEFT JOIN daily_aggregates d ON s.id = d.source_id
             WHERE {} GROUP BY s.id ORDER BY SUM(d.total_tokens) DESC LIMIT 8",
            date_filter
        )
    };
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<(String, i64, f64)> {
        let name: String = row.get(0)?;
        let tokens: i64 = row.get(1)?;
        let cost: f64 = row.get::<_, i64>(2)? as f64;
        Ok((name, tokens, cost))
    };
    let result = if date_filter.is_empty() {
        stmt.query_map([], map_row)
    } else {
        stmt.query_map([date_value], map_row)
    };
    result
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

pub(super) fn format_compact_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000_000 {
        format!("{:.2}B", tokens as f64 / 1_000_000_000.0)
    } else if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub(super) fn chrono_days_ago(n_days: u64) -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (dur / 86400).saturating_sub(n_days);
    let (y, m, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

pub(super) fn chrono_today() -> String {
    // 简易 UTC 日期格式 YYYY-MM-DD
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = dur / 86400;
    // 粗略公历算法
    let (y, m, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

pub(super) fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = dur / 86400;
    let secs_of_day = dur % 86400;
    let (y, m, d) = days_to_ymd(days);
    let h = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, h, min, s)
}

pub(super) fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };
    (yr, m, d)
}
