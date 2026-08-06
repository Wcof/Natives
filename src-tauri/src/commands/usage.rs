// ── Tauri Commands: usage_get_cached & usage_sync ──
// Replaces the old usage_refresh command.
// usage_get_cached: read-only snapshot read (no scanning)
// usage_sync: manual sync (runs all scanners, writes snapshot)

use crate::usage::snapshot::{
    self, UsageCacheMetadata, UsageCacheReadResult, UsageDashboardSnapshot,
};
use crate::usage::{
    self, build_dashboard_response, UsageDashboardRange, UsageDashboardRequest,
    UsageDashboardResponse, UsagePeriodData,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

/// ── New IPC Contract Types ──
/// Preset for the requested time range.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageRangePreset {
    Today,
    #[serde(rename = "24h")]
    TwentyFourHour,
    #[serde(rename = "7d")]
    SevenDay,
    #[serde(rename = "30d")]
    ThirtyDay,
    #[serde(rename = "90d")]
    NinetyDay,
    Custom,
}

/// Request for reading a cached dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageViewRequest {
    pub preset: UsageRangePreset,
    pub time_zone: String,
    pub project_path: Option<String>,
    pub custom_start_ms: Option<i64>,
    pub custom_end_ms: Option<i64>,
}

/// Request for syncing a fresh dashboard snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSyncRequest {
    pub time_zone: String,
    pub current_view: UsageViewRequest,
}

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSyncResult {
    pub metadata: UsageCacheMetadata,
    pub response: UsageDashboardResponse,
}

/// Read cached dashboard data. Never scans logs or runs ccusage.
#[tauri::command]
pub async fn usage_get_cached(
    state: State<'_, AppState>,
    query: UsageViewRequest,
) -> Result<UsageCacheReadResult> {
    // Validate timezone
    let _: chrono_tz::Tz = query
        .time_zone
        .parse()
        .map_err(|_| Error::InvalidInput("Invalid IANA timeZone".into()))?;

    // Try memory cache first
    let tz = &query.time_zone;
    if let Some(cached) = state.usage_cache.get_snapshot(tz) {
        let response = snapshot::slice_response_with_custom(
            &cached,
            preset_to_str(&query.preset),
            usage::now_ms(),
            tz,
            query.project_path.as_deref(),
            query.custom_start_ms,
            query.custom_end_ms,
        );
        let metadata = UsageCacheMetadata {
            schema_version: cached.schema_version,
            generated_at_ms: cached.generated_at_ms,
            coverage_start_ms: cached.coverage_start_ms,
            coverage_end_ms: cached.coverage_end_ms,
            time_zone: cached.time_zone.clone(),
        };
        return Ok(UsageCacheReadResult::Ready { metadata, response });
    }

    // Try SQLite cache
    match snapshot::read_snapshot(tz) {
        Ok(Some(snapshot_data)) => {
            // Cache in memory for this session
            state.usage_cache.set_snapshot(tz, snapshot_data.clone());

            let response = snapshot::slice_response_with_custom(
                &snapshot_data,
                preset_to_str(&query.preset),
                usage::now_ms(),
                tz,
                query.project_path.as_deref(),
                query.custom_start_ms,
                query.custom_end_ms,
            );
            let metadata = UsageCacheMetadata {
                schema_version: snapshot_data.schema_version,
                generated_at_ms: snapshot_data.generated_at_ms,
                coverage_start_ms: snapshot_data.coverage_start_ms,
                coverage_end_ms: snapshot_data.coverage_end_ms,
                time_zone: snapshot_data.time_zone.clone(),
            };
            Ok(UsageCacheReadResult::Ready { metadata, response })
        }
        Ok(None) => Ok(UsageCacheReadResult::Missing {
            metadata: None,
            response: None,
        }),
        Err(e) => {
            // Corrupt snapshot — treat as missing
            eprintln!("Corrupt snapshot for {tz}: {e}");
            Ok(UsageCacheReadResult::Missing {
                metadata: None,
                response: None,
            })
        }
    }
}

