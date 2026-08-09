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
    #[allow(clippy::type_complexity)] // pre-existing type shape
    let mut groups: HashMap<
        (String, Option<String>, Option<String>),
        (i64, i64, i64, i64, i64),
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

#[allow(clippy::type_complexity)] // pre-existing type shape
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
    breadcrumbs: &[UsageBreadcrumb],
) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "claude".into(),
        label: "Claude".into(),
        kind: UsageSourceKind::External,
        state: state.clone(),
        breadcrumbs: breadcrumbs.to_vec(),
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
#[path = "claude_tests.rs"]
mod claude_tests;
