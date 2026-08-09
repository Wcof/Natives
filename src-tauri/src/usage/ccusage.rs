// ── ccusage Source Data Collection ──
// Compatible with ccusage >= 20.x.
//
// Primary strategy (agent-separated, preferred for Claude/Codex reconciliation):
//   ccusage claude daily -j -O -b -z <tz> -s <since> -u <until>
//   ccusage codex   daily -j -O    -z <tz> -s <since> -u <until>
// Fallback (aggregate all agents; agent field may be "all"):
//   ccusage daily -j -O --all -z <tz> -s <since> -u <until>
//
// Note: `--by-agent` was removed in ccusage 20 and must not be used.

#![allow(unused_imports, dead_code, unused_variables)]
use crate::usage::{
    now_ms, BreadcrumbKind, DurationMethod, SourceCapabilities, UsageBreadcrumb, UsageDailyRecord,
    UsageDimension, UsageQuality, UsageSourceKind, UsageSourceState, UsageSourceStatus,
    UsageWarning, UsageWarningCode,
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
    /// ccusage may emit either `date` or `period`.
    #[serde(default, alias = "period", alias = "date")]
    pub date: String,

    #[serde(
        default,
        alias = "agent",
        alias = "source",
        alias = "sourceId",
        alias = "source_id"
    )]
    pub agent: Option<String>,

    #[serde(
        default,
        alias = "input_tokens",
        alias = "inputTokens",
        alias = "input"
    )]
    pub input_tokens: i64,

    #[serde(
        default,
        alias = "output_tokens",
        alias = "outputTokens",
        alias = "output"
    )]
    pub output_tokens: i64,

    #[serde(
        default,
        alias = "cache_creation_tokens",
        alias = "cacheCreationTokens",
        alias = "cacheCreation"
    )]
    pub cache_creation_tokens: i64,

    #[serde(
        default,
        alias = "cache_read_tokens",
        alias = "cacheReadTokens",
        alias = "cacheRead"
    )]
    pub cache_read_tokens: i64,

    #[serde(
        default,
        alias = "total_tokens",
        alias = "totalTokens",
        alias = "total"
    )]
    pub total_tokens: i64,

    #[serde(
        default,
        alias = "totalCost",
        alias = "total_cost",
        alias = "cost",
        alias = "costUSD",
        alias = "cost_usd"
    )]
    pub cost: Option<f64>,

    /// Array form: `modelBreakdowns: [{ modelName, ... }]`
    #[serde(
        default,
        alias = "model_breakdowns",
        alias = "modelBreakdowns",
        alias = "breakdown",
        alias = "breakdowns"
    )]
    pub breakdowns: Vec<CcusageModelBreakdown>,

    /// Object form used by `ccusage codex daily`: `models: { "gpt-5.5": { ... } }`
    #[serde(default, alias = "models")]
    pub models_map: HashMap<String, CcusageModelBreakdown>,
}

impl CcusageDailyEntry {
    fn effective_breakdowns(&self) -> Vec<CcusageModelBreakdown> {
        if !self.breakdowns.is_empty() {
            return self.breakdowns.clone();
        }
        if self.models_map.is_empty() {
            return Vec::new();
        }
        self.models_map
            .iter()
            .map(|(name, bd)| {
                let mut out = bd.clone();
                if out.model.trim().is_empty() {
                    out.model = name.clone();
                }
                if out.total_tokens == 0 {
                    out.total_tokens = out.input_tokens
                        + out.output_tokens
                        + out.cache_creation_tokens
                        + out.cache_read_tokens;
                }
                out
            })
            .collect()
    }

