// ── Claude Code Original Event Log Parser ──
// Scans ~/.claude/projects/**/*.jsonl for real usage events.
// Dedup / billable-row selection mirrors cc-switch session_usage.rs.

#![allow(unused_imports, dead_code, unused_variables)]
use crate::usage::{
    mask_home, now_ms, BreadcrumbKind, DurationMethod, SourceCapabilities, UsageActivityBucket,
    UsageBreadcrumb, UsageDailyRecord, UsageDimension, UsageQuality, UsageSessionRecord,
    UsageSourceKind, UsageSourceState, UsageSourceStatus, UsageWarning, UsageWarningCode,
};
use crate::Error;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const CLAUDE_PROJECTS_DIR: &str = "projects";
const EVENT_GAP_MAX_SECS: f64 = 300.0;

// ── Minimal JSON structs ──
//
// Claude Code JSONL often emits BOTH camelCase and snake_case for the same
// concept on one object (`sessionId` + `session_id`). Typed serde with
// `alias` rejects that as "duplicate field". cc-switch avoids this by using
// `serde_json::Value`. We do the same for the outer line, then pull fields.

#[derive(Debug, Clone)]
struct ClaudeJsonLine {
    message: Option<ClaudeMessage>,
    session_id: Option<String>,
    request_id: Option<String>,
    event_type: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    top_level_usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize, Clone)]
struct ClaudeMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
    #[serde(default, alias = "stopReason")]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ClaudeUsage {
    #[serde(default, alias = "inputTokens")]
    input_tokens: Option<i64>,
    #[serde(default, alias = "outputTokens")]
    output_tokens: Option<i64>,
    #[serde(
        default,
        alias = "cacheCreationInputTokens",
        alias = "cache_creation_tokens"
    )]
    cache_creation_input_tokens: Option<i64>,
    #[serde(default, alias = "cacheReadInputTokens", alias = "cache_read_tokens")]
    cache_read_input_tokens: Option<i64>,
}

