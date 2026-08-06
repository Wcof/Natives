// ── Natives usage scanner ──
// Primary: natives.db usage_stats + Daemon protocol tables (conversation/message/run).
// Fallback: legacy assistant_sessions / assistant_messages if present.
// No external CLI dependency.

#![allow(unused_imports, dead_code, unused_variables)]
use crate::db;
use crate::usage::{
    mask_home, now_ms, BreadcrumbKind, DurationMethod, SourceCapabilities, UsageActivityBucket,
    UsageBreadcrumb, UsageDailyRecord, UsageQuality, UsageSessionRecord, UsageSourceKind,
    UsageSourceState, UsageSourceStatus, UsageWarning, UsageWarningCode,
};
use chrono::{NaiveDateTime, TimeZone, Utc};
use rusqlite::params;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct NativesScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

/// Scan Natives local DBs for usage data.
pub fn scan_natives_db(start_ms: i64, end_ms: i64) -> NativesScanResult {
    let mut warnings = Vec::new();
    let mut daily: Vec<UsageDailyRecord> = Vec::new();
    let mut activity: Vec<UsageActivityBucket> = Vec::new();
    let mut sessions: Vec<UsageSessionRecord> = Vec::new();
    let mut breadcrumbs = Vec::new();
    let mut state = UsageSourceState::Ok;
    let mut any_source = false;

    // 1) usage_stats in natives.db (authoritative local aggregate written by engine/host)
    match scan_usage_stats_table(start_ms, end_ms) {
        Ok((d, crumbs)) => {
            if !d.is_empty() {
                any_source = true;
            }
            daily.extend(d);
            breadcrumbs.extend(crumbs);
        }
        Err(e) => warnings.push(warn("natives", UsageWarningCode::SourceUnavailable, e)),
    }

    // 2) Daemon protocol tables in natives.db / assistant.db (conversation/message/run)
    for (label, open) in candidate_db_openers() {
        match open() {
            Ok(conn) => {
                if table_exists(&conn, "run") || table_exists(&conn, "message") {
                    let (d, a, s, found) = scan_protocol_tables(&conn, start_ms, end_ms);
                    if found {
                        any_source = true;
                        daily.extend(d);
                        activity.extend(a);
                        sessions.extend(s);
                        breadcrumbs.push(UsageBreadcrumb {
                            kind: BreadcrumbKind::Database,
                            label: label.clone(),
                        });
                    }
                } else if table_exists(&conn, "assistant_sessions")
                    || table_exists(&conn, "assistant_messages")
                {
                    let (d, a, s, found, partial) =
                        scan_legacy_assistant_tables(&conn, start_ms, end_ms);
                    if found {
                        any_source = true;
                        daily.extend(d);
                        activity.extend(a);
                        sessions.extend(s);
                        breadcrumbs.push(UsageBreadcrumb {
                            kind: BreadcrumbKind::Database,
                            label: label.clone(),
                        });
                        if partial {
                            state = UsageSourceState::Partial;
                            warnings.push(warn(
                                "natives",
                                UsageWarningCode::NativesHistoryPartial,
                                "legacy assistant_messages missing token fields",
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                // Soft: path may simply not exist yet.
                let _ = e;
            }
        }
    }

    if breadcrumbs.is_empty() {
        breadcrumbs.push(UsageBreadcrumb {
            kind: BreadcrumbKind::Database,
            label: "~/.natives/natives.db".into(),
        });
    }

    if !any_source {
        // Empty is still OK — means no Natives runs yet, not unavailable.
        state = UsageSourceState::Ok;
    }

    // Merge same-day/source/model/project daily rows
    let daily = merge_daily(daily);

    NativesScanResult {
        daily,
        activity,
        sessions,
        state,
        breadcrumbs,
        warnings,
    }
}

#[allow(clippy::type_complexity)] // pre-existing type shape
fn candidate_db_openers() -> Vec<(
    String,
    Box<dyn Fn() -> Result<rusqlite::Connection, String>>,
)> {
    let mut out: Vec<(
        String,
        Box<dyn Fn() -> Result<rusqlite::Connection, String>>,
    )> = Vec::new();

    // Prefer main natives.db (Daemon protocol tables live here in current installs).
    out.push((
        "~/.natives/natives.db".into(),
        Box::new(|| {
            let conn = db::get_main_conn().map_err(|e| e.to_string())?;
            // r2d2 pooled connection derefs to rusqlite::Connection; open a fresh
            // file connection for scan isolation / no long pool hold.
            let path = natives_db_path().ok_or_else(|| "no natives.db path".to_string())?;
            rusqlite::Connection::open(path).map_err(|e| e.to_string())
        }),
    ));

    out.push((
        "~/.natives/assistant.db".into(),
        Box::new(|| {
            let path = assistant_db_path().ok_or_else(|| "no assistant.db path".to_string())?;
            rusqlite::Connection::open(path).map_err(|e| e.to_string())
        }),
    ));

    out
}

fn natives_db_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NATIVES_DB_PATH") {
        let pb = PathBuf::from(p);
        if !pb.as_os_str().is_empty() {
            return Some(pb);
        }
    }
    dirs::home_dir().map(|h| h.join(".natives").join("natives.db"))
}

fn assistant_db_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NATIVES_ASSISTANT_DB_PATH") {
        let pb = PathBuf::from(p);
        if !pb.as_os_str().is_empty() {
            return Some(pb);
        }
    }
    dirs::home_dir().map(|h| h.join(".natives").join("assistant.db"))
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        params![name],
        |_| Ok(()),
    )
    .is_ok()
}

