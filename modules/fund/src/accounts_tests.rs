//! 账户元数据域测试（R2 T03 / A13、A17）。

use super::*;
use crate::ledger::{self, LedgerError};
use crate::storage::Store;

fn create(store: &Store, name: &str) -> i64 {
    store
        .with_write(|conn| ledger::ensure_account(conn, name))
        .unwrap()
}

fn meta(store: &Store, id: i64) -> AccountMeta {
    store
        .with_read(|conn| get_account(conn, id))
        .unwrap()
        .unwrap()
}

fn update(
    store: &Store,
    id: i64,
    name: Option<&str>,
    order: Option<i64>,
    expected: i64,
    rid: &str,
) -> Result<String, LedgerError> {
    store.with_write(|conn| update_account(conn, id, name, order, expected, rid))
}

#[test]
fn new_accounts_carry_default_metadata() {
    let store = Store::open_in_memory().unwrap();
    let a = create(&store, "招商");
    let b = create(&store, "东财");
    assert_eq!(
        meta(&store, a),
        crate::accounts::AccountMeta {
            id: a,
            name: "招商".into(),
            version: 1,
            order: 0,
            archived: false,
        }
    );
    let (rows, total) = store
        .with_read(|conn| crate::accounts::list_detail(conn, "active", "", 50, 0))
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a, b]);
}

#[test]
fn rename_reorder_archive_restore_keep_history_and_version() {
    let store = Store::open_in_memory().unwrap();
    let id = create(&store, "旧名");
    // 改名：version 1→2。
    update(&store, id, Some("新名"), None, 1, "r1").unwrap();
    assert_eq!(meta(&store, id).version, 2);
    assert_eq!(meta(&store, id).name, "新名");
    // 排序：2→3；旧名称数组按 (sort_order,id) 呈现。
    let other = create(&store, "另一户");
    update(&store, id, None, Some(5), 2, "r2").unwrap();
    let names = store
        .with_read(|conn| crate::accounts::active_names(conn))
        .unwrap();
    assert_eq!(names, vec!["另一户".to_string(), "新名".to_string()]);
    assert_eq!(meta(&store, id).version, 3);
    // 陈旧版本 → 冲突。
    assert!(matches!(
        update(&store, id, Some("再改"), None, 1, "r3"),
        Err(LedgerError::Conflict(_))
    ));
    // 归档：隐藏于旧名称数组与 active 视图，归档视图仍含历史关联（ID 不变）。
    store
        .with_write(|conn| crate::accounts::archive_account(conn, id, 3, "r4"))
        .unwrap();
    assert!(meta(&store, id).archived);
    assert_eq!(
        store
            .with_read(|conn| crate::accounts::active_names(conn))
            .unwrap(),
        vec!["另一户".to_string()]
    );
    // 自动建档不能命中归档账户。
    assert!(matches!(
        store.with_write(|conn| ledger::ensure_account(conn, "新名")),
        Err(LedgerError::Conflict(_))
    ));
    // 恢复：同一 ID。
    store
        .with_write(|conn| crate::accounts::restore_account(conn, id, 4, "r5"))
        .unwrap();
    let restored = meta(&store, id);
    assert_eq!(restored.id, id);
    assert!(!restored.archived);
    assert_eq!(restored.version, 5);
    // 重复恢复 → 冲突（状态不符）。
    assert!(matches!(
        store.with_write(|conn| crate::accounts::restore_account(conn, id, 5, "r6")),
        Err(LedgerError::Conflict(_))
    ));
    let _ = other;
}

#[test]
fn request_id_idempotency() {
    let store = Store::open_in_memory().unwrap();
    let id = create(&store, "甲户");
    let first = update(&store, id, Some("乙户"), None, 1, "same-rid").unwrap();
    assert_eq!(meta(&store, id).name, "乙户");
    // 同 rid 同载荷：返回缓存结果，版本不再推进。
    let again = update(&store, id, Some("乙户"), None, 1, "same-rid").unwrap();
    assert_eq!(first, again);
    assert_eq!(meta(&store, id).version, 2);
    // 同 rid 异载荷：冲突。
    assert!(matches!(
        update(&store, id, Some("丙户"), None, 2, "same-rid"),
        Err(LedgerError::Conflict(_))
    ));
    assert_eq!(meta(&store, id).name, "乙户");
}

#[test]
fn name_validation_and_uniqueness() {
    let store = Store::open_in_memory().unwrap();
    let a = create(&store, "甲");
    let b = create(&store, "乙");
    assert!(matches!(
        update(&store, a, Some(""), None, 1, "n1"),
        Err(LedgerError::Invalid(_))
    ));
    let long = "x".repeat(70);
    assert!(matches!(
        update(&store, a, Some(&long), None, 1, "n2"),
        Err(LedgerError::Invalid(_))
    ));
    // 与既有账户（含归档）重名拒绝。
    assert!(matches!(
        update(&store, a, Some("乙"), None, 1, "n3"),
        Err(LedgerError::Invalid(_))
    ));
    store
        .with_write(|conn| crate::accounts::archive_account(conn, b, 1, "n4"))
        .unwrap();
    assert!(matches!(
        update(&store, a, Some("乙"), None, 1, "n5"),
        Err(LedgerError::Invalid(_))
    ));
    // 名称未变（只传原 name）：changed=false，不推进版本。
    let result = update(&store, a, Some("甲"), None, 1, "n6").unwrap();
    assert!(result.contains("\"changed\":false"));
    assert_eq!(meta(&store, a).version, 1);
}

#[test]
fn q_filter_escapes_like_wildcards() {
    let store = Store::open_in_memory().unwrap();
    create(&store, "a_b");
    create(&store, "axb");
    let (rows, total) = store
        .with_read(|conn| crate::accounts::list_detail(conn, "active", "a_b", 50, 0))
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0].name, "a_b");
}

#[test]
fn list_detail_pagination_and_views() {
    let store = Store::open_in_memory().unwrap();
    for name in ["c1", "c2", "c3"] {
        create(&store, name);
    }
    let c2 = meta(&store, 2).id;
    store
        .with_write(|conn| crate::accounts::archive_account(conn, c2, 1, "pa"))
        .unwrap();
    let (rows, total) = store
        .with_read(|conn| crate::accounts::list_detail(conn, "all", "", 2, 1))
        .unwrap();
    assert_eq!(total, 3);
    assert_eq!(
        rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["c2", "c3"]
    );
    let (rows, total) = store
        .with_read(|conn| crate::accounts::list_detail(conn, "archived", "", 50, 0))
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0].name, "c2");
}
