// ── ccusage Source Data Collection ──
// Executes `ccusage daily --all --by-agent --json --offline --breakdown --timezone <tz> --since <since> --until <until>`
// in a single process invocation and parses the response.

#![allow(unused_imports, dead_code, unused_variables)]
use crate::usage::{
    now_ms, UsageDailyRecord, UsageQuality, UsageSourceState, UsageSourceStatus,
    UsageWarning, UsageWarningCode, SourceCapabilities, DurationMethod, UsageSourceKind,
    UsageDimension, UsageBreadcrumb, BreadcrumbKind,
};
use crate::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Result of scanning a single ccusage source.
#[derive(Debug, Clone)]
pub struct CcusageSourceResult {
    pub source_id: String,
    pub label: String,
    pub daily: Vec<UsageDailyRecord>,
    pub state: UsageSourceState,
    pub breadcrumbs: Vec<UsageBreadcrumb>,
    pub warnings: Vec<UsageWarning>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CcusageResponse {
    #[serde(default, alias = "data")]
    pub daily: Vec<CcusageDailyEntry>,
    #[serde(default, alias = "summary")]
    pub totals: Option<CcusageTotals>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CcusageDailyEntry {
    #[serde(alias = "period", alias = "date")]
    pub date: String,
    
    #[serde(default, alias = "agent", alias = "source", alias = "sourceId", alias = "source_id")]
    pub agent: Option<String>,
    
    #[serde(default, alias = "input_tokens", alias = "inputTokens", alias = "input")]
    pub input_tokens: i64,
    
    #[serde(default, alias = "output_tokens", alias = "outputTokens", alias = "output")]
    pub output_tokens: i64,
    
    #[serde(default, alias = "cache_creation_tokens", alias = "cacheCreationTokens", alias = "cacheCreation")]
    pub cache_creation_tokens: i64,
    
    #[serde(default, alias = "cache_read_tokens", alias = "cacheReadTokens", alias = "cacheRead")]
    pub cache_read_tokens: i64,
    
    #[serde(default, alias = "total_tokens", alias = "totalTokens", alias = "total")]
    pub total_tokens: i64,
    
    #[serde(default, alias = "totalCost", alias = "total_cost", alias = "cost", alias = "costUSD", alias = "cost_usd")]
    pub cost: Option<f64>,
    
    #[serde(default, alias = "model_breakdowns", alias = "modelBreakdowns", alias = "breakdown", alias = "breakdowns", alias = "models")]
    pub breakdowns: Vec<CcusageModelBreakdown>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CcusageModelBreakdown {
    #[serde(alias = "model", alias = "modelName", alias = "model_name")]
    pub model: String,
    
    #[serde(default, alias = "input_tokens", alias = "inputTokens", alias = "input")]
    pub input_tokens: i64,
    
    #[serde(default, alias = "output_tokens", alias = "outputTokens", alias = "output")]
    pub output_tokens: i64,
    
    #[serde(default, alias = "total_tokens", alias = "totalTokens", alias = "total")]
    pub total_tokens: i64,
    
    #[serde(default, alias = "totalCost", alias = "total_cost", alias = "cost", alias = "costUSD", alias = "cost_usd")]
    pub cost: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CcusageTotals {
    #[serde(default, alias = "input_tokens", alias = "inputTokens", alias = "input")]
    pub input_tokens: i64,
    #[serde(default, alias = "output_tokens", alias = "outputTokens", alias = "output")]
    pub output_tokens: i64,
    #[serde(default, alias = "cache_creation_tokens", alias = "cacheCreationTokens", alias = "cacheCreation")]
    pub cache_creation_tokens: i64,
    #[serde(default, alias = "cache_read_tokens", alias = "cacheReadTokens", alias = "cacheRead")]
    pub cache_read_tokens: i64,
    #[serde(default, alias = "total_tokens", alias = "totalTokens", alias = "total")]
    pub total_tokens: i64,
    #[serde(default, alias = "totalCost", alias = "total_cost", alias = "cost", alias = "costUSD", alias = "cost_usd")]
    pub cost: Option<f64>,
}

pub struct CcusageScanResult {
    pub results: Vec<CcusageSourceResult>,
    pub verification: HashMap<String, Vec<CcusageDailyEntry>>,
    pub warnings: Vec<UsageWarning>,
}

/// Find the ccusage binary in system PATH in a controlled manner.
fn find_ccusage_path() -> Option<std::path::PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let exe_path = dir.join("ccusage");
            if exe_path.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if let Ok(meta) = std::fs::metadata(&exe_path) {
                        if meta.mode() & 0o111 != 0 {
                            return Some(exe_path);
                        }
                    }
                }
                #[cfg(not(unix))]
                return Some(exe_path);
            }
        }
    }
    None
}