fn warn(source: &str, code: UsageWarningCode, detail: impl Into<String>) -> UsageWarning {
    let mut details = HashMap::new();
    details.insert("detail".into(), serde_json::Value::String(detail.into()));
    UsageWarning {
        source_id: Some(source.into()),
        code,
        details,
    }
}

fn scan_usage_stats_table(
    start_ms: i64,
    end_ms: i64,
) -> Result<(Vec<UsageDailyRecord>, Vec<UsageBreadcrumb>), String> {
    let path = natives_db_path().ok_or_else(|| "natives.db path missing".to_string())?;
    if !path.exists() {
        return Ok((vec![], vec![]));
    }
    let conn = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
    if !table_exists(&conn, "usage_stats") {
        return Ok((vec![], vec![]));
    }
    let start_date = ms_to_date_str(start_ms);
    // end exclusive → include dates up to previous day of end in UTC date form; keep inclusive by date string compare with end-1s
    let end_date = ms_to_date_str(end_ms.saturating_sub(1));
    let mut stmt = conn
        .prepare(
            "SELECT date, source, model, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens, request_count, cost_usd
             FROM usage_stats
             WHERE date >= ?1 AND date <= ?2 AND source = 'natives'",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![start_date, end_date], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, f64>(8)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut daily = Vec::new();
    for r in rows.flatten() {
        let (date, source, model, input, output, cache_c, cache_r, _req, cost) = r;
        let total = input + output + cache_c + cache_r;
        daily.push(UsageDailyRecord {
            date,
            source_id: source,
            model_id: if model.is_empty() { None } else { Some(model) },
            project_id: None,
            terminal_id: None,
            input_tokens: Some(input),
            output_tokens: Some(output),
            cache_creation_tokens: Some(cache_c),
            cache_read_tokens: Some(cache_r),
            total_tokens: Some(total),
            cost_usd: if cost > 0.0 { Some(cost) } else { None },
            cost_quality: if cost > 0.0 {
                UsageQuality::Reported
            } else {
                UsageQuality::Unavailable
            },
        });
    }
    let crumbs = if daily.is_empty() {
        vec![]
    } else {
        vec![UsageBreadcrumb {
            kind: BreadcrumbKind::Database,
            label: "~/.natives/natives.db#usage_stats".into(),
        }]
    };
    Ok((daily, crumbs))
}

fn scan_protocol_tables(
    conn: &rusqlite::Connection,
    start_ms: i64,
    end_ms: i64,
) -> (
    Vec<UsageDailyRecord>,
    Vec<UsageActivityBucket>,
    Vec<UsageSessionRecord>,
    bool,
) {
    let start_str = ms_to_sqlite_str(start_ms);
    let end_str = ms_to_sqlite_str(end_ms);
    let mut daily: Vec<UsageDailyRecord> = Vec::new();
    let mut activity: Vec<UsageActivityBucket> = Vec::new();
    let mut sessions: Vec<UsageSessionRecord> = Vec::new();
    let mut found = false;

    // Prefer run aggregates (updated by usage_updated projection).
    if table_exists(conn, "run") {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, conversation_id, provider_id, model_id,
                    COALESCE(total_input_tokens,0), COALESCE(total_output_tokens,0),
                    COALESCE(started_at, created_at), finished_at, created_at, project_path
             FROM run
             WHERE COALESCE(started_at, created_at) >= ?1
               AND COALESCE(started_at, created_at) < ?2",
        ) {
            if let Ok(iter) = stmt.query_map(params![start_str, end_str], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            }) {
                for row in iter.flatten() {
                    let (
                        id,
                        _conv,
                        _provider,
                        model,
                        input,
                        output,
                        started,
                        finished,
                        created,
                        project,
                    ) = row;
                    let started_ms = started
                        .as_deref()
                        .map(parse_natives_timestamp)
                        .unwrap_or_else(|| parse_natives_timestamp(&created));
                    let finished_ms = finished
                        .as_deref()
                        .map(parse_natives_timestamp)
                        .unwrap_or(started_ms);
                    // Mark source found for any run in range (even zero-token runs).
                    let date = ms_to_date_str(started_ms);
                    let total = input + output;
                    if total > 0 {
                        daily.push(UsageDailyRecord {
                            date: date.clone(),
                            source_id: "natives".into(),
                            model_id: Some(model.clone()),
                            project_id: project.clone(),
                            terminal_id: None,
                            input_tokens: Some(input),
                            output_tokens: Some(output),
                            cache_creation_tokens: None,
                            cache_read_tokens: None,
                            total_tokens: Some(total),
                            cost_usd: None,
                            cost_quality: UsageQuality::Unavailable,
                        });
                        let hour = (started_ms / 3_600_000) * 3_600_000;
                        activity.push(UsageActivityBucket {
                            hour_start_ms: hour,
                            source_id: "natives".into(),
                            model_id: Some(model.clone()),
                            project_id: project.clone(),
                            terminal_id: None,
                            total_tokens: Some(total),
                            user_messages: 0,
                            assistant_messages: 1,
                            active_seconds: None,
                        });
                    }
                    sessions.push(UsageSessionRecord {
                        session_id: format!("natives-run:{id}"),
                        source_id: "natives".into(),
                        model_id: Some(model),
                        project_id: project,
                        terminal_id: None,
                        started_at_ms: started_ms,
                        ended_at_ms: finished_ms,
                        user_messages: 0,
                        assistant_messages: 1,
                        active_seconds: if finished_ms > started_ms {
                            Some(((finished_ms - started_ms) / 1000).min(86400))
                        } else {
                            None
                        },
                        duration_quality: UsageQuality::Estimated,
                    });
                    found = true;
                }
            }
        }
    }

    // Also fold message-level tokens if present (conversation history).
    if table_exists(conn, "message") {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, conversation_id, role,
                    COALESCE(input_tokens,0), COALESCE(output_tokens,0), created_at
             FROM message
             WHERE created_at >= ?1 AND created_at < ?2
               AND (COALESCE(input_tokens,0) > 0 OR COALESCE(output_tokens,0) > 0)",
        ) {
            if let Ok(iter) = stmt.query_map(params![start_str, end_str], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            }) {
                for row in iter.flatten() {
                    let (_id, _conv, role, input, output, created) = row;
                    let ts = parse_natives_timestamp(&created);
                    let date = ms_to_date_str(ts);
                    let total = input + output;
                    if total <= 0 {
                        continue;
                    }
                    found = true;
                    daily.push(UsageDailyRecord {
                        date,
                        source_id: "natives".into(),
                        model_id: None,
                        project_id: None,
                        terminal_id: None,
                        input_tokens: Some(input),
                        output_tokens: Some(output),
                        cache_creation_tokens: None,
                        cache_read_tokens: None,
                        total_tokens: Some(total),
                        cost_usd: None,
                        cost_quality: UsageQuality::Unavailable,
                    });
                    let hour = (ts / 3_600_000) * 3_600_000;
                    activity.push(UsageActivityBucket {
                        hour_start_ms: hour,
                        source_id: "natives".into(),
                        model_id: None,
                        project_id: None,
                        terminal_id: None,
                        total_tokens: Some(total),
                        user_messages: if role == "user" { 1 } else { 0 },
                        assistant_messages: if role == "assistant" { 1 } else { 0 },
                        active_seconds: None,
                    });
                }
            }
        }
    }

    (daily, activity, sessions, found)
}

