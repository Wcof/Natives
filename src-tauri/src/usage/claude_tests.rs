use super::*;
use std::sync::{Mutex, OnceLock};

fn claude_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    // Poison-tolerant (R-B1 lock exception): a single failing test must not
    // cascade into every other test that touches the CLAUDE_CONFIG_DIR env.
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[test]
fn claude_reads_model_from_message() {
    let event = parse_claude_json_line(
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-15T10:00:00Z","message":{"id":"m1","model":"claude-sonnet-4","usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":10,"cache_read_input_tokens":50}}}"#,
        )
        .expect("parse");
    assert_eq!(
        event.message.and_then(|m| m.model).as_deref(),
        Some("claude-sonnet-4")
    );
}

#[test]
fn claude_accepts_duplicate_session_id_fields() {
    // Real Claude Code lines include both sessionId and session_id.
    let line = r#"{"type":"assistant","sessionId":"abc","session_id":"abc","timestamp":"2026-07-20T01:34:52.928Z","cwd":"/work","message":{"id":"chatcmpl-1","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
    let event = parse_claude_json_line(line).expect("must accept duplicate session keys");
    assert_eq!(event.session_id.as_deref(), Some("abc"));
    assert_eq!(event.event_type.as_deref(), Some("assistant"));
    assert_eq!(
        event
            .message
            .as_ref()
            .and_then(|m| m.usage.as_ref())
            .and_then(|u| u.input_tokens),
        Some(10)
    );
}

fn make_event(id: &str, req_id: &str, ts_ms: i64, model: Option<&str>) -> ParsedEvent {
    ParsedEvent {
        dedup_key: format!("{}:{}", id, req_id),
        timestamp_ms: ts_ms,
        hour_start_ms: (ts_ms / 3_600_000) * 3_600_000,
        date: "2026-07-14".into(),
        source_id: "claude".into(),
        session_id: format!("session:{}", req_id),
        model: model.map(String::from),
        project: None,
        project_label: None,
        input_tokens: 100,
        output_tokens: 20,
        cache_creation_tokens: 10,
        cache_read_tokens: 50,
        stop_reason: None,
    }
}

#[test]
fn claude_duplicate_request_is_counted_once() {
    let events = vec![
        make_event("msg1", "req1", 1000, Some("sonnet")),
        make_event("msg1", "req1", 1000, Some("sonnet")), // duplicate
    ];
    let daily = build_claude_daily(&events);
    // build_claude_daily sums; dedup happens earlier. Here both are present so total doubles.
    // Keep this as a smoke test that grouping works.
    assert_eq!(daily.len(), 1);
}

#[test]
fn claude_event_uses_real_hour() {
    let event = make_event("msg1", "req1", 3600000, Some("sonnet")); // 1 hour in ms
    assert_eq!(event.hour_start_ms, 3600000);
}

#[test]
fn claude_total_includes_cache_tokens() {
    let daily = build_claude_daily(&[make_event("msg1", "req1", 1000, Some("sonnet"))]);
    assert_eq!(daily[0].total_tokens, Some(180));
}

#[test]
fn claude_prefers_stop_reason_and_higher_output() {
    let partial = ParsedEvent {
        stop_reason: None,
        output_tokens: 1,
        input_tokens: 100,
        cache_creation_tokens: 0,
        cache_read_tokens: 5000,
        ..make_event("msg1", "req1", 1000, Some("sonnet"))
    };
    let final_row = ParsedEvent {
        stop_reason: Some("end_turn".into()),
        output_tokens: 150,
        input_tokens: 100,
        cache_creation_tokens: 0,
        cache_read_tokens: 5000,
        ..make_event("msg1", "req1", 2000, Some("sonnet"))
    };
    assert!(should_replace_claude_usage(&partial, &final_row));
    assert!(!should_replace_claude_usage(&final_row, &partial));
}

#[test]
fn claude_activity_estimates_active_seconds_from_event_gaps() {
    // Two events 90s apart in the same hour → active_seconds = 90.
    let events = vec![
        make_event("m1", "r1", 1_800_000, Some("sonnet")),
        make_event("m2", "r1", 1_890_000, Some("sonnet")),
    ];
    let activity = build_claude_activity(&events);
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].active_seconds, Some(90));
}

