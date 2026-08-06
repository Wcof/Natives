// ── Usage Dashboard Snapshot Cache ──
// Per-timezone SQLite-backed cache for dashboard snapshots.
// The snapshot is built by usage_sync and read by usage_get_cached.

use crate::db;
use crate::usage::{
    RtkSummary, UsageDashboardResponse, UsagePeriodData, UsageSourceStatus, UsageWarning,
};
use chrono::{Datelike, TimeZone};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

/// Schema version for snapshot serialization.
/// Increment when the payload structure changes.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 6;

/// Internal snapshot structure stored in SQLite as JSON.
/// Not directly exposed to the frontend — the frontend gets a sliced
/// UsageDashboardResponse instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboardSnapshot {
    pub schema_version: u32,
    pub generated_at_ms: i64,
    pub coverage_start_ms: i64,
    pub coverage_end_ms: i64,
    pub time_zone: String,

    pub calendar: UsagePeriodData,
    pub rolling_24h: UsagePeriodData,
    pub rolling_24h_comparison: UsagePeriodData,

    pub sources: Vec<UsageSourceStatus>,
    pub warnings: Vec<UsageWarning>,
    pub rtk: Option<RtkSummary>,
}

/// Result of reading a snapshot from cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UsageCacheReadResult {
    Ready {
        metadata: UsageCacheMetadata,
        response: UsageDashboardResponse,
    },
    Missing {
        metadata: Option<UsageCacheMetadata>,
        response: Option<UsageDashboardResponse>,
    },
}

/// Metadata about a cached snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCacheMetadata {
    pub schema_version: u32,
    pub generated_at_ms: i64,
    pub coverage_start_ms: i64,
    pub coverage_end_ms: i64,
    pub time_zone: String,
}

/// Read a snapshot from SQLite by timezone.
pub fn read_snapshot(time_zone: &str) -> Result<Option<UsageDashboardSnapshot>, String> {
    let conn = db::get_main_conn().map_err(|e| format!("DB connection failed: {e}"))?;

    let result: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM usage_dashboard_snapshots WHERE time_zone = ?1",
            [time_zone],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| format!("Snapshot read failed: {e}"))?;

    match result {
        Some(json) => {
            let snapshot: UsageDashboardSnapshot = serde_json::from_str(&json)
                .map_err(|e| format!("Snapshot deserialization failed: {e}"))?;

            if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
                return Ok(None); // Schema version mismatch — treat as missing
            }

            Ok(Some(snapshot))
        }
        None => Ok(None),
    }
}

