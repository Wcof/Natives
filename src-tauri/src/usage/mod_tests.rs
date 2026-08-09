use super::*;

    #[test]
    fn invalid_range_is_rejected() {
        let req = UsageDashboardRequest {
            start_ms: -1,
            end_ms: 100,
            include_comparison: false,
            time_zone: "UTC".into(),
        };
        assert!(validate_request(&req).is_err());

        let req = UsageDashboardRequest {
            start_ms: 100,
            end_ms: 100,
            include_comparison: false,
            time_zone: "UTC".into(),
        };
        assert!(validate_request(&req).is_err());

        let req = UsageDashboardRequest {
            start_ms: 200,
            end_ms: 100,
            include_comparison: false,
            time_zone: "UTC".into(),
        };
        assert!(validate_request(&req).is_err());
    }

    #[test]
    fn range_end_is_exclusive() {
        let req = UsageDashboardRequest {
            start_ms: 0,
            end_ms: 100,
            include_comparison: false,
            time_zone: "UTC".into(),
        };
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
        let rtk = RtkSummary {
            total_saved_tokens: 5000,
            total_commands: 10,
        };
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
    fn localized_hour_start_is_real_epoch_not_painted_utc() {
        // 2026-07-20 09:34:52 UTC+8 = 01:34:52Z
        let ts = 1_784_511_292_928i64;
        let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let (date, hour_start) = localized_time_metrics(ts, &tz);
        assert_eq!(date, "2026-07-20");
        // Local 09:00 Asia/Shanghai = 01:00Z
        assert_eq!(hour_start, 1_784_509_200_000);
        // Must NOT be the old "paint local 09:00 as 09:00Z" value.
        assert_ne!(hour_start, 1_784_538_000_000);
    }

    #[test]
    fn response_serializes_as_camel_case() {
        let resp = UsageDashboardResponse {
            generated_at_ms: 12345,
            range: UsageDashboardRange {
                start_ms: 0,
                end_ms: 1000,
            },
            daily: vec![],
            activity: vec![],
            sessions: vec![],
            comparison: None,
            dimensions: UsageDashboardDimensions {
                sources: vec![],
                models: vec![],
                projects: vec![],
                terminals: vec![],
            },
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
                m.insert(
                    "ccusage_total".into(),
                    serde_json::Value::Number(serde_json::Number::from(100)),
                );
                m.insert(
                    "scanned_total".into(),
                    serde_json::Value::Number(serde_json::Number::from(95)),
                );
                m
            },
        };
        assert_eq!(warning.source_id.unwrap(), "claude");
        assert!(matches!(warning.code, UsageWarningCode::TotalMismatch));
    }

    #[test]
    fn serialization_contracts_match_typescript() {
        // Assert WarningCode serialization matches SCREAMING_SNAKE_CASE
        assert_eq!(
            serde_json::to_string(&UsageWarningCode::CliNotFound).unwrap(),
            "\"CLI_NOT_FOUND\""
        );
        assert_eq!(
            serde_json::to_string(&UsageWarningCode::CliTimeout).unwrap(),
            "\"CLI_TIMEOUT\""
        );
        assert_eq!(
            serde_json::to_string(&UsageWarningCode::SourceUnavailable).unwrap(),
            "\"SOURCE_UNAVAILABLE\""
        );
        assert_eq!(
            serde_json::to_string(&UsageWarningCode::SourceParsePartial).unwrap(),
            "\"SOURCE_PARSE_PARTIAL\""
        );
        assert_eq!(
            serde_json::to_string(&UsageWarningCode::TotalMismatch).unwrap(),
            "\"TOTAL_MISMATCH\""
        );
        assert_eq!(
            serde_json::to_string(&UsageWarningCode::CostUnavailable).unwrap(),
            "\"COST_UNAVAILABLE\""
        );
        assert_eq!(
            serde_json::to_string(&UsageWarningCode::NativesHistoryPartial).unwrap(),
            "\"NATIVES_HISTORY_PARTIAL\""
        );

        // Assert DurationMethod serialization matches snake_case
        assert_eq!(
            serde_json::to_string(&DurationMethod::EventGapEstimate).unwrap(),
            "\"event_gap_estimate\""
        );
        assert_eq!(
            serde_json::to_string(&DurationMethod::SessionBounds).unwrap(),
            "\"session_bounds\""
        );

        // Assert UsageQuality serialization matches camelCase (lowercase start)
        assert_eq!(
            serde_json::to_string(&UsageQuality::Reported).unwrap(),
            "\"reported\""
        );
        assert_eq!(
            serde_json::to_string(&UsageQuality::Estimated).unwrap(),
            "\"estimated\""
        );
        assert_eq!(
            serde_json::to_string(&UsageQuality::Unavailable).unwrap(),
            "\"unavailable\""
        );

        // Assert UsageSourceState serialization matches camelCase (lowercase start)
        assert_eq!(
            serde_json::to_string(&UsageSourceState::Ok).unwrap(),
            "\"ok\""
        );
        assert_eq!(
            serde_json::to_string(&UsageSourceState::Partial).unwrap(),
            "\"partial\""
        );
        assert_eq!(
            serde_json::to_string(&UsageSourceState::Unavailable).unwrap(),
            "\"unavailable\""
        );
        assert_eq!(
            serde_json::to_string(&UsageSourceState::Detected).unwrap(),
            "\"detected\""
        );

        // Assert BreadcrumbKind serialization matches snake_case
        assert_eq!(
            serde_json::to_string(&BreadcrumbKind::Cli).unwrap(),
            "\"cli\""
        );
        assert_eq!(
            serde_json::to_string(&BreadcrumbKind::RawLog).unwrap(),
            "\"raw_log\""
        );
        assert_eq!(
            serde_json::to_string(&BreadcrumbKind::Database).unwrap(),
            "\"database\""
        );
    }