fn scan_legacy_assistant_tables(
    conn: &rusqlite::Connection,
    start_ms: i64,
    end_ms: i64,
) -> (
    Vec<UsageDailyRecord>,
    Vec<UsageActivityBucket>,
    Vec<UsageSessionRecord>,
    bool,
    bool,
) {
    let start_str = ms_to_sqlite_str(start_ms);
    let end_str = ms_to_sqlite_str(end_ms);
    let mut daily = Vec::new();
    let mut activity = Vec::new();
    let mut sessions = Vec::new();
    let mut found = false;
    let mut partial = false;

    // Prefer message token columns if present.
    let has_input = column_exists(conn, "assistant_messages", "input_tokens");
    let has_token_count = column_exists(conn, "assistant_messages", "token_count");
    if table_exists(conn, "assistant_messages") && (has_input || has_token_count) {
        let sql = if has_input {
            "SELECT conversation_id, role,
                    COALESCE(input_tokens,0), COALESCE(output_tokens,0), created_at
             FROM assistant_messages
             WHERE created_at >= ?1 AND created_at < ?2"
                .to_string()
        } else {
            "SELECT conversation_id, role,
                    0, COALESCE(token_count,0), created_at
             FROM assistant_messages
             WHERE created_at >= ?1 AND created_at < ?2"
                .to_string()
        };
        if let Ok(mut stmt) = conn.prepare(&sql) {
            if let Ok(iter) = stmt.query_map(params![start_str, end_str], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            }) {
                for row in iter.flatten() {
                    let (conv, role, input, output, created) = row;
                    let total = input + output;
                    if total <= 0 {
                        continue;
                    }
                    found = true;
                    let ts = parse_natives_timestamp(&created);
                    daily.push(UsageDailyRecord {
                        date: ms_to_date_str(ts),
                        source_id: "natives".into(),
                        model_id: None,
                        project_id: conv,
                        terminal_id: None,
                        input_tokens: if has_input { Some(input) } else { None },
                        output_tokens: Some(output),
                        cache_creation_tokens: None,
                        cache_read_tokens: None,
                        total_tokens: Some(total),
                        cost_usd: None,
                        cost_quality: UsageQuality::Unavailable,
                    });
                    let hour = (ts / 3_600_000) * 3_600_000;
                    activity.push(UsageActivityBucket {
                        hour_start_ms: hour,
                        source_id: "natives".into(),
                        model_id: None,
                        project_id: None,
                        terminal_id: None,
                        total_tokens: Some(total),
                        user_messages: if role.as_deref() == Some("user") {
                            1
                        } else {
                            0
                        },
                        assistant_messages: if role.as_deref() == Some("assistant") {
                            1
                        } else {
                            0
                        },
                        active_seconds: None,
                    });
                }
            }
        }
        if !has_input {
            partial = true;
        }
    }

    if table_exists(conn, "assistant_sessions") {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, project_id, model_id, provider_id, created_at, updated_at, COALESCE(token_used,0)
             FROM assistant_sessions
             WHERE created_at >= ?1 AND created_at < ?2",
        ) {
            if let Ok(iter) = stmt.query_map(params![start_str, end_str], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            }) {
                for row in iter.flatten() {
                    let (id, project, model, provider, created, updated, token_used) = row;
                    found = true;
                    let started = parse_natives_timestamp(&created);
                    let ended = updated
                        .as_deref()
                        .map(parse_natives_timestamp)
                        .unwrap_or(started);
                    sessions.push(UsageSessionRecord {
                        session_id: format!("natives:{id}"),
                        source_id: "natives".into(),
                        model_id: model.or(provider),
                        project_id: project,
                        terminal_id: None,
                        started_at_ms: started,
                        ended_at_ms: ended,
                        user_messages: 0,
                        assistant_messages: 0,
                        active_seconds: if ended > started {
                            Some(((ended - started) / 1000).min(86400))
                        } else {
                            None
                        },
                        duration_quality: UsageQuality::Estimated,
                    });
                    if token_used > 0 {
                        daily.push(UsageDailyRecord {
                            date: ms_to_date_str(started),
                            source_id: "natives".into(),
                            model_id: None,
                            project_id: None,
                            terminal_id: None,
                            input_tokens: None,
                            output_tokens: None,
                            cache_creation_tokens: None,
                            cache_read_tokens: None,
                            total_tokens: Some(token_used),
                            cost_usd: None,
                            cost_quality: UsageQuality::Unavailable,
                        });
                    }
                }
            }
        }
    }

    (daily, activity, sessions, found, partial)
}

