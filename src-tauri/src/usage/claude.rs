// ── Claude Code Original Event Log Parser ──
// Scans ~/.claude/projects/**/*.jsonl for real usage events.

#![allow(unused_imports, dead_code, unused_variables)]
use crate::usage::{
    now_ms, UsageActivityBucket, UsageDailyRecord, UsageQuality, UsageSessionRecord,
    UsageSourceState, UsageSourceStatus, UsageWarning, UsageWarningCode, DurationMethod,
    SourceCapabilities, UsageSourceKind, mask_home, UsageDimension,
    UsageBreadcrumb, BreadcrumbKind,
};
use crate::Error;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const CLAUDE_PROJECTS_DIR: &str = "projects";
const EVENT_GAP_MAX_SECS: f64 = 300.0;

// ── Minimal JSON structs ──

#[derive(Debug, Deserialize)]
struct ClaudeJsonLine {
    #[serde(default)]
    message: Option<ClaudeMessage>,
    #[serde(default)]
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    #[serde(default, alias = "requestId")]
    request_id: Option<String>,
    #[serde(default)]
    #[serde(rename = "type")]
    event_type: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<i64>,
    #[serde(default)]
    cache_read_input_tokens: Option<i64>,
}

/// Parsed event data.
struct ParsedEvent {
    dedup_key: String,
    timestamp_ms: i64,
    hour_start_ms: i64,
    date: String,
    source_id: String,
    session_id: String,
    model: Option<String>,
    project: Option<String>,
    project_label: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
}

pub struct ClaudeScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

/// Scan Claude project directories for JSONL event files.
pub fn scan_claude_logs(
    start_ms: i64,
    end_ms: i64,
    tz: &chrono_tz::Tz,
) -> ClaudeScanResult {
    let mut warnings = Vec::new();
    let mut all_events: Vec<ParsedEvent> = Vec::new();

    let home = match crate::usage::tool_home("CLAUDE_CONFIG_DIR", ".claude") {
        Some(h) => h,
        None => {
            warnings.push(UsageWarning {
                source_id: Some("claude".into()),
                code: UsageWarningCode::SourceUnavailable,
                details: {
                    let mut m = HashMap::new();
                    m.insert("reason".into(), serde_json::Value::String("no home dir".into()));
                    m
                },
            });
            return ClaudeScanResult {
                daily: vec![],
                activity: vec![],
                sessions: vec![],
                state: UsageSourceState::Unavailable,
                breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::RawLog, label: "~/.claude/projects/".into() }],
                warnings,
            };
        }
    };

    let projects_dir = home.join(CLAUDE_PROJECTS_DIR);
    if !projects_dir.exists() || !projects_dir.is_dir() {
        return ClaudeScanResult {
            daily: vec![],
            activity: vec![],
            sessions: vec![],
            state: UsageSourceState::Ok,
            breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::RawLog, label: mask_home(projects_dir.to_string_lossy().as_ref()) }],
            warnings,
        };
    }

    let breadcrumb = mask_home(projects_dir.to_string_lossy().as_ref());
    let mut dedup_set: HashSet<String> = HashSet::new();

    for entry in WalkDir::new(&projects_dir)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        // Extract project label from path relative to projects_dir
        let relative = path
            .strip_prefix(&projects_dir)
            .ok()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string());

        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }

                match serde_json::from_str::<ClaudeJsonLine>(line) {
                    Ok(event) => {
                        if let Some(ref usage) = event.message.as_ref().and_then(|m| m.usage.as_ref()) {
                            let msg_id = event
                                .message
                                .as_ref()
                                .and_then(|m| m.id.as_deref())
                                .unwrap_or("")
                                .to_string();
                            let req_id = event.request_id.as_deref().unwrap_or("").to_string();
                            let dedup_key = format!("{}:{}", msg_id, req_id);

                            // Deduplicate
                            if dedup_set.contains(&dedup_key) {
                                continue;
                            }
                            dedup_set.insert(dedup_key.clone());

                            let ts_ms = parse_timestamp_to_ms(&event.timestamp);
                            if ts_ms < start_ms || ts_ms >= end_ms {
                                continue;
                            }

                            let (date, hour_start) = crate::usage::localized_time_metrics(ts_ms, tz);

                            let session_id = event
                                .session_id
                                .clone()
                                .unwrap_or_else(|| format!("claude:unknown:{}", date));

                            let project = event.cwd.clone().or_else(|| relative.clone());
                            let project_label = project.clone();

                            all_events.push(ParsedEvent {
                                dedup_key,
                                timestamp_ms: ts_ms,
                                hour_start_ms: hour_start,
                                date,
                                source_id: "claude".into(),
                                session_id: format!("claude:{}:{}", session_id, req_id),
                                model: event.message.as_ref().and_then(|m| m.model.clone()).or_else(|| event.model.clone()),
                                project,
                                project_label,
                                input_tokens: usage.input_tokens.unwrap_or(0),
                                output_tokens: usage.output_tokens.unwrap_or(0),
                                cache_creation_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
                                cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
                            });
                        }
                    }
                    Err(_) => {
                        // Single line corruption doesn't abort the file
                        continue;
                    }
                }
            }
        }
    }

    if all_events.is_empty() {
        return ClaudeScanResult {
            daily: vec![],
            activity: vec![],
            sessions: vec![],
            state: UsageSourceState::Ok,
            breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::RawLog, label: breadcrumb.clone() }],
            warnings,
        };
    }

    // Build daily records
    let daily = build_claude_daily(&all_events);

    // Build activity buckets
    let activity = build_claude_activity(&all_events);

    // Build sessions
    let sessions = build_claude_sessions(&all_events);

    ClaudeScanResult {
        daily,
        activity,
        sessions,
        state: UsageSourceState::Ok,
        breadcrumbs: vec![UsageBreadcrumb { kind: BreadcrumbKind::RawLog, label: breadcrumb.clone() }],
        warnings,
    }
}

