use super::*;

#[test]
fn task_registry_cancel_semantics() {
    let registry = TaskRegistry::new();
    let g1 = registry.start();
    assert!(!registry.is_cancelled(g1));
    let g2 = registry.start();
    assert!(
        registry.is_cancelled(g1),
        "previous generation must be cancelled"
    );
    assert!(!registry.is_cancelled(g2));
    registry.cancel_current();
    assert!(registry.is_cancelled(g2));
    let mut wrote = false;
    let result = registry
        .commit(g2, || {
            wrote = true;
            Ok::<_, ()>(())
        })
        .unwrap();
    assert!(result.is_none() && !wrote);
}

#[test]
fn review_alerts_idempotent_and_reopen_on_content_change() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::migration::SCHEMA_V11_SQL)
        .unwrap();
    conn.execute_batch(crate::migration_late::SCHEMA_V17_SQL)
        .unwrap();
    let now = "2026-10-01T00:00:00Z";
    let source_ref = "em_stock_ann:AN1";
    upsert_review_alert(
        &conn,
        "security:CN:stock:600519",
        source_ref,
        1,
        "回购公告",
        "repurchase",
        now,
    )
    .unwrap();
    upsert_review_alert(
        &conn,
        "security:CN:stock:600519",
        source_ref,
        1,
        "回购公告",
        "repurchase",
        now,
    )
    .unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM information_alerts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "重采/双窗口不重复提醒");
    // 解除后同版本重采：不重新提醒。
    conn.execute(
        "UPDATE information_alerts SET status='resolved', resolved_at=?1 WHERE source_ref=?2",
        rusqlite::params![now, source_ref],
    )
    .unwrap();
    upsert_review_alert(
        &conn,
        "security:CN:stock:600519",
        source_ref,
        1,
        "回购公告",
        "repurchase",
        now,
    )
    .unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM information_alerts WHERE source_ref=?1",
            [source_ref],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "resolved", "同版本重采不重新提醒");
    // 内容更正（版本升级）：重新提醒。
    upsert_review_alert(
        &conn,
        "security:CN:stock:600519",
        source_ref,
        2,
        "回购公告（更正）",
        "repurchase",
        now,
    )
    .unwrap();
    let (status, ver): (String, i64) = conn
        .query_row(
            "SELECT status, content_version FROM information_alerts WHERE source_ref=?1",
            [source_ref],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "open", "实质内容变化重新提醒");
    assert_eq!(ver, 2);
    // 解除后无变化重采：不重新提醒。
    conn.execute(
        "UPDATE information_alerts SET status='resolved', resolved_at=?1 WHERE source_ref=?2",
        rusqlite::params![now, source_ref],
    )
    .unwrap();
    upsert_review_alert(
        &conn,
        "security:CN:stock:600519",
        source_ref,
        2,
        "回购公告（更正）",
        "repurchase",
        now,
    )
    .unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM information_alerts WHERE source_ref=?1",
            [source_ref],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "resolved");
}

#[test]
fn instrument_refs_are_stable_and_typed() {
    assert_eq!(
        instrument_ref_for_code("600519"),
        "security:CN:stock:600519"
    );
    assert_eq!(instrument_ref_for_fund("510300"), "security:CN:fund:510300");
    assert_eq!(instrument_ref_for_keyword("贵州茅台"), "keyword:贵州茅台");
}

#[test]
fn public_source_symbols_accept_local_prefixes_without_collapsing_foreign_markets() {
    assert_eq!(public_stock_code("sh600519"), Some("600519".into()));
    assert_eq!(
        public_stock_code("security:SSE:600519"),
        Some("600519".into())
    );
    for code in [
        "security:HKEX:600519",
        "security:SZSE:600519",
        "hk00700",
        "AAPL",
        "00700",
    ] {
        assert_eq!(public_stock_code(code), None);
    }
}

#[test]
fn collection_windows_and_identity_batches_have_exact_boundaries() {
    assert_eq!(
        normalize::window_start("2026-03-01T00:00:00Z", 7).unwrap(),
        "2026-02-23T00:00:00Z"
    );
    assert_eq!(
        normalize::window_start("2026-03-01T00:00:00Z", 1).unwrap(),
        "2026-03-01T00:00:00Z"
    );
    assert!(normalize::window_start("2026-02-30T00:00:00Z", 7).is_none());
    let funds: Vec<String> = (0..35).map(|n| format!("fund{n}")).collect();
    let stocks: Vec<String> = (0..25).map(|n| format!("stock{n}")).collect();
    let (first_funds, first_stocks, next) = scope::identity_batch(&funds, &stocks, 0);
    assert_eq!(
        (first_funds.len(), first_stocks.len(), next),
        (35, 15, Some(50))
    );
    let (second_funds, second_stocks, next) = scope::identity_batch(&funds, &stocks, 50);
    assert_eq!(
        (second_funds.len(), second_stocks.len(), next),
        (0, 10, None)
    );
    assert_eq!(second_stocks[0], "stock15");
}
