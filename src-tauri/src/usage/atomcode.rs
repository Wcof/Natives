use crate::usage::{
    localized_time_metrics, mask_home, BreadcrumbKind, DurationMethod, SourceCapabilities,
    UsageActivityBucket, UsageBreadcrumb, UsageDailyRecord, UsageQuality, UsageSessionRecord,
    UsageSourceKind, UsageSourceState, UsageSourceStatus, UsageWarning,
};
use chrono::TimeZone;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use walkdir::WalkDir;

const DATALOG_DIR: &str = ".atomcode/datalog";

struct Turn {
    timestamp_ms: i64,
    date: String,
    hour_start_ms: i64,
    session_id: String,
    model: Option<String>,
    project: String,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    active_seconds: i64,
}

pub struct AtomcodeScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

pub fn scan_atomcode_logs(start_ms: i64, end_ms: i64, tz: &chrono_tz::Tz) -> AtomcodeScanResult {
    let Some(home) = dirs::home_dir() else {
        return empty(UsageSourceState::Unavailable, vec![]);
    };
    let root = home.join(DATALOG_DIR);
    let breadcrumbs = vec![UsageBreadcrumb {
        kind: BreadcrumbKind::RawLog,
        label: mask_home(root.to_string_lossy().as_ref()),
    }];
    if !root.is_dir() {
        return empty(UsageSourceState::Ok, breadcrumbs);
    }

    let mut turns = Vec::new();
    for entry in WalkDir::new(&root)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md")
            || path.file_name().and_then(|n| n.to_str()) == Some("goal-evaluator.md")
        {
            continue;
        }
        let Some(timestamp_ms) = timestamp_from_path(path, tz) else {
            continue;
        };
        if timestamp_ms < start_ms || timestamp_ms >= end_ms {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let project = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into());
        if let Some(mut turn) = parse_turn(&content, &project, timestamp_ms) {
            let (date, hour_start_ms) = localized_time_metrics(timestamp_ms, tz);
            turn.date = date;
            turn.hour_start_ms = hour_start_ms;
            turns.push(turn);
        }
    }
    AtomcodeScanResult {
        daily: daily(&turns),
        activity: activity(&turns),
        sessions: sessions(&turns),
        state: UsageSourceState::Ok,
        breadcrumbs,
        warnings: vec![],
    }
}

fn empty(state: UsageSourceState, breadcrumbs: Vec<UsageBreadcrumb>) -> AtomcodeScanResult {
    AtomcodeScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state,
        breadcrumbs,
        warnings: vec![],
    }
}

fn parse_turn(content: &str, project: &str, timestamp_ms: i64) -> Option<Turn> {
    let env = Regex::new(r"\*\*env:\*\* model=([^,]+).*session=([^,]+)").ok()?;
    let token = Regex::new(r"tokens: prompt=(\d+)\+completion=(\d+)(?:, cache=(\d+)tok)?").ok()?;
    let duration = Regex::new(r"\*\*Stats:\*\* .*?, ([0-9.]+)s,").ok()?;
    let captures = env.captures(content)?;
    let mut input_tokens = 0;
    let mut output_tokens = 0;
    let mut cached_tokens = 0;
    for c in token.captures_iter(content) {
        input_tokens += c[1].parse::<i64>().ok()?;
        output_tokens += c[2].parse::<i64>().ok()?;
        cached_tokens += c.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
    }
    Some(Turn {
        timestamp_ms,
        date: String::new(),
        hour_start_ms: 0,
        model: Some(captures[1].to_string()),
        session_id: captures[2].to_string(),
        project: project.into(),
        input_tokens,
        output_tokens,
        cached_tokens,
        active_seconds: duration
            .captures(content)
            .and_then(|c| c[1].parse::<f64>().ok())
            .unwrap_or(0.0) as i64,
    })
}

fn timestamp_from_path(path: &std::path::Path, tz: &chrono_tz::Tz) -> Option<i64> {
    let stem = path.file_stem()?.to_str()?;
    let stamp = stem.get(..19)?;
    let naive = chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d_%H-%M-%S").ok()?;
    tz.from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
}