/// Runs a single ccusage command using tokio.
async fn run_ccusage_process(
    timezone: &str,
    since: &str,
    until: &str,
) -> Result<CcusageResponse, String> {
    let ccusage_bin = find_ccusage_path()
        .ok_or_else(|| "CLI_NOT_FOUND".to_string())?;

    let mut cmd = Command::new(ccusage_bin);
    cmd.arg("daily")
        .arg("--all")
        .arg("--by-agent")
        .arg("--json")
        .arg("--offline")
        .arg("--breakdown")
        .arg("--timezone")
        .arg(timezone)
        .arg("--since")
        .arg(since)
        .arg("--until")
        .arg(until);

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    let child = cmd.spawn().map_err(|e| format!("spawn error: {}", e))?;

    let wait_output = timeout(Duration::from_secs(30), child.wait_with_output()).await;

    match wait_output {
        Ok(Ok(output)) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("not found") || stderr.contains("No such file") {
                    return Err("CLI_NOT_FOUND".to_string());
                }
                return Err(format!("ccusage exited with error: {}", stderr));
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            serde_json::from_str::<CcusageResponse>(&stdout)
                .map_err(|e| format!("SOURCE_PARSE_PARTIAL: {}", e))
        }
        Ok(Err(e)) => Err(format!("ccusage wait error: {}", e)),
        Err(_) => {
            Err("CLI_TIMEOUT".to_string())
        }
    }
}

/// Fetch ccusage data for all sources in one go.
pub async fn scan_ccusage_all(
    since: &str,
    until: &str,
    timezone: &str,
) -> CcusageScanResult {
    let mut warnings = Vec::new();
    let mut results = Vec::new();
    let mut verification = HashMap::new();

    let resp = match run_ccusage_process(timezone, since, until).await {
        Ok(r) => r,
        Err(e) => {
            let code = match e.as_str() {
                "CLI_NOT_FOUND" => UsageWarningCode::CliNotFound,
                "CLI_TIMEOUT" => UsageWarningCode::CliTimeout,
                ref other if other.starts_with("SOURCE_PARSE_PARTIAL") => UsageWarningCode::SourceParsePartial,
                _ => UsageWarningCode::SourceUnavailable,
            };
            let mut details = HashMap::new();
            details.insert("detail".to_string(), serde_json::Value::String(e));
            warnings.push(UsageWarning {
                source_id: None,
                code,
                details,
            });
            return CcusageScanResult { results, verification, warnings };
        }
    };

    // Group entries by agent
    let mut agent_daily: HashMap<String, Vec<UsageDailyRecord>> = HashMap::new();

    for entry in resp.daily {
        let agent_name = entry.agent.clone().unwrap_or_else(|| "unknown".to_string());
        
        if agent_name == "claude" || agent_name == "codex" {
            verification.entry(agent_name).or_default().push(entry.clone());
            continue;
        }

        let (cost_usd, cost_quality) = if let Some(c) = entry.cost {
            if c == 0.0 && entry.total_tokens > 0 {
                (None, UsageQuality::Unavailable)
            } else {
                (Some(c), UsageQuality::Reported)
            }
        } else {
            (None, UsageQuality::Unavailable)
        };

        if entry.breakdowns.is_empty() {
            let record = UsageDailyRecord {
                date: entry.date.clone(),
                source_id: agent_name.clone(),
                model_id: None,
                project_id: None,
                terminal_id: None,
                input_tokens: Some(entry.input_tokens),
                output_tokens: Some(entry.output_tokens),
                cache_creation_tokens: Some(entry.cache_creation_tokens),
                cache_read_tokens: Some(entry.cache_read_tokens),
                total_tokens: Some(entry.total_tokens),
                cost_usd,
                cost_quality,
            };
            agent_daily.entry(agent_name).or_default().push(record);
        } else {
            for bd in entry.breakdowns {
                let bd_cost = bd.cost.unwrap_or(0.0);
                let (bd_cost_usd, bd_cost_quality) = if bd_cost == 0.0 && bd.total_tokens > 0 {
                    (None, UsageQuality::Unavailable)
                } else {
                    (Some(bd_cost), UsageQuality::Reported)
                };
                let bd_record = UsageDailyRecord {
                    date: entry.date.clone(),
                    source_id: agent_name.clone(),
                    model_id: Some(bd.model.clone()),
                    project_id: None,
                    terminal_id: None,
                    input_tokens: Some(bd.input_tokens),
                    output_tokens: Some(bd.output_tokens),
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                    total_tokens: Some(bd.total_tokens),
                    cost_usd: bd_cost_usd,
                    cost_quality: bd_cost_quality,
                };
                agent_daily.entry(agent_name.clone()).or_default().push(bd_record);
            }
        }
    }

    for (agent_name, daily_records) in agent_daily {
        results.push(CcusageSourceResult {
            source_id: agent_name.clone(),
            label: capitalize_source(&agent_name),
            daily: daily_records,
            state: UsageSourceState::Ok,
            breadcrumbs: vec![],
            warnings: vec![],
        });
    }

    CcusageScanResult { results, verification, warnings }
}

fn capitalize_source(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

pub fn build_ccusage_source_states(
    results: &[CcusageSourceResult],
) -> Vec<UsageSourceStatus> {
    results
        .iter()
        .map(|r| {
            UsageSourceStatus {
                id: r.source_id.clone(),
                label: r.label.clone(),
                kind: UsageSourceKind::External,
                state: r.state.clone(),
                breadcrumbs: r.breadcrumbs.clone(),
                capabilities: SourceCapabilities {
                    total_tokens: true,
                    token_breakdown: true,
                    cache: false,
                    cost: true,
                    hourly: false,
                    project: false,
                    messages: false,
                    sessions: false,
                    duration: false,
                },
                duration_method: None,
            }
        })
        .collect()
}