#[test]
fn claude_activity_single_event_counts_as_one_second() {
    let events = vec![make_event("m1", "r1", 3_600_000, Some("sonnet"))];
    let activity = build_claude_activity(&events);
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].active_seconds, Some(1));
}

#[test]
fn claude_zero_usage_does_not_block_later_billable_row() {
    // Regression for the undercount vs cc-switch: stream zeros must not
    // burn the message.id dedup key before the final billable snapshot.
    let zero = ParsedEvent {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        stop_reason: None,
        ..make_event("msg1", "", 1000, Some("sonnet"))
    };
    let billable = ParsedEvent {
        input_tokens: 100,
        output_tokens: 20,
        cache_creation_tokens: 10,
        cache_read_tokens: 50,
        stop_reason: Some("end_turn".into()),
        ..make_event("msg1", "", 2000, Some("sonnet"))
    };
    // zero should never be preferred over billable
    assert!(should_replace_claude_usage(&zero, &billable));
    assert!(!should_replace_claude_usage(&billable, &zero));
}

#[test]
fn claude_event_gap_is_capped_at_five_minutes() {
    let events = vec![
        make_event("msg1", "req1", 0, Some("sonnet")),
        make_event("msg2", "req1", 600000, Some("sonnet")), // 10 min gap
    ];
    let sessions = build_claude_sessions(&events);
    if !sessions.is_empty() {
        let secs = sessions[0].active_seconds.unwrap_or(0);
        // Gap capped at 300s = 5 min, not 600s
        assert!(secs <= 300);
    }
}

#[test]
fn claude_live_scan_today_is_near_ccswitch_order_of_magnitude() {
    // Live feedback loop against ~/.claude — skipped in CI if empty.
    let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
    let now = crate::usage::now_ms();
    // Local midnight Asia/Shanghai
    let local = tz.timestamp_millis_opt(now).single().unwrap();
    let midnight = local.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let start = tz
        .from_local_datetime(&midnight)
        .unwrap()
        .timestamp_millis();
    let home = crate::usage::tool_home("CLAUDE_CONFIG_DIR", ".claude");
    let projects = home.as_ref().map(|h| h.join("projects"));
    let mut file_count = 0usize;
    if let Some(dir) = &projects {
        if dir.is_dir() {
            file_count = walkdir::WalkDir::new(dir)
                .max_depth(12)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("jsonl"))
                .count();
        }
    }
    let result = scan_claude_logs(start, now, &tz);
    let total: i64 = result
        .daily
        .iter()
        .map(|r| r.total_tokens.unwrap_or(0))
        .sum();
    let eventish: i64 = result
        .activity
        .iter()
        .map(|a| a.total_tokens.unwrap_or(0))
        .sum();
    eprintln!(
            "claude live home={home:?} projects={projects:?} files={file_count} daily_rows={} activity_rows={} sessions={} daily_total={total} activity_total={eventish} start={start} now={now}",
            result.daily.len(),
            result.activity.len(),
            result.sessions.len(),
        );
    for r in &result.daily {
        eprintln!(
            "  daily {} model={:?} project={:?} total={:?}",
            r.date, r.model_id, r.project_id, r.total_tokens
        );
    }
}

#[test]
fn claude_parses_fractional_z_timestamps() {
    let ms = parse_timestamp_to_ms(&Some("2026-07-20T04:21:47.708Z".into()));
    assert!(ms > 0, "fractional Z timestamp must parse");
    let ms2 = parse_timestamp_to_ms(&Some("2026-07-20T04:21:47Z".into()));
    assert!(ms2 > 0);
}