fn column_exists(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return false;
    };
    let Ok(iter) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    let names: Vec<String> = iter.flatten().collect();
    names.iter().any(|c| c == column)
}

fn merge_daily(rows: Vec<UsageDailyRecord>) -> Vec<UsageDailyRecord> {
    let mut map: HashMap<(String, String, Option<String>, Option<String>), UsageDailyRecord> =
        HashMap::new();
    for r in rows {
        let key = (
            r.date.clone(),
            r.source_id.clone(),
            r.model_id.clone(),
            r.project_id.clone(),
        );
        map.entry(key)
            .and_modify(|acc| {
                acc.input_tokens =
                    Some(acc.input_tokens.unwrap_or(0) + r.input_tokens.unwrap_or(0));
                acc.output_tokens =
                    Some(acc.output_tokens.unwrap_or(0) + r.output_tokens.unwrap_or(0));
                acc.cache_creation_tokens = Some(
                    acc.cache_creation_tokens.unwrap_or(0) + r.cache_creation_tokens.unwrap_or(0),
                );
                acc.cache_read_tokens =
                    Some(acc.cache_read_tokens.unwrap_or(0) + r.cache_read_tokens.unwrap_or(0));
                acc.total_tokens =
                    Some(acc.total_tokens.unwrap_or(0) + r.total_tokens.unwrap_or(0));
                if acc.cost_usd.is_none() {
                    acc.cost_usd = r.cost_usd;
                    acc.cost_quality = r.cost_quality.clone();
                }
            })
            .or_insert(r);
    }
    map.into_values().collect()
}