/// Manually sync dashboard data. Runs all scanners, builds snapshot, persists to SQLite.
#[tauri::command]
pub async fn usage_sync(
    state: State<'_, AppState>,
    request: UsageSyncRequest,
) -> Result<UsageSyncResult> {
    // Validate timezone
    let _: chrono_tz::Tz = request
        .time_zone
        .parse()
        .map_err(|_| Error::InvalidInput("Invalid IANA timeZone".into()))?;

    let generated_at_ms = usage::now_ms();
    let tz_str = &request.time_zone;

    // ── 1. Calendar collection: 90 current + 90 comparison = 180 days ──
    let calendar_end_ms = generated_at_ms;
    let calendar_start_ms = generated_at_ms - 180 * 86_400_000;

    let calendar_req = UsageDashboardRequest {
        start_ms: calendar_start_ms,
        end_ms: calendar_end_ms,
        include_comparison: false,
        time_zone: tz_str.clone(),
    };

    let calendar_response = build_dashboard_response(&calendar_req).await;

    // ── 2. 24h collection: 24h current + 24h comparison = 48h ──
    let rolling_end_ms = generated_at_ms;
    let rolling_start_ms = generated_at_ms - 48 * 3600 * 1000;

    let rolling_24h_req = UsageDashboardRequest {
        start_ms: rolling_start_ms,
        end_ms: rolling_end_ms,
        include_comparison: false,
        time_zone: tz_str.clone(),
    };

    let rolling_24h_response = build_dashboard_response(&rolling_24h_req).await;

    // Separate current 24h from comparison 24h
    let current_24h_start = generated_at_ms - 24 * 3600 * 1000;
    let comp_24h_end = current_24h_start;

    let rolling_24h_daily: Vec<_> = rolling_24h_response
        .daily
        .iter()
        .filter(|r| {
            let date_ms = date_to_ms(&r.date);
            date_ms >= current_24h_start && date_ms < rolling_end_ms
        })
        .cloned()
        .collect();
    let rolling_24h_activity: Vec<_> = rolling_24h_response
        .activity
        .iter()
        .filter(|a| a.hour_start_ms >= current_24h_start && a.hour_start_ms < rolling_end_ms)
        .cloned()
        .collect();
    let rolling_24h_sessions: Vec<_> = rolling_24h_response
        .sessions
        .iter()
        .filter(|s| s.started_at_ms >= current_24h_start && s.started_at_ms < rolling_end_ms)
        .cloned()
        .collect();

    let comp_24h_daily: Vec<_> = rolling_24h_response
        .daily
        .iter()
        .filter(|r| {
            let date_ms = date_to_ms(&r.date);
            date_ms >= rolling_start_ms && date_ms < comp_24h_end
        })
        .cloned()
        .collect();
    let comp_24h_activity: Vec<_> = rolling_24h_response
        .activity
        .iter()
        .filter(|a| a.hour_start_ms >= rolling_start_ms && a.hour_start_ms < comp_24h_end)
        .cloned()
        .collect();
    let comp_24h_sessions: Vec<_> = rolling_24h_response
        .sessions
        .iter()
        .filter(|s| s.started_at_ms >= rolling_start_ms && s.started_at_ms < comp_24h_end)
        .cloned()
        .collect();

    // ── 3. Build snapshot ──
    let snapshot = UsageDashboardSnapshot {
        schema_version: snapshot::SNAPSHOT_SCHEMA_VERSION,
        generated_at_ms,
        coverage_start_ms: calendar_start_ms,
        coverage_end_ms: calendar_end_ms,
        time_zone: tz_str.clone(),
        calendar: UsagePeriodData {
            range: UsageDashboardRange {
                start_ms: calendar_start_ms,
                end_ms: calendar_end_ms,
            },
            daily: calendar_response.daily,
            activity: calendar_response.activity,
            sessions: calendar_response.sessions,
        },
        rolling_24h: UsagePeriodData {
            range: UsageDashboardRange {
                start_ms: current_24h_start,
                end_ms: rolling_end_ms,
            },
            daily: rolling_24h_daily,
            activity: rolling_24h_activity,
            sessions: rolling_24h_sessions,
        },
        rolling_24h_comparison: UsagePeriodData {
            range: UsageDashboardRange {
                start_ms: rolling_start_ms,
                end_ms: comp_24h_end,
            },
            daily: comp_24h_daily,
            activity: comp_24h_activity,
            sessions: comp_24h_sessions,
        },
        sources: calendar_response.sources,
        warnings: calendar_response.warnings,
        rtk: calendar_response.rtk,
    };

    // ── 4. Persist to SQLite ──
    snapshot::write_snapshot(&snapshot).map_err(Error::from)?;

    // ── 5. Update memory cache ──
    state.usage_cache.set_snapshot(tz_str, snapshot.clone());

    // ── 6. Build response for the current view ──
    let response = snapshot::slice_response_with_custom(
        &snapshot,
        preset_to_str(&request.current_view.preset),
        generated_at_ms,
        tz_str,
        request.current_view.project_path.as_deref(),
        request.current_view.custom_start_ms,
        request.current_view.custom_end_ms,
    );

    let metadata = UsageCacheMetadata {
        schema_version: snapshot.schema_version,
        generated_at_ms,
        coverage_start_ms: calendar_start_ms,
        coverage_end_ms: calendar_end_ms,
        time_zone: tz_str.clone(),
    };

    Ok(UsageSyncResult { metadata, response })
}

