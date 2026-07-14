// ── Natives (assistant.db) Data Source ──
// Reads from ~/.natives/assistant.db for real session, message, project, model data.

use crate::usage::{
    now_ms, UsageActivityBucket, UsageDailyRecord, UsageQuality, UsageSessionRecord,
    UsageSourceState, UsageSourceStatus, UsageWarning, UsageWarningCode, DurationMethod,
    SourceCapabilities, UsageSourceKind, mask_home, UsageDimension,
    UsageBreadcrumb, BreadcrumbKind,
};
use crate::db;
use chrono::{NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Statement};
use std::collections::HashMap;

pub struct NativesScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

/// Scan Natives assistant.db for usage data.
pub fn scan_natives_db(start_ms: i64, end_ms: i64) -> NativesScanResult {
    let mut warnings = Vec::new();
    let mut state = UsageSourceState::Ok;

    let conn = match db::get_assistant_db_conn() {
        Ok(c) => c,
        Err(e) => {
            warnings.push(UsageWarning {
                source_id: Some("natives".into()),
                code: UsageWarningCode::SourceUnavailable,
                details: {
                    let mut m = HashMap::new();
                    m.insert("db_error".into(), serde_json::Value::String(e.to_string()));
                    m
                },
            });
            return NativesScanResult {
                daily: vec![],
                activity: vec![],
                sessions: vec![],
                state: UsageSourceState::Unavailable,
                breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::Database, label: "~/.natives/assistant.db".into() }],
                warnings,
            };
        }
    };

    let start_str = ms_to_sqlite_str(start_ms);
    let end_str = ms_to_sqlite_str(end_ms);

    // Fetch sessions within range
    let mut stmt = match conn.prepare(
        "SELECT id, project_id, model_id, provider_id, created_at, updated_at
         FROM assistant_sessions
         WHERE created_at >= ?1 AND created_at < ?2"
    ) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(UsageWarning {
                source_id: Some("natives".into()),
                code: UsageWarningCode::SourceUnavailable,
                details: {
                    let mut m = HashMap::new();
                    m.insert("error".into(), serde_json::Value::String(e.to_string()));
                    m
                },
            });
            return NativesScanResult {
                daily: vec![],
                activity: vec![],
                sessions: vec![],
                state: UsageSourceState::Unavailable,
                breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::Database, label: "~/.natives/assistant.db".into() }],
                warnings,
            };
        }
    };

    // Structure to track sessions, messages, and activities
    let mut sessions_map: HashMap<String, SessionData> = HashMap::new();
    let mut missing_token_warning = false;

    let sessions_iter = stmt.query_map(params![start_str, end_str], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    });

    let mut session_ids: Vec<String> = Vec::new();

    if let Ok(iter) = sessions_iter {
        for session_result in iter {
            if let Ok((id, project_id, model_id, provider_id, created_at, updated_at)) = session_result {
                session_ids.push(id.clone());

                let created_ts = parse_natives_timestamp(&created_at);
                let updated_ts = updated_at
                    .as_ref()
                    .and_then(|u| Some(parse_natives_timestamp(u)))
                    .unwrap_or(created_ts);

                sessions_map.insert(id.clone(), SessionData {
                    id: id.clone(),
                    project_id,
                    model_id: model_id.or(provider_id),
                    created_at_ms: created_ts,
                    updated_at_ms: updated_ts,
                    user_messages: 0,
                    assistant_messages: 0,
                    total_tokens: None,
                    message_hours: Vec::new(),
                });
            }
        }
    }

    // Track token availability — Natives history doesn't prove input/output split
    let mut has_real_tokens = false;

    // Fetch messages for each session
    if !session_ids.is_empty() {
        // Use batch query for messages using IN clause
        let placeholders: Vec<String> = session_ids.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT session_id, role, token_count, created_at
             FROM assistant_messages
             WHERE session_id IN ({})",
            placeholders.join(",")
        );

        if let Ok(mut msg_stmt) = conn.prepare(&sql) {
            let params: Vec<&dyn rusqlite::types::ToSql> = session_ids
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();

            if let Ok(msg_iter) = msg_stmt.query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            }) {
                for msg_result in msg_iter {
                    if let Ok((session_id, role, token_count, created_at)) = msg_result {
                        if let Some(session) = sessions_map.get_mut(&session_id) {
                            match role.as_deref() {
                                Some("user") | Some("human") => session.user_messages += 1,
                                Some("assistant") | Some("ai") => session.assistant_messages += 1,
                                _ => {
                                    session.user_messages += 1;
                                }
                            }

                            if let Some(tc) = token_count {
                                if tc > 0 {
                                    let current = session.total_tokens.unwrap_or(0);
                                    session.total_tokens = Some(current + tc);
                                    has_real_tokens = true;
                                }
                            }

                            if let Some(ref ts) = created_at {
                                let msg_ts = parse_natives_timestamp(ts);
                                if msg_ts > 0 {
                                    let hour_start = (msg_ts / 3_600_000) * 3_600_000;
                                    session.message_hours.push((hour_start, token_count.unwrap_or(0)));
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // No messages table or columns - return what we have
            warnings.push(UsageWarning {
                source_id: Some("natives".into()),
                code: UsageWarningCode::NativesHistoryPartial,
                details: {
                    let mut m = HashMap::new();
                    m.insert("reason".into(), serde_json::Value::String("no messages table found".into()));
                    m
                },
            });
            missing_token_warning = true;
        }

        if !has_real_tokens && !sessions_map.is_empty() {
            warnings.push(UsageWarning {
                source_id: Some("natives".into()),
                code: UsageWarningCode::NativesHistoryPartial,
                details: {
                    let mut m = HashMap::new();
                    m.insert("reason".into(), serde_json::Value::String(
                        "assistant_messages.token_count is 0 or null; not treated as real zeros".into()
                    ));
                    m
                },
            });
            state = UsageSourceState::Partial;
        }
    }

    build_natives_result(sessions_map, has_real_tokens, state, warnings, missing_token_warning)
}

struct SessionData {
    id: String,
    project_id: Option<String>,
    model_id: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    user_messages: i64,
    assistant_messages: i64,
    total_tokens: Option<i64>,
    message_hours: Vec<(i64, i64)>,
}

fn build_natives_result(
    sessions_map: HashMap<String, SessionData>,
    has_real_tokens: bool,
    state: UsageSourceState,
    warnings: Vec<UsageWarning>,
    _missing_token_warning: bool,
) -> NativesScanResult {
    let mut daily_map: HashMap<(String, String, Option<String>, Option<String>), i64> = HashMap::new();
    let mut activity_map: HashMap<(i64, String, Option<String>, Option<String>), (i64, i64)> = HashMap::new();
    let mut sessions = Vec::new();

    for (_id, session) in sessions_map {
        // Session record (with total tokens but no input/output/cost)
        let session_duration = session.updated_at_ms - session.created_at_ms;
        let span_seconds = if session_duration > 0 {
            Some((session_duration / 1000).min(86400)) // cap at 24h per session
        } else {
            None
        };

        sessions.push(UsageSessionRecord {
            session_id: format!("natives:{}", session.id),
            source_id: "natives".into(),
            model_id: session.model_id.clone(),
            project_id: session.project_id.clone(),
                terminal_id: None,
            started_at_ms: session.created_at_ms,
            ended_at_ms: session.updated_at_ms,
            user_messages: session.user_messages,
            assistant_messages: session.assistant_messages,
            active_seconds: span_seconds,
            duration_quality: UsageQuality::Estimated,
        });

        // Daily record (date-scoped)
        let date = ms_to_date_str(session.created_at_ms);
        let key = (
            date,
            "natives".to_string(),
            session.model_id.clone(),
            session.project_id.clone(),
        );
        let entry = daily_map.entry(key).or_insert(0);
        *entry += session.total_tokens.unwrap_or(0);

        // Activity buckets from message timestamps
        for (hour_start, tokens) in &session.message_hours {
            let key = (
                *hour_start,
                "natives".to_string(),
                session.model_id.clone(),
                session.project_id.clone(),
            );
            let entry = activity_map.entry(key).or_insert((0, 0));
            entry.0 += 1; // message count
            entry.1 += tokens;
        }
    }

    // Build daily records
    let daily: Vec<UsageDailyRecord> = daily_map
        .into_iter()
        .map(|((date, source_id, model_id, project_id), total_tokens)| {
            UsageDailyRecord {
                date,
                source_id,
                model_id,
                project_id,
                terminal_id: None,
                input_tokens: None,
                output_tokens: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
                total_tokens: if has_real_tokens { Some(total_tokens) } else { None },
                cost_usd: None,
                cost_quality: UsageQuality::Unavailable,
            }
        })
        .collect();

    // Build activity buckets
    let activity: Vec<UsageActivityBucket> = activity_map
        .into_iter()
        .map(|((hour_start, source_id, model_id, project_id), (msg_count, tokens))| {
            UsageActivityBucket {
                hour_start_ms: hour_start,
                source_id,
                model_id,
                project_id,
                terminal_id: None,
                total_tokens: if has_real_tokens { Some(tokens) } else { None },
                user_messages: msg_count,
                assistant_messages: 0, // We don't track role per hour bucket
                active_seconds: None,
            }
        })
        .collect();

    NativesScanResult {
        daily,
        activity,
        sessions,
        state,
        breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::Database, label: "~/.natives/assistant.db".into() }],
        warnings,
    }
}

fn parse_natives_timestamp(ts: &str) -> i64 {
    // Try ISO 8601
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return dt.timestamp_millis();
    }
    // Try naive datetime
    if let Ok(naive) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f") {
        if let Some(dt) = Utc.from_local_datetime(&naive).single() {
            return dt.timestamp_millis();
        }
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
        if let Some(dt) = Utc.from_local_datetime(&naive).single() {
            return dt.timestamp_millis();
        }
    }
    // Try epoch ms
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
        breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::Database, label: "~/.natives/assistant.db".into() }],
        capabilities: SourceCapabilities {
            total_tokens: true,
            token_breakdown: false,
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
