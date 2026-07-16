use crate::usage::{
    localized_time_metrics, mask_home, BreadcrumbKind, DurationMethod, SourceCapabilities,
    UsageActivityBucket, UsageBreadcrumb, UsageDailyRecord, UsageQuality, UsageSessionRecord,
    UsageSourceKind, UsageSourceState, UsageSourceStatus, UsageWarning, UsageWarningCode,
};
use rusqlite::OpenFlags;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct OpenCodeScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

struct ParsedMessage {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    total_tokens: i64,
    model: Option<String>,
    timestamp_ms: i64,
    cost_usd: Option<f64>,
}

struct Event {
    session_id: String,
    project: String,
    usage: ParsedMessage,
}

fn parse_message(value: &serde_json::Value) -> Option<ParsedMessage> {
    if value.get("role").and_then(|v| v.as_str()) != Some("assistant")
        || value.pointer("/time/completed").is_none()
    {
        return None;
    }
    let tokens = value.get("tokens")?;
    let input_tokens = tokens
        .get("input")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0);
    let output_tokens = tokens
        .get("output")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0)
        + tokens
            .get("reasoning")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .max(0);
    let cache_read_tokens = tokens
        .pointer("/cache/read")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0);
    let cache_creation_tokens = tokens
        .pointer("/cache/write")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0);
    if input_tokens + output_tokens + cache_read_tokens + cache_creation_tokens == 0 {
        return None;
    }
    Some(ParsedMessage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        total_tokens: crate::usage::token_total(
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        ),
        model: value
            .get("modelID")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        timestamp_ms: value
            .pointer("/time/created")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        cost_usd: value
            .get("cost")
            .and_then(|v| v.as_f64())
            .filter(|cost| *cost > 0.0),
    })
}

pub fn opencode_db_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(path).join("opencode/opencode.db");
        if path.is_file() {
            return Some(path);
        }
    }
    let path = dirs::home_dir()?.join(".local/share/opencode/opencode.db");
    path.is_file().then_some(path)
}

pub fn scan_opencode_db(
    path: &Path,
    start_ms: i64,
    end_ms: i64,
    tz: &chrono_tz::Tz,
) -> OpenCodeScanResult {
    let breadcrumb = UsageBreadcrumb {
        kind: BreadcrumbKind::Database,
        label: mask_home(path.to_string_lossy().as_ref()),
    };
    let mut result = OpenCodeScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state: UsageSourceState::Ok,
        breadcrumbs: vec![breadcrumb],
        warnings: vec![],
    };
    let conn = match rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => conn,
        Err(error) => {
            result.state = UsageSourceState::Unavailable;
            result.warnings.push(source_warning(error.to_string()));
            return result;
        }
    };
    let mut stmt = match conn.prepare(
        "SELECT m.session_id, s.directory, m.data
         FROM message m JOIN session s ON s.id = m.session_id",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            result.state = UsageSourceState::Unavailable;
            result.warnings.push(source_warning(error.to_string()));
            return result;
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(error) => {
            result.state = UsageSourceState::Unavailable;
            result.warnings.push(source_warning(error.to_string()));
            return result;
        }
    };
    let mut events = Vec::new();
    for row in rows {
        let Ok((session_id, project, data)) = row else {
            result.state = UsageSourceState::Partial;
            continue;
        };
        let Some(usage) = serde_json::from_str(&data)
            .ok()
            .and_then(|value| parse_message(&value))
        else {
            continue;
        };
        if usage.timestamp_ms >= start_ms && usage.timestamp_ms < end_ms {
            events.push(Event {
                session_id,
                project,
                usage,
            });
        }
    }
    fill_records(&events, tz, &mut result);
    result
}

