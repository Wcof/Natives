use crate::usage::{
    localized_time_metrics, mask_home, BreadcrumbKind, DurationMethod, SourceCapabilities,
    UsageActivityBucket, UsageBreadcrumb, UsageDailyRecord, UsageQuality, UsageSessionRecord,
    UsageSourceKind, UsageSourceState, UsageSourceStatus, UsageWarning,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

const SESSIONS_DIR: &str = "sessions";

struct Turn {
    timestamp_ms: i64,
    date: String,
    hour_start_ms: i64,
    session_id: String,
    project: String,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    active_seconds: i64,
}

#[derive(Deserialize)]
struct TurnRecord {
    ts: i64,
    session_id: String,
    #[serde(default)]
    turn_id: u64,
    usage: TokenUsage,
}

#[derive(Default, Deserialize)]
struct TokenUsage {
    prompt: i64,
    completion: i64,
    #[serde(default)]
    cached: i64,
}

#[derive(Deserialize)]
struct SessionSnapshot {
    #[serde(default)]
    messages: Vec<SnapshotMessage>,
}

#[derive(Deserialize)]
struct SnapshotMessage {
    meta: Option<SnapshotMessageMeta>,
}

#[derive(Deserialize)]
struct SnapshotMessageMeta {
    session_id: String,
    turn_id: u64,
    tokens: TokenUsage,
}

#[derive(Default, Deserialize)]
struct SessionMeta {
    #[serde(default)]
    working_dir: String,
    #[serde(default)]
    turn_stats: Vec<TurnStat>,
}

#[derive(Deserialize)]
struct TurnStat {
    duration_ms: u64,
    #[serde(default)]
    total_tokens: i64,
    #[serde(default)]
    turn_count: i64,
}

#[derive(Deserialize)]
struct ArchivedSession {
    id: String,
    #[serde(default)]
    working_dir: String,
    created_at: i64,
    updated_at: i64,
    #[serde(default)]
    turn_stats: Vec<TurnStat>,
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
    let Some(home) = crate::usage::tool_home("ATOMCODE_HOME", ".atomcode") else {
        return empty(UsageSourceState::Unavailable, vec![]);
    };
    let root = home.join(SESSIONS_DIR);
    let breadcrumbs = vec![UsageBreadcrumb {
        kind: BreadcrumbKind::RawLog,
        label: mask_home(root.to_string_lossy().as_ref()),
    }];
    if !root.is_dir() {
        return empty(UsageSourceState::Ok, breadcrumbs);
    }

    let turns = scan_session_logs(&root, start_ms, end_ms, tz);
    let current_ids: HashSet<String> = turns.iter().map(|turn| turn.session_id.clone()).collect();
    let mut archived = scan_archived_sessions(&root, start_ms, end_ms, tz, &current_ids);
    let mut daily = daily(&turns);
    daily.append(&mut archived.daily);
    let mut activity = activity(&turns);
    activity.append(&mut archived.activity);
    let mut sessions = sessions(&turns);
    sessions.append(&mut archived.sessions);
    AtomcodeScanResult {
        daily,
        activity,
        sessions,
        state: UsageSourceState::Ok,
        breadcrumbs,
        warnings: vec![],
    }
}

fn scan_archived_sessions(
    root: &Path,
    start_ms: i64,
    end_ms: i64,
    tz: &chrono_tz::Tz,
    current_ids: &HashSet<String>,
) -> AtomcodeScanResult {
    let mut result = empty(UsageSourceState::Ok, vec![]);
    for entry in WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        let Ok(session) = serde_json::from_slice::<ArchivedSession>(&bytes) else {
            continue;
        };
        if current_ids.contains(&session.id) {
            continue;
        }
        let updated_at_ms = timestamp_ms(session.updated_at);
        if updated_at_ms < start_ms || updated_at_ms >= end_ms {
            continue;
        }
        let created_at_ms = timestamp_ms(session.created_at);
        let total_tokens = session
            .turn_stats
            .iter()
            .map(|stat| stat.total_tokens.max(0))
            .sum::<i64>();
        let active_seconds = session
            .turn_stats
            .iter()
            .map(|stat| stat.duration_ms.div_ceil(1000) as i64)
            .sum::<i64>();
        let user_messages = session.turn_stats.len() as i64;
        let assistant_messages = session
            .turn_stats
            .iter()
            .map(|stat| stat.turn_count.max(1))
            .sum::<i64>();
        let project = (!session.working_dir.is_empty()).then_some(session.working_dir);
        let (date, hour_start_ms) = localized_time_metrics(updated_at_ms, tz);

        if total_tokens > 0 {
            result.daily.push(UsageDailyRecord {
                date,
                source_id: "atomcode".into(),
                model_id: None,
                project_id: project.clone(),
                terminal_id: None,
                input_tokens: None,
                output_tokens: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
                total_tokens: Some(total_tokens),
                cost_usd: None,
                cost_quality: UsageQuality::Unavailable,
            });
            result.activity.push(UsageActivityBucket {
                hour_start_ms,
                source_id: "atomcode".into(),
                model_id: None,
                project_id: project.clone(),
                terminal_id: None,
                total_tokens: Some(total_tokens),
                user_messages,
                assistant_messages,
                active_seconds: Some(active_seconds),
            });
        }
        result.sessions.push(UsageSessionRecord {
            session_id: format!("atomcode:{}", session.id),
            source_id: "atomcode".into(),
            model_id: None,
            project_id: project,
            terminal_id: None,
            started_at_ms: created_at_ms,
            ended_at_ms: updated_at_ms,
            user_messages,
            assistant_messages,
            active_seconds: Some(active_seconds),
            duration_quality: UsageQuality::Reported,
        });
    }
    result
}

