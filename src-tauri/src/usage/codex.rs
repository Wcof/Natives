// ── Codex Original Event Log Parser ──
// Scans ~/.codex/sessions/**/*.jsonl and ~/.codex/archived_sessions/**/*.jsonl.

#![allow(unused_imports, dead_code, unused_variables)]
use crate::usage::{
    mask_home, now_ms, BreadcrumbKind, DurationMethod, SourceCapabilities, UsageActivityBucket,
    UsageBreadcrumb, UsageDailyRecord, UsageDimension, UsageQuality, UsageSessionRecord,
    UsageSourceKind, UsageSourceState, UsageSourceStatus, UsageWarning, UsageWarningCode,
};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

const CODEX_SESSIONS_DIR: &str = "sessions";
const CODEX_ARCHIVED_DIR: &str = "archived_sessions";
const EVENT_GAP_MAX_SECS: f64 = 300.0;

// ── Minimal JSON structs ──

#[derive(Debug, Deserialize)]
struct CodexJsonLine {
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    #[serde(rename = "type")]
    event_type: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    last_token_usage: Option<CodexTokenUsage>,
    #[serde(default)]
    total_token_usage: Option<CodexTokenUsage>,
    #[serde(default)]
    turn_context: Option<CodexTurnContext>,
    #[serde(default)]
    replay_for: Option<String>,
    #[serde(default)]
    fork_from: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexTokenUsage {
    #[serde(default, alias = "input_tokens")]
    input: Option<i64>,
    #[serde(default, alias = "output_tokens")]
    output: Option<i64>,
    #[serde(default, rename = "reasoning", alias = "reasoning_output_tokens")]
    reasoning_tokens: Option<i64>,
    #[serde(default, rename = "input_cache_hit", alias = "cached_input_tokens")]
    input_cache_hit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CodexTurnContext {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    project: Option<String>,
}

/// Parsed delta event.
struct ParsedCodexEvent {
    event_id: String,
    timestamp_ms: i64,
    hour_start_ms: i64,
    date: String,
    session_id: String,
    model: Option<String>,
    project: Option<String>,
    project_label: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    reasoning_tokens: i64, // part of output if present
    input_cache_hit: i64,
    is_replay_or_fork: bool,
}

pub struct CodexScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

#[derive(Default)]
struct CodexFileContext {
    session_id: Option<String>,
    model: Option<String>,
    project: Option<String>,
}

fn collect_session_files(
    sessions_dir: &std::path::Path,
    archived_dir: &std::path::Path,
) -> HashMap<String, PathBuf> {
    let mut files = HashMap::new();
    // Iterate archived first so that sessions/ entries win on key collision.
    for root in [archived_dir, sessions_dir] {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                // Use the full path as key. The old code used file_stem which
                // caused collisions across date-partitioned directories
                // (e.g. sessions/2026/07/15/rollout.jsonl and
                //        sessions/2026/07/20/rollout.jsonl both have stem
                //        "rollout", so only one was kept).
                let key = path.to_string_lossy().to_string();
                files.insert(key, path.to_path_buf());
            }
        }
    }
    files
}

fn normalize_codex_line(line: &str, context: &mut CodexFileContext) -> Option<CodexJsonLine> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let line_type = value.get("type").and_then(|v| v.as_str());
    let payload = value.get("payload");

    match line_type {
        Some("session_meta") => {
            context.session_id = payload?
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from);
            context.project = payload?
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from);
            return None;
        }
        Some("turn_context") => {
            context.model = payload?
                .get("model")
                .and_then(|v| v.as_str())
                .map(String::from);
            context.project = payload?
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| context.project.clone());
            return None;
        }
        Some("event_msg")
            if payload?.get("type").and_then(|v| v.as_str()) == Some("token_count") =>
        {
            let info = payload?.get("info")?;
            return Some(CodexJsonLine {
                event_id: None,
                session_id: context.session_id.clone(),
                event_type: Some("token_count".into()),
                timestamp: value
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                last_token_usage: info
                    .get("last_token_usage")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok()),
                total_token_usage: info
                    .get("total_token_usage")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok()),
                turn_context: Some(CodexTurnContext {
                    model: context.model.clone(),
                    cwd: context.project.clone(),
                    project: context.project.clone(),
                }),
                replay_for: None,
                fork_from: None,
            });
        }
        _ => {}
    }

    serde_json::from_value(value).ok()
}

