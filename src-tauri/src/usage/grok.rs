use crate::usage::{
    localized_time_metrics, mask_home, BreadcrumbKind, DurationMethod, SourceCapabilities,
    UsageActivityBucket, UsageBreadcrumb, UsageDailyRecord, UsageQuality, UsageSessionRecord,
    UsageSourceKind, UsageSourceState, UsageSourceStatus, UsageWarning,
};
use chrono::DateTime;
use serde_json::Value;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct GrokScanResult {
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

pub fn grok_root() -> Option<PathBuf> {
    std::env::var_os("GROK_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".grok")))
}

pub fn scan_grok_root(
    root: &Path,
    start_ms: i64,
    end_ms: i64,
    tz: &chrono_tz::Tz,
) -> GrokScanResult {
    let sessions_root = root.join("sessions");
    let mut result = GrokScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state: if sessions_root.is_dir() {
            UsageSourceState::Ok
        } else {
            UsageSourceState::Detected
        },
        breadcrumbs: vec![UsageBreadcrumb {
            kind: BreadcrumbKind::RawLog,
            label: mask_home(sessions_root.to_string_lossy().as_ref()),
        }],
        warnings: vec![],
    };
    if !sessions_root.is_dir() {
        return result;
    }
    for entry in WalkDir::new(&sessions_root)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) != Some("signals.json") {
            continue;
        }
        let Some(signals) = read_json(path) else {
            result.state = UsageSourceState::Partial;
            continue;
        };
        let usage = signals.get("usage").unwrap_or(&signals);
        let input = number(usage, &["input_tokens", "inputTokens"]);
        let output = number(usage, &["output_tokens", "outputTokens"]);
        let cache = number(
            usage,
            &[
                "cache_read_input_tokens",
                "cacheReadInputTokens",
                "cached_input_tokens",
            ],
        );
        if input + output + cache == 0 {
            continue;
        }
        let session_dir = path.parent().unwrap_or(&sessions_root);
        let summary = read_json(&session_dir.join("summary.json")).unwrap_or(Value::Null);
        let updated_at_ms = timestamp(
            summary
                .get("updated_at")
                .or_else(|| summary.get("updatedAt")),
        );
        if updated_at_ms < start_ms || updated_at_ms >= end_ms {
            continue;
        }
        let created_at_ms = timestamp(
            summary
                .get("created_at")
                .or_else(|| summary.get("createdAt")),
        );
        let session_id = summary
            .get("id")
            .or_else(|| summary.get("session_id"))
            .and_then(Value::as_str)
            .or_else(|| session_dir.file_name().and_then(|name| name.to_str()))
            .unwrap_or("unknown");
        let model = summary
            .get("model")
            .or_else(|| summary.get("model_id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let project =
            std::fs::read_to_string(session_dir.parent().unwrap_or(session_dir).join(".cwd"))
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    session_dir
                        .parent()
                        .and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().into_owned())
                });
        let (date, hour_start_ms) = localized_time_metrics(updated_at_ms, tz);
        let user_messages = number(&summary, &["user_message_count", "userMessageCount"]);
        let assistant_messages = number(
            &summary,
            &["assistant_message_count", "assistantMessageCount"],
        );
        let total = crate::usage::token_total(input, output, 0, cache);
        result.daily.push(UsageDailyRecord {
            date,
            source_id: "grok".into(),
            model_id: model.clone(),
            project_id: project.clone(),
            terminal_id: None,
            input_tokens: Some(input),
            output_tokens: Some(output),
            cache_creation_tokens: None,
            cache_read_tokens: Some(cache),
            total_tokens: Some(total),
            cost_usd: None,
            cost_quality: UsageQuality::Unavailable,
        });
        result.activity.push(UsageActivityBucket {
            hour_start_ms,
            source_id: "grok".into(),
            model_id: model.clone(),
            project_id: project.clone(),
            terminal_id: None,
            total_tokens: Some(total),
            user_messages,
            assistant_messages,
            active_seconds: None,
        });
        result.sessions.push(UsageSessionRecord {
            session_id: format!("grok:{session_id}"),
            source_id: "grok".into(),
            model_id: model,
            project_id: project,
            terminal_id: None,
            started_at_ms: created_at_ms,
            ended_at_ms: updated_at_ms,
            user_messages,
            assistant_messages,
            active_seconds: None,
            duration_quality: UsageQuality::Unavailable,
        });
    }
    result
}

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn number(value: &Value, keys: &[&str]) -> i64 {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_i64))
        .unwrap_or(0)
        .max(0)
}

fn timestamp(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::String(value)) => DateTime::parse_from_rfc3339(value)
            .map(|date| date.timestamp_millis())
            .unwrap_or(0),
        Some(Value::Number(value)) => {
            let value = value.as_i64().unwrap_or(0);
            if value.abs() < 10_000_000_000 {
                value.saturating_mul(1000)
            } else {
                value
            }
        }
        _ => 0,
    }
}

pub fn grok_source_status(result: &GrokScanResult) -> UsageSourceStatus {
    UsageSourceStatus {
        id: "grok".into(),
        label: "Grok CLI".into(),
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
        duration_method: Some(DurationMethod::SessionBounds),
    }
}

#[cfg(test)]
mod tests {
    use super::scan_grok_root;

    #[test]
    fn scans_documented_grok_session_usage() {
        let root = std::env::temp_dir().join(format!("natives-grok-{}", std::process::id()));
        let session = root.join("sessions/work/session-1");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(
            session.join("summary.json"),
            r#"{"id":"s1","model":"grok-code-fast","created_at":"2026-07-15T07:11:00Z","updated_at":"2026-07-15T07:12:00Z","user_message_count":1,"assistant_message_count":1}"#,
        )
        .unwrap();
        std::fs::write(
            session.join("signals.json"),
            r#"{"usage":{"input_tokens":7210,"cache_read_input_tokens":41000,"output_tokens":1893}}"#,
        )
        .unwrap();

        let result = scan_grok_root(&root, 1_784_080_000_000, 1_784_100_000_000, &chrono_tz::UTC);
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(result.daily[0].input_tokens, Some(7_210));
        assert_eq!(result.daily[0].cache_read_tokens, Some(41_000));
        assert_eq!(result.daily[0].output_tokens, Some(1_893));
        assert_eq!(result.daily[0].total_tokens, Some(50_103));
    }
}