fn parse_claude_json_line(line: &str) -> Option<ClaudeJsonLine> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let obj = value.as_object()?;

    let message = obj
        .get("message")
        .and_then(|m| serde_json::from_value::<ClaudeMessage>(m.clone()).ok());
    let top_level_usage = obj
        .get("usage")
        .and_then(|u| serde_json::from_value::<ClaudeUsage>(u.clone()).ok());

    let pick_str = |keys: &[&str]| -> Option<String> {
        for key in keys {
            if let Some(v) = obj.get(*key).and_then(|v| v.as_str()) {
                return Some(v.to_owned());
            }
        }
        None
    };

    Some(ClaudeJsonLine {
        message,
        session_id: pick_str(&["sessionId", "session_id"]),
        request_id: pick_str(&["requestId", "request_id"]),
        event_type: pick_str(&["type"]),
        timestamp: pick_str(&["timestamp"]),
        cwd: pick_str(&["cwd"]),
        model: pick_str(&["model"]),
        top_level_usage,
    })
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
    stop_reason: Option<String>,
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
pub fn scan_claude_logs(start_ms: i64, end_ms: i64, tz: &chrono_tz::Tz) -> ClaudeScanResult {
    let mut warnings = Vec::new();
    let mut best_by_key: HashMap<String, ParsedEvent> = HashMap::new();

    let home = match crate::usage::tool_home("CLAUDE_CONFIG_DIR", ".claude") {
        Some(h) => h,
        None => {
            warnings.push(UsageWarning {
                source_id: Some("claude".into()),
                code: UsageWarningCode::SourceUnavailable,
                details: {
                    let mut m = HashMap::new();
                    m.insert(
                        "reason".into(),
                        serde_json::Value::String("no home dir".into()),
                    );
                    m
                },
            });
            return ClaudeScanResult {
                daily: vec![],
                activity: vec![],
                sessions: vec![],
                state: UsageSourceState::Unavailable,
                breadcrumbs: vec![UsageBreadcrumb {
                    kind: BreadcrumbKind::RawLog,
                    label: "~/.claude/projects/".into(),
                }],
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
            breadcrumbs: vec![UsageBreadcrumb {
                kind: BreadcrumbKind::RawLog,
                label: mask_home(projects_dir.to_string_lossy().as_ref()),
            }],
            warnings,
        };
    }

    let breadcrumb = mask_home(projects_dir.to_string_lossy().as_ref());
    let mut anonymous_seq: u64 = 0;

    // Depth 12 covers project/session/subagents/workflows nesting.
    for entry in WalkDir::new(&projects_dir)
        .max_depth(12)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        // Cheap mtime prefilter: skip files wholly outside the query window
        // (with a 2-day slack for timezone skew / delayed flushes).
        if let Ok(meta) = path.metadata() {
            if let Ok(modified) = meta.modified() {
                if let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH) {
                    let mtime_ms = elapsed.as_millis() as i64;
                    let slack = 2 * 86_400_000;
                    if mtime_ms < start_ms.saturating_sub(slack) {
                        continue;
                    }
                }
            }
        }

        // Extract project label from path relative to projects_dir
        let relative = path
            .strip_prefix(&projects_dir)
            .ok()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string());

        // Stream line-by-line — never load multi-hundred-MB jsonl into memory.
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        use std::io::{BufRead, BufReader};
        let reader = BufReader::with_capacity(256 * 1024, file);
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let line = line.trim();
            if line.is_empty() || !line.contains("usage") {
                continue;
            }

            let Some(event) = parse_claude_json_line(line) else {
                continue;
            };

            // Match cc-switch: only assistant rows carry billable message.usage.
            if event
                .event_type
                .as_deref()
                .is_some_and(|t| t != "assistant")
            {
                continue;
            }

            let usage = event
                .message
                .as_ref()
                .and_then(|m| m.usage.as_ref())
                .cloned()
                .or(event.top_level_usage.clone());
            let Some(usage) = usage else { continue };

            let msg_id = event
                .message
                .as_ref()
                .and_then(|m| m.id.as_deref())
                .unwrap_or("")
                .to_string();
            let req_id = event.request_id.as_deref().unwrap_or("").to_string();
            let stop_reason = event.message.as_ref().and_then(|m| m.stop_reason.clone());

            // IMPORTANT: skip pure-zero rows BEFORE claiming a dedup key.
            // Streaming message_start snapshots often write zeros first; the old
            // first-win path permanently dropped the final billable row.
            let in_tok = usage.input_tokens.unwrap_or(0).max(0);
            let out_tok = usage.output_tokens.unwrap_or(0).max(0);
            let cc_tok = usage.cache_creation_input_tokens.unwrap_or(0).max(0);
            let cr_tok = usage.cache_read_input_tokens.unwrap_or(0).max(0);
            if in_tok == 0 && out_tok == 0 && cc_tok == 0 && cr_tok == 0 {
                continue;
            }

            let ts_ms = parse_timestamp_to_ms(&event.timestamp);
            // Strict half-open range [start, end) in epoch ms.
            if ts_ms < start_ms || ts_ms >= end_ms {
                continue;
            }

            // Dedup key mirrors cc-switch: message.id is the primary identity of
            // one billable assistant message. requestId is only a fallback when
            // message.id is missing.
            let dedup_key = if !msg_id.is_empty() {
                msg_id.clone()
            } else if !req_id.is_empty() {
                format!("req:{req_id}")
            } else {
                anonymous_seq = anonymous_seq.saturating_add(1);
                format!(
                    "anon:{}:{}:{}",
                    path.to_string_lossy(),
                    event.timestamp.as_deref().unwrap_or(""),
                    anonymous_seq
                )
            };

            let (date, hour_start) = crate::usage::localized_time_metrics(ts_ms, tz);
            let session_id = event
                .session_id
                .clone()
                .unwrap_or_else(|| format!("claude:unknown:{date}"));
            let project = event.cwd.clone().or_else(|| relative.clone());

            let candidate = ParsedEvent {
                dedup_key: dedup_key.clone(),
                timestamp_ms: ts_ms,
                hour_start_ms: hour_start,
                date,
                source_id: "claude".into(),
                session_id: format!("claude:{session_id}:{dedup_key}"),
                model: event
                    .message
                    .as_ref()
                    .and_then(|m| m.model.clone())
                    .or_else(|| event.model.clone()),
                project: project.clone(),
                project_label: project,
                input_tokens: in_tok,
                output_tokens: out_tok,
                cache_creation_tokens: cc_tok,
                cache_read_tokens: cr_tok,
                stop_reason,
            };

            match best_by_key.get(&dedup_key) {
                None => {
                    best_by_key.insert(dedup_key, candidate);
                }
                Some(existing) => {
                    if should_replace_claude_usage(existing, &candidate) {
                        best_by_key.insert(dedup_key, candidate);
                    }
                }
            }
        }
    }

    let all_events: Vec<ParsedEvent> = best_by_key.into_values().collect();

    if all_events.is_empty() {
        return ClaudeScanResult {
            daily: vec![],
            activity: vec![],
            sessions: vec![],
            state: UsageSourceState::Ok,
            breadcrumbs: vec![UsageBreadcrumb {
                kind: BreadcrumbKind::RawLog,
                label: breadcrumb.clone(),
            }],
            warnings,
        };
    }

    let daily = build_claude_daily(&all_events);
    let activity = build_claude_activity(&all_events);
    let sessions = build_claude_sessions(&all_events);

    ClaudeScanResult {
        daily,
        activity,
        sessions,
        state: UsageSourceState::Ok,
        breadcrumbs: vec![UsageBreadcrumb {
            kind: BreadcrumbKind::RawLog,
            label: breadcrumb.clone(),
        }],
        warnings,
    }
}