fn timestamp_ms(value: i64) -> i64 {
    if value.abs() < 10_000_000_000 {
        value.saturating_mul(1000)
    } else {
        value
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

fn scan_session_logs(root: &Path, start_ms: i64, end_ms: i64, tz: &chrono_tz::Tz) -> Vec<Turn> {
    let mut turns = Vec::new();
    for entry in WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let meta = fs::read(path.with_extension("meta"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SessionMeta>(&bytes).ok())
            .unwrap_or_default();
        let project = if meta.working_dir.is_empty() {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown".into())
        } else {
            meta.working_dir
        };
        let snapshot_usage = read_snapshot_usage(&path.with_extension("snapshot"));
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            let Ok(record) = serde_json::from_str::<TurnRecord>(line) else {
                continue;
            };
            if record.ts < start_ms || record.ts >= end_ms {
                continue;
            }
            let usage = snapshot_usage
                .get(&(record.session_id.clone(), record.turn_id))
                .unwrap_or(&record.usage);
            let (input_tokens, output_tokens, cached_tokens) = (
                usage.prompt.saturating_sub(usage.cached),
                usage.completion,
                usage.cached,
            );
            let (date, hour_start_ms) = localized_time_metrics(record.ts, tz);
            turns.push(Turn {
                timestamp_ms: record.ts,
                date,
                hour_start_ms,
                session_id: record.session_id,
                project: project.clone(),
                input_tokens,
                output_tokens,
                cached_tokens,
                active_seconds: meta
                    .turn_stats
                    .get(index)
                    .map(|stat| stat.duration_ms.div_ceil(1000) as i64)
                    .unwrap_or(0),
            });
        }
    }
    turns
}

fn read_snapshot_usage(path: &Path) -> HashMap<(String, u64), TokenUsage> {
    let Ok(bytes) = fs::read(path) else {
        return HashMap::new();
    };
    let Ok(snapshot) = serde_json::from_slice::<SessionSnapshot>(&bytes) else {
        return HashMap::new();
    };
    let mut usage = HashMap::<(String, u64), TokenUsage>::new();
    for meta in snapshot
        .messages
        .into_iter()
        .filter_map(|message| message.meta)
    {
        let entry = usage.entry((meta.session_id, meta.turn_id)).or_default();
        entry.prompt = entry.prompt.saturating_add(meta.tokens.prompt);
        entry.completion = entry.completion.saturating_add(meta.tokens.completion);
        entry.cached = entry.cached.saturating_add(meta.tokens.cached);
    }
    usage
}

fn daily(turns: &[Turn]) -> Vec<UsageDailyRecord> {
    let mut grouped = HashMap::new();
    for turn in turns {
        let entry = grouped
            .entry((turn.date.clone(), turn.project.clone()))
            .or_insert((0, 0, 0));
        entry.0 += turn.input_tokens;
        entry.1 += turn.output_tokens;
        entry.2 += turn.cached_tokens;
    }
    grouped
        .into_iter()
        .map(
            |((date, project_id), (input, output, cache))| UsageDailyRecord {
                date,
                source_id: "atomcode".into(),
                model_id: None,
                project_id: Some(project_id),
                terminal_id: None,
                input_tokens: Some(input),
                output_tokens: Some(output),
                cache_creation_tokens: None,
                cache_read_tokens: Some(cache),
                total_tokens: Some(crate::usage::token_total(input, output, 0, cache)),
                cost_usd: None,
                cost_quality: UsageQuality::Unavailable,
            },
        )
        .collect()
}

fn activity(turns: &[Turn]) -> Vec<UsageActivityBucket> {
    let mut grouped = HashMap::new();
    for turn in turns {
        let entry = grouped
            .entry((turn.hour_start_ms, turn.project.clone()))
            .or_insert((0, 0, 0));
        entry.0 += crate::usage::token_total(
            turn.input_tokens,
            turn.output_tokens,
            0,
            turn.cached_tokens,
        );
        entry.1 += 1;
        entry.2 += turn.active_seconds;
    }
    grouped
        .into_iter()
        .map(
            |((hour_start_ms, project_id), (tokens, messages, seconds))| UsageActivityBucket {
                hour_start_ms,
                source_id: "atomcode".into(),
                model_id: None,
                project_id: Some(project_id),
                terminal_id: None,
                total_tokens: Some(tokens),
                user_messages: messages,
                assistant_messages: messages,
                active_seconds: Some(seconds),
            },
        )
        .collect()
}

fn sessions(turns: &[Turn]) -> Vec<UsageSessionRecord> {
    let mut grouped: HashMap<&str, Vec<&Turn>> = HashMap::new();
    for turn in turns {
        grouped.entry(&turn.session_id).or_default().push(turn);
    }
    grouped
        .into_iter()
        .map(|(id, turns)| {
            let first = turns[0];
            UsageSessionRecord {
                session_id: format!("atomcode:{id}"),
                source_id: "atomcode".into(),
                model_id: None,
                project_id: Some(first.project.clone()),
                terminal_id: None,
                started_at_ms: turns
                    .iter()
                    .map(|turn| turn.timestamp_ms)
                    .min()
                    .unwrap_or(0),
                ended_at_ms: turns
                    .iter()
                    .map(|turn| turn.timestamp_ms)
                    .max()
                    .unwrap_or(0),
                user_messages: turns.len() as i64,
                assistant_messages: turns.len() as i64,
                active_seconds: Some(turns.iter().map(|turn| turn.active_seconds).sum()),
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
    use super::{scan_archived_sessions, scan_session_logs};
    use std::collections::HashSet;

    #[test]
    fn scans_atomcode_session_ledger() {
        let root = std::env::temp_dir().join(format!("natives-atomcode-{}", std::process::id()));
        let project = root.join("sessions/project-hash");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("session-1.jsonl"),
            r#"{"v":1,"ts":1784020263399,"iso":"2026-07-14T09:11:03.399+00:00","session_id":"session-1","turn_id":2,"undone":false,"user":"secret","assistant":"secret","usage":{"prompt":100,"completion":20,"cached":40}}
"#,
        ).unwrap();
        std::fs::write(
            project.join("session-1.meta"),
            r#"{"v":1,"id":"session-1","name":"session-1","user_renamed":false,"working_dir":"/work/natives","created_at":1784020103511,"updated_at":1784020263399,"turn_count":1,"message_count":2,"turn_stats":[{"after_message":2,"tool_call_count":0,"duration_ms":3000,"total_tokens":120,"errored":false}]}"#,
        ).unwrap();

        let turns = scan_session_logs(
            &root.join("sessions"),
            1_784_020_000_000,
            1_784_030_000_000,
            &chrono_tz::UTC,
        );
        std::fs::remove_dir_all(root).unwrap();
        let daily = super::daily(&turns);
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].project_id.as_deref(), Some("/work/natives"));
        assert_eq!(daily[0].input_tokens, Some(60));
        assert_eq!(daily[0].cache_read_tokens, Some(40));
        assert_eq!(daily[0].total_tokens, Some(120));
        assert_eq!(super::sessions(&turns)[0].active_seconds, Some(3));
    }

    #[test]
    fn uses_snapshot_request_usage_for_multi_round_turn() {
        let root =
            std::env::temp_dir().join(format!("natives-atomcode-snapshot-{}", std::process::id()));
        let project = root.join("sessions/project-hash");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("session-1.jsonl"),
            r#"{"v":1,"ts":1784020263399,"session_id":"session-1","turn_id":2,"usage":{"prompt":100,"completion":20,"cached":40}}
"#,
        )
        .unwrap();
        std::fs::write(
            project.join("session-1.snapshot"),
            r#"{"messages":[{"meta":{"session_id":"session-1","turn_id":2,"tokens":{"prompt":100,"completion":10,"cached":40}}},{"meta":{"session_id":"session-1","turn_id":2,"tokens":{"prompt":150,"completion":20,"cached":100}}}]}"#,
        )
        .unwrap();

        let turns = scan_session_logs(
            &root.join("sessions"),
            1_784_020_000_000,
            1_784_030_000_000,
            &chrono_tz::UTC,
        );
        std::fs::remove_dir_all(root).unwrap();
        let daily = super::daily(&turns);
        assert_eq!(daily[0].input_tokens, Some(110));
        assert_eq!(daily[0].output_tokens, Some(30));
        assert_eq!(daily[0].cache_read_tokens, Some(140));
        assert_eq!(daily[0].total_tokens, Some(280));
    }

    #[test]
    fn scans_standalone_archived_sessions_without_inventing_breakdown() {
        let root =
            std::env::temp_dir().join(format!("natives-atomcode-archive-{}", std::process::id()));
        let project = root.join("sessions/project-hash");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("archived-session.json"),
            r#"{"id":"archive-1","working_dir":"/work/archive","created_at":1784020103,"updated_at":1784020263,"turn_stats":[{"duration_ms":3000,"total_tokens":900}]}"#,
        )
        .unwrap();

        let result = scan_archived_sessions(
            &root.join("sessions"),
            1_784_020_000_000,
            1_784_030_000_000,
            &chrono_tz::UTC,
            &HashSet::new(),
        );
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(result.daily.len(), 1);
        assert_eq!(result.daily[0].total_tokens, Some(900));
        assert_eq!(result.daily[0].input_tokens, None);
        assert_eq!(result.sessions[0].session_id, "atomcode:archive-1");
    }
}
