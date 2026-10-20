//! sources 领域回归测试：链接/摘录区分、校验、收藏与论点引用。

use super::*;
use crate::storage::Store;

fn setup() -> Store {
    Store::open_in_memory().unwrap()
}

fn link_input() -> SourceInput {
    SourceInput {
        name: "公司公告".into(),
        kind: "link".into(),
        url: "https://example.com/a".into(),
        related_symbol: "600519".into(),
        thesis_id: None,
        publisher: "交易所".into(),
        published_at: Some("2026-09-30".into()),
        excerpt_text: String::new(),
    }
}

#[test]
fn link_requires_url_and_excerpt_requires_text() {
    let store = setup();
    store
        .with_write(|conn| create_source(conn, &link_input()))
        .unwrap();
    // 摘录缺正文拒绝。
    let mut excerpt = link_input();
    excerpt.kind = "excerpt".into();
    excerpt.url = String::new();
    let err = store.with_write(|conn| create_source(conn, &excerpt));
    assert!(matches!(err, Err(SourceError::Invalid(_))));
    excerpt.excerpt_text = "2026Q3 营收同比增长 12%".into();
    store
        .with_write(|conn| create_source(conn, &excerpt))
        .unwrap();
    // 链接缺 URL 拒绝；URL 协议校验。
    let mut no_url = link_input();
    no_url.url = String::new();
    assert!(matches!(
        store.with_write(|conn| create_source(conn, &no_url)),
        Err(SourceError::Invalid(_))
    ));
    let mut bad_url = link_input();
    bad_url.url = "ftp://x".into();
    assert!(matches!(
        store.with_write(|conn| create_source(conn, &bad_url)),
        Err(SourceError::Invalid(_))
    ));
}

#[test]
fn excerpt_fetched_at_set_on_create_and_refreshed_on_text_change() {
    let store = setup();
    let mut input = link_input();
    input.kind = "excerpt".into();
    input.excerpt_text = "原文 A".into();
    let id = store
        .with_write(|conn| create_source(conn, &input))
        .unwrap();
    let row =
        store
            .with_read(|conn| {
                conn.query_row(
                "SELECT excerpt_text, excerpt_fetched_at IS NOT NULL FROM info_sources WHERE id=?1",
                params![id],
                |r| Ok((r.get::<_, String>(0).unwrap(), r.get::<_, i64>(1).unwrap() == 1)),
            )
            })
            .unwrap();
    assert_eq!(row.0, "原文 A");
    assert!(row.1, "摘录创建时记录取得时间");
    // 正文不变：取得时间不伪刷新。
    input.excerpt_text = "原文 A".into();
    store
        .with_write(|conn| update_source(conn, id, &input))
        .unwrap();
    // 正文变化：取得时间刷新（值更新，非 NULL）。
    input.excerpt_text = "原文 B（修订）".into();
    store
        .with_write(|conn| update_source(conn, id, &input))
        .unwrap();
    let text: String = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT excerpt_text FROM info_sources WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(text, "原文 B（修订）");
}

/// 策略版本行为（方案 05 §2）：编辑递增 version；归档后可恢复；
/// 归档及恢复递增 version；列表输出 version。
#[test]
fn thesis_update_bumps_version_and_restore_works() {
    let store = setup();
    let id = store
        .with_write(|conn| {
            create_thesis(
                conn,
                &ThesisInput {
                    name: "策略A".into(),
                    holding_reason: "理由".into(),
                    horizon: "3 年".into(),
                    verify_metrics: "指标".into(),
                    invalid_condition: "失效".into(),
                    related_symbols: vec!["600519".into()],
                },
            )
        })
        .unwrap();
    // 编辑递增 version。
    store
        .with_write(|conn| {
            update_thesis(
                conn,
                id,
                &ThesisInput {
                    name: "策略A v2".into(),
                    holding_reason: "新理由".into(),
                    horizon: "5 年".into(),
                    verify_metrics: "新指标".into(),
                    invalid_condition: "新失效".into(),
                    related_symbols: vec!["600519".into()],
                },
            )
        })
        .unwrap();
    let version: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT version FROM investment_theses WHERE id=?1",
                [id],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(version, 2, "编辑必须递增版本");
    // 归档 → 不在默认列表 → 恢复 → 回到列表且旧版本失效。
    store.with_write(|conn| archive_thesis(conn, id)).unwrap();
    assert!(store
        .with_read(|conn| list_theses(conn))
        .unwrap()
        .is_empty());
    store.with_write(|conn| restore_thesis(conn, id)).unwrap();
    let rows = store.with_read(|conn| list_theses(conn)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].version, 4, "归档及恢复推进版本");
    assert!(matches!(
        store.with_read(|conn| require_thesis_version(conn, id, 2)),
        Err(SourceError::Conflict)
    ));
    assert_eq!(rows[0].name, "策略A v2");
}

#[test]
fn favorite_read_archive_and_thesis_reference() {
    let store = setup();
    let thesis_id = store
        .with_write(|conn| {
            create_thesis(
                conn,
                &ThesisInput {
                    name: "消费龙头长持".into(),
                    holding_reason: "品牌护城河".into(),
                    horizon: "3 年".into(),
                    verify_metrics: "营收/份额".into(),
                    invalid_condition: "份额连续两季下滑".into(),
                    related_symbols: vec!["600519".into()],
                },
            )
        })
        .unwrap();
    let mut input = link_input();
    input.thesis_id = Some(thesis_id);
    let id = store
        .with_write(|conn| create_source(conn, &input))
        .unwrap();
    // 不存在的论点拒绝。
    let mut bad = link_input();
    bad.thesis_id = Some(999);
    assert!(matches!(
        store.with_write(|conn| create_source(conn, &bad)),
        Err(SourceError::NotFound(_))
    ));
    // 收藏与已读。
    store
        .with_write(|conn| set_source_flags(conn, id, Some(true), Some(true)))
        .unwrap();
    let rows = store.with_read(|conn| list_sources(conn, false)).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].favorite && rows[0].read_status);
    // 归档保留历史论点及引用，不删除用户数据。
    store
        .with_write(|conn| archive_thesis(conn, thesis_id))
        .unwrap();
    let kept: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM investment_theses WHERE id=?1 AND archived=1",
                [thesis_id],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(kept, 1);
    store.with_write(|conn| archive_source(conn, id)).unwrap();
    let remaining = store.with_read(|conn| list_sources(conn, false)).unwrap();
    assert!(remaining.is_empty());
    // 归档信息源在 include_archived 下可见。
    let all = store.with_read(|conn| list_sources(conn, true)).unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].archived);
}
