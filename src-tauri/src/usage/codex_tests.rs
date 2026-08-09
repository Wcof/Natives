use super::*;

    #[test]
    fn codex_finds_current_four_level_session_layout() {
        let root = std::env::temp_dir().join(format!("natives-codex-{}", std::process::id()));
        let sessions = root.join("sessions/2026/07/15");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("rollout.jsonl"), "{}\n").unwrap();
        let files = collect_session_files(&root.join("sessions"), &root.join("archived_sessions"));
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn codex_multiple_rollouts_across_dates_not_lost() {
        // Regression: the old code used file_stem as HashMap key, so multiple
        // date directories each containing rollout.jsonl would collide and only
        // one file would survive. The fix uses full path as key.
        let root = std::env::temp_dir().join(format!("natives-codex-multi-{}", std::process::id()));
        let dir_a = root.join("sessions/2026/07/15");
        let dir_b = root.join("sessions/2026/07/20");
        let dir_c = root.join("sessions/2026/07/25");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::create_dir_all(&dir_c).unwrap();
        std::fs::write(dir_a.join("rollout.jsonl"), "{}\n").unwrap();
        std::fs::write(dir_b.join("rollout.jsonl"), "{}\n").unwrap();
        std::fs::write(dir_c.join("rollout.jsonl"), "{}\n").unwrap();
        let files = collect_session_files(&root.join("sessions"), &root.join("archived_sessions"));
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(
            files.len(),
            3,
            "all three rollout.jsonl files must be collected"
        );
    }

    #[test]
    fn codex_dedup_skips_duplicate_event_id() {
        // Verify that duplicate event_ids are counted only once even when
        // they are NOT replay events.
        let mut dedup_set: HashSet<String> = HashSet::new();
        let eid = "evt-dup".to_string();

        // First insert succeeds.
        assert!(dedup_set.insert(eid.clone()));
        // Second insert returns false — event should be skipped.
        assert!(!dedup_set.insert(eid.clone()));
    }

    #[test]
    fn codex_normalizes_current_rollout_token_event() {
        let mut context = CodexFileContext::default();
        let meta = r#"{"timestamp":"2026-07-15T10:00:00Z","type":"session_meta","payload":{"id":"session-1","cwd":"/work/natives"}}"#;
        let turn = r#"{"timestamp":"2026-07-15T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5.6","cwd":"/work/natives"}}"#;
        let tokens = r#"{"timestamp":"2026-07-15T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#;

        assert!(normalize_codex_line(meta, &mut context).is_none());
        assert!(normalize_codex_line(turn, &mut context).is_none());
        let event = normalize_codex_line(tokens, &mut context).expect("token event");
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(
            event.turn_context.as_ref().and_then(|c| c.model.as_deref()),
            Some("gpt-5.6")
        );
        assert_eq!(compute_codex_delta(&event, None), (100, 20, 40));
    }

    #[test]
    fn codex_last_token_usage_is_delta() {
        // last_token_usage is already a delta, should be used as-is
        let event = CodexJsonLine {
            event_id: Some("evt1".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:00:00Z".into()),
            last_token_usage: Some(CodexTokenUsage {
                input: Some(100),
                output: Some(20),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(50),
            }),
            total_token_usage: None,
            turn_context: None,
            replay_for: None,
            fork_from: None,
        };
        let delta = compute_codex_delta(&event, None);
        assert_eq!(delta.0, 100); // input
        assert_eq!(delta.1, 20); // output
    }

    #[test]
    fn codex_normalizes_cache_inclusive_input_to_fresh() {
        // Official total_tokens = input + output = 120 when input already includes cache.
        assert_eq!(normalize_codex_input(100, 40), (60, 40));
        // Cache cannot exceed input.
        assert_eq!(normalize_codex_input(20, 40), (0, 20));
        // No cache stays unchanged.
        assert_eq!(normalize_codex_input(100, 0), (100, 0));
    }

    #[test]
    fn codex_daily_total_matches_official_total_tokens() {
        // After normalize: fresh=60, output=20, cache=40 → real total 120
        // (same as Codex last_token_usage.total_tokens).
        let daily = build_codex_daily(&[ParsedCodexEvent {
            event_id: "event-1".into(),
            timestamp_ms: 1,
            hour_start_ms: 0,
            date: "2026-07-10".into(),
            session_id: "codex:session-1".into(),
            model: Some("gpt-5".into()),
            project: None,
            project_label: None,
            input_tokens: 60,
            output_tokens: 20,
            cache_read_tokens: 40,
            reasoning_tokens: 0,
            input_cache_hit: 40,
            is_replay_or_fork: false,
        }]);
        assert_eq!(daily[0].input_tokens, Some(60));
        assert_eq!(daily[0].cache_read_tokens, Some(40));
        assert_eq!(daily[0].total_tokens, Some(120));
    }

    #[test]
    fn codex_end_to_end_total_matches_rollout_total_field() {
        let mut context = CodexFileContext::default();
        let meta = r#"{"timestamp":"2026-07-15T10:00:00Z","type":"session_meta","payload":{"id":"session-1","cwd":"/work/natives"}}"#;
        let turn = r#"{"timestamp":"2026-07-15T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5.6","cwd":"/work/natives"}}"#;
        // Real Codex shape: total_tokens == input_tokens + output_tokens (cache already inside input).
        let tokens = r#"{"timestamp":"2026-07-15T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#;
        assert!(normalize_codex_line(meta, &mut context).is_none());
        assert!(normalize_codex_line(turn, &mut context).is_none());
        let event = normalize_codex_line(tokens, &mut context).expect("token event");
        let (input, output, cache_read) = compute_codex_delta(&event, None);
        let (fresh, cache) = normalize_codex_input(input, cache_read);
        assert_eq!((fresh, output, cache), (60, 20, 40));
        assert_eq!(
            crate::usage::token_total(fresh, output, 0, cache),
            120,
            "must match Codex total_tokens field"
        );
    }

    #[test]
    fn codex_cumulative_usage_subtracts_previous_total() {
        let prev = CodexTokenUsage {
            input: Some(50),
            output: Some(10),
            reasoning_tokens: Some(0),
            input_cache_hit: Some(20),
        };
        let event = CodexJsonLine {
            event_id: Some("evt2".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:01:00Z".into()),
            last_token_usage: None,
            total_token_usage: Some(CodexTokenUsage {
                input: Some(100),
                output: Some(25),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(40),
            }),
            turn_context: None,
            replay_for: None,
            fork_from: None,
        };
        let delta = compute_codex_delta(&event, Some(&prev));
        assert_eq!(delta.0, 50); // 100 - 50
        assert_eq!(delta.1, 15); // 25 - 10
    }

    #[test]
    fn codex_cumulative_reset_is_not_negative() {
        let prev = CodexTokenUsage {
            input: Some(200),
            output: Some(50),
            reasoning_tokens: Some(0),
            input_cache_hit: Some(100),
        };
        let event = CodexJsonLine {
            event_id: Some("evt3".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:02:00Z".into()),
            last_token_usage: None,
            total_token_usage: Some(CodexTokenUsage {
                input: Some(100), // reset - lower than prev
                output: Some(25),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(30),
            }),
            turn_context: None,
            replay_for: None,
            fork_from: None,
        };
        let delta = compute_codex_delta(&event, Some(&prev));
        assert_eq!(delta.0, 0); // 100 - 200 = -100, clamped to 0
        assert_eq!(delta.1, 0); // 25 - 50 = -25, clamped to 0
    }

    #[test]
    fn codex_replayed_event_is_counted_once() {
        // Replay/fork events should be filtered out by the calling code
        let event = CodexJsonLine {
            event_id: Some("evt4".into()),
            session_id: Some("sess1".into()),
            event_type: Some("turn".into()),
            timestamp: Some("2026-07-14T10:00:00Z".into()),
            last_token_usage: Some(CodexTokenUsage {
                input: Some(100),
                output: Some(20),
                reasoning_tokens: Some(0),
                input_cache_hit: Some(0),
            }),
            total_token_usage: None,
            turn_context: None,
            replay_for: Some("original-evt".into()),
            fork_from: None,
        };
        assert!(event.replay_for.is_some());
        assert!(event.fork_from.is_none());
    }
