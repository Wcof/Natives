// ── Usage Data Aggregation & Reconciliation ──
// Merges data from all sources, reconciles totals, builds final response.

use crate::usage::{
    atomcode::{atomcode_source_status, scan_atomcode_logs, AtomcodeScanResult},
    ccusage::{build_ccusage_source_states, scan_ccusage_all, CcusageDailyEntry},
    claude::{claude_source_status, scan_claude_logs, ClaudeScanResult},
    codex::{codex_source_status, scan_codex_logs, CodexScanResult},
    collect_dimensions,
    detected::detect_unmeasurable_sources,
    gemini::{gemini_root, gemini_source_status, scan_gemini_root},
    grok::{grok_root, grok_source_status, scan_grok_root},
    natives::{natives_source_status, scan_natives_db, NativesScanResult},
    now_ms,
    opencode::{opencode_db_path, opencode_source_status, scan_opencode_db},
    RtkSummary, UsageActivityBucket, UsageDailyRecord, UsageDashboardRange, UsageDashboardRequest,
    UsageDashboardResponse, UsagePeriodData, UsageQuality, UsageSessionRecord, UsageSourceState,
    UsageWarning, UsageWarningCode,
};
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use std::collections::HashMap;

/// Helper to convert milliseconds to IANA timezone ccusage YYYYMMDD date format.
fn ms_to_ccusage_date_str(ms: i64, tz: &Tz) -> String {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    let local_dt = tz
        .timestamp_opt(secs, nanos)
        .single()
        .unwrap_or_else(|| Utc.timestamp_millis_opt(ms).unwrap().with_timezone(tz));
    local_dt.format("%Y%m%d").to_string()
}

/// Helper to convert milliseconds to IANA timezone YYYY-MM-DD date format.
fn ms_to_local_date_str(ms: i64, tz: &Tz) -> String {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    let local_dt = tz
        .timestamp_opt(secs, nanos)
        .single()
        .unwrap_or_else(|| Utc.timestamp_millis_opt(ms).unwrap().with_timezone(tz));
    local_dt.format("%Y-%m-%d").to_string()
}

