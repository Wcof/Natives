use rusqlite::Connection;
use tempfile::tempdir;
use tokenusage_module::collector::host_sync::sync_from_host;
use tokenusage_module::storage::Store;

fn setup_mock_usage_db(dir: &std::path::Path) -> std::path::PathBuf {
    let db_path = dir.join("usage.db");
    let conn = Connection::open(&db_path).unwrap();

    conn.execute_batch(
        r#"
        CREATE TABLE usage_events (
            id TEXT PRIMARY KEY,
            requested_at TEXT NOT NULL,
            latency_ms INTEGER NOT NULL,
            ttft_ms INTEGER NOT NULL,
            provider TEXT NOT NULL,
            account_id TEXT NOT NULL DEFAULT '',
            model TEXT NOT NULL,
            model_alias TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL DEFAULT '',
            endpoint TEXT NOT NULL DEFAULT '',
            access_key_id TEXT NOT NULL DEFAULT '',
            access_key_name TEXT NOT NULL DEFAULT '',
            result TEXT NOT NULL,
            http_status INTEGER NOT NULL,
            error_code TEXT NOT NULL DEFAULT '',
            error_summary TEXT NOT NULL DEFAULT '',
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            cache_write_tokens INTEGER NOT NULL DEFAULT 0,
            reasoning_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            cost_micro INTEGER NOT NULL DEFAULT 0,
            service_tier TEXT NOT NULL DEFAULT 'default',
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            source_id TEXT NOT NULL DEFAULT '',
            collector_kind TEXT NOT NULL DEFAULT '',
            source_record_id TEXT NOT NULL DEFAULT '',
            parser_version TEXT NOT NULL DEFAULT '',
            record_kind TEXT NOT NULL DEFAULT 'request',
            billing_mode TEXT NOT NULL DEFAULT 'unknown',
            cost_basis TEXT NOT NULL DEFAULT 'api_estimate',
            evidence_level TEXT NOT NULL DEFAULT 'local_estimate',
            billing_atom TEXT NOT NULL DEFAULT '',
            session_id TEXT NOT NULL DEFAULT '',
            project_id TEXT NOT NULL DEFAULT '',
            tool_id TEXT NOT NULL DEFAULT '',
            source_instance_id TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE usage_budgets (
            id TEXT PRIMARY KEY,
            scope TEXT NOT NULL,
            scope_key TEXT NOT NULL DEFAULT '',
            currency TEXT NOT NULL DEFAULT 'USD',
            amount_micro INTEGER NOT NULL,
            period TEXT NOT NULL,
            timezone TEXT NOT NULL DEFAULT 'UTC',
            thresholds_json TEXT NOT NULL DEFAULT '[80,100]',
            enabled INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        INSERT INTO usage_events (
            id, requested_at, latency_ms, ttft_ms, provider, model, source, result, http_status,
            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
            total_tokens, cost_micro, tool_id, session_id
        ) VALUES (
            'evt_1', '2026-09-20T10:00:00Z', 100, 50, 'anthropic', 'claude-3-5-sonnet-20241022',
            'cli', 'success', 200, 1000, 200, 500, 0, 0, 1200, 15000, 'claude-code', 'sess_test_1'
        );

        INSERT INTO usage_events (
            id, requested_at, latency_ms, ttft_ms, provider, model, source, result, http_status,
            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
            total_tokens, cost_micro, tool_id, session_id
        ) VALUES (
            'evt_2', '2026-09-20T11:00:00Z', 120, 60, 'openai', 'gpt-4o',
            'cli', 'success', 200, 2000, 500, 0, 0, 0, 2500, 30000, 'codex', 'sess_test_2'
        );

        INSERT INTO usage_budgets (
            id, scope, scope_key, currency, amount_micro, period
        ) VALUES (
            'b_1', 'api_estimate', '', 'USD', 1000000, 'daily'
        );
        "#,
    )
    .unwrap();

    db_path
}

#[test]
fn test_sync_from_host_and_api_queries() {
    let temp = tempdir().unwrap();
    let usage_db = setup_mock_usage_db(temp.path());

    let store_dir = temp.path().join("store");
    let store = Store::open(&store_dir).unwrap();

    // 1. 同步
    let summary = sync_from_host(&store, Some(&usage_db)).unwrap();
    assert_eq!(summary.events_synced, 2);
    assert_eq!(summary.total_tokens, 3700);
    assert_eq!(summary.total_cost_micros, 45000);
    assert_eq!(summary.budgets_synced, 1);

    // 2. 检查 store 内部数据
    store
        .with_read(|conn| {
            let record_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
                .unwrap();
            assert_eq!(record_count, 2);

            let session_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
                .unwrap();
            assert_eq!(session_count, 2);

            let agg_tokens: i64 = conn
                .query_row(
                    "SELECT SUM(total_tokens) FROM daily_aggregates WHERE date = '2026-09-20'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(agg_tokens, 3700);

            let limit_status: String = conn
                .query_row(
                    "SELECT status FROM limits_cache WHERE window_kind = 'daily'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(limit_status, "ok");

            Ok(())
        })
        .unwrap();

    // 3. 检查 API
    let req = app_runtime_core::http::HttpRequest::for_internal("GET", "/api/overview", Vec::new());
    let resp = tokenusage_module::api::handle_api_request(&store, &req, "/api/overview").unwrap();
    assert!(resp.contains("\"totalTokens\":3700"));
    assert!(resp.contains("\"thisWeek\""));
    assert!(resp.contains("\"thisMonth\""));

    let tray_req =
        app_runtime_core::http::HttpRequest::for_internal("GET", "/api/tray/state", Vec::new());
    let tray_resp =
        tokenusage_module::api::handle_api_request(&store, &tray_req, "/api/tray/state").unwrap();
    println!("tray_resp: {}", tray_resp);
    assert!(tray_resp.contains("\"displayText\":"));
    assert!(tray_resp.contains("3.7K"));
    assert!(tray_resp.contains("\"overview\":{\"periods\":{"));
    assert!(tray_resp.contains("\"worstLimit\""));

    // 4. 重复同步幂等性
    let summary2 = sync_from_host(&store, Some(&usage_db)).unwrap();
    assert_eq!(summary2.events_synced, 0); // 增量无新事件
}
