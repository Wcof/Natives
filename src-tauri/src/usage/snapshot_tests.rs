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