/// Build the complete UsageDashboardResponse for a given range (with comparison if requested).
pub async fn build_dashboard_response(req: &UsageDashboardRequest) -> UsageDashboardResponse {
    let generated_at_ms = now_ms();
    let tz: Tz = req.time_zone.parse().unwrap_or(chrono_tz::UTC);

    // 1. Determine comparison and total range
    let query_start_ms = if req.include_comparison {
        req.start_ms - (req.end_ms - req.start_ms)
    } else {
        req.start_ms
    };
    let query_end_ms = req.end_ms;

    // 2. Native-first scans (no external CLI required).
    // ccusage is optional enrichment for cost verification / extra agents.
    let start_ms = query_start_ms;
    let end_ms = query_end_ms;
    let tz_clone = tz;

    let claude_handle =
        tokio::task::spawn_blocking(move || scan_claude_logs(start_ms, end_ms, &tz_clone));

    let codex_handle =
        tokio::task::spawn_blocking(move || scan_codex_logs(start_ms, end_ms, &tz_clone));

    let tz_clone = tz;
    let atomcode_handle =
        tokio::task::spawn_blocking(move || scan_atomcode_logs(start_ms, end_ms, &tz_clone));

    let natives_handle = tokio::task::spawn_blocking(move || scan_natives_db(start_ms, end_ms));

    let opencode_handle = tokio::task::spawn_blocking(move || {
        opencode_db_path().map(|path| scan_opencode_db(&path, start_ms, end_ms, &tz_clone))
    });

    let tz_clone = tz;
    let gemini_handle = tokio::task::spawn_blocking(move || {
        gemini_root()
            .filter(|root| root.join("tmp").is_dir())
            .map(|root| scan_gemini_root(&root, start_ms, end_ms, &tz_clone))
    });

    let tz_clone = tz;
    let grok_handle = tokio::task::spawn_blocking(move || {
        grok_root()
            .filter(|root| root.exists())
            .map(|root| scan_grok_root(&root, start_ms, end_ms, &tz_clone))
    });

    // Optional external enricher. Default OFF to keep dashboard native-only.
    // Enable with NATIVES_USAGE_ENABLE_CCUSAGE=1 or settings key usage:ccusage_enabled=true.
    let since_str = ms_to_ccusage_date_str(start_ms, &tz);
    let until_str = ms_to_ccusage_date_str(end_ms - 1, &tz);
    let tz_name = req.time_zone.clone();
    let ccusage_enabled = crate::usage::ccusage_enabled();

    let (
        claude_res,
        codex_res,
        atomcode_res,
        natives_res,
        opencode_res,
        gemini_res,
        grok_res,
        ccusage_res,
    ) = if ccusage_enabled {
        let ccusage_fut = scan_ccusage_all(&since_str, &until_str, &tz_name);
        tokio::join!(
            claude_handle,
            codex_handle,
            atomcode_handle,
            natives_handle,
            opencode_handle,
            gemini_handle,
            grok_handle,
            ccusage_fut
        )
    } else {
        let empty = async {
            crate::usage::CcusageScanResult {
                results: vec![],
                verification: std::collections::HashMap::new(),
                warnings: vec![],
            }
        };
        tokio::join!(
            claude_handle,
            codex_handle,
            atomcode_handle,
            natives_handle,
            opencode_handle,
            gemini_handle,
            grok_handle,
            empty
        )
    };

    let claude_native = claude_res.unwrap_or_else(|_| ClaudeScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state: UsageSourceState::Unavailable,
        breadcrumbs: vec![],
        warnings: vec![],
    });

    let codex_native = codex_res.unwrap_or_else(|_| CodexScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state: UsageSourceState::Unavailable,
        breadcrumbs: vec![],
        warnings: vec![],
    });

    let atomcode_native = atomcode_res.unwrap_or_else(|_| AtomcodeScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state: UsageSourceState::Unavailable,
        breadcrumbs: vec![],
        warnings: vec![],
    });

    let natives_native = natives_res.unwrap_or_else(|_| NativesScanResult {
        daily: vec![],
        activity: vec![],
        sessions: vec![],
        state: UsageSourceState::Unavailable,
        breadcrumbs: vec![],
        warnings: vec![],
    });
    let opencode_native = opencode_res.ok().flatten();
    let gemini_native = gemini_res.ok().flatten();
    let grok_native = grok_res.ok().flatten();

    // 3. Reconciliation & Cost Allocation for Claude/Codex
    let mut reconciliation_warnings = Vec::new();

    let claude_daily = reconcile_source(
        "claude",
        claude_native.daily,
        ccusage_res.verification.get("claude"),
        &mut reconciliation_warnings,
    );

    let codex_daily = reconcile_source(
        "codex",
        codex_native.daily,
        ccusage_res.verification.get("codex"),
        &mut reconciliation_warnings,
    );

    // 4. Merge all daily, activity, and sessions
    let mut all_daily: Vec<UsageDailyRecord> = claude_daily;
    all_daily.extend(codex_daily);
    all_daily.extend(atomcode_native.daily);
    if let Some(result) = &opencode_native {
        all_daily.extend(result.daily.clone());
    }
    if let Some(result) = &gemini_native {
        all_daily.extend(result.daily.clone());
    }
    if let Some(result) = &grok_native {
        all_daily.extend(result.daily.clone());
    }

    for result in &ccusage_res.results {
        if !is_native_usage_source(&result.source_id) {
            all_daily.extend(result.daily.clone());
        }
    }
    all_daily.extend(natives_native.daily);

    let mut all_activity: Vec<UsageActivityBucket> = claude_native.activity;
    all_activity.extend(codex_native.activity);
    all_activity.extend(atomcode_native.activity);
    if let Some(result) = &opencode_native {
        all_activity.extend(result.activity.clone());
    }
    if let Some(result) = &gemini_native {
        all_activity.extend(result.activity.clone());
    }
    if let Some(result) = &grok_native {
        all_activity.extend(result.activity.clone());
    }
    all_activity.extend(natives_native.activity);

    let mut all_sessions: Vec<UsageSessionRecord> = claude_native.sessions;
    all_sessions.extend(codex_native.sessions);
    all_sessions.extend(atomcode_native.sessions);
    if let Some(result) = &opencode_native {
        all_sessions.extend(result.sessions.clone());
    }
    if let Some(result) = &gemini_native {
        all_sessions.extend(result.sessions.clone());
    }
    if let Some(result) = &grok_native {
        all_sessions.extend(result.sessions.clone());
    }
    all_sessions.extend(natives_native.sessions);

    // 5. Partition into Current vs Comparison periods in memory
    let local_start_date = ms_to_local_date_str(req.start_ms, &tz);
    let local_end_date = ms_to_local_date_str(req.end_ms - 1, &tz);

    // Current period filter
    let mut curr_daily = all_daily.clone();
    curr_daily.retain(|d| d.date >= local_start_date && d.date <= local_end_date);

    let mut curr_activity = all_activity.clone();
    curr_activity.retain(|a| a.hour_start_ms >= req.start_ms && a.hour_start_ms < req.end_ms);

    let mut curr_sessions = all_sessions.clone();
    curr_sessions.retain(|s| s.started_at_ms >= req.start_ms && s.started_at_ms < req.end_ms);

    // Comparison period filter (only if requested)
    let comparison_data = if req.include_comparison {
        let comp_start_ms = query_start_ms;
        let comp_end_ms = req.start_ms;
        let local_comp_start = ms_to_local_date_str(comp_start_ms, &tz);
        let local_comp_end = ms_to_local_date_str(comp_end_ms - 1, &tz);

        let mut comp_daily = all_daily.clone();
        comp_daily.retain(|d| d.date >= local_comp_start && d.date <= local_comp_end);

        let mut comp_activity = all_activity.clone();
        comp_activity.retain(|a| a.hour_start_ms >= comp_start_ms && a.hour_start_ms < comp_end_ms);

        let mut comp_sessions = all_sessions.clone();
        comp_sessions.retain(|s| s.started_at_ms >= comp_start_ms && s.started_at_ms < comp_end_ms);

        Some(UsagePeriodData {
            range: UsageDashboardRange {
                start_ms: comp_start_ms,
                end_ms: comp_end_ms,
            },
            daily: comp_daily,
            activity: comp_activity,
            sessions: comp_sessions,
        })
    } else {
        None
    };

    // 6. Build source statuses (only show sources actually detected/configured)
    let mut sources = vec![
        claude_source_status(&claude_native.state, &claude_native.breadcrumbs),
        codex_source_status(&codex_native.state, &codex_native.breadcrumbs),
        atomcode_source_status(&atomcode_native.state, &atomcode_native.breadcrumbs),
        natives_source_status(&natives_native.state),
    ];
    if let Some(result) = &opencode_native {
        sources.push(opencode_source_status(result));
    }
    if let Some(result) = &gemini_native {
        sources.push(gemini_source_status(result));
    }
    if let Some(result) = &grok_native {
        sources.push(grok_source_status(result));
    }
    sources.extend(detect_unmeasurable_sources());

    // Add ccusage active sources dynamically
    let external_ccusage_results: Vec<_> = ccusage_res
        .results
        .iter()
        .filter(|result| !is_native_usage_source(&result.source_id))
        .cloned()
        .collect();
    let ccs_states = build_ccusage_source_states(&external_ccusage_results);
    sources.extend(ccs_states);

    // 7. Collect warnings
    let mut all_warnings = Vec::new();
    all_warnings.extend(claude_native.warnings);
    all_warnings.extend(codex_native.warnings);
    all_warnings.extend(atomcode_native.warnings);
    all_warnings.extend(natives_native.warnings);
    if let Some(result) = opencode_native {
        all_warnings.extend(result.warnings);
    }
    if let Some(result) = gemini_native {
        all_warnings.extend(result.warnings);
    }
    if let Some(result) = grok_native {
        all_warnings.extend(result.warnings);
    }
    all_warnings.extend(ccusage_res.warnings);
    all_warnings.extend(reconciliation_warnings);

    // 8. Dimensions collection (based on current period)
    let dimensions = collect_dimensions(&curr_daily, &curr_activity, &curr_sessions);

    let rtk = read_rtk_summary();

    UsageDashboardResponse {
        generated_at_ms,
        range: UsageDashboardRange {
            start_ms: req.start_ms,
            end_ms: req.end_ms,
        },
        daily: curr_daily,
        activity: curr_activity,
        sessions: curr_sessions,
        comparison: comparison_data,
        dimensions,
        sources,
        rtk,
        warnings: all_warnings,
    }
}