/// Whether optional ccusage enrichment is enabled (default false).
#[tauri::command]
pub fn usage_get_ccusage_enabled() -> Result<bool> {
    Ok(crate::usage::ccusage_enabled())
}

/// Enable/disable optional ccusage enrichment. Does not install the binary.
#[tauri::command]
pub fn usage_set_ccusage_enabled(enabled: bool) -> Result<bool> {
    crate::usage::set_ccusage_enabled(enabled).map_err(Error::from)?;
    Ok(crate::usage::ccusage_enabled())
}

/// Detect whether the ccusage binary is available (independent of enable flag).
#[tauri::command]
pub fn usage_detect_ccusage() -> Result<Option<String>> {
    // Reuse plugin detect path semantics without requiring plugin UI wiring.
    crate::commands::plugins::plugin_detect("ccusage".into())
}

/// Convert a UsageRangePreset to its string representation.
fn preset_to_str(preset: &UsageRangePreset) -> &'static str {
    match preset {
        UsageRangePreset::Today => "today",
        UsageRangePreset::TwentyFourHour => "24h",
        UsageRangePreset::SevenDay => "7d",
        UsageRangePreset::ThirtyDay => "30d",
        UsageRangePreset::NinetyDay => "90d",
        UsageRangePreset::Custom => "custom",
    }
}

/// Convert a YYYY-MM-DD date string to milliseconds since epoch.
fn date_to_ms(date: &str) -> i64 {
    if let Ok(dt) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        dt.and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_to_str() {
        assert_eq!(preset_to_str(&UsageRangePreset::Today), "today");
        assert_eq!(preset_to_str(&UsageRangePreset::TwentyFourHour), "24h");
        assert_eq!(preset_to_str(&UsageRangePreset::SevenDay), "7d");
        assert_eq!(preset_to_str(&UsageRangePreset::ThirtyDay), "30d");
        assert_eq!(preset_to_str(&UsageRangePreset::NinetyDay), "90d");
        assert_eq!(preset_to_str(&UsageRangePreset::Custom), "custom");
    }

    #[test]
    fn test_date_to_ms() {
        let ms = date_to_ms("2026-01-15");
        assert_eq!(ms, 1768435200000);
    }
}
