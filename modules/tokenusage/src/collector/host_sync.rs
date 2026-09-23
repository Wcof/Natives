//! host_sync: 从系统 Model Host 权威数据库 (~/.natives/usage.db) 同步数据至 tokenusage.db。

use crate::collector::pricing::calculate_cost_micros;
use crate::storage::Store;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct HostSyncSummary {
    pub events_synced: usize,
    pub total_tokens: i64,
    pub total_cost_micros: i64,
    pub budgets_synced: usize,
}

pub fn resolve_usage_db_path(override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_path {
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    if let Ok(p) = std::env::var("NATIVES_USAGE_DB_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(p) = std::env::var("USAGE_DB_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let p = home.join(".natives").join("usage.db");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn normalize_source_id(tool_id: &str, source: &str) -> &'static str {
    let s = if !tool_id.is_empty() { tool_id } else { source };
    match s {
        "claude-code" | "claude" => "claude",
        "codex" => "codex",
        "cursor" => "cursor",
        "pi" => "pi",
        "hermes" => "hermes",
        "opencode" => "opencode",
        "cline" => "cline",
        "openclaw" => "openclaw",
        "antigravity" => "antigravity",
        "kimi" | "kimi-code" => "kimi",
        "qwen" => "qwen",
        "grok" | "grok-build" => "grok",
        "copilot" => "copilot",
        "atomcode" => "atomcode",
        "zcode" => "zcode",
        "zed" => "zed",
        _ if s.is_empty() => "local-proxy",
        _ => "other",
    }
}

fn source_meta(source_id: &str) -> (&'static str, &'static str) {
    match source_id {
        "claude" => ("Claude Code", "cli"),
        "codex" => ("Codex", "cli"),
        "cursor" => ("Cursor", "ide"),
        "pi" => ("Pi", "cli"),
        "hermes" => ("Hermes Agent", "cli"),
        "opencode" => ("OpenCode", "cli"),
        "cline" => ("Cline", "ide"),
        "openclaw" => ("OpenClaw", "cli"),
        "antigravity" => ("Antigravity", "ide"),
        "kimi" => ("Kimi", "cli"),
        "qwen" => ("Qwen", "cli"),
        "grok" => ("Grok Build", "cli"),
        "copilot" => ("GitHub Copilot", "ide"),
        "atomcode" => ("AtomCode", "ide"),
        "zcode" => ("ZCode", "cli"),
        "zed" => ("Zed", "ide"),
        "local-proxy" => ("Local Gateway", "proxy"),
        _ => ("Other AI Tool", "other"),
    }
}

pub fn sync_from_host(
    store: &Store,
    usage_db_override: Option<&Path>,
) -> Result<HostSyncSummary, String> {
    let Some(db_path) = resolve_usage_db_path(usage_db_override) else {
        return Ok(HostSyncSummary::default());
    };

    let host_conn = match Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c,
        Err(_) => return Ok(HostSyncSummary::default()),
    };

    // 1. 查询本地最后同步的事件时间戳，实现高效增量
    let last_recorded_at: Option<String> = store
        .with_read(|conn| {
            let res: Result<Option<String>, rusqlite::Error> = conn.query_row(
                "SELECT MAX(recorded_at) FROM usage_records WHERE record_hash LIKE 'evt_%'",
                [],
                |r| r.get(0),
            );
            Ok(res.unwrap_or(None))
        })
        .unwrap_or(None);

    // 2. 从 usage.db 读取新事件
    let mut stmt = match last_recorded_at {
        Some(_) => host_conn.prepare(
            "SELECT id, requested_at, provider, model, source, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens, total_tokens,
                    cost_micro, tool_id, session_id, source_record_id
             FROM usage_events
             WHERE requested_at > ?1
             ORDER BY requested_at ASC",
        ),
        None => host_conn.prepare(
            "SELECT id, requested_at, provider, model, source, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens, total_tokens,
                    cost_micro, tool_id, session_id, source_record_id
             FROM usage_events
             ORDER BY requested_at ASC",
        ),
    }
    .map_err(|e| format!("prepare host usage_events: {e}"))?;

    struct RawEvent {
        id: String,
        requested_at: String,
        _provider: String,
        model: String,
        source: String,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_write_tokens: i64,
        reasoning_tokens: i64,
        _total_tokens: i64,
        cost_micro: i64,
        tool_id: String,
        session_id: String,
        source_record_id: String,
    }

    let params: &[&dyn rusqlite::ToSql] = match last_recorded_at {
        Some(ref l) => &[l],
        None => &[],
    };

    let events: Vec<RawEvent> = stmt
        .query_map(params, |row| {
            Ok(RawEvent {
                id: row.get(0)?,
                requested_at: row.get(1)?,
                _provider: row.get(2)?,
                model: row.get(3)?,
                source: row.get(4)?,
                input_tokens: row.get(5)?,
                output_tokens: row.get(6)?,
                cache_read_tokens: row.get(7)?,
                cache_write_tokens: row.get(8)?,
                reasoning_tokens: row.get(9)?,
                _total_tokens: row.get(10)?,
                cost_micro: row.get(11)?,
                tool_id: row.get(12)?,
                session_id: row.get(13)?,
                source_record_id: row.get(14)?,
            })
        })
        .map_err(|e| format!("query host usage_events: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    drop(stmt);

    let mut summary = HostSyncSummary::default();

    // 3. 批量写入 tokenusage.db
    if !events.is_empty() {
        let now = chrono_now();
        store.with_write(|conn| {
            let mut rec_stmt = conn.prepare(
                "INSERT OR IGNORE INTO usage_records (
                    record_hash, source_id, session_id, turn_id, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    reasoning_tokens, cost_micros, recorded_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            ).map_err(|e| e.to_string())?;

            let mut src_stmt = conn.prepare(
                "INSERT INTO usage_sources (id, display_name, category, enabled, updated_at, status, last_scanned_at)
                 VALUES (?1, ?2, ?3, 1, ?4, 'ok', ?4)
                 ON CONFLICT(id) DO UPDATE SET last_scanned_at = excluded.last_scanned_at, status = 'ok'",
            ).map_err(|e| e.to_string())?;

            let mut sess_stmt = conn.prepare(
                "INSERT INTO sessions (
                    session_id, source_id, title, project_path, started_at, last_used_at,
                    total_input_tokens, total_output_tokens, total_cache_read_tokens,
                    total_cache_write_tokens, total_reasoning_tokens, total_cost_micros,
                    message_count, created_at, updated_at
                ) VALUES (?1, ?2, ?3, '', ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?11)
                ON CONFLICT(session_id) DO UPDATE SET
                    last_used_at = CASE WHEN excluded.last_used_at > last_used_at THEN excluded.last_used_at ELSE last_used_at END,
                    started_at = CASE WHEN excluded.started_at < started_at THEN excluded.started_at ELSE started_at END,
                    total_input_tokens = total_input_tokens + excluded.total_input_tokens,
                    total_output_tokens = total_output_tokens + excluded.total_output_tokens,
                    total_cache_read_tokens = total_cache_read_tokens + excluded.total_cache_read_tokens,
                    total_cache_write_tokens = total_cache_write_tokens + excluded.total_cache_write_tokens,
                    total_reasoning_tokens = total_reasoning_tokens + excluded.total_reasoning_tokens,
                    total_cost_micros = total_cost_micros + excluded.total_cost_micros,
                    message_count = message_count + 1,
                    updated_at = excluded.updated_at",
            ).map_err(|e| e.to_string())?;

            let mut agg_stmt = conn.prepare(
                "INSERT INTO daily_aggregates (
                    date, source_id, model, total_tokens, input_tokens, output_tokens,
                    cache_read_tokens, cost_micros, session_count
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
                ON CONFLICT(date, source_id, model) DO UPDATE SET
                    total_tokens = total_tokens + excluded.total_tokens,
                    input_tokens = input_tokens + excluded.input_tokens,
                    output_tokens = output_tokens + excluded.output_tokens,
                    cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
                    cost_micros = cost_micros + excluded.cost_micros,
                    session_count = session_count + 1",
            ).map_err(|e| e.to_string())?;

            for ev in &events {
                let source_id = normalize_source_id(&ev.tool_id, &ev.source);
                let (disp_name, cat) = source_meta(source_id);

                let date_str = if ev.requested_at.len() >= 10 {
                    &ev.requested_at[..10]
                } else {
                    "unknown"
                };

                let sid = if !ev.session_id.is_empty() {
                    ev.session_id.clone()
                } else {
                    format!("{}_{}", source_id, date_str)
                };

                // 计价补齐：若 cost_micro 为 0，通过本地定价引擎估算
                let cost_micros = if ev.cost_micro > 0 {
                    ev.cost_micro
                } else {
                    calculate_cost_micros(
                        &ev.model,
                        ev.input_tokens,
                        ev.output_tokens,
                        ev.cache_read_tokens,
                        ev.cache_write_tokens,
                        ev.reasoning_tokens,
                    )
                };

                let total_toks = ev.input_tokens + ev.output_tokens;
                let rec_hash = format!("evt_{}", ev.id);

                src_stmt.execute(rusqlite::params![
                    source_id,
                    disp_name,
                    cat,
                    now,
                ]).map_err(|e| e.to_string())?;

                sess_stmt.execute(rusqlite::params![
                    sid,
                    source_id,
                    format!("{} Session", disp_name),
                    ev.requested_at,
                    ev.input_tokens,
                    ev.output_tokens,
                    ev.cache_read_tokens,
                    ev.cache_write_tokens,
                    ev.reasoning_tokens,
                    cost_micros,
                    now,
                ]).map_err(|e| e.to_string())?;

                rec_stmt.execute(rusqlite::params![
                    rec_hash,
                    source_id,
                    sid,
                    ev.source_record_id,
                    ev.model,
                    ev.input_tokens,
                    ev.output_tokens,
                    ev.cache_read_tokens,
                    ev.cache_write_tokens,
                    ev.reasoning_tokens,
                    cost_micros,
                    ev.requested_at,
                ]).map_err(|e| e.to_string())?;

                agg_stmt.execute(rusqlite::params![
                    date_str,
                    source_id,
                    ev.model,
                    total_toks,
                    ev.input_tokens,
                    ev.output_tokens,
                    ev.cache_read_tokens,
                    cost_micros,
                ]).map_err(|e| e.to_string())?;

                summary.events_synced += 1;
                summary.total_tokens += total_toks;
                summary.total_cost_micros += cost_micros;
            }

            Ok(())
        })?;
    }

    // 4. 同步 usage_budgets 到 limits_cache
    if let Ok(mut budget_stmt) = host_conn.prepare(
        "SELECT id, scope, scope_key, currency, amount_micro, period, timezone
         FROM usage_budgets WHERE enabled = 1",
    ) {
        struct BudgetRow {
            _id: String,
            _scope: String,
            scope_key: String,
            _currency: String,
            amount_micro: i64,
            period: String,
            _timezone: String,
        }

        let budgets: Vec<BudgetRow> = budget_stmt
            .query_map([], |row| {
                Ok(BudgetRow {
                    _id: row.get(0)?,
                    _scope: row.get(1)?,
                    scope_key: row.get(2)?,
                    _currency: row.get(3)?,
                    amount_micro: row.get(4)?,
                    period: row.get(5)?,
                    _timezone: row.get(6)?,
                })
            })
            .map(|r| r.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();

        if !budgets.is_empty() {
            let now = chrono_now();
            let today = if now.len() >= 10 {
                &now[..10]
            } else {
                "2026-09-20"
            };
            let month_start = format!("{}-01", &today[..7]);

            store.with_write(|conn| {
                let mut limit_stmt = conn.prepare(
                    "INSERT INTO limits_cache (
                        provider_id, account_id, window_kind, label, used_percent, remaining_percent,
                        used_units, total_units, unit_type, resets_at, fetched_at, status
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'usd', ?9, ?10, 'ok')
                    ON CONFLICT(provider_id, account_id, window_kind) DO UPDATE SET
                        label = excluded.label,
                        used_percent = excluded.used_percent,
                        remaining_percent = excluded.remaining_percent,
                        used_units = excluded.used_units,
                        total_units = excluded.total_units,
                        unit_type = excluded.unit_type,
                        resets_at = excluded.resets_at,
                        fetched_at = excluded.fetched_at,
                        status = excluded.status",
                ).map_err(|e| e.to_string())?;

                for b in &budgets {
                    let prov = if !b.scope_key.is_empty() {
                        b.scope_key.as_str()
                    } else {
                        "all"
                    };

                    let used_cost_micros: i64 = if b.period == "daily" {
                        conn.query_row(
                            "SELECT COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date = ?1",
                            [today],
                            |r| r.get(0),
                        ).unwrap_or(0)
                    } else {
                        conn.query_row(
                            "SELECT COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1",
                            [&month_start],
                            |r| r.get(0),
                        ).unwrap_or(0)
                    };

                    let used_units = used_cost_micros as f64 / 1_000_000.0;
                    let total_units = b.amount_micro as f64 / 1_000_000.0;
                    let used_pct = if total_units > 0.0 {
                        (used_units / total_units * 100.0).min(100.0)
                    } else {
                        0.0
                    };
                    let rem_pct = (100.0 - used_pct).max(0.0);

                    let label = if b.period == "daily" {
                        "每日预算"
                    } else {
                        "每月预算"
                    };

                    limit_stmt.execute(rusqlite::params![
                        prov,
                        "default",
                        b.period,
                        label,
                        used_pct,
                        rem_pct,
                        used_units,
                        total_units,
                        "",
                        now,
                    ]).map_err(|e| e.to_string())?;

                    summary.budgets_synced += 1;
                }

                Ok(())
            })?;
        }
    }

    Ok(summary)
}

fn chrono_now() -> String {
    // 简化生成 ISO8601 UTC 时间戳
    let now = std::time::SystemTime::now();
    let since_epoch = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 基础年月日计算
    let days = since_epoch / 86400;
    let secs_of_day = since_epoch % 86400;
    let hours = secs_of_day / 3600;
    let minutes = (secs_of_day % 3600) / 60;
    let seconds = secs_of_day % 60;

    let (year, month, day) = days_to_ymd(days as i64);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // 简易历法换算 (公历 1970 纪元)
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mp_i = mp as i32;
    let m = mp_i + (if mp < 10 { 3 } else { -9 });
    let y = y + (if m <= 2 { 1 } else { 0 });
    (y as i32, m as u32, d)
}