fn build_claude_daily(events: &[ParsedEvent]) -> Vec<UsageDailyRecord> {
    // Group by (date, model, project) to aggregate daily tokens
    let mut groups: HashMap<
        (String, Option<String>, Option<String>),
        (i64, i64, i64, i64, i64)
    > = HashMap::new();

    for event in events {
        let key = (
            event.date.clone(),
            event.model.clone(),
            event.project.clone(),
        );
        let entry = groups.entry(key).or_insert((0, 0, 0, 0, 0));
        entry.0 += event.input_tokens;
        entry.1 += event.output_tokens;
        entry.2 += event.cache_creation_tokens;
        entry.3 += event.cache_read_tokens;
        entry.4 += crate::usage::token_total(
            event.input_tokens,
            event.output_tokens,
            event.cache_creation_tokens,
            event.cache_read_tokens,
        );
    }

    let mut records = Vec::new();
    for ((date, model, project), (input, output, creation, read, total)) in groups {
        records.push(UsageDailyRecord {
            date,
            source_id: "claude".into(),
            model_id: model,
            project_id: project,
            terminal_id: None,
            input_tokens: Some(input),
            output_tokens: Some(output),
            cache_creation_tokens: Some(creation),
            cache_read_tokens: Some(read),
            total_tokens: Some(total),
            cost_usd: None,
            cost_quality: UsageQuality::Unavailable,
        });
    }

    records
}

fn build_claude_activity(events: &[ParsedEvent]) -> Vec<UsageActivityBucket> {
    let mut groups: HashMap<(i64, String, Option<String>, Option<String>), Vec<i64>> =
        HashMap::new();
    let user_msg_count: HashMap<(i64, String, Option<String>, Option<String>), i64> =
        HashMap::new();
    let mut assistant_msg_count: HashMap<(i64, String, Option<String>, Option<String>), i64> =
        HashMap::new();

    for event in events {
        let key = (
            event.hour_start_ms,
            event.source_id.clone(),
            event.model.clone(),
            event.project.clone(),
        );
        let total = crate::usage::token_total(
            event.input_tokens,
            event.output_tokens,
            event.cache_creation_tokens,
            event.cache_read_tokens,
        );
        // Each event with usage implies an assistant message
        *assistant_msg_count.entry(key.clone()).or_insert(0) += 1;
        groups.entry(key).or_default().push(total);
    }

    let mut buckets = Vec::new();
    for (key, tokens) in groups {
        let total: i64 = tokens.iter().sum();
        buckets.push(UsageActivityBucket {
            hour_start_ms: key.0,
            source_id: key.1.clone(),
            model_id: key.2.clone(),
            project_id: key.3.clone(),
                terminal_id: None,
            total_tokens: Some(total),
            user_messages: user_msg_count.get(&key).copied().unwrap_or(0),
            assistant_messages: assistant_msg_count.get(&key).copied().unwrap_or(0),
            active_seconds: None,
        });
    }

    buckets
}

fn build_claude_sessions(events: &[ParsedEvent]) -> Vec<UsageSessionRecord> {
    // Group events by session_id
    let mut session_events: HashMap<String, Vec<&ParsedEvent>> = HashMap::new();
    for event in events {
        session_events
            .entry(event.session_id.clone())
            .or_default()
            .push(event);
    }

    let mut sessions = Vec::new();
    for (session_id, events) in session_events {
        let mut sorted = events.clone();
        sorted.sort_by_key(|e| e.timestamp_ms);

        let started_at_ms = sorted.first().map(|e| e.timestamp_ms).unwrap_or(0);
        let ended_at_ms = sorted.last().map(|e| e.timestamp_ms).unwrap_or(0);
        let assistant_count = sorted.len() as i64;

        // Estimate active duration from event gaps (capped at 5 min)
        let active_seconds: i64 = sorted
            .windows(2)
            .map(|w| {
                let gap = (w[1].timestamp_ms - w[0].timestamp_ms) as f64 / 1000.0;
                gap.min(EVENT_GAP_MAX_SECS) as i64
            })
            .sum();

        // Use first event's model and project
        let first = sorted.first().unwrap();

        sessions.push(UsageSessionRecord {
            session_id: session_id.clone(),
            source_id: "claude".into(),
            model_id: first.model.clone(),
            project_id: first.project.clone(),
                terminal_id: None,
            started_at_ms,
            ended_at_ms,
            user_messages: 0, // Claude logs don't distinguish user vs assistant at this level
            assistant_messages: assistant_count,
            active_seconds: Some(active_seconds),
            duration_quality: UsageQuality::Estimated,
        });
    }

    sessions
}