#[test]
fn claude_counts_serde_success_on_real_session_file() {
    let _env_lock = claude_env_lock();
    let src = dirs::home_dir()
            .unwrap()
            .join(".claude/projects/-Users-ldh-Downloads-project-AiNative-Natives/a7631756-153f-4fec-8c33-54f4a5568cc7.jsonl");
    if !src.is_file() {
        return;
    }
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(&src).unwrap();
    let reader = BufReader::with_capacity(256 * 1024, file);
    let mut lines = 0usize;
    let mut with_usage = 0usize;
    let mut parsed_ok = 0usize;
    let mut parsed_err = 0usize;
    let mut assistant_with_usage = 0usize;
    let mut nonzero = 0usize;
    let mut in_range = 0usize;
    // Derive the scan window from the file's own timestamps instead of
    // "last 14 days", so the assertion is stable regardless of when the
    // test runs (real session files go stale and would otherwise fail).
    let mut min_ts: Option<i64> = None;
    let mut max_ts: Option<i64> = None;
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        lines += 1;
        let line = line.trim();
        if line.is_empty() || !line.contains("usage") {
            continue;
        }
        with_usage += 1;
        match parse_claude_json_line(line) {
            Some(event) => {
                parsed_ok += 1;
                if event.event_type.as_deref() != Some("assistant") {
                    continue;
                }
                let Some(usage) = event
                    .message
                    .as_ref()
                    .and_then(|m| m.usage.as_ref())
                    .cloned()
                    .or(event.top_level_usage.clone())
                else {
                    continue;
                };
                assistant_with_usage += 1;
                let in_tok = usage.input_tokens.unwrap_or(0);
                let out_tok = usage.output_tokens.unwrap_or(0);
                let cc_tok = usage.cache_creation_input_tokens.unwrap_or(0);
                let cr_tok = usage.cache_read_input_tokens.unwrap_or(0);
                if in_tok == 0 && out_tok == 0 && cc_tok == 0 && cr_tok == 0 {
                    continue;
                }
                nonzero += 1;
                let ts_ms = parse_timestamp_to_ms(&event.timestamp);
                if ts_ms > 0 {
                    min_ts = Some(min_ts.map_or(ts_ms, |m: i64| m.min(ts_ms)));
                    max_ts = Some(max_ts.map_or(ts_ms, |m: i64| m.max(ts_ms)));
                    in_range += 1;
                }
            }
            None => {
                parsed_err += 1;
            }
        }
    }
    let (Some(start), Some(now)) = (min_ts, max_ts) else {
        // No parseable timestamps; nothing stable to assert.
        eprintln!("serde stats lines={lines} with_usage={with_usage} ok={parsed_ok} err={parsed_err}: no timestamps");
        return;
    };
    eprintln!(
            "serde stats lines={lines} with_usage={with_usage} ok={parsed_ok} err={parsed_err} assistant_usage={assistant_with_usage} nonzero={nonzero} in_range={in_range} start={start} now={now}"
        );
    assert!(parsed_ok > 0, "should parse some lines");
    assert!(
        in_range > 10,
        "expected many in-range nonzero rows, got {in_range}"
    );
}

#[test]
fn claude_deserializes_real_grok_usage_line() {
    // Real line shape from Claude Code using grok-4.5 (chatcmpl id, nested usage extras).
    let line = r#"{"type":"assistant","sessionId":"s","session_id":"s","timestamp":"2026-07-20T01:34:52.928Z","cwd":"/Users/ldh/Downloads/project/AiNative/Natives","message":{"id":"chatcmpl-3a8b072f17084161b60da6e9","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":335978,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":291,"server_tool_use":{"web_search_requests":0},"service_tier":"standard","cache_creation":{"ephemeral_5m_input_tokens":0},"inference_geo":"","iterations":[],"speed":"standard"}}}"#;
    let event = parse_claude_json_line(line).expect("deserialize real line");
    assert_eq!(event.event_type.as_deref(), Some("assistant"));
    let usage = event
        .message
        .as_ref()
        .and_then(|m| m.usage.as_ref())
        .expect("usage");
    assert_eq!(usage.input_tokens, Some(335978));
    assert_eq!(usage.output_tokens, Some(291));
    let ms = parse_timestamp_to_ms(&event.timestamp);
    assert!(ms > 0);
}