fn daily(turns: &[Turn]) -> Vec<UsageDailyRecord> {
    let mut grouped = HashMap::new();
    for t in turns {
        let e = grouped
            .entry((t.date.clone(), t.model.clone(), t.project.clone()))
            .or_insert((0, 0, 0));
        e.0 += t.input_tokens;
        e.1 += t.output_tokens;
        e.2 += t.cached_tokens;
    }
    grouped
        .into_iter()
        .map(
            |((date, model_id, project_id), (input, output, cache))| UsageDailyRecord {
                date,
                source_id: "atomcode".into(),
                model_id,
                project_id: Some(project_id),
                terminal_id: None,
                input_tokens: Some(input),
                output_tokens: Some(output),
                cache_creation_tokens: None,
                cache_read_tokens: Some(cache),
                total_tokens: Some(input + output + cache),
                cost_usd: None,
                cost_quality: UsageQuality::Unavailable,
            },
        )
        .collect()
}

fn activity(turns: &[Turn]) -> Vec<UsageActivityBucket> {
    let mut grouped = HashMap::new();
    for t in turns {
        let e = grouped
            .entry((t.hour_start_ms, t.model.clone(), t.project.clone()))
            .or_insert((0, 0, 0));
        e.0 += t.input_tokens + t.output_tokens + t.cached_tokens;
        e.1 += 1;
        e.2 += t.active_seconds;
    }
    grouped
        .into_iter()
        .map(
            |((hour_start_ms, model_id, project_id), (tokens, messages, seconds))| {
                UsageActivityBucket {
                    hour_start_ms,
                    source_id: "atomcode".into(),
                    model_id,
                    project_id: Some(project_id),
                    terminal_id: None,
                    total_tokens: Some(tokens),
                    user_messages: messages,
                    assistant_messages: messages,
                    active_seconds: Some(seconds),
                }
            },
        )
        .collect()
}

fn sessions(turns: &[Turn]) -> Vec<UsageSessionRecord> {
    let mut grouped: HashMap<&str, Vec<&Turn>> = HashMap::new();
    for t in turns {
        grouped.entry(&t.session_id).or_default().push(t);
    }
    grouped
        .into_iter()
        .map(|(id, ts)| {
            let first = ts[0];
            let started_at_ms = ts.iter().map(|t| t.timestamp_ms).min().unwrap_or(0);
            let ended_at_ms = ts.iter().map(|t| t.timestamp_ms).max().unwrap_or(0);
            UsageSessionRecord {
                session_id: format!("atomcode:{id}"),
                source_id: "atomcode".into(),
                model_id: first.model.clone(),
                project_id: Some(first.project.clone()),
                terminal_id: None,
                started_at_ms,
                ended_at_ms,
                user_messages: ts.len() as i64,
                assistant_messages: ts.len() as i64,
                active_seconds: Some(ts.iter().map(|t| t.active_seconds).sum()),
                duration_quality: UsageQuality::Reported,
            }
        })
        .collect()
}

pub fn atomcode_source_status(
    state: &UsageSourceState,
    breadcrumbs: &Vec<UsageBreadcrumb>,
) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "atomcode".into(),
        label: "Atomcode".into(),
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
        duration_method: Some(DurationMethod::SessionBounds),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_turn;
    #[test]
    fn parses_atomcode_turn_statistics_without_reading_message_content() {
        let turn = parse_turn("**env:** model=gpt-5, ctx_window=128000, session=session-1, cwd=/work/natives\n  _[tokens: prompt=100+completion=20, cache=40tok]_\n**Stats:** 1 turns, 2 tool calls, 3.0s, 120 tokens\n", "project-a", 1_700_000_000_000).expect("turn");
        assert_eq!(
            (turn.input_tokens, turn.output_tokens, turn.cached_tokens),
            (100, 20, 40)
        );
        assert_eq!(turn.model.as_deref(), Some("gpt-5"));
        assert_eq!(turn.session_id, "session-1");
        assert_eq!(turn.project, "project-a");
        let records = super::daily(&[turn]);
        assert_eq!(records[0].total_tokens, Some(160));
    }
}
