use crate::usage::{
    localized_time_metrics, mask_home, BreadcrumbKind, DurationMethod, SourceCapabilities,
    UsageActivityBucket, UsageBreadcrumb, UsageDailyRecord, UsageQuality, UsageSessionRecord,
    UsageSourceKind, UsageSourceState, UsageSourceStatus, UsageWarning,
};
use chrono::DateTime;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct GeminiScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiSession {
    session_id: String,
    #[serde(default)]
    messages: Vec<GeminiMessage>,
}

#[derive(Deserialize)]
struct GeminiMessage {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    model: Option<String>,
    timestamp: String,
    tokens: Option<GeminiTokens>,
}

#[derive(Deserialize)]
struct GeminiTokens {
    #[serde(default)]
    input: i64,
    #[serde(default)]
    output: i64,
    #[serde(default)]
    thoughts: i64,
    #[serde(default)]
    cached: i64,
}

struct Event {
    session_id: String,
    project: String,
    model: Option<String>,
    timestamp_ms: i64,
    input: i64,
    output: i64,
    cached: i64,
}

pub fn gemini_root() -> Option<PathBuf> {
    std::env::var_os("GEMINI_CLI_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".gemini")))
}

pub fn scan_gemini_root(
    root: &Path,
    start_ms: i64,
    end_ms: i64,
    tz: &chrono_tz::Tz,
) -> GeminiScanResult {
    let mut result = GeminiScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state: UsageSourceState::Ok,
        breadcrumbs: vec![UsageBreadcrumb {
            kind: BreadcrumbKind::RawLog,
            label: mask_home(root.join("tmp/*/chats").to_string_lossy().as_ref()),
        }],
        warnings: vec![],
    };
    let tmp = root.join("tmp");
    if !tmp.is_dir() {
        return result;
    }
    let mut events = Vec::new();
    let mut seen_events = HashSet::new();
    for entry in WalkDir::new(&tmp)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("session-") || !name.ends_with(".json") {
            continue;
        }
        let Some(session) = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<GeminiSession>(&bytes).ok())
        else {
            result.state = UsageSourceState::Partial;
            continue;
        };
        let project = path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into());
        for message in session.messages {
            if message.kind != "gemini" {
                continue;
            }
            if !seen_events.insert((session.session_id.clone(), message.id)) {
                continue;
            }
            let Some(tokens) = message.tokens else {
                continue;
            };
            let timestamp_ms = DateTime::parse_from_rfc3339(&message.timestamp)
                .map(|date| date.timestamp_millis())
                .unwrap_or(0);
            if timestamp_ms < start_ms || timestamp_ms >= end_ms {
                continue;
            }
            let input = tokens.input.max(0);
            let output = tokens.output.max(0) + tokens.thoughts.max(0);
            let cached = tokens.cached.max(0);
            if input + output + cached > 0 {
                events.push(Event {
                    session_id: session.session_id.clone(),
                    project: project.clone(),
                    model: message.model,
                    timestamp_ms,
                    input,
                    output,
                    cached,
                });
            }
        }
    }
    fill_records(&events, tz, &mut result);
    result
}

fn fill_records(events: &[Event], tz: &chrono_tz::Tz, result: &mut GeminiScanResult) {
    let mut daily = HashMap::<(String, Option<String>, String), (i64, i64, i64)>::new();
    let mut activity = HashMap::<(i64, Option<String>, String), (i64, i64)>::new();
    let mut sessions = HashMap::<&str, Vec<&Event>>::new();
    for event in events {
        let (date, hour) = localized_time_metrics(event.timestamp_ms, tz);
        let value = daily
            .entry((date, event.model.clone(), event.project.clone()))
            .or_default();
        value.0 += event.input;
        value.1 += event.output;
        value.2 += event.cached;
        let bucket = activity
            .entry((hour, event.model.clone(), event.project.clone()))
            .or_default();
        bucket.0 += crate::usage::token_total(event.input, event.output, 0, event.cached);
        bucket.1 += 1;
        sessions.entry(&event.session_id).or_default().push(event);
    }
    result.daily = daily
        .into_iter()
        .map(|((date, model_id, project_id), value)| UsageDailyRecord {
            date,
            source_id: "gemini".into(),
            model_id,
            project_id: Some(project_id),
            terminal_id: None,
            input_tokens: Some(value.0),
            output_tokens: Some(value.1),
            cache_creation_tokens: None,
            cache_read_tokens: Some(value.2),
            total_tokens: Some(crate::usage::token_total(value.0, value.1, 0, value.2)),
            cost_usd: None,
            cost_quality: UsageQuality::Unavailable,
        })
        .collect();
    result.activity = activity
        .into_iter()
        .map(
            |((hour_start_ms, model_id, project_id), value)| UsageActivityBucket {
                hour_start_ms,
                source_id: "gemini".into(),
                model_id,
                project_id: Some(project_id),
                terminal_id: None,
                total_tokens: Some(value.0),
                user_messages: value.1,
                assistant_messages: value.1,
                active_seconds: None,
            },
        )
        .collect();
    result.sessions = sessions
        .into_iter()
        .map(|(id, values)| {
            let first = values[0];
            UsageSessionRecord {
                session_id: format!("gemini:{id}"),
                source_id: "gemini".into(),
                model_id: first.model.clone(),
                project_id: Some(first.project.clone()),
                terminal_id: None,
                started_at_ms: values
                    .iter()
                    .map(|value| value.timestamp_ms)
                    .min()
                    .unwrap_or(0),
                ended_at_ms: values
                    .iter()
                    .map(|value| value.timestamp_ms)
                    .max()
                    .unwrap_or(0),
                user_messages: values.len() as i64,
                assistant_messages: values.len() as i64,
                active_seconds: None,
                duration_quality: UsageQuality::Unavailable,
            }
        })
        .collect();
}

pub fn gemini_source_status(result: &GeminiScanResult) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "gemini".into(),
        label: "Gemini CLI".into(),
        kind: UsageSourceKind::External,
        state: result.state.clone(),
        breadcrumbs: result.breadcrumbs.clone(),
        capabilities: SourceCapabilities {
            total_tokens: true,
            token_breakdown: true,
            cache: true,
            cost: false,
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
    use super::scan_gemini_root;

    #[test]
    fn scans_gemini_cli_token_messages() {
        let root = std::env::temp_dir().join(format!("natives-gemini-{}", std::process::id()));
        let chats = root.join("tmp/project/chats");
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::write(
            chats.join("session-1.json"),
            r#"{"sessionId":"s1","messages":[{"id":"m1","type":"gemini","model":"gemini-2.5-pro","timestamp":"2026-07-15T07:11:03Z","tokens":{"input":10,"output":4,"thoughts":2,"cached":8}},{"id":"m1","type":"gemini","model":"gemini-2.5-pro","timestamp":"2026-07-15T07:11:03Z","tokens":{"input":10,"output":4,"thoughts":2,"cached":8}}]}"#,
        )
        .unwrap();

        let result = scan_gemini_root(&root, 1_784_080_000_000, 1_784_100_000_000, &chrono_tz::UTC);
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(result.daily[0].input_tokens, Some(10));
        assert_eq!(result.daily[0].output_tokens, Some(6));
        assert_eq!(result.daily[0].cache_read_tokens, Some(8));
        assert_eq!(result.daily[0].total_tokens, Some(24));
    }
}