    fn normalized_date(&self) -> String {
        // Accept YYYYMMDD or YYYY-MM-DD
        let d = self.date.trim();
        if d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()) {
            format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
        } else {
            d.to_string()
        }
    }

    fn effective_total_tokens(&self) -> i64 {
        if self.total_tokens > 0 {
            return self.total_tokens;
        }
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct CcusageModelBreakdown {
    #[serde(default, alias = "model", alias = "modelName", alias = "model_name")]
    pub model: String,

    #[serde(
        default,
        alias = "input_tokens",
        alias = "inputTokens",
        alias = "input"
    )]
    pub input_tokens: i64,

    #[serde(
        default,
        alias = "output_tokens",
        alias = "outputTokens",
        alias = "output"
    )]
    pub output_tokens: i64,

    #[serde(
        default,
        alias = "cache_creation_tokens",
        alias = "cacheCreationTokens",
        alias = "cacheCreation"
    )]
    pub cache_creation_tokens: i64,

    #[serde(
        default,
        alias = "cache_read_tokens",
        alias = "cacheReadTokens",
        alias = "cacheRead"
    )]
    pub cache_read_tokens: i64,

    #[serde(
        default,
        alias = "total_tokens",
        alias = "totalTokens",
        alias = "total"
    )]
    pub total_tokens: i64,

    #[serde(
        default,
        alias = "totalCost",
        alias = "total_cost",
        alias = "cost",
        alias = "costUSD",
        alias = "cost_usd"
    )]
    pub cost: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CcusageTotals {
    #[serde(
        default,
        alias = "input_tokens",
        alias = "inputTokens",
        alias = "input"
    )]
    pub input_tokens: i64,
    #[serde(
        default,
        alias = "output_tokens",
        alias = "outputTokens",
        alias = "output"
    )]
    pub output_tokens: i64,
    #[serde(
        default,
        alias = "cache_creation_tokens",
        alias = "cacheCreationTokens",
        alias = "cacheCreation"
    )]
    pub cache_creation_tokens: i64,
    #[serde(
        default,
        alias = "cache_read_tokens",
        alias = "cacheReadTokens",
        alias = "cacheRead"
    )]
    pub cache_read_tokens: i64,
    #[serde(
        default,
        alias = "total_tokens",
        alias = "totalTokens",
        alias = "total"
    )]
    pub total_tokens: i64,
    #[serde(
        default,
        alias = "totalCost",
        alias = "total_cost",
        alias = "cost",
        alias = "costUSD",
        alias = "cost_usd"
    )]
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

/// Run one ccusage argv list and parse JSON stdout.
async fn run_ccusage_argv(args: &[&str]) -> Result<CcusageResponse, String> {
    let ccusage_bin = find_ccusage_path().ok_or_else(|| "CLI_NOT_FOUND".to_string())?;

    let mut cmd = Command::new(ccusage_bin);
    for a in args {
        cmd.arg(a);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    let child = cmd.spawn().map_err(|e| format!("spawn error: {e}"))?;
    let wait_output = timeout(Duration::from_secs(45), child.wait_with_output()).await;

    match wait_output {
        Ok(Ok(output)) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("not found") || stderr.contains("No such file") {
                    return Err("CLI_NOT_FOUND".to_string());
                }
                return Err(format!("ccusage exited with error: {stderr}"));
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut parsed = serde_json::from_str::<CcusageResponse>(&stdout)
                .map_err(|e| format!("SOURCE_PARSE_PARTIAL: {e}"))?;
            // Normalize date fields after parse.
            for entry in &mut parsed.daily {
                entry.date = entry.normalized_date();
                if entry.total_tokens == 0 {
                    entry.total_tokens = entry.effective_total_tokens();
                }
            }
            Ok(parsed)
        }
        Ok(Err(e)) => Err(format!("ccusage wait error: {e}")),
        Err(_) => Err("CLI_TIMEOUT".to_string()),
    }
}

fn push_cli_error_warning(warnings: &mut Vec<UsageWarning>, source_id: Option<&str>, err: &str) {
    let code = match err {
        "CLI_NOT_FOUND" => UsageWarningCode::CliNotFound,
        "CLI_TIMEOUT" => UsageWarningCode::CliTimeout,
        other if other.starts_with("SOURCE_PARSE_PARTIAL") => UsageWarningCode::SourceParsePartial,
        _ => UsageWarningCode::SourceUnavailable,
    };
    let mut details = HashMap::new();
    details.insert(
        "detail".to_string(),
        serde_json::Value::String(err.to_string()),
    );
    if let Some(sid) = source_id {
        details.insert(
            "source".to_string(),
            serde_json::Value::String(sid.to_string()),
        );
    }
    warnings.push(UsageWarning {
        source_id: source_id.map(|s| s.to_string()),
        code,
        details,
    });
}