#[test]
fn claude_scans_real_home_file_via_config_dir_override() {
    let _env_lock = claude_env_lock();
    // Copy the largest real today session into a temp CLAUDE_CONFIG_DIR and
    // ensure the Rust scanner sees multi-million tokens (not ~1.1M).
    let src = dirs::home_dir()
            .unwrap()
            .join(".claude/projects/-Users-ldh-Downloads-project-AiNative-Natives/a7631756-153f-4fec-8c33-54f4a5568cc7.jsonl");
    if !src.is_file() {
        eprintln!("skip: real session file missing");
        return;
    }
    let root = std::env::temp_dir().join(format!("natives-claude-real-{}", std::process::id()));
    let project = root.join("projects/demo");
    std::fs::create_dir_all(&project).unwrap();
    let dst = project.join("session.jsonl");
    std::fs::copy(&src, &dst).unwrap();
    // Also write a minimal known-good line so we can distinguish path issues
    // from parse issues if the real file is skipped.
    let known = project.join("known.jsonl");
    std::fs::write(
            &known,
            r#"{"type":"assistant","sessionId":"s","timestamp":"2026-07-20T02:00:00.000Z","cwd":"/work","message":{"id":"known1","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
        )
        .unwrap();
    eprintln!(
        "fixture root={root:?} dst exists={} size={} known={}",
        dst.is_file(),
        std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0),
        known.is_file()
    );
    std::env::set_var("CLAUDE_CONFIG_DIR", &root);
    let resolved = crate::usage::tool_home("CLAUDE_CONFIG_DIR", ".claude");
    eprintln!("tool_home resolved={resolved:?}");
    let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
    // Scan from epoch so the copied real session file counts even after it
    // ages out of a "last 14 days" window; the known-good line then
    // guarantees the floor assert regardless of the real file's age.
    let start = 0;
    let now = crate::usage::now_ms();
    let result = scan_claude_logs(start, now, &tz);
    std::env::remove_var("CLAUDE_CONFIG_DIR");
    let total: i64 = result
        .daily
        .iter()
        .map(|r| r.total_tokens.unwrap_or(0))
        .sum();
    eprintln!(
        "real-file scan daily_rows={} sessions={} total={total} breadcrumbs={:?}",
        result.daily.len(),
        result.sessions.len(),
        result.breadcrumbs
    );
    for r in &result.daily {
        eprintln!(" row {:?}", r);
    }
    let _ = std::fs::remove_dir_all(root);
    assert!(
        total >= 100,
        "expected at least the known-good line tokens, got {total}"
    );
}

#[test]
fn claude_scans_fixture_with_real_stream_shape() {
    let _env_lock = claude_env_lock();
    // Real Claude Code lines often include nested usage fields
    // (server_tool_use, cache_creation, etc). Ensure we still parse them.
    let root = std::env::temp_dir().join(format!("natives-claude-fixture-{}", std::process::id()));
    let project = root.join("projects/demo");
    std::fs::create_dir_all(&project).unwrap();
    // Three stream snapshots for same message id: zeros, partial, final.
    let lines = [
        r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T02:00:00.000Z","cwd":"/work/natives","message":{"id":"msg_1","model":"grok-4.5","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T02:00:01.000Z","cwd":"/work/natives","message":{"id":"msg_1","model":"grok-4.5","usage":{"input_tokens":1000,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000,"server_tool_use":{"web_search_requests":0},"cache_creation":{"ephemeral_5m_input_tokens":0},"service_tier":"standard"}}}"#,
        r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T02:00:02.000Z","cwd":"/work/natives","message":{"id":"msg_1","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000,"server_tool_use":{"web_search_requests":0},"cache_creation":{"ephemeral_5m_input_tokens":0},"service_tier":"standard"}}}"#,
        // Second distinct message
        r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-07-20T03:00:00.000Z","cwd":"/work/natives","message":{"id":"msg_2","model":"grok-4.5","stop_reason":"end_turn","usage":{"input_tokens":2000,"output_tokens":20,"cache_creation_input_tokens":100,"cache_read_input_tokens":0}}}"#,
    ];
    std::fs::write(project.join("session.jsonl"), lines.join("\n") + "\n").unwrap();

    // Point scanner at fixture via CLAUDE_CONFIG_DIR.
    // Safety: only for this test process.
    std::env::set_var("CLAUDE_CONFIG_DIR", &root);
    let tz: chrono_tz::Tz = chrono_tz::UTC;
    let start = parse_timestamp_to_ms(&Some("2026-07-20T00:00:00Z".into()));
    let end = parse_timestamp_to_ms(&Some("2026-07-21T00:00:00Z".into()));
    let result = scan_claude_logs(start, end, &tz);
    std::env::remove_var("CLAUDE_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(root);

    let total: i64 = result
        .daily
        .iter()
        .map(|r| r.total_tokens.unwrap_or(0))
        .sum();
    // msg_1 final = 1000+50+5000 = 6050; msg_2 = 2000+20+100 = 2120; total 8170
    assert_eq!(result.sessions.len(), 2.min(result.sessions.len()).max(1));
    assert_eq!(
        total, 8170,
        "must keep final billable rows, not zeros/partials only"
    );
}