/// Claude and Codex are already parsed from their local event logs. ccusage
/// may report the same tools, but must only validate/enrich them, never add a
/// second set of usage rows.
fn is_native_usage_source(source_id: &str) -> bool {
    matches!(
        source_id.to_ascii_lowercase().as_str(),
        "claude" | "codex" | "gemini" | "opencode"
    )
}

/// Reconciliation and cost allocation function for a specific source.
fn reconcile_source(
    source_id: &str,
    native_records: Vec<UsageDailyRecord>,
    cc_entries: Option<&Vec<CcusageDailyEntry>>,
    reconciliation_warnings: &mut Vec<UsageWarning>,
) -> Vec<UsageDailyRecord> {
    if cc_entries.is_none() || cc_entries.unwrap().is_empty() {
        return native_records;
    }

    let cc_entries = cc_entries.unwrap();

    // Group native records by date
    let mut native_groups: HashMap<String, Vec<UsageDailyRecord>> = HashMap::new();
    for r in native_records {
        native_groups.entry(r.date.clone()).or_default().push(r);
    }

    // Group ccusage entries by date
    let mut cc_groups: HashMap<String, &CcusageDailyEntry> = HashMap::new();
    for entry in cc_entries {
        cc_groups.insert(entry.date.clone(), entry);
    }

    let mut reconciled_records = Vec::new();

    // Reconcile dates present in ccusage or native
    let mut all_dates: std::collections::HashSet<String> = native_groups.keys().cloned().collect();
    for date in cc_groups.keys() {
        all_dates.insert(date.clone());
    }

    for date in all_dates {
        let native_list = native_groups.get_mut(&date);
        let cc_entry = cc_groups.get(&date);

        match (native_list, cc_entry) {
            (Some(natives), Some(cc)) => {
                let native_total: i64 = natives.iter().map(|r| r.total_tokens.unwrap_or(0)).sum();
                let cc_total = cc.total_tokens;

                let diff = (cc_total - native_total).abs();
                let threshold = std::cmp::max(10, (native_total as f64 * 0.01) as i64);

                if diff > threshold {
                    // Total mismatch - generate warning and do not assign cost
                    reconciliation_warnings.push(UsageWarning {
                        source_id: Some(source_id.to_string()),
                        code: UsageWarningCode::TotalMismatch,
                        details: {
                            let mut m = HashMap::new();
                            m.insert("date".into(), serde_json::Value::String(date.clone()));
                            m.insert(
                                "ccusage_total".into(),
                                serde_json::Value::Number(cc_total.into()),
                            );
                            m.insert(
                                "scanned_total".into(),
                                serde_json::Value::Number(native_total.into()),
                            );
                            m
                        },
                    });
                    // Push unmodified native records
                    reconciled_records.append(natives);
                } else {
                    // Match - allocate cost
                    if let Some(total_cost) = cc.cost {
                        if !cc.breakdowns.is_empty() {
                            // Try model-level breakdowns
                            for r in natives.iter_mut() {
                                if let Some(ref model) = r.model_id {
                                    if let Some(bd) =
                                        cc.breakdowns.iter().find(|b| &b.model == model)
                                    {
                                        if bd.total_tokens > 0 {
                                            let ratio = r.total_tokens.unwrap_or(0) as f64
                                                / bd.total_tokens as f64;
                                            if let Some(bd_cost) = bd.cost {
                                                r.cost_usd = Some(bd_cost * ratio);
                                                r.cost_quality = if (ratio - 1.0).abs() < 0.001 {
                                                    UsageQuality::Reported
                                                } else {
                                                    UsageQuality::Estimated
                                                };
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // For records that still don't have cost, apportion from remaining day-level cost
                        let allocated_sum: f64 =
                            natives.iter().map(|r| r.cost_usd.unwrap_or(0.0)).sum();
                        let remaining_cost = (total_cost - allocated_sum).max(0.0);

                        let unallocated_tokens: i64 = natives
                            .iter()
                            .filter(|r| r.cost_usd.is_none())
                            .map(|r| r.total_tokens.unwrap_or(0))
                            .sum();

                        if unallocated_tokens > 0 && remaining_cost > 0.0 {
                            for r in natives.iter_mut() {
                                if r.cost_usd.is_none() {
                                    let ratio = r.total_tokens.unwrap_or(0) as f64
                                        / unallocated_tokens as f64;
                                    r.cost_usd = Some(remaining_cost * ratio);
                                    r.cost_quality = UsageQuality::Estimated;
                                }
                            }
                        }
                    }
                    reconciled_records.append(natives);
                }
            }
            (Some(natives), None) => {
                // ccusage has no data, just push native records
                reconciled_records.append(natives);
            }
            (None, Some(cc)) => {
                // Native scanner is empty but ccusage has daily record - use ccusage as fallback
                let (cost_usd, cost_quality) = if let Some(c) = cc.cost {
                    if c == 0.0 && cc.total_tokens > 0 {
                        (None, UsageQuality::Unavailable)
                    } else {
                        (Some(c), UsageQuality::Reported)
                    }
                } else {
                    (None, UsageQuality::Unavailable)
                };

                if cc.breakdowns.is_empty() {
                    reconciled_records.push(UsageDailyRecord {
                        date: date.clone(),
                        source_id: source_id.to_string(),
                        model_id: None,
                        project_id: None,
                        terminal_id: None,
                        input_tokens: Some(cc.input_tokens),
                        output_tokens: Some(cc.output_tokens),
                        cache_creation_tokens: Some(cc.cache_creation_tokens),
                        cache_read_tokens: Some(cc.cache_read_tokens),
                        total_tokens: Some(cc.total_tokens),
                        cost_usd,
                        cost_quality,
                    });
                } else {
                    for bd in &cc.breakdowns {
                        let bd_cost = bd.cost.unwrap_or(0.0);
                        let (bd_cost_usd, bd_cost_quality) =
                            if bd_cost == 0.0 && bd.total_tokens > 0 {
                                (None, UsageQuality::Unavailable)
                            } else {
                                (Some(bd_cost), UsageQuality::Reported)
                            };
                        reconciled_records.push(UsageDailyRecord {
                            date: date.clone(),
                            source_id: source_id.to_string(),
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
                        });
                    }
                }
            }
            (None, None) => {}
        }
    }

    reconciled_records
}

/// Read RTK usage from files (real implementation).
fn read_rtk_summary() -> Option<RtkSummary> {
    let rtk_dir = dirs::home_dir()?.join(".rtk");
    let stats_file = rtk_dir.join("stats.json");
    let content = std::fs::read_to_string(stats_file).ok()?;

    #[derive(serde::Deserialize)]
    struct RtkStats {
        #[serde(default)]
        total_tokens_saved: i64,
        #[serde(default)]
        total_commands: i64,
    }

    let stats: RtkStats = serde_json::from_str(&content).ok()?;
    Some(RtkSummary {
        total_saved_tokens: stats.total_tokens_saved,
        total_commands: stats.total_commands,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_sources_are_not_added_again_from_ccusage() {
        assert!(is_native_usage_source("claude"));
        assert!(is_native_usage_source("codex"));
        assert!(is_native_usage_source("gemini"));
        assert!(is_native_usage_source("opencode"));
    }
}