fn entry_to_records(source_id: &str, entry: &CcusageDailyEntry) -> Vec<UsageDailyRecord> {
    let total = entry.effective_total_tokens();
    let (cost_usd, cost_quality) = if let Some(c) = entry.cost {
        if c == 0.0 && total > 0 {
            (None, UsageQuality::Unavailable)
        } else {
            (Some(c), UsageQuality::Reported)
        }
    } else {
        (None, UsageQuality::Unavailable)
    };

    let breakdowns = entry.effective_breakdowns();
    if breakdowns.is_empty() {
        return vec![UsageDailyRecord {
            date: entry.normalized_date(),
            source_id: source_id.to_string(),
            model_id: None,
            project_id: None,
            terminal_id: None,
            input_tokens: Some(entry.input_tokens),
            output_tokens: Some(entry.output_tokens),
            cache_creation_tokens: Some(entry.cache_creation_tokens),
            cache_read_tokens: Some(entry.cache_read_tokens),
            total_tokens: Some(total),
            cost_usd,
            cost_quality,
        }];
    }

    breakdowns
        .into_iter()
        .map(|bd| {
            let bd_total = if bd.total_tokens > 0 {
                bd.total_tokens
            } else {
                bd.input_tokens + bd.output_tokens + bd.cache_creation_tokens + bd.cache_read_tokens
            };
            let bd_cost = bd.cost.unwrap_or(0.0);
            let (bd_cost_usd, bd_cost_quality) = if bd_cost == 0.0 && bd_total > 0 {
                (None, UsageQuality::Unavailable)
            } else if bd.cost.is_some() {
                (Some(bd_cost), UsageQuality::Reported)
            } else {
                (None, UsageQuality::Unavailable)
            };
            UsageDailyRecord {
                date: entry.normalized_date(),
                source_id: source_id.to_string(),
                model_id: if bd.model.is_empty() {
                    None
                } else {
                    Some(bd.model)
                },
                project_id: None,
                terminal_id: None,
                input_tokens: Some(bd.input_tokens),
                output_tokens: Some(bd.output_tokens),
                cache_creation_tokens: Some(bd.cache_creation_tokens),
                cache_read_tokens: Some(bd.cache_read_tokens),
                total_tokens: Some(bd_total),
                cost_usd: bd_cost_usd,
                cost_quality: bd_cost_quality,
            }
        })
        .collect()
}