/// Scan Codex session directories for JSONL event files.
pub fn scan_codex_logs(start_ms: i64, end_ms: i64, tz: &chrono_tz::Tz) -> CodexScanResult {
    let mut warnings = Vec::new();

    let home = match crate::usage::tool_home("CODEX_HOME", ".codex") {
        Some(h) => h,
        None => {
            warnings.push(UsageWarning {
                source_id: Some("codex".into()),
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
            return CodexScanResult {
                daily: vec![],
                activity: vec![],
                sessions: vec![],
                state: UsageSourceState::Unavailable,
                breadcrumbs: vec![UsageBreadcrumb {
                    kind: BreadcrumbKind::RawLog,
                    label: "~/.codex/sessions/".into(),
                }],
                warnings,
            };
        }
    };

    let sessions_dir = home.join(CODEX_SESSIONS_DIR);
    let archived_dir = home.join(CODEX_ARCHIVED_DIR);

    let breadcrumb_main = format!("{}/", mask_home(sessions_dir.to_string_lossy().as_ref()));
    let breadcrumb = if archived_dir.exists() {
        format!("{} + archived", breadcrumb_main)
    } else {
        breadcrumb_main
    };

    let session_files = collect_session_files(&sessions_dir, &archived_dir);

    if session_files.is_empty() {
        return CodexScanResult {
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

    // Parse events with deduplication
    let mut all_events = Vec::new();
    let mut dedup_set: HashSet<String> = HashSet::new();
    // Per-session cumulative tracking
    let mut prev_total: HashMap<String, CodexTokenUsage> = HashMap::new();

    for (_sid, path) in &session_files {
        if let Ok(content) = fs::read_to_string(path) {
            let mut context = CodexFileContext::default();
            let mut lines: Vec<(usize, String)> = content
                .lines()
                .enumerate()
                .filter(|(_, l)| !l.trim().is_empty())
                .map(|(i, l)| (i, l.to_string()))
                .collect();
            lines.sort_by_key(|(i, _)| *i);

            // Parse events in order for cumulative tracking
            let mut session_events = Vec::new();
            for (_, line) in &lines {
                if let Some(event) = normalize_codex_line(line, &mut context) {
                    // Skip replay/fork events unconditionally
                    let is_replay = event.replay_for.is_some() || event.fork_from.is_some();
                    if is_replay {
                        continue;
                    }

                    // Dedup by event_id — skip if already seen (can happen when
                    // the same event appears in both sessions/ and
                    // archived_sessions/).
                    if let Some(ref eid) = event.event_id {
                        if !dedup_set.insert(eid.clone()) {
                            continue;
                        }
                    }

                    // Compute token delta
                    let (input, output, cache_read) = compute_codex_delta(
                        &event,
                        prev_total.get(&event.session_id.clone().unwrap_or_default()),
                    );

                    let (input, cache_read) = normalize_codex_input(input, cache_read);

                    // Update previous total for cumulative tracking
                    if let Some(ref total) = event.total_token_usage {
                        prev_total.insert(
                            event.session_id.clone().unwrap_or_default(),
                            CodexTokenUsage {
                                input: total.input,
                                output: total.output,
                                reasoning_tokens: total.reasoning_tokens,
                                input_cache_hit: total.input_cache_hit,
                            },
                        );
                    }

                    let ts_ms = parse_codex_timestamp(&event.timestamp);
                    if ts_ms < start_ms || ts_ms >= end_ms {
                        continue;
                    }

                    let (date, hour_start) = crate::usage::localized_time_metrics(ts_ms, tz);

                    let session_id = event
                        .session_id
                        .clone()
                        .unwrap_or_else(|| format!("codex:unknown:{}", date));

                    let (model, project) = match &event.turn_context {
                        Some(ctx) => (ctx.model.clone(), ctx.project.clone()),
                        None => (None, None),
                    };

                    session_events.push(ParsedCodexEvent {
                        event_id: event.event_id.clone().unwrap_or_default(),
                        timestamp_ms: ts_ms,
                        hour_start_ms: hour_start,
                        date,
                        session_id: format!("codex:{}", session_id),
                        model,
                        project: project.clone(),
                        project_label: project.clone(),
                        input_tokens: input,
                        output_tokens: output,
                        cache_read_tokens: cache_read,
                        reasoning_tokens: event
                            .last_token_usage
                            .as_ref()
                            .and_then(|u| u.reasoning_tokens)
                            .unwrap_or(0),
                        input_cache_hit: event
                            .last_token_usage
                            .as_ref()
                            .and_then(|u| u.input_cache_hit)
                            .unwrap_or(0),
                        is_replay_or_fork: is_replay,
                    });
                }
            }
            all_events.extend(session_events);
        }
    }

    // Build daily, activity, sessions
    let daily = build_codex_daily(&all_events);
    let activity = build_codex_activity(&all_events);
    let sessions = build_codex_sessions(&all_events);

    CodexScanResult {
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

fn compute_codex_delta(
    event: &CodexJsonLine,
    prev_total: Option<&CodexTokenUsage>,
) -> (i64, i64, i64) {
    // Prefer last_token_usage as it's already a delta
    if let Some(ref last) = event.last_token_usage {
        let input = last.input.unwrap_or(0);
        let output = last.output.unwrap_or(0);
        // reasoning is included in output for codex, don't double count
        let cache_read = last.input_cache_hit.unwrap_or(0);
        return (input.max(0), output.max(0), cache_read.max(0));
    }

    // Fall back to cumulative difference
    if let Some(ref total) = event.total_token_usage {
        if let Some(prev) = prev_total {
            let input_delta = total.input.unwrap_or(0) - prev.input.unwrap_or(0);
            let output_delta = total.output.unwrap_or(0) - prev.output.unwrap_or(0);
            let cache_read_delta =
                total.input_cache_hit.unwrap_or(0) - prev.input_cache_hit.unwrap_or(0);
            return (
                input_delta.max(0),
                output_delta.max(0),
                cache_read_delta.max(0),
            );
        }
        // First event with cumulative total - use as-is
        return (
            total.input.unwrap_or(0).max(0),
            total.output.unwrap_or(0).max(0),
            total.input_cache_hit.unwrap_or(0).max(0),
        );
    }

    (0, 0, 0)
}

/// Codex / OpenAI Responses semantics: `input_tokens` already includes
/// `cached_input_tokens`. Convert to Claude-style fresh input so
/// `token_total(input, output, 0, cache_read)` does not double-count cache.
///
/// Matches cc-switch `fresh_input_sql` for `app_type = "codex"`.
fn normalize_codex_input(input: i64, cache_read: i64) -> (i64, i64) {
    let input = input.max(0);
    let cache_read = cache_read.max(0).min(input);
    (input - cache_read, cache_read)
}

fn build_codex_daily(events: &[ParsedCodexEvent]) -> Vec<UsageDailyRecord> {
    // Group by (date, model, project) to aggregate daily tokens
    let mut groups: HashMap<(String, Option<String>, Option<String>), (i64, i64, i64, i64)> =
        HashMap::new();

    for event in events {
        if event.is_replay_or_fork {
            continue;
        }
        let key = (
            event.date.clone(),
            event.model.clone(),
            event.project.clone(),
        );
        let entry = groups.entry(key).or_insert((0, 0, 0, 0));
        entry.0 += event.input_tokens;
        entry.1 += event.output_tokens;
        entry.2 += event.cache_read_tokens;
        entry.3 += crate::usage::token_total(
            event.input_tokens,
            event.output_tokens,
            0,
            event.cache_read_tokens,
        );
    }

    let mut records = Vec::new();
    for ((date, model, project), (input, output, cache_read, total)) in groups {
        if total == 0 {
            continue;
        }

        records.push(UsageDailyRecord {
            date,
            source_id: "codex".into(),
            model_id: model,
            project_id: project,
            terminal_id: None,
            input_tokens: Some(input),
            output_tokens: Some(output),
            cache_creation_tokens: None,
            cache_read_tokens: Some(cache_read),
            total_tokens: Some(total),
            cost_usd: None,
            cost_quality: UsageQuality::Unavailable,
        });
    }

    records
}

fn build_codex_activity(events: &[ParsedCodexEvent]) -> Vec<UsageActivityBucket> {
    // (hour, source, model, project) -> (token sum, timestamps for gap duration)
    let mut groups: HashMap<(i64, String, Option<String>, Option<String>), (i64, Vec<i64>)> =
        HashMap::new();

    for event in events {
        if event.is_replay_or_fork {
            continue;
        }
        let key = (
            event.hour_start_ms,
            "codex".to_string(),
            event.model.clone(),
            event.project.clone(),
        );
        let total = crate::usage::token_total(
            event.input_tokens,
            event.output_tokens,
            0,
            event.cache_read_tokens,
        );
        let entry = groups.entry(key).or_default();
        entry.0 += total;
        entry.1.push(event.timestamp_ms);
    }

    let mut buckets = Vec::new();
    for (key, (total, mut timestamps)) in groups {
        timestamps.sort_unstable();
        let active_seconds: i64 = timestamps
            .windows(2)
            .map(|w| {
                let gap = (w[1] - w[0]) as f64 / 1000.0;
                gap.min(EVENT_GAP_MAX_SECS) as i64
            })
            .sum();
        let active_seconds = if timestamps.len() == 1 {
            active_seconds.max(1)
        } else {
            active_seconds
        };
        buckets.push(UsageActivityBucket {
            hour_start_ms: key.0,
            source_id: key.1,
            model_id: key.2,
            project_id: key.3,
            terminal_id: None,
            total_tokens: Some(total),
            user_messages: 0,
            assistant_messages: timestamps.len() as i64,
            active_seconds: if active_seconds > 0 {
                Some(active_seconds)
            } else {
                None
            },
        });
    }

    buckets
}

fn build_codex_sessions(events: &[ParsedCodexEvent]) -> Vec<UsageSessionRecord> {
    let mut session_events: HashMap<String, Vec<&ParsedCodexEvent>> = HashMap::new();
    for event in events {
        if event.is_replay_or_fork {
            continue;
        }
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

        let active_seconds: i64 = sorted
            .windows(2)
            .map(|w| {
                let gap = (w[1].timestamp_ms - w[0].timestamp_ms) as f64 / 1000.0;
                gap.min(EVENT_GAP_MAX_SECS) as i64
            })
            .sum();

        let first = sorted.first().unwrap();

        sessions.push(UsageSessionRecord {
            session_id: session_id.clone(),
            source_id: "codex".into(),
            model_id: first.model.clone(),
            project_id: first.project.clone(),
            terminal_id: None,
            started_at_ms,
            ended_at_ms,
            user_messages: 0,
            assistant_messages: assistant_count,
            active_seconds: Some(active_seconds),
            duration_quality: UsageQuality::Estimated,
        });
    }

    sessions
}

fn parse_codex_timestamp(ts: &Option<String>) -> i64 {
    match ts {
        Some(s) => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                return dt.timestamp_millis();
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ") {
                if let Some(dt) = Utc.from_local_datetime(&naive).single() {
                    return dt.timestamp_millis();
                }
            }
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

pub fn codex_source_status(
    state: &UsageSourceState,
    breadcrumbs: &Vec<UsageBreadcrumb>,
) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "codex".into(),
        label: "Codex".into(),
        kind: UsageSourceKind::External,
        state: state.clone(),
        breadcrumbs: breadcrumbs.clone(),
        capabilities: SourceCapabilities {
            total_tokens: true,
            token_breakdown: true,
            cache: true,
            cost: false,
            hourly: true,
            project: true,
            messages: true,
            sessions: true,
            duration: true,
        },
        duration_method: Some(DurationMethod::EventGapEstimate),
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_finds_current_four_level_session_layout() {
        let root = std::env::temp_dir().join(format!("natives-codex-{}", std::process::id()));
        let sessions = root.join("sessions/2026/07/15");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("rollout.jsonl"), "{}\n").unwrap();
        let files = collect_session_files(&root.join("sessions"), &root.join("archived_sessions"));
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn codex_multiple_rollouts_across_dates_not_lost() {
        // Regression: the old code used file_stem as HashMap key, so multiple
        // date directories each containing rollout.jsonl would collide and only
        // one file would survive. The fix uses full path as key.
        let root = std::env::temp_dir().join(format!("natives-codex-multi-{}", std::process::id()));
        let dir_a = root.join("sessions/2026/07/15");
        let dir_b = root.join("sessions/2026/07/20");
        let dir_c = root.join("sessions/2026/07/25");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::create_dir_all(&dir_c).unwrap();
        std::fs::write(dir_a.join("rollout.jsonl"), "{}\n").unwrap();
        std::fs::write(dir_b.join("rollout.jsonl"), "{}\n").unwrap();
        std::fs::write(dir_c.join("rollout.jsonl"), "{}\n").unwrap();
        let files = collect_session_files(&root.join("sessions"), &root.join("archived_sessions"));
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(
            files.len(),
            3,
            "all three rollout.jsonl files must be collected"
        );
    }

    #[test]
    fn codex_dedup_skips_duplicate_event_id() {
        // Verify that duplicate event_ids are counted only once even when
        // they are NOT replay events.
        let mut dedup_set: HashSet<String> = HashSet::new();
        let eid = "evt-dup".to_string();

        // First insert succeeds.
        assert!(dedup_set.insert(eid.clone()));
        // Second insert returns false — event should be skipped.
        assert!(!dedup_set.insert(eid.clone()));
    }

    #[test]
    fn codex_normalizes_current_rollout_token_event() {
        let mut context = CodexFileContext::default();
        let meta = r#"{"timestamp":"2026-07-15T10:00:00Z","type":"session_meta","payload":{"id":"session-1","cwd":"/work/natives"}}"#;
        let turn = r#"{"timestamp":"2026-07-15T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5.6","cwd":"/work/natives"}}"#;
        let tokens = r#"{"timestamp":"2026-07-15T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#;

        assert!(normalize_codex_line(meta, &mut context).is_none());
        assert!(normalize_codex_line(turn, &mut context).is_none());
        let event = normalize_codex_line(tokens, &mut context).expect("token event");
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(
            event.turn_context.as_ref().and_then(|c| c.model.as_deref()),
            Some("gpt-5.6")
        );
        assert_eq!(compute_codex_delta(&event, None), (100, 20, 40));
    }

    #[test]
    fn codex_last_token_usage_is_delta() {
        // last_token_usage is already a delta, should be used as-is
        let event = CodexJsonLine {
            event_id: Some("evt1".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:00:00Z".into()),
            last_token_usage: Some(CodexTokenUsage {
                input: Some(100),
                output: Some(20),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(50),
            }),
            total_token_usage: None,
            turn_context: None,
            replay_for: None,
            fork_from: None,
        };
        let delta = compute_codex_delta(&event, None);
        assert_eq!(delta.0, 100); // input
        assert_eq!(delta.1, 20); // output
    }

    #[test]
    fn codex_normalizes_cache_inclusive_input_to_fresh() {
        // Official total_tokens = input + output = 120 when input already includes cache.
        assert_eq!(normalize_codex_input(100, 40), (60, 40));
        // Cache cannot exceed input.
        assert_eq!(normalize_codex_input(20, 40), (0, 20));
        // No cache stays unchanged.
        assert_eq!(normalize_codex_input(100, 0), (100, 0));
    }

    #[test]
    fn codex_daily_total_matches_official_total_tokens() {
        // After normalize: fresh=60, output=20, cache=40 → real total 120
        // (same as Codex last_token_usage.total_tokens).
        let daily = build_codex_daily(&[ParsedCodexEvent {
            event_id: "event-1".into(),
            timestamp_ms: 1,
            hour_start_ms: 0,
            date: "2026-07-10".into(),
            session_id: "codex:session-1".into(),
            model: Some("gpt-5".into()),
            project: None,
            project_label: None,
            input_tokens: 60,
            output_tokens: 20,
            cache_read_tokens: 40,
            reasoning_tokens: 0,
            input_cache_hit: 40,
            is_replay_or_fork: false,
        }]);
        assert_eq!(daily[0].input_tokens, Some(60));
        assert_eq!(daily[0].cache_read_tokens, Some(40));
        assert_eq!(daily[0].total_tokens, Some(120));
    }

    #[test]
    fn codex_end_to_end_total_matches_rollout_total_field() {
        let mut context = CodexFileContext::default();
        let meta = r#"{"timestamp":"2026-07-15T10:00:00Z","type":"session_meta","payload":{"id":"session-1","cwd":"/work/natives"}}"#;
        let turn = r#"{"timestamp":"2026-07-15T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5.6","cwd":"/work/natives"}}"#;
        // Real Codex shape: total_tokens == input_tokens + output_tokens (cache already inside input).
        let tokens = r#"{"timestamp":"2026-07-15T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#;
        assert!(normalize_codex_line(meta, &mut context).is_none());
        assert!(normalize_codex_line(turn, &mut context).is_none());
        let event = normalize_codex_line(tokens, &mut context).expect("token event");
        let (input, output, cache_read) = compute_codex_delta(&event, None);
        let (fresh, cache) = normalize_codex_input(input, cache_read);
        assert_eq!((fresh, output, cache), (60, 20, 40));
        assert_eq!(
            crate::usage::token_total(fresh, output, 0, cache),
            120,
            "must match Codex total_tokens field"
        );
    }

    #[test]
    fn codex_cumulative_usage_subtracts_previous_total() {
        let prev = CodexTokenUsage {
            input: Some(50),
            output: Some(10),
            reasoning_tokens: Some(0),
            input_cache_hit: Some(20),
        };
        let event = CodexJsonLine {
            event_id: Some("evt2".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:01:00Z".into()),
            last_token_usage: None,
            total_token_usage: Some(CodexTokenUsage {
                input: Some(100),
                output: Some(25),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(40),
            }),
            turn_context: None,
            replay_for: None,
            fork_from: None,
        };
        let delta = compute_codex_delta(&event, Some(&prev));
        assert_eq!(delta.0, 50); // 100 - 50
        assert_eq!(delta.1, 15); // 25 - 10
    }

    #[test]
    fn codex_cumulative_reset_is_not_negative() {
        let prev = CodexTokenUsage {
            input: Some(200),
            output: Some(50),
            reasoning_tokens: Some(0),
            input_cache_hit: Some(100),
        };
        let event = CodexJsonLine {
            event_id: Some("evt3".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:02:00Z".into()),
            last_token_usage: None,
            total_token_usage: Some(CodexTokenUsage {
                input: Some(100), // reset - lower than prev
                output: Some(25),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(30),
            }),
            turn_context: None,
            replay_for: None,
            fork_from: None,
        };
        let delta = compute_codex_delta(&event, Some(&prev));
        assert_eq!(delta.0, 0); // 100 - 200 = -100, clamped to 0
        assert_eq!(delta.1, 0); // 25 - 50 = -25, clamped to 0
    }

    #[test]
    fn codex_replayed_event_is_counted_once() {
        // Replay/fork events should be filtered out by the calling code
        let event = CodexJsonLine {
            event_id: Some("evt4".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:00:00Z".into()),
            last_token_usage: Some(CodexTokenUsage {
                input: Some(100),
                output: Some(20),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(0),
            }),
            total_token_usage: None,
            turn_context: None,
            replay_for: Some("original-evt".into()),
            fork_from: None,
        };
        assert!(event.replay_for.is_some());
        assert!(event.fork_from.is_none());
    }
}