fn parse_natives_timestamp(ts: &str) -> i64 {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return dt.timestamp_millis();
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f") {
        return naive.and_utc().timestamp_millis();
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
        return naive.and_utc().timestamp_millis();
    }
    if let Ok(ms) = ts.parse::<i64>() {
        return ms;
    }
    0
}

fn ms_to_sqlite_str(ms: i64) -> String {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    if let Some(dt) = Utc.timestamp_opt(secs, nanos).single() {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        "1970-01-01 00:00:00".into()
    }
}

fn ms_to_date_str(ms: i64) -> String {
    let secs = ms / 1000;
    if let Some(dt) = Utc.timestamp_opt(secs, 0).single() {
        dt.format("%Y-%m-%d").to_string()
    } else {
        "unknown".into()
    }
}

pub fn natives_source_status(state: &UsageSourceState) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "natives".into(),
        label: "Natives".into(),
        kind: UsageSourceKind::Natives,
        state: state.clone(),
        breadcrumbs: vec![UsageBreadcrumb {
            kind: BreadcrumbKind::Database,
            label: "~/.natives/*.db".into(),
        }],
        capabilities: SourceCapabilities {
            total_tokens: true,
            token_breakdown: true,
            cache: false,
            cost: false,
            hourly: true,
            project: true,
            messages: true,
            sessions: true,
            duration: true,
        },
        duration_method: Some(DurationMethod::SessionBounds),
    }
}