/// Fetch ccusage data for all sources.
///
/// **Optional enricher only.** Dashboard correctness must not depend on ccusage
/// being installed. When the binary is missing, return empty verification with
/// a single soft warning and let native log scanners provide the numbers.
///
/// Claude/Codex use dedicated subcommands so verification can reconcile against
/// native JSONL scanners. Other agents come from the aggregate `daily` command.
pub async fn scan_ccusage_all(since: &str, until: &str, timezone: &str) -> CcusageScanResult {
    let mut warnings = Vec::new();
    let mut results = Vec::new();
    let mut verification: HashMap<String, Vec<CcusageDailyEntry>> = HashMap::new();

    // Fail soft when CLI is absent — native scanners remain authoritative.
    if find_ccusage_path().is_none() {
        let mut details = HashMap::new();
        details.insert(
            "detail".into(),
            serde_json::Value::String(
                "ccusage not installed; using native local log scanners only".into(),
            ),
        );
        details.insert(
            "hint".into(),
            serde_json::Value::String(
                "optional: install ccusage for cost verification, or set NATIVES_USAGE_ENABLE_CCUSAGE=0".into(),
            ),
        );
        warnings.push(UsageWarning {
            source_id: None,
            code: UsageWarningCode::CliNotFound,
            details,
        });
        return CcusageScanResult {
            results,
            verification,
            warnings,
        };
    }

    // 1) Per-agent verification (ccusage 20.x)
    // Claude supports -b/--breakdown; codex returns models object without that flag.
    let claude_args = [
        "claude",
        "daily",
        "--json",
        "--offline",
        "--breakdown",
        "--timezone",
        timezone,
        "--since",
        since,
        "--until",
        until,
    ];
    let codex_args = [
        "codex",
        "daily",
        "--json",
        "--offline",
        "--timezone",
        timezone,
        "--since",
        since,
        "--until",
        until,
    ];

    match run_ccusage_argv(&claude_args).await {
        Ok(resp) => {
            let mut entries = resp.daily;
            for e in &mut entries {
                e.agent = Some("claude".into());
                e.date = e.normalized_date();
                e.total_tokens = e.effective_total_tokens();
            }
            verification.insert("claude".into(), entries);
        }
        Err(e) => push_cli_error_warning(&mut warnings, Some("claude"), &e),
    }

    match run_ccusage_argv(&codex_args).await {
        Ok(resp) => {
            let mut entries = resp.daily;
            for e in &mut entries {
                e.agent = Some("codex".into());
                e.date = e.normalized_date();
                e.total_tokens = e.effective_total_tokens();
                // Promote models map into breakdowns for reconcile_source.
                if e.breakdowns.is_empty() && !e.models_map.is_empty() {
                    e.breakdowns = e.effective_breakdowns();
                }
            }
            verification.insert("codex".into(), entries);
        }
        Err(e) => push_cli_error_warning(&mut warnings, Some("codex"), &e),
    }

    // 2) Aggregate daily for other agents / overall signal
    // Do NOT pass --by-agent (removed in ccusage 20).
    // --breakdown is accepted on aggregate daily on some versions; keep it optional by retry.
    let aggregate = {
        let with_breakdown = [
            "daily",
            "--all",
            "--json",
            "--offline",
            "--breakdown",
            "--timezone",
            timezone,
            "--since",
            since,
            "--until",
            until,
        ];
        match run_ccusage_argv(&with_breakdown).await {
            Ok(r) => Ok(r),
            Err(e) if e.contains("Unknown option") && e.contains("breakdown") => {
                let without = [
                    "daily",
                    "--all",
                    "--json",
                    "--offline",
                    "--timezone",
                    timezone,
                    "--since",
                    since,
                    "--until",
                    until,
                ];
                run_ccusage_argv(&without).await
            }
            Err(e) => Err(e),
        }
    };

    match aggregate {
        Ok(resp) => {
            let mut agent_daily: HashMap<String, Vec<UsageDailyRecord>> = HashMap::new();
            for entry in resp.daily {
                let agent_name = entry
                    .agent
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
                    .to_ascii_lowercase();

                // Claude/Codex already handled via verification subcommands.
                // Aggregate rows may be agent="all" — keep as external fallback source only
                // when no dedicated verification exists, otherwise skip to avoid double count.
                if agent_name == "claude" || agent_name == "codex" {
                    continue;
                }
                if agent_name == "all" || agent_name == "unknown" {
                    // Prefer dedicated verification; skip combined rows.
                    continue;
                }

                let records = entry_to_records(&agent_name, &entry);
                agent_daily.entry(agent_name).or_default().extend(records);
            }

            for (agent_name, daily_records) in agent_daily {
                results.push(CcusageSourceResult {
                    source_id: agent_name.clone(),
                    label: capitalize_source(&agent_name),
                    daily: daily_records,
                    state: UsageSourceState::Ok,
                    breadcrumbs: vec![UsageBreadcrumb {
                        kind: BreadcrumbKind::Cli,
                        label: format!("ccusage {agent_name} daily"),
                    }],
                    warnings: vec![],
                });
            }
        }
        Err(e) => {
            // Only warn hard if both agent subcommands also failed.
            if verification.is_empty() {
                push_cli_error_warning(&mut warnings, None, &e);
            } else {
                // Soft note: aggregate failed but verification may still work.
                push_cli_error_warning(&mut warnings, Some("ccusage-daily"), &e);
            }
        }
    }

    // If verification has data but native scanners later miss it, reconcile_source
    // will fall back to ccusage entries (None, Some(cc)) branch.
    CcusageScanResult {
        results,
        verification,
        warnings,
    }
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

pub fn build_ccusage_source_states(results: &[CcusageSourceResult]) -> Vec<UsageSourceStatus> {
    results
        .iter()
        .map(|r| UsageSourceStatus {
            id: r.source_id.clone(),
            label: r.label.clone(),
            kind: UsageSourceKind::External,
            state: r.state.clone(),
            breadcrumbs: r.breadcrumbs.clone(),
            capabilities: SourceCapabilities {
                total_tokens: true,
                token_breakdown: true,
                cache: true,
                cost: true,
                hourly: false,
                project: false,
                messages: false,
                sessions: false,
                duration: false,
            },
            duration_method: None,
        })
        .collect()
}

#[cfg(test)]
#[path = "ccusage_tests.rs"]
mod ccusage_tests;