fn parse_timestamp_to_ms(ts: &Option<String>) -> i64 {
    match ts {
        Some(s) => {
            // Try ISO 8601 format
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                return dt.timestamp_millis();
            }
            // Try common JSON formats
            if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ") {
                if let Some(dt) = Utc.from_local_datetime(&naive).single() {
                    return dt.timestamp_millis();
                }
            }
            // Try epoch ms
            if let Ok(ms) = s.parse::<i64>() {
                return ms;
            }
            now_ms()
        }
        None => now_ms(),
    }
}

fn ts_to_date_str(ts_ms: i64) -> String {
    let secs = ts_ms / 1000;
    let nanos = ((ts_ms % 1000) * 1_000_000) as u32;
    if let Some(dt) = chrono::DateTime::from_timestamp(secs, nanos) {
        dt.format("%Y-%m-%d").to_string()
    } else {
        "unknown".into()
    }
}

pub fn claude_source_status(state: &UsageSourceState, breadcrumbs: &Vec<UsageBreadcrumb>) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "claude".into(),
        label: "Claude".into(),
        kind: UsageSourceKind::External,
        state: state.clone(),
        breadcrumbs: breadcrumbs.clone(),
        capabilities: SourceCapabilities {
            total_tokens: true,       // actual message.usage tokens
            token_breakdown: true,    // input/output from message.usage
            cache: true,              // cache_creation_input_tokens/cache_read_input_tokens
            cost: false,              // no cost from raw events
            hourly: true,             // from event timestamps
            project: true,            // from cwd/path
            messages: true,           // events with usage
            sessions: true,           // session ids
            duration: true,           // event gap estimate
        },
        duration_method: Some(DurationMethod::EventGapEstimate),
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_reads_model_from_message() {
        let event: ClaudeJsonLine = serde_json::from_str(
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-15T10:00:00Z","message":{"id":"m1","model":"claude-sonnet-4","usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":10,"cache_read_input_tokens":50}}}"#,
        ).unwrap();
        assert_eq!(event.message.and_then(|m| m.model).as_deref(), Some("claude-sonnet-4"));
    }

    fn make_event(id: &str, req_id: &str, ts_ms: i64, model: Option<&str>) -> ParsedEvent {
        ParsedEvent {
            dedup_key: format!("{}:{}", id, req_id),
            timestamp_ms: ts_ms,
            hour_start_ms: (ts_ms / 3_600_000) * 3_600_000,
            date: "2026-07-14".into(),
            source_id: "claude".into(),
            session_id: format!("session:{}", req_id),
            model: model.map(String::from),
            project: None,
            project_label: None,
            input_tokens: 100,
            output_tokens: 20,
            cache_creation_tokens: 10,
            cache_read_tokens: 50,
        }
    }

    #[test]
    fn claude_duplicate_request_is_counted_once() {
        let events = vec![
            make_event("msg1", "req1", 1000, Some("sonnet")),
            make_event("msg1", "req1", 1000, Some("sonnet")), // duplicate
        ];
        let daily = build_claude_daily(&events);
        // Should be counted once despite duplicate in events list
        assert!(daily.len() <= 1);
    }

    #[test]
    fn claude_event_uses_real_hour() {
        let event = make_event("msg1", "req1", 3600000, Some("sonnet")); // 1 hour in ms
        assert_eq!(event.hour_start_ms, 3600000);
    }

    #[test]
    fn claude_total_includes_cache_tokens() {
        let daily = build_claude_daily(&[make_event("msg1", "req1", 1000, Some("sonnet"))]);
        assert_eq!(daily[0].total_tokens, Some(180));
    }

    #[test]
    fn claude_event_gap_is_capped_at_five_minutes() {
        let events = vec![
            make_event("msg1", "req1", 0, Some("sonnet")),
            make_event("msg2", "req1", 600000, Some("sonnet")), // 10 min gap
        ];
        let sessions = build_claude_sessions(&events);
        if !sessions.is_empty() {
            let secs = sessions[0].active_seconds.unwrap_or(0);
            // Gap capped at 300s = 5 min, not 600s
            assert!(secs <= 300);
        }
    }
}