/// Write a snapshot to SQLite (transactional upsert by timezone).
/// Serialization happens before the transaction, so a serialization failure
/// never touches the old snapshot.
pub fn write_snapshot(snapshot: &UsageDashboardSnapshot) -> Result<(), String> {
    let conn = db::get_main_conn().map_err(|e| format!("DB connection failed: {e}"))?;

    let payload_json = serde_json::to_string(snapshot)
        .map_err(|e| format!("Snapshot serialization failed: {e}"))?;

    let now = chrono::Utc::now().to_rfc3339();

    // Use an explicit transaction so a partial write never corrupts the cache.
    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(|e| format!("Failed to begin transaction: {e}"))?;

    let result = conn.execute(
        "INSERT INTO usage_dashboard_snapshots
         (time_zone, schema_version, generated_at_ms, coverage_start_ms, coverage_end_ms, payload_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(time_zone) DO UPDATE SET
           schema_version = excluded.schema_version,
           generated_at_ms = excluded.generated_at_ms,
           coverage_start_ms = excluded.coverage_start_ms,
           coverage_end_ms = excluded.coverage_end_ms,
           payload_json = excluded.payload_json,
           updated_at = excluded.updated_at",
        rusqlite::params![
            snapshot.time_zone,
            snapshot.schema_version as i64,
            snapshot.generated_at_ms,
            snapshot.coverage_start_ms,
            snapshot.coverage_end_ms,
            payload_json,
            now,
        ],
    );

    match result {
        Ok(_) => {
            conn.execute_batch("COMMIT")
                .map_err(|e| format!("Failed to commit transaction: {e}"))?;
            Ok(())
        }
        Err(e) => {
            // Rollback on failure — old snapshot is preserved.
            let _ = conn.execute_batch("ROLLBACK");
            Err(format!("Snapshot write failed: {e}"))
        }
    }
}

/// Delete a snapshot by timezone (for testing / cleanup).
#[allow(dead_code)]
pub fn delete_snapshot(time_zone: &str) -> Result<(), String> {
    let conn = db::get_main_conn().map_err(|e| format!("DB connection failed: {e}"))?;
    conn.execute(
        "DELETE FROM usage_dashboard_snapshots WHERE time_zone = ?1",
        [time_zone],
    )
    .map_err(|e| format!("Snapshot delete failed: {e}"))?;
    Ok(())
}

/// Build a UsageDashboardResponse from a snapshot for a given preset and optional project filter.
/// The snapshot contains the full 180-day calendar; we slice it to the requested range.
pub fn slice_response(
    snapshot: &UsageDashboardSnapshot,
    preset: &str,
    now_ms: i64,
    time_zone: &str,
    project_path: Option<&str>,
) -> UsageDashboardResponse {
    slice_response_with_custom(
        snapshot,
        preset,
        now_ms,
        time_zone,
        project_path,
        None,
        None,
    )
}

/// Same as [`slice_response`], but accepts an optional custom range for the `custom` preset.
pub fn slice_response_with_custom(
    snapshot: &UsageDashboardSnapshot,
    preset: &str,
    now_ms: i64,
    time_zone: &str,
    project_path: Option<&str>,
    custom_start_ms: Option<i64>,
    custom_end_ms: Option<i64>,
) -> UsageDashboardResponse {
    use crate::usage::UsageDashboardRange;

    let (start_ms, end_ms) = compute_range(
        preset,
        now_ms,
        time_zone,
        snapshot,
        custom_start_ms,
        custom_end_ms,
    );

    // Filter daily records to the range
    let daily: Vec<_> = snapshot
        .calendar
        .daily
        .iter()
        .filter(|r| {
            // Simple date-based filtering: records have date strings YYYY-MM-DD
            // We convert to timestamp for comparison
            let date_ms = date_to_ms(&r.date);
            date_ms >= start_ms && date_ms < end_ms
        })
        .filter(|r| {
            // Apply project filter
            project_path.is_none_or(|proj| {
                r.project_id
                    .as_ref()
                    .is_some_and(|pid| is_project_match(pid, proj))
            })
        })
        .cloned()
        .collect();

    // Filter activity to the range
    let activity: Vec<_> = snapshot
        .calendar
        .activity
        .iter()
        .filter(|a| a.hour_start_ms >= start_ms && a.hour_start_ms < end_ms)
        .filter(|a| {
            project_path.is_none_or(|proj| {
                a.project_id
                    .as_ref()
                    .is_some_and(|pid| is_project_match(pid, proj))
            })
        })
        .cloned()
        .collect();

    // Filter sessions to the range
    let sessions: Vec<_> = snapshot
        .calendar
        .sessions
        .iter()
        .filter(|s| s.started_at_ms >= start_ms && s.started_at_ms < end_ms)
        .filter(|s| {
            project_path.is_none_or(|proj| {
                s.project_id
                    .as_ref()
                    .is_some_and(|pid| is_project_match(pid, proj))
            })
        })
        .cloned()
        .collect();

    // Build comparison data
    let comparison = build_comparison(
        snapshot,
        preset,
        now_ms,
        time_zone,
        project_path,
        custom_start_ms,
        custom_end_ms,
    );

    // Handle 24h preset specially — use rolling_24h data
    let (daily, activity, sessions) = if preset == "24h" {
        let rolling_daily: Vec<_> = snapshot
            .rolling_24h
            .daily
            .iter()
            .filter(|r| {
                project_path.is_none_or(|proj| {
                    r.project_id
                        .as_ref()
                        .is_some_and(|pid| is_project_match(pid, proj))
                })
            })
            .cloned()
            .collect();
        let rolling_activity: Vec<_> = snapshot
            .rolling_24h
            .activity
            .iter()
            .filter(|a| {
                project_path.is_none_or(|proj| {
                    a.project_id
                        .as_ref()
                        .is_some_and(|pid| is_project_match(pid, proj))
                })
            })
            .cloned()
            .collect();
        let rolling_sessions: Vec<_> = snapshot
            .rolling_24h
            .sessions
            .iter()
            .filter(|s| {
                project_path.is_none_or(|proj| {
                    s.project_id
                        .as_ref()
                        .is_some_and(|pid| is_project_match(pid, proj))
                })
            })
            .cloned()
            .collect();
        (rolling_daily, rolling_activity, rolling_sessions)
    } else {
        (daily, activity, sessions)
    };

    // Build dimensions from the FULL snapshot (not filtered by project)
    // so that project dropdown options are never lost after selecting one.
    let dimensions = crate::usage::collect_dimensions(
        &snapshot.calendar.daily,
        &snapshot.calendar.activity,
        &snapshot.calendar.sessions,
    );

    UsageDashboardResponse {
        generated_at_ms: snapshot.generated_at_ms,
        range: UsageDashboardRange { start_ms, end_ms },
        daily,
        activity,
        sessions,
        comparison,
        dimensions,
        sources: snapshot.sources.clone(),
        rtk: snapshot.rtk.clone(),
        warnings: snapshot.warnings.clone(),
    }
}

/// Compute the request range for a given preset.
///
/// For `custom`, uses `custom_start_ms`/`custom_end_ms` when both are valid
/// (`end > start`). Otherwise falls back to the snapshot coverage window.
fn compute_range(
    preset: &str,
    now_ms: i64,
    time_zone: &str,
    snapshot: &UsageDashboardSnapshot,
    custom_start_ms: Option<i64>,
    custom_end_ms: Option<i64>,
) -> (i64, i64) {
    let day_start_ms = {
        let tz: chrono_tz::Tz = time_zone.parse().unwrap_or(chrono_tz::UTC);
        let secs = now_ms / 1000;
        let nanos = ((now_ms % 1000) * 1_000_000) as u32;
        let dt = chrono::Utc.timestamp_opt(secs, nanos).unwrap();
        let local_dt = dt.with_timezone(&tz);
        let local_midnight =
            chrono::NaiveDate::from_ymd_opt(local_dt.year(), local_dt.month(), local_dt.day())
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap();
        // Convert local midnight back to UTC milliseconds
        let local_midnight_utc = tz.from_local_datetime(&local_midnight).unwrap();
        local_midnight_utc.timestamp_millis()
    };

    match preset {
        "today" => (day_start_ms, now_ms),
        "24h" => (now_ms - 24 * 3600 * 1000, now_ms),
        "7d" => (day_start_ms - 6 * 86_400_000, now_ms),
        "30d" => (day_start_ms - 29 * 86_400_000, now_ms),
        "90d" => (day_start_ms - 89 * 86_400_000, now_ms),
        "custom" => match (custom_start_ms, custom_end_ms) {
            (Some(start), Some(end)) if end > start => (start, end),
            _ => (snapshot.coverage_start_ms, snapshot.coverage_end_ms),
        },
        _ => (snapshot.coverage_start_ms, snapshot.coverage_end_ms),
    }
}

/// Build comparison data for the same range immediately before the current range.
fn build_comparison(
    snapshot: &UsageDashboardSnapshot,
    preset: &str,
    now_ms: i64,
    time_zone: &str,
    project_path: Option<&str>,
    custom_start_ms: Option<i64>,
    custom_end_ms: Option<i64>,
) -> Option<UsagePeriodData> {
    let (current_start_ms, current_end_ms) = compute_range(
        preset,
        now_ms,
        time_zone,
        snapshot,
        custom_start_ms,
        custom_end_ms,
    );
    let span = current_end_ms - current_start_ms;
    let comp_start_ms = current_start_ms - span;
    let comp_end_ms = current_start_ms;

    // For 24h, use the dedicated rolling_24h_comparison
    if preset == "24h" {
        let comp_daily: Vec<_> = snapshot
            .rolling_24h_comparison
            .daily
            .iter()
            .filter(|r| {
                project_path.is_none_or(|proj| {
                    r.project_id
                        .as_ref()
                        .is_some_and(|pid| is_project_match(pid, proj))
                })
            })
            .cloned()
            .collect();
        let comp_activity: Vec<_> = snapshot
            .rolling_24h_comparison
            .activity
            .iter()
            .filter(|a| {
                project_path.is_none_or(|proj| {
                    a.project_id
                        .as_ref()
                        .is_some_and(|pid| is_project_match(pid, proj))
                })
            })
            .cloned()
            .collect();
        let comp_sessions: Vec<_> = snapshot
            .rolling_24h_comparison
            .sessions
            .iter()
            .filter(|s| {
                project_path.is_none_or(|proj| {
                    s.project_id
                        .as_ref()
                        .is_some_and(|pid| is_project_match(pid, proj))
                })
            })
            .cloned()
            .collect();
        return Some(UsagePeriodData {
            range: crate::usage::UsageDashboardRange {
                start_ms: comp_start_ms,
                end_ms: comp_end_ms,
            },
            daily: comp_daily,
            activity: comp_activity,
            sessions: comp_sessions,
        });
    }

    // For calendar-based presets, slice from snapshot.calendar
    let comp_daily: Vec<_> = snapshot
        .calendar
        .daily
        .iter()
        .filter(|r| {
            let date_ms = date_to_ms(&r.date);
            date_ms >= comp_start_ms && date_ms < comp_end_ms
        })
        .filter(|r| {
            project_path.is_none_or(|proj| {
                r.project_id
                    .as_ref()
                    .is_some_and(|pid| is_project_match(pid, proj))
            })
        })
        .cloned()
        .collect();
    let comp_activity: Vec<_> = snapshot
        .calendar
        .activity
        .iter()
        .filter(|a| a.hour_start_ms >= comp_start_ms && a.hour_start_ms < comp_end_ms)
        .filter(|a| {
            project_path.is_none_or(|proj| {
                a.project_id
                    .as_ref()
                    .is_some_and(|pid| is_project_match(pid, proj))
            })
        })
        .cloned()
        .collect();
    let comp_sessions: Vec<_> = snapshot
        .calendar
        .sessions
        .iter()
        .filter(|s| s.started_at_ms >= comp_start_ms && s.started_at_ms < comp_end_ms)
        .filter(|s| {
            project_path.is_none_or(|proj| {
                s.project_id
                    .as_ref()
                    .is_some_and(|pid| is_project_match(pid, proj))
            })
        })
        .cloned()
        .collect();

    Some(UsagePeriodData {
        range: crate::usage::UsageDashboardRange {
            start_ms: comp_start_ms,
            end_ms: comp_end_ms,
        },
        daily: comp_daily,
        activity: comp_activity,
        sessions: comp_sessions,
    })
}

/// Check if a project_id matches a selected project path.
/// The project_id is a normalized absolute path; the selected path is also normalized.
/// Returns true if the project_id equals the selected path, or is a subdirectory of it.
fn is_project_match(project_id: &str, selected_path: &str) -> bool {
    if selected_path.is_empty() {
        return true; // "all projects"
    }
    // Normalize both paths
    let pid = std::path::Path::new(project_id);
    let sel = std::path::Path::new(selected_path);
    if pid == sel {
        return true;
    }
    // Check if pid is a subdirectory of sel
    if let Ok(rel) = pid.strip_prefix(sel) {
        // Ensure it's a real subdirectory, not a false prefix match
        // Path::strip_prefix handles component-level matching, so "/foo/bar" won't match "/foo/barley"
        return rel.components().count() > 0;
    }
    false
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
    fn cache_miss_serializes_to_the_frontend_discriminant() {
        let value = serde_json::to_value(UsageCacheReadResult::Missing {
            metadata: None,
            response: None,
        })
        .expect("serialize cache miss");

        assert_eq!(value["state"], "missing");
        assert!(value.get("Missing").is_none());
    }

    #[test]
    fn test_project_match_exact() {
        assert!(is_project_match("/home/user/proj", "/home/user/proj"));
    }

    #[test]
    fn test_project_match_subdir() {
        assert!(is_project_match("/home/user/proj/src", "/home/user/proj"));
    }

    #[test]
    fn test_project_match_no_false_prefix() {
        // "/foo/bar" should NOT match "/foo/barley"
        assert!(!is_project_match("/foo/barley", "/foo/bar"));
    }

    #[test]
    fn test_project_match_empty_selects_all() {
        assert!(is_project_match("/some/path", ""));
    }

    #[test]
    fn test_project_match_null_project_excluded() {
        // A null project_id should not match any non-empty selected path
        assert!(!is_project_match("", "/some/path"));
    }

    #[test]
    fn test_date_to_ms() {
        let ms = date_to_ms("2026-01-15");
        assert!(ms > 0);
        // 2026-01-15 00:00:00 UTC = 1768435200000ms from epoch
        assert_eq!(ms, 1768435200000);
    }

    #[test]
    fn test_compute_range_today() {
        let now = 1767571200000; // 2026-01-05 00:00:00 UTC
        let snapshot = UsageDashboardSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generated_at_ms: now,
            coverage_start_ms: now - 180 * 86_400_000,
            coverage_end_ms: now,
            time_zone: "UTC".to_string(),
            calendar: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h_comparison: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            sources: vec![],
            warnings: vec![],
            rtk: None,
        };
        let (start, end) = compute_range("today", now, "UTC", &snapshot, None, None);
        // 2026-01-05 00:00:00 UTC = 1767571200000ms
        assert_eq!(start, 1767571200000); // day start = same as now (already midnight)
        assert_eq!(end, now);
    }

    #[test]
    fn test_compute_range_24h() {
        let now = 1767571200000;
        let snapshot = UsageDashboardSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generated_at_ms: now,
            coverage_start_ms: now - 180 * 86_400_000,
            coverage_end_ms: now,
            time_zone: "UTC".to_string(),
            calendar: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h_comparison: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            sources: vec![],
            warnings: vec![],
            rtk: None,
        };
        let (start, end) = compute_range("24h", now, "UTC", &snapshot, None, None);
        assert_eq!(start, now - 24 * 3600 * 1000);
        assert_eq!(end, now);
    }

    #[test]
    fn test_compute_range_custom_uses_explicit_bounds() {
        let now = 1767571200000;
        let coverage_start = now - 180 * 86_400_000;
        let snapshot = UsageDashboardSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generated_at_ms: now,
            coverage_start_ms: coverage_start,
            coverage_end_ms: now,
            time_zone: "UTC".to_string(),
            calendar: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h_comparison: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            sources: vec![],
            warnings: vec![],
            rtk: None,
        };

        let custom_start = now - 10 * 86_400_000;
        let custom_end = now - 3 * 86_400_000;
        let (start, end) = compute_range(
            "custom",
            now,
            "UTC",
            &snapshot,
            Some(custom_start),
            Some(custom_end),
        );
        assert_eq!(start, custom_start);
        assert_eq!(end, custom_end);

        // Invalid / missing custom bounds fall back to full coverage.
        let (start, end) = compute_range("custom", now, "UTC", &snapshot, None, None);
        assert_eq!(start, coverage_start);
        assert_eq!(end, now);
        let (start, end) = compute_range(
            "custom",
            now,
            "UTC",
            &snapshot,
            Some(custom_end),
            Some(custom_start),
        );
        assert_eq!(start, coverage_start);
        assert_eq!(end, now);
    }

    #[test]
    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    fn test_write_read_snapshot_roundtrip() {
        // Integration test: write and read back a snapshot
        let snapshot = UsageDashboardSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generated_at_ms: 1000,
            coverage_start_ms: 0,
            coverage_end_ms: 2000,
            time_zone: "test_roundtrip".to_string(),
            calendar: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 2000,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h_comparison: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            sources: vec![],
            warnings: vec![],
            rtk: None,
        };

        // Write
        assert!(write_snapshot(&snapshot).is_ok());

        // Read back
        let read = read_snapshot("test_roundtrip").unwrap();
        assert!(read.is_some());
        let read = read.unwrap();
        assert_eq!(read.generated_at_ms, 1000);
        assert_eq!(read.time_zone, "test_roundtrip");

        // Cleanup
        let _ = delete_snapshot("test_roundtrip");
    }

    #[test]
    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    fn test_schema_mismatch_returns_none() {
        let snapshot = UsageDashboardSnapshot {
            schema_version: 999, // Wrong version
            generated_at_ms: 1000,
            coverage_start_ms: 0,
            coverage_end_ms: 2000,
            time_zone: "test_schema_mismatch".to_string(),
            calendar: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 2000,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            rolling_24h_comparison: UsagePeriodData {
                range: crate::usage::UsageDashboardRange {
                    start_ms: 0,
                    end_ms: 0,
                },
                daily: vec![],
                activity: vec![],
                sessions: vec![],
            },
            sources: vec![],
            warnings: vec![],
            rtk: None,
        };

        assert!(write_snapshot(&snapshot).is_ok());
        let read = read_snapshot("test_schema_mismatch").unwrap();
        assert!(read.is_none()); // Schema mismatch → missing

        let _ = delete_snapshot("test_schema_mismatch");
    }
}
