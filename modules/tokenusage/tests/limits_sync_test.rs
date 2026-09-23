//! limits_sync_test: 验证额度监控（零假数据原则）与多设备 Hub 同步协议。

use tempfile::tempdir;
use tokenusage_module::limits::{record_limit_window, refresh_limits, LimitWindow};
use tokenusage_module::storage::Store;
use tokenusage_module::sync::{
    generate_local_sync_payload, reconcile_sync_payload, DailySyncItem, DeviceSyncPayload,
    PeriodStats,
};

#[test]
fn test_limits_refresh_zero_fake_data() {
    std::env::set_var("NATIVES_MODEL_HOST_CONFIG_DIR", "none");
    let temp = tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let store = Store::open(&data_dir).unwrap();

    // 初始刷新：未配置凭据时必须如实返回 not_configured，无假数据（None 百分比）
    let count = refresh_limits(&store).unwrap();
    assert!(count >= 8);

    store
        .with_read(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT provider_id, status, used_percent, remaining_percent FROM limits_cache",
                )
                .unwrap();

            let rows = stmt
                .query_map([], |r| {
                    let pid: String = r.get(0)?;
                    let status: String = r.get(1)?;
                    let used: Option<f64> = r.get(2)?;
                    let rem: Option<f64> = r.get(3)?;
                    Ok((pid, status, used, rem))
                })
                .unwrap();

            for r in rows {
                let (pid, status, used, rem) = r.unwrap();
                assert_eq!(
                    status, "not_configured",
                    "Provider {} must be not_configured",
                    pid
                );
                assert!(
                    used.is_none(),
                    "Provider {} used_percent must be None (zero fake data)",
                    pid
                );
                assert!(
                    rem.is_none(),
                    "Provider {} remaining_percent must be None (zero fake data)",
                    pid
                );
            }
            Ok(())
        })
        .unwrap();

    // 记录真实额度窗口
    let window = LimitWindow {
        provider_id: "claude".into(),
        account_id: "test-acc".into(),
        window_kind: "session".into(),
        label: "Claude Code 5h".into(),
        used_percent: Some(75.0),
        remaining_percent: Some(25.0),
        used_units: None,
        total_units: None,
        unit_type: "percent".into(),
        resets_at: Some("2026-09-20T15:00:00Z".into()),
        fetched_at: "2026-09-20T10:00:00Z".into(),
        status: "ok".into(),
    };
    record_limit_window(&store, &window).unwrap();

    store
        .with_read(|conn| {
            let (status, rem): (String, f64) = conn
                .query_row(
                    "SELECT status, remaining_percent FROM limits_cache WHERE provider_id = 'claude' AND account_id = 'test-acc'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(status, "ok");
            assert_eq!(rem, 25.0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn test_hub_sync_reconciliation_and_monotonic_timestamp() {
    let temp = tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let store = Store::open(&data_dir).unwrap();

    // 本地生成 payload
    let local_payload = generate_local_sync_payload(&store, "dev-local", "MacBook Pro").unwrap();
    assert_eq!(local_payload.device_id, "dev-local");

    // 模拟接收远端设备同步
    let remote_payload = DeviceSyncPayload {
        device_id: "dev-remote".into(),
        device_name: "Desktop Linux".into(),
        timestamp: "2026-09-20T12:00:00Z".into(),
        today: PeriodStats {
            total_tokens: 150_000,
            cost_usd: 0.45,
            session_count: 5,
        },
        all_time: PeriodStats {
            total_tokens: 2_000_000,
            cost_usd: 6.20,
            session_count: 80,
        },
        daily: vec![DailySyncItem {
            date: "2026-09-20".into(),
            source_id: "codex".into(),
            model: "gpt-4o".into(),
            total_tokens: 150_000,
            cost_micros: 450_000,
            session_count: 5,
        }],
    };

    reconcile_sync_payload(&store, &remote_payload).unwrap();

    // 验证远端设备已入库
    store
        .with_read(|conn| {
            let (name, status, last_synced): (String, String, String) = conn
                .query_row(
                    "SELECT device_name, status, last_synced_at FROM sync_state WHERE device_id = 'dev-remote'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(name, "Desktop Linux");
            assert_eq!(status, "online");
            assert_eq!(last_synced, "2026-09-20T12:00:00Z");

            // 验证 daily_aggregates 包含远端记录
            let remote_source = "dev-remote:codex";
            let tokens: i64 = conn
                .query_row(
                    "SELECT total_tokens FROM daily_aggregates WHERE date = '2026-09-20' AND source_id = ?1",
                    [remote_source],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(tokens, 150_000);
            Ok(())
        })
        .unwrap();

    // 模拟更早时间戳的过时 payload（单调递增防护）
    let stale_payload = DeviceSyncPayload {
        device_id: "dev-remote".into(),
        device_name: "Desktop Linux (old)".into(),
        timestamp: "2026-09-20T11:00:00Z".into(), // 早于 12:00:00Z
        today: PeriodStats {
            total_tokens: 50_000,
            cost_usd: 0.15,
            session_count: 2,
        },
        all_time: PeriodStats {
            total_tokens: 1_000_000,
            cost_usd: 3.10,
            session_count: 40,
        },
        daily: vec![],
    };

    reconcile_sync_payload(&store, &stale_payload).unwrap();

    // 验证状态未被回退
    store
        .with_read(|conn| {
            let (name, last_synced): (String, String) = conn
                .query_row(
                    "SELECT device_name, last_synced_at FROM sync_state WHERE device_id = 'dev-remote'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(name, "Desktop Linux", "Device name must not be overwritten by stale payload");
            assert_eq!(last_synced, "2026-09-20T12:00:00Z", "Timestamp must not regress");
            Ok(())
        })
        .unwrap();
}