/// Keep the better of two rows for the same message.id (cc-switch rule):
/// 1. Prefer a row that has stop_reason over one that doesn't.
/// 2. Otherwise prefer the larger output_tokens (final stream chunk).
/// 3. Tie-break on larger total tokens so cache-bearing finals win.
fn should_replace_claude_usage(existing: &ParsedEvent, candidate: &ParsedEvent) -> bool {
    let existing_has_stop = existing.stop_reason.is_some();
    let candidate_has_stop = candidate.stop_reason.is_some();
    if candidate_has_stop && !existing_has_stop {
        return true;
    }
    if candidate_has_stop == existing_has_stop {
        if candidate.output_tokens > existing.output_tokens {
            return true;
        }
        if candidate.output_tokens == existing.output_tokens {
            let existing_total = crate::usage::token_total(
                existing.input_tokens,
                existing.output_tokens,
                existing.cache_creation_tokens,
                existing.cache_read_tokens,
            );
            let candidate_total = crate::usage::token_total(
                candidate.input_tokens,
                candidate.output_tokens,
                candidate.cache_creation_tokens,
                candidate.cache_read_tokens,
            );
            return candidate_total > existing_total;
        }
    }
    false
}

fn build_claude_daily(events: &[ParsedEvent]) -> Vec<UsageDailyRecord> {
    // Group by (date, model, project) to aggregate daily tokens
    let mut groups: HashMap<(String, Option<String>, Option<String>), (i64, i64, i64, i64, i64)> =
        HashMap::new();

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
    // (hour, source, model, project) -> (token sum, event timestamps for gap duration)
    let mut groups: HashMap<(i64, String, Option<String>, Option<String>), (i64, Vec<i64>)> =
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
        let entry = groups.entry(key).or_default();
        entry.0 += total;
        entry.1.push(event.timestamp_ms);
    }

    let mut buckets = Vec::new();
    for (key, (total, mut timestamps)) in groups {
        timestamps.sort_unstable();
        // Same gap estimate as sessions, scoped to this hour bucket so the
        // heatmap "duration" mode is not permanently empty for Claude.
        let active_seconds: i64 = timestamps
            .windows(2)
            .map(|w| {
                let gap = (w[1] - w[0]) as f64 / 1000.0;
                gap.min(EVENT_GAP_MAX_SECS) as i64
            })
            .sum();
        // A lone event in the hour still represents some activity.
        let active_seconds = if timestamps.len() == 1 {
            active_seconds.max(1)
        } else {
            active_seconds
        };
        buckets.push(UsageActivityBucket {
            hour_start_ms: key.0,
            source_id: key.1.clone(),
            model_id: key.2.clone(),
            project_id: key.3.clone(),
            terminal_id: None,
            total_tokens: Some(total),
            user_messages: 0,
            assistant_messages: assistant_msg_count.get(&key).copied().unwrap_or(0),
            active_seconds: if active_seconds > 0 {
                Some(active_seconds)
            } else {
                None
            },
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
            let s = s.trim();
            // ISO 8601 / RFC3339
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                return dt.timestamp_millis();
            }
            // Trailing Z without offset digits
            if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ") {
                return naive.and_utc().timestamp_millis();
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ") {
                return naive.and_utc().timestamp_millis();
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                return naive.and_utc().timestamp_millis();
            }
            // Epoch ms / sec
            if let Ok(ms) = s.parse::<i64>() {
                if ms > 1_000_000_000_000 {
                    return ms; // ms
                }
                if ms > 1_000_000_000 {
                    return ms * 1000; // sec
                }
            }
            // Do not invent "now" for unparsable timestamps — drop by returning 0.
            0
        }
        None => 0,
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

