use super::*;
use crate::storage::Store;

fn open_fresh(data_dir: &Path) -> Connection {
    std::fs::create_dir_all(data_dir).unwrap();
    Connection::open(data_dir.join("fund.db")).unwrap()
}

#[test]
fn fresh_database_migrates_to_target_and_commits() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    let result = run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    assert_eq!(result, 2);
    let journal = read_journal(&dir.path().join(".migration.json")).unwrap();
    assert_eq!(journal.state, "committed");
    // 备份存在。
    assert!(dir
        .path()
        .join("backups")
        .join(&journal.backup_id)
        .join("fund.db.bak")
        .exists());
    // 库可用（Store 打开 + 写入）。
    let store = Store::open(dir.path()).unwrap();
    store
        .with_write(|c| {
            c.execute(
                "INSERT INTO accounts(name, created_at, updated_at) VALUES('A','x','x')",
                [],
            )
        })
        .unwrap();
}

#[test]
fn idempotent_when_already_at_target() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    // 再次运行：无变化、不新增备份。
    run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    let backups = std::fs::read_dir(dir.path().join("backups"))
        .unwrap()
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn pending_journal_blocks_migration() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    backup_database(dir.path(), "m0-to-1-x", &conn).unwrap();
    let journal = MigrationJournal {
        migration_id: "m0-to-1-x".into(),
        from_schema: 0,
        to_schema: 1,
        from_app_version: "0.0.9".into(),
        to_app_version: "0.1.0".into(),
        state: "migrating".into(),
        backup_id: "m0-to-1-x".into(),
        has_committed_new_writes: false,
        previous_version_compatible: true,
        migrated_at: None,
    };
    write_journal_atomic(&dir.path().join(".migration.json"), &journal).unwrap();
    let err = run_migrations(dir.path(), &conn, "0.1.0").unwrap_err();
    assert!(matches!(err, MigrationError::PendingJournal(_)));
    // 恢复前关闭旧连接，避免替换底层 db 文件导致句柄失效
    drop(conn);
    assert!(recover_pending_journal(dir.path()).unwrap());
    let conn = open_fresh(dir.path());
    run_migrations(dir.path(), &conn, "0.1.0").unwrap();
}

#[test]
fn backup_snapshot_is_readable_standalone() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    conn.execute_batch(crate::storage::SCHEMA_V1).unwrap();
    conn.execute(
        "INSERT INTO accounts(name, created_at, updated_at) VALUES('快照','x','x')",
        [],
    )
    .unwrap();
    let backup_path = backup_database(dir.path(), "snap-test", &conn).unwrap();
    let restored = Connection::open(&backup_path).unwrap();
    let name: String = restored
        .query_row("SELECT name FROM accounts LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "快照");
}

#[test]
fn recover_refuses_when_committed_new_writes() {
    let dir = tempfile::tempdir().unwrap();
    let journal = MigrationJournal {
        migration_id: "m0-to-1-fail".into(),
        from_schema: 0,
        to_schema: 1,
        from_app_version: "0.0.9".into(),
        to_app_version: "0.1.0".into(),
        state: "migrating".into(),
        backup_id: "m0-to-1-fail".into(),
        has_committed_new_writes: true,
        previous_version_compatible: true,
        migrated_at: None,
    };
    write_journal_atomic(&dir.path().join(".migration.json"), &journal).unwrap();
    let err = recover_pending_journal(dir.path()).unwrap_err();
    assert!(err.contains("committed new writes"));
}

#[test]
fn read_data_status_returns_accurate_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    run_migrations(dir.path(), &conn, "0.1.0").unwrap();

    let status = read_data_status(dir.path(), "0.1.0");
    assert_eq!(status.current_schema, 2);
    assert_eq!(status.migration_state, "committed");
    assert_eq!(status.has_committed_new_writes, false);

    let store = Store::open(dir.path()).unwrap();
    // 计划 §27.2：writer 版本由 Runtime 注入（产品版本），不再用模块 Cargo 版本。
    store.set_writer_version("0.1.0");
    store
        .with_write(|c| {
            c.execute(
                "INSERT INTO accounts(name, created_at, updated_at) VALUES('测试','x','x')",
                [],
            )
        })
        .unwrap();

    let status_after = read_data_status(dir.path(), "0.1.0");
    assert_eq!(status_after.has_committed_new_writes, true);
    assert_eq!(status_after.last_data_writer_version, "0.1.0");
}

#[test]
fn migrates_from_v1_to_v2() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    // 模拟已存在且只应用了 v1 的旧库
    conn.execute_batch(crate::storage::SCHEMA_V1).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();

    let target = run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    assert_eq!(target, 2);

    let user_ver: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(user_ver, 2);

    // 验证 v2 新增的 watchlists / chart_drawings 表已存在且可读写
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM watchlists", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