fn fill_records(events: &[Event], tz: &chrono_tz::Tz, result: &mut OpenCodeScanResult) {
    let mut daily =
        HashMap::<(String, Option<String>, String), (i64, i64, i64, i64, i64, f64, bool)>::new();
    let mut activity = HashMap::<(i64, Option<String>, String), (i64, i64)>::new();
    let mut sessions = HashMap::<&str, Vec<&Event>>::new();
    for event in events {
        let (date, hour) = localized_time_metrics(event.usage.timestamp_ms, tz);
        let key = (date, event.usage.model.clone(), event.project.clone());
        let entry = daily.entry(key).or_insert((0, 0, 0, 0, 0, 0.0, false));
        entry.0 += event.usage.input_tokens;
        entry.1 += event.usage.output_tokens;
        entry.2 += event.usage.cache_read_tokens;
        entry.3 += event.usage.cache_creation_tokens;
        entry.4 += event.usage.total_tokens;
        if let Some(cost) = event.usage.cost_usd {
            entry.5 += cost;
            entry.6 = true;
        }
        let activity_entry = activity
            .entry((hour, event.usage.model.clone(), event.project.clone()))
            .or_insert((0, 0));
        activity_entry.0 += event.usage.total_tokens;
        activity_entry.1 += 1;
        sessions.entry(&event.session_id).or_default().push(event);
    }
    result.daily = daily
        .into_iter()
        .map(|((date, model_id, project_id), value)| UsageDailyRecord {
            date,
            source_id: "opencode".into(),
            model_id,
            project_id: Some(project_id),
            terminal_id: None,
            input_tokens: Some(value.0),
            output_tokens: Some(value.1),
            cache_read_tokens: Some(value.2),
            cache_creation_tokens: Some(value.3),
            total_tokens: Some(value.4),
            cost_usd: value.6.then_some(value.5),
            cost_quality: if value.6 {
                UsageQuality::Reported
            } else {
                UsageQuality::Unavailable
            },
        })
        .collect();
    result.activity = activity
        .into_iter()
        .map(
            |((hour_start_ms, model_id, project_id), (tokens, count))| UsageActivityBucket {
                hour_start_ms,
                source_id: "opencode".into(),
                model_id,
                project_id: Some(project_id),
                terminal_id: None,
                total_tokens: Some(tokens),
                user_messages: count,
                assistant_messages: count,
                active_seconds: None,
            },
        )
        .collect();
    result.sessions = sessions
        .into_iter()
        .map(|(session_id, events)| {
            let started = events
                .iter()
                .map(|event| event.usage.timestamp_ms)
                .min()
                .unwrap_or(0);
            let ended = events
                .iter()
                .map(|event| event.usage.timestamp_ms)
                .max()
                .unwrap_or(0);
            let first = events[0];
            UsageSessionRecord {
                session_id: format!("opencode:{session_id}"),
                source_id: "opencode".into(),
                model_id: first.usage.model.clone(),
                project_id: Some(first.project.clone()),
                terminal_id: None,
                started_at_ms: started,
                ended_at_ms: ended,
                user_messages: events.len() as i64,
                assistant_messages: events.len() as i64,
                active_seconds: None,
                duration_quality: UsageQuality::Unavailable,
            }
        })
        .collect();
}

fn source_warning(reason: String) -> UsageWarning {
    UsageWarning {
        source_id: Some("opencode".into()),
        code: UsageWarningCode::SourceUnavailable,
        details: HashMap::from([("reason".into(), serde_json::Value::String(reason))]),
    }
}

pub fn opencode_source_status(result: &OpenCodeScanResult) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "opencode".into(),
        label: "OpenCode".into(),
        kind: UsageSourceKind::External,
        state: result.state.clone(),
        breadcrumbs: result.breadcrumbs.clone(),
        capabilities: SourceCapabilities {
            total_tokens: true,
            token_breakdown: true,
            cache: true,
            cost: true,
            hourly: true,
            project: true,
            messages: true,
            sessions: true,
            duration: false,
        },
        duration_method: Some(DurationMethod::EventGapEstimate),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_message, scan_opencode_db};

    #[test]
    fn parses_completed_assistant_usage() {
        let value = serde_json::json!({
            "role": "assistant",
            "modelID": "gpt-5",
            "cost": 0.25,
            "time": { "created": 1784020263000_i64, "completed": 1784020264000_i64 },
            "tokens": {
                "input": 10,
                "output": 3,
                "reasoning": 2,
                "cache": { "read": 7, "write": 3 }
            }
        });
        let message = parse_message(&value).unwrap();

        assert_eq!(message.input_tokens, 10);
        assert_eq!(message.output_tokens, 5);
        assert_eq!(message.cache_read_tokens, 7);
        assert_eq!(message.cache_creation_tokens, 3);
        assert_eq!(message.total_tokens, 25);
        assert_eq!(message.model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn scans_completed_messages_from_database() {
        let path = std::env::temp_dir().join(format!("natives-opencode-{}.db", std::process::id()));
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT NOT NULL);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute("INSERT INTO session VALUES ('s1', '/work/open')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3)",
            rusqlite::params![
                "m1",
                "s1",
                serde_json::json!({
                    "role":"assistant","modelID":"gpt-5","time":{"created":1784020263000_i64,"completed":1784020264000_i64},
                    "tokens":{"input":10,"output":3,"reasoning":2,"cache":{"read":7,"write":3}}
                }).to_string()
            ],
        )
        .unwrap();
        drop(conn);

        let result = scan_opencode_db(&path, 1_784_020_000_000, 1_784_030_000_000, &chrono_tz::UTC);
        std::fs::remove_file(path).unwrap();

        assert_eq!(result.daily[0].total_tokens, Some(25));
        assert_eq!(result.daily[0].project_id.as_deref(), Some("/work/open"));
        assert_eq!(result.sessions.len(), 1);
    }
}