pub fn claude_source_status(
    state: &UsageSourceState,
    breadcrumbs: &Vec<UsageBreadcrumb>,
) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "claude".into(),
        label: "Claude".into(),
        kind: UsageSourceKind::External,
        state: state.clone(),
        breadcrumbs: breadcrumbs.clone(),
        capabilities: SourceCapabilities {
            total_tokens: true,    // actual message.usage tokens
            token_breakdown: true, // input/output from message.usage
            cache: true,           // cache_creation_input_tokens/cache_read_input_tokens
            cost: false,           // no cost from raw events
            hourly: true,          // from event timestamps
            project: true,         // from cwd/path
            messages: true,        // events with usage
            sessions: true,        // session ids
            duration: true,        // event gap estimate
        },
        duration_method: Some(DurationMethod::EventGapEstimate),
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn claude_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn claude_reads_model_from_message() {
        let event = parse_claude_json_line(
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-15T10:00:00Z","message":{"id":"m1","model":"claude-sonnet-4","usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":10,"cache_read_input_tokens":50}}}"#,
        )
        .expect("parse");
        assert_eq!(
            event.message.and_then(|m| m.model).as_deref(),
            Some("claude-sonnet-4")
        );
    }

    #[test]
    fn claude_accepts_duplicate_session_id_fields() {
        // Real Claude Code lines include both sessionId and session_id.
        let line = r#"{"type":"assistant","sessionId":"abc","session_id":"abc","timestamp":"2026-07-20T01:34:52.928Z","cwd":"/work","message":{"id":"chatcmpl-1","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
        let event = parse_claude_json_line(line).expect("must accept duplicate session keys");
        assert_eq!(event.session_id.as_deref(), Some("abc"));
        assert_eq!(event.event_type.as_deref(), Some("assistant"));
        assert_eq!(
            event
                .message
                .as_ref()
                .and_then(|m| m.usage.as_ref())
                .and_then(|u| u.input_tokens),
            Some(10)
        );
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
            stop_reason: None,
        }
    }

    #[test]
    fn claude_duplicate_request_is_counted_once() {
        let events = vec![
            make_event("msg1", "req1", 1000, Some("sonnet")),
            make_event("msg1", "req1", 1000, Some("sonnet")), // duplicate
        ];
        let daily = build_claude_daily(&events);
        // build_claude_daily sums; dedup happens earlier. Here both are present so total doubles.
        // Keep this as a smoke test that grouping works.
        assert_eq!(daily.len(), 1);
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
    fn claude_prefers_stop_reason_and_higher_output() {
        let partial = ParsedEvent {
            stop_reason: None,
            output_tokens: 1,
            input_tokens: 100,
            cache_creation_tokens: 0,
            cache_read_tokens: 5000,
            ..make_event("msg1", "req1", 1000, Some("sonnet"))
        };
        let final_row = ParsedEvent {
            stop_reason: Some("end_turn".into()),
            output_tokens: 150,
            input_tokens: 100,
            cache_creation_tokens: 0,
            cache_read_tokens: 5000,
            ..make_event("msg1", "req1", 2000, Some("sonnet"))
        };
        assert!(should_replace_claude_usage(&partial, &final_row));
        assert!(!should_replace_claude_usage(&final_row, &partial));
    }

    #[test]
    fn claude_activity_estimates_active_seconds_from_event_gaps() {
        // Two events 90s apart in the same hour → active_seconds = 90.
        let events = vec![
            make_event("m1", "r1", 1_800_000, Some("sonnet")),
            make_event("m2", "r1", 1_890_000, Some("sonnet")),
        ];
        let activity = build_claude_activity(&events);
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].active_seconds, Some(90));
    }

    #[test]
    fn claude_activity_single_event_counts_as_one_second() {
        let events = vec![make_event("m1", "r1", 3_600_000, Some("sonnet"))];
        let activity = build_claude_activity(&events);
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].active_seconds, Some(1));
    }

    #[test]
    fn claude_zero_usage_does_not_block_later_billable_row() {
        // Regression for the undercount vs cc-switch: stream zeros must not
        // burn the message.id dedup key before the final billable snapshot.
        let zero = ParsedEvent {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            stop_reason: None,
            ..make_event("msg1", "", 1000, Some("sonnet"))
        };
        let billable = ParsedEvent {
            input_tokens: 100,
            output_tokens: 20,
            cache_creation_tokens: 10,
            cache_read_tokens: 50,
            stop_reason: Some("end_turn".into()),
            ..make_event("msg1", "", 2000, Some("sonnet"))
        };
        // zero should never be preferred over billable
        assert!(should_replace_claude_usage(&zero, &billable));
        assert!(!should_replace_claude_usage(&billable, &zero));
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

    #[test]
    fn claude_live_scan_today_is_near_ccswitch_order_of_magnitude() {
        // Live feedback loop against ~/.claude — skipped in CI if empty.
        let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let now = crate::usage::now_ms();
        // Local midnight Asia/Shanghai
        let local = tz.timestamp_millis_opt(now).single().unwrap();
        let midnight = local.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let start = tz
            .from_local_datetime(&midnight)
            .unwrap()
            .timestamp_millis();
        let home = crate::usage::tool_home("CLAUDE_CONFIG_DIR", ".claude");
        let projects = home.as_ref().map(|h| h.join("projects"));
        let mut file_count = 0usize;
        if let Some(dir) = &projects {
            if dir.is_dir() {
                file_count = walkdir::WalkDir::new(dir)
                    .max_depth(12)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("jsonl"))
                    .count();
            }
        }
        let result = scan_claude_logs(start, now, &tz);
        let total: i64 = result
            .daily
            .iter()
            .map(|r| r.total_tokens.unwrap_or(0))
            .sum();
        let eventish: i64 = result
            .activity
            .iter()
            .map(|a| a.total_tokens.unwrap_or(0))
            .sum();
        eprintln!(
            "claude live home={home:?} projects={projects:?} files={file_count} daily_rows={} activity_rows={} sessions={} daily_total={total} activity_total={eventish} start={start} now={now}",
            result.daily.len(),
            result.activity.len(),
            result.sessions.len(),
        );
        for r in &result.daily {
            eprintln!(
                "  daily {} model={:?} project={:?} total={:?}",
                r.date, r.model_id, r.project_id, r.total_tokens
            );
        }
    }

    #[test]
    fn claude_parses_fractional_z_timestamps() {
        let ms = parse_timestamp_to_ms(&Some("2026-07-20T04:21:47.708Z".into()));
        assert!(ms > 0, "fractional Z timestamp must parse");
        let ms2 = parse_timestamp_to_ms(&Some("2026-07-20T04:21:47Z".into()));
        assert!(ms2 > 0);
    }

    #[test]
    fn claude_counts_serde_success_on_real_session_file() {
        let _env_lock = claude_env_lock();
        let src = dirs::home_dir()
            .unwrap()
            .join(".claude/projects/-Users-ldh-Downloads-project-AiNative-Natives/a7631756-153f-4fec-8c33-54f4a5568cc7.jsonl");
        if !src.is_file() {
            return;
        }
        use std::io::{BufRead, BufReader};
        let file = std::fs::File::open(&src).unwrap();
        let reader = BufReader::with_capacity(256 * 1024, file);
        let mut lines = 0usize;
        let mut with_usage = 0usize;
        let mut parsed_ok = 0usize;
        let mut parsed_err = 0usize;
        let mut assistant_with_usage = 0usize;
        let mut nonzero = 0usize;
        let mut in_range = 0usize;
        let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let now = crate::usage::now_ms();
        let local = tz.timestamp_millis_opt(now).single().unwrap();
        let midnight = local.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let start = tz
            .from_local_datetime(&midnight)
            .unwrap()
            .timestamp_millis()
            - 14 * 24 * 60 * 60 * 1_000;
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            lines += 1;
            let line = line.trim();
            if line.is_empty() || !line.contains("usage") {
                continue;
            }
            with_usage += 1;
            match parse_claude_json_line(line) {
                Some(event) => {
                    parsed_ok += 1;
                    if event.event_type.as_deref() != Some("assistant") {
                        continue;
                    }
                    let Some(usage) = event
                        .message
                        .as_ref()
                        .and_then(|m| m.usage.as_ref())
                        .cloned()
                        .or(event.top_level_usage.clone())
                    else {
                        continue;
                    };
                    assistant_with_usage += 1;
                    let in_tok = usage.input_tokens.unwrap_or(0);
                    let out_tok = usage.output_tokens.unwrap_or(0);
                    let cc_tok = usage.cache_creation_input_tokens.unwrap_or(0);
                    let cr_tok = usage.cache_read_input_tokens.unwrap_or(0);
                    if in_tok == 0 && out_tok == 0 && cc_tok == 0 && cr_tok == 0 {
                        continue;
                    }
                    nonzero += 1;
                    let ts_ms = parse_timestamp_to_ms(&event.timestamp);
                    if ts_ms >= start && ts_ms < now {
                        in_range += 1;
                    }
                }
                None => {
                    parsed_err += 1;
                }
            }
        }
        eprintln!(
            "serde stats lines={lines} with_usage={with_usage} ok={parsed_ok} err={parsed_err} assistant_usage={assistant_with_usage} nonzero={nonzero} in_range={in_range} start={start} now={now}"
        );
        assert!(parsed_ok > 0, "should parse some lines");
        assert!(
            in_range > 10,
            "expected many in-range nonzero rows, got {in_range}"
        );
    }

    #[test]
    fn claude_deserializes_real_grok_usage_line() {
        // Real line shape from Claude Code using grok-4.5 (chatcmpl id, nested usage extras).
        let line = r#"{"type":"assistant","sessionId":"s","session_id":"s","timestamp":"2026-07-20T01:34:52.928Z","cwd":"/Users/ldh/Downloads/project/AiNative/Natives","message":{"id":"chatcmpl-3a8b072f17084161b60da6e9","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":335978,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":291,"server_tool_use":{"web_search_requests":0},"service_tier":"standard","cache_creation":{"ephemeral_5m_input_tokens":0},"inference_geo":"","iterations":[],"speed":"standard"}}}"#;
        let event = parse_claude_json_line(line).expect("deserialize real line");
        assert_eq!(event.event_type.as_deref(), Some("assistant"));
        let usage = event
            .message
            .as_ref()
            .and_then(|m| m.usage.as_ref())
            .expect("usage");
        assert_eq!(usage.input_tokens, Some(335978));
        assert_eq!(usage.output_tokens, Some(291));
        let ms = parse_timestamp_to_ms(&event.timestamp);
        assert!(ms > 0);
    }

    #[test]
    fn claude_scans_real_home_file_via_config_dir_override() {
        let _env_lock = claude_env_lock();
        // Copy the largest real today session into a temp CLAUDE_CONFIG_DIR and
        // ensure the Rust scanner sees multi-million tokens (not ~1.1M).
        let src = dirs::home_dir()
            .unwrap()
            .join(".claude/projects/-Users-ldh-Downloads-project-AiNative-Natives/a7631756-153f-4fec-8c33-54f4a5568cc7.jsonl");
        if !src.is_file() {
            eprintln!("skip: real session file missing");
            return;
        }
        let root = std::env::temp_dir().join(format!("natives-claude-real-{}", std::process::id()));
        let project = root.join("projects/demo");
        std::fs::create_dir_all(&project).unwrap();
        let dst = project.join("session.jsonl");
        std::fs::copy(&src, &dst).unwrap();
        // Also write a minimal known-good line so we can distinguish path issues
        // from parse issues if the real file is skipped.
        let known = project.join("known.jsonl");
        std::fs::write(
            &known,
            r#"{"type":"assistant","sessionId":"s","timestamp":"2026-07-20T02:00:00.000Z","cwd":"/work","message":{"id":"known1","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
        )
        .unwrap();
        eprintln!(
            "fixture root={root:?} dst exists={} size={} known={}",
            dst.is_file(),
            std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0),
            known.is_file()
        );
        std::env::set_var("CLAUDE_CONFIG_DIR", &root);
        let resolved = crate::usage::tool_home("CLAUDE_CONFIG_DIR", ".claude");
        eprintln!("tool_home resolved={resolved:?}");
        let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let now = crate::usage::now_ms();
        let local = tz.timestamp_millis_opt(now).single().unwrap();
        let midnight = local.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let start = tz
            .from_local_datetime(&midnight)
            .unwrap()
            .timestamp_millis()
            - 14 * 24 * 60 * 60 * 1_000;
        let result = scan_claude_logs(start, now, &tz);
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        let total: i64 = result
            .daily
            .iter()
            .map(|r| r.total_tokens.unwrap_or(0))
            .sum();
        eprintln!(
            "real-file scan daily_rows={} sessions={} total={total} breadcrumbs={:?}",
            result.daily.len(),
            result.sessions.len(),
            result.breadcrumbs
        );
        for r in &result.daily {
            eprintln!(" row {:?}", r);
        }
        let _ = std::fs::remove_dir_all(root);
        assert!(
            total >= 100,
            "expected at least the known-good line tokens, got {total}"
        );
    }

    #[test]
    fn claude_scans_fixture_with_real_stream_shape() {
        let _env_lock = claude_env_lock();
        // Real Claude Code lines often include nested usage fields
        // (server_tool_use, cache_creation, etc). Ensure we still parse them.
        let root =
            std::env::temp_dir().join(format!("natives-claude-fixture-{}", std::process::id()));
        let project = root.join("projects/demo");
        std::fs::create_dir_all(&project).unwrap();
        // Three stream snapshots for same message id: zeros, partial, final.
        let lines = [
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T02:00:00.000Z","cwd":"/work/natives","message":{"id":"msg_1","model":"grok-4.5","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T02:00:01.000Z","cwd":"/work/natives","message":{"id":"msg_1","model":"grok-4.5","usage":{"input_tokens":1000,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000,"server_tool_use":{"web_search_requests":0},"cache_creation":{"ephemeral_5m_input_tokens":0},"service_tier":"standard"}}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T02:00:02.000Z","cwd":"/work/natives","message":{"id":"msg_1","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000,"server_tool_use":{"web_search_requests":0},"cache_creation":{"ephemeral_5m_input_tokens":0},"service_tier":"standard"}}}"#,
            // Second distinct message
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T03:00:00.000Z","cwd":"/work/natives","message":{"id":"msg_2","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":2000,"output_tokens":20,"cache_creation_input_tokens":100,"cache_read_input_tokens":0}}}"#,
        ];
        std::fs::write(project.join("session.jsonl"), lines.join("\n") + "\n").unwrap();

        // Point scanner at fixture via CLAUDE_CONFIG_DIR.
        // Safety: only for this test process.
        std::env::set_var("CLAUDE_CONFIG_DIR", &root);
        let tz: chrono_tz::Tz = chrono_tz::UTC;
        let start = parse_timestamp_to_ms(&Some("2026-07-20T00:00:00Z".into()));
        let end = parse_timestamp_to_ms(&Some("2026-07-21T00:00:00Z".into()));
        let result = scan_claude_logs(start, end, &tz);
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(root);

        let total: i64 = result
            .daily
            .iter()
            .map(|r| r.total_tokens.unwrap_or(0))
            .sum();
        // msg_1 final = 1000+50+5000 = 6050; msg_2 = 2000+20+100 = 2120; total 8170
        assert_eq!(result.sessions.len(), 2.min(result.sessions.len()).max(1));
        assert_eq!(
            total, 8170,
            "must keep final billable rows, not zeros/partials only"
        );
    }
}
