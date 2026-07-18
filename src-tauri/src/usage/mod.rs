// ── Natives Usage Module ──
// Real data collection from ccusage, Claude logs, Codex logs, and Natives DB.

mod aggregate;
mod atomcode;
mod ccusage;
mod claude;
mod codex;
mod detected;
mod gemini;
mod grok;
mod natives;
mod opencode;
pub mod snapshot;

use crate::Error;
use chrono::{DateTime, Utc, Datelike, Timelike, TimeZone};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

// ── Re-exports ──
pub use aggregate::*;
pub use atomcode::*;
pub use ccusage::*;
pub use claude::*;
pub use codex::*;
pub use detected::*;
pub use gemini::*;
pub use grok::*;
pub use natives::*;
pub use opencode::*;
pub use snapshot::*;

// ── IPC Contract Types (mirror frontend types/usage.ts) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboardRequest {
    pub start_ms: i64,
    pub end_ms: i64,
    pub include_comparison: bool,
    pub time_zone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriodData {
    pub range: UsageDashboardRange,
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboardResponse {
    pub generated_at_ms: i64,
    pub range: UsageDashboardRange,
    pub daily: Vec<UsageDailyRecord>,
    pub activity: Vec<UsageActivityBucket>,
    pub sessions: Vec<UsageSessionRecord>,
    pub comparison: Option<UsagePeriodData>,
    pub dimensions: UsageDashboardDimensions,
    pub sources: Vec<UsageSourceStatus>,
    pub rtk: Option<RtkSummary>,
    pub warnings: Vec<UsageWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboardRange {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboardDimensions {
    pub sources: Vec<UsageDimension>,
    pub models: Vec<UsageDimension>,
    pub projects: Vec<UsageDimension>,
    pub terminals: Vec<UsageDimension>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageQuality {
    Reported,
    Estimated,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSourceState {
    Ok,
    Detected,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDimension {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDailyRecord {
    pub date: String,
    pub source_id: String,
    pub model_id: Option<String>,
    pub project_id: Option<String>,
    pub terminal_id: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
    pub cost_quality: UsageQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageActivityBucket {
    pub hour_start_ms: i64,
    pub source_id: String,
    pub model_id: Option<String>,
    pub project_id: Option<String>,
    pub terminal_id: Option<String>,
    pub total_tokens: Option<i64>,
    pub user_messages: i64,
    pub assistant_messages: i64,
    pub active_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSessionRecord {
    pub session_id: String,
    pub source_id: String,
    pub model_id: Option<String>,
    pub project_id: Option<String>,
    pub terminal_id: Option<String>,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub user_messages: i64,
    pub assistant_messages: i64,
    pub active_seconds: Option<i64>,
    pub duration_quality: UsageQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBreadcrumb {
    pub kind: BreadcrumbKind,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreadcrumbKind {
    Cli,
    RawLog,
    Database,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSourceStatus {
    pub id: String,
    pub label: String,
    pub kind: UsageSourceKind,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub capabilities: SourceCapabilities,
    pub duration_method: Option<DurationMethod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSourceKind {
    Natives,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCapabilities {
    pub total_tokens: bool,
    pub token_breakdown: bool,
    pub cache: bool,
    pub cost: bool,
    pub hourly: bool,
    pub project: bool,
    pub messages: bool,
    pub sessions: bool,
    pub duration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationMethod {
    EventGapEstimate,
    SessionBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UsageWarningCode {
    CliNotFound,
    CliTimeout,
    SourceUnavailable,
    SourceParsePartial,
    TotalMismatch,
    CostUnavailable,
    NativesHistoryPartial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWarning {
    pub source_id: Option<String>,
    pub code: UsageWarningCode,
    pub details: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RtkSummary {
    pub total_saved_tokens: i64,
    pub total_commands: i64,
}

pub struct UsageCache {
    snapshot_cache: Mutex<HashMap<String, crate::usage::snapshot::UsageDashboardSnapshot>>,
}

impl UsageCache {
    pub fn new() -> Self {
        Self {
            snapshot_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Get a snapshot from memory cache (no TTL — lasts until session end or re-sync).
    pub fn get_snapshot(&self, time_zone: &str) -> Option<crate::usage::snapshot::UsageDashboardSnapshot> {
        let map = self.snapshot_cache.lock().ok()?;
        map.get(time_zone).cloned()
    }

    /// Set a snapshot in memory cache.
    pub fn set_snapshot(&self, time_zone: &str, snapshot: crate::usage::snapshot::UsageDashboardSnapshot) {
        if let Ok(mut map) = self.snapshot_cache.lock() {
            map.insert(time_zone.to_string(), snapshot);
        }
    }
}

// ── Validation ──

pub fn validate_request(req: &UsageDashboardRequest) -> Result<(), Error> {
    if req.start_ms < 0 {
        return Err(Error::InvalidInput("startMs must be >= 0".into()));
    }
    if req.end_ms <= req.start_ms {
        return Err(Error::InvalidInput("endMs must be > startMs".into()));
    }
    let max_span_ms: i64 = 366 * 24 * 60 * 60 * 1000; // 366 days
    if req.end_ms - req.start_ms > max_span_ms {
        return Err(Error::InvalidInput("range must be <= 366 days".into()));
    }
    let _: chrono_tz::Tz = req.time_zone.parse()
        .map_err(|_| Error::InvalidInput("Invalid IANA timeZone".into()))?;
    Ok(())
}

// ── Helpers ──

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn localized_time_metrics(ts_ms: i64, tz: &Tz) -> (String, i64) {
    let local_dt = tz.timestamp_opt(ts_ms / 1000, ((ts_ms % 1000) * 1_000_000) as u32)
        .single()
        .unwrap_or_else(|| {
            let naive = chrono::DateTime::from_timestamp_millis(ts_ms)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default();
            tz.from_utc_datetime(&naive)
        });
    let date_str = local_dt.format("%Y-%m-%d").to_string();

    let hour_start_utc = DateTime::<Utc>::from_naive_utc_and_offset(
        chrono::NaiveDate::from_ymd_opt(local_dt.year(), local_dt.month(), local_dt.day()).unwrap()
            .and_hms_opt(local_dt.hour(), 0, 0).unwrap(),
        Utc
    );
    (date_str, hour_start_utc.timestamp_millis())
}

/// Collect all dimension labels from records into a sorted unique list.
pub fn collect_dimensions(
    daily: &[UsageDailyRecord],
    activity: &[UsageActivityBucket],
    sessions: &[UsageSessionRecord],
) -> UsageDashboardDimensions {
    let mut source_map: HashMap<String, String> = HashMap::new();
    let mut model_map: HashMap<String, String> = HashMap::new();
    let mut project_map: HashMap<String, String> = HashMap::new();

    for record in daily {
        source_map.entry(record.source_id.clone()).or_insert_with(|| record.source_id.clone());
        if let Some(ref model) = record.model_id {
            model_map.entry(model.clone()).or_insert_with(|| model.clone());
        }
        if let Some(ref project) = record.project_id {
            project_map.entry(project.clone()).or_insert_with(|| project.clone());
        }
    }

    for bucket in activity {
        source_map.entry(bucket.source_id.clone()).or_insert_with(|| bucket.source_id.clone());
        if let Some(ref model) = bucket.model_id {
            model_map.entry(model.clone()).or_insert_with(|| model.clone());
        }
        if let Some(ref project) = bucket.project_id {
            project_map.entry(project.clone()).or_insert_with(|| project.clone());
        }
    }

    for session in sessions {
        source_map.entry(session.source_id.clone()).or_insert_with(|| session.source_id.clone());
        if let Some(ref model) = session.model_id {
            model_map.entry(model.clone()).or_insert_with(|| model.clone());
        }
        if let Some(ref project) = session.project_id {
            project_map.entry(project.clone()).or_insert_with(|| project.clone());
        }
    }

    let mut sources: Vec<UsageDimension> = source_map
        .into_iter()
        .map(|(id, label)| UsageDimension { id, label })
        .collect();
    sources.sort_by(|a, b| a.id.cmp(&b.id));

    let mut models: Vec<UsageDimension> = model_map
        .into_iter()
        .map(|(id, label)| UsageDimension { id, label })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));

    let mut projects: Vec<UsageDimension> = project_map
        .into_iter()
        .map(|(id, label)| UsageDimension { id, label })
        .collect();
    projects.sort_by(|a, b| a.id.cmp(&b.id));

    UsageDashboardDimensions { sources, models, projects, terminals: vec![] }
}

/// Mask home directory paths to ~
pub fn mask_home(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy().to_string();
        path.replace(&home_str, "~")
    } else {
        path.to_string()
    }
}

pub fn select_tool_home(custom: Option<PathBuf>, default: PathBuf) -> PathBuf {
    custom.unwrap_or(default)
}

pub fn tool_home(env_key: &str, default_name: &str) -> Option<PathBuf> {
    let custom = std::env::var_os(env_key).map(PathBuf::from);
    let default = dirs::home_dir()?.join(default_name);
    Some(select_tool_home(custom, default))
}

pub(crate) fn token_total(input: i64, output: i64, cache_creation: i64, cache_read: i64) -> i64 {
    input
        .saturating_add(output)
        .saturating_add(cache_creation)
        .saturating_add(cache_read)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_range_is_rejected() {
        let req = UsageDashboardRequest { start_ms: -1, end_ms: 100, include_comparison: false, time_zone: "UTC".into() };
        assert!(validate_request(&req).is_err());

        let req = UsageDashboardRequest { start_ms: 100, end_ms: 100, include_comparison: false, time_zone: "UTC".into() };
        assert!(validate_request(&req).is_err());

        let req = UsageDashboardRequest { start_ms: 200, end_ms: 100, include_comparison: false, time_zone: "UTC".into() };
        assert!(validate_request(&req).is_err());
    }

    #[test]
    fn range_end_is_exclusive() {
        let req = UsageDashboardRequest { start_ms: 0, end_ms: 100, include_comparison: false, time_zone: "UTC".into() };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn home_path_is_masked() {
        let home = dirs::home_dir().unwrap();
        let home_str = home.to_string_lossy().to_string();
        let test_path = format!("{}/some/nested/path", home_str);
        let masked = mask_home(&test_path);
        assert_eq!(masked, "~/some/nested/path");
    }

    #[test]
    fn rtk_is_separate_from_usage() {
        let rtk = RtkSummary { total_saved_tokens: 5000, total_commands: 10 };
        assert!(rtk.total_saved_tokens > 0);
        assert!(rtk.total_commands > 0);
    }

    #[test]
    fn dimensions_only_include_real_values() {
        let dims = collect_dimensions(&[], &[], &[]);
        assert!(dims.sources.is_empty());
        assert!(dims.models.is_empty());
        assert!(dims.projects.is_empty());
    }

    #[test]
    fn custom_tool_home_overrides_default_home() {
        let custom = PathBuf::from("/custom/tool");
        let default = PathBuf::from("/default/tool");
        assert_eq!(select_tool_home(Some(custom.clone()), default), custom);
    }

    #[test]
    fn total_tokens_include_all_cache_tokens() {
        assert_eq!(token_total(60, 20, 10, 40), 130);
    }

    #[test]
    fn response_serializes_as_camel_case() {
        let resp = UsageDashboardResponse {
            generated_at_ms: 12345,
            range: UsageDashboardRange { start_ms: 0, end_ms: 1000 },
            daily: vec![],
            activity: vec![],
            sessions: vec![],
            comparison: None,
            dimensions: UsageDashboardDimensions { sources: vec![], models: vec![], projects: vec![], terminals: vec![] },
            sources: vec![],
            rtk: None,
            warnings: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("generatedAtMs").is_some());
        assert!(json.get("generated_at_ms").is_none());
    }

    #[test]
    fn mismatch_returns_structured_warning() {
        let warning = UsageWarning {
            source_id: Some("claude".into()),
            code: UsageWarningCode::TotalMismatch,
            details: {
                let mut m = std::collections::HashMap::new();
                m.insert("ccusage_total".into(), serde_json::Value::Number(serde_json::Number::from(100)));
                m.insert("scanned_total".into(), serde_json::Value::Number(serde_json::Number::from(95)));
                m
            },
        };
        assert_eq!(warning.source_id.unwrap(), "claude");
        assert!(matches!(warning.code, UsageWarningCode::TotalMismatch));
    }

    #[test]
    fn serialization_contracts_match_typescript() {
        // Assert WarningCode serialization matches SCREAMING_SNAKE_CASE
        assert_eq!(serde_json::to_string(&UsageWarningCode::CliNotFound).unwrap(), "\"CLI_NOT_FOUND\"");
        assert_eq!(serde_json::to_string(&UsageWarningCode::CliTimeout).unwrap(), "\"CLI_TIMEOUT\"");
        assert_eq!(serde_json::to_string(&UsageWarningCode::SourceUnavailable).unwrap(), "\"SOURCE_UNAVAILABLE\"");
        assert_eq!(serde_json::to_string(&UsageWarningCode::SourceParsePartial).unwrap(), "\"SOURCE_PARSE_PARTIAL\"");
        assert_eq!(serde_json::to_string(&UsageWarningCode::TotalMismatch).unwrap(), "\"TOTAL_MISMATCH\"");
        assert_eq!(serde_json::to_string(&UsageWarningCode::CostUnavailable).unwrap(), "\"COST_UNAVAILABLE\"");
        assert_eq!(serde_json::to_string(&UsageWarningCode::NativesHistoryPartial).unwrap(), "\"NATIVES_HISTORY_PARTIAL\"");

        // Assert DurationMethod serialization matches snake_case
        assert_eq!(serde_json::to_string(&DurationMethod::EventGapEstimate).unwrap(), "\"event_gap_estimate\"");
        assert_eq!(serde_json::to_string(&DurationMethod::SessionBounds).unwrap(), "\"session_bounds\"");

        // Assert UsageQuality serialization matches camelCase (lowercase start)
        assert_eq!(serde_json::to_string(&UsageQuality::Reported).unwrap(), "\"reported\"");
        assert_eq!(serde_json::to_string(&UsageQuality::Estimated).unwrap(), "\"estimated\"");
        assert_eq!(serde_json::to_string(&UsageQuality::Unavailable).unwrap(), "\"unavailable\"");

        // Assert UsageSourceState serialization matches camelCase (lowercase start)
        assert_eq!(serde_json::to_string(&UsageSourceState::Ok).unwrap(), "\"ok\"");
        assert_eq!(serde_json::to_string(&UsageSourceState::Partial).unwrap(), "\"partial\"");
        assert_eq!(serde_json::to_string(&UsageSourceState::Unavailable).unwrap(), "\"unavailable\"");
        assert_eq!(serde_json::to_string(&UsageSourceState::Detected).unwrap(), "\"detected\"");

        // Assert BreadcrumbKind serialization matches snake_case
        assert_eq!(serde_json::to_string(&BreadcrumbKind::Cli).unwrap(), "\"cli\"");
        assert_eq!(serde_json::to_string(&BreadcrumbKind::RawLog).unwrap(), "\"raw_log\"");
        assert_eq!(serde_json::to_string(&BreadcrumbKind::Database).unwrap(), "\"database\"");
    }
}
