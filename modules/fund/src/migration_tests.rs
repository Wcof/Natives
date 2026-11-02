use super::*;
use crate::finance::FinanceError;
use crate::storage::Store;

// health/initialize 均来自 BuiltInAppModule trait，测试模块需显式引入。
use app_runtime_core::module::BuiltInAppModule;

fn open_fresh(data_dir: &Path) -> Connection {
    std::fs::create_dir_all(data_dir).unwrap();
    Connection::open(data_dir.join("fund.db")).unwrap()
}

#[test]
fn v28_information_decisions_upgrade_preserves_v27_data_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    for step in all_migrations().iter().filter(|step| step.version <= 27) {
        conn.execute_batch(step.sql).unwrap();
        conn.pragma_update(None, "user_version", step.version)
            .unwrap();
    }
    conn.execute(
        "INSERT INTO accounts(name,created_at,updated_at) VALUES('preserved','0','0')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO information_items(source_id,external_id,title,retrieved_at,content_hash,created_at,updated_at) VALUES('fixture','preserved','preserved','0','hash','0','0')", []).unwrap();
    assert_eq!(
        run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    assert_eq!(
        run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    assert_eq!(
        conn.query_row("SELECT name FROM accounts", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
    assert_eq!(
        conn.query_row("SELECT title FROM information_items", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM information_decisions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    assert!(conn.execute("INSERT INTO information_decisions(information_id,report_id,action,note,request_id,created_at) VALUES(999,999,'observe','invalid','invalid-reference','0')", []).is_err());
}

#[test]
fn fresh_database_migrates_to_target_and_commits() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    let result = run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    assert_eq!(result, crate::storage::SCHEMA_VERSION);
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
    assert_eq!(status.current_schema, crate::storage::SCHEMA_VERSION);
    assert_eq!(status.readiness, "ready");
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
fn migrates_from_v1_to_target() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    // 模拟已存在且只应用了 v1 的旧库
    conn.execute_batch(crate::storage::SCHEMA_V1).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();

    let target = run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    assert_eq!(target, crate::storage::SCHEMA_VERSION);

    let user_ver: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(user_ver, crate::storage::SCHEMA_VERSION);

    // 验证 v2 新增的 watchlists / chart_drawings 表已存在且可读写
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM watchlists", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    // v3/v4 新增表同样存在。
    for table in [
        "manual_assets",
        "fund_asset_metadata",
        "investment_goals",
        "allocation_targets",
    ] {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

/// 顺序迁移：v2 旧库 → v4 保留全部既有数据（基金/流水/净值/自选），
/// 幂等重跑不丢数据（方案 03 P1 验收）。
#[test]
fn migrates_v2_to_v4_preserves_data_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    conn.execute_batch(crate::storage::SCHEMA_V1).unwrap();
    conn.execute_batch(SCHEMA_V2_SQL).unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    // v2 时代的既有业务数据。
    conn.execute(
        "INSERT INTO accounts(name, created_at, updated_at) VALUES('旧账户','x','x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO funds(code, name, updated_at) VALUES('110022','消费行业','x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transactions(account_id, fund_id, type, quantity_raw, price_raw, amount_raw, trade_date, created_at)
         SELECT a.id, f.id, 'BUY', 1000000, 1234500, 1234500, '2026-01-01', 'x'
         FROM accounts a, funds f WHERE a.name='旧账户' AND f.code='110022'",
        [],
    )
    .unwrap();
    let tx_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();

    let target = run_migrations(dir.path(), &conn, "2.5.0").unwrap();
    assert_eq!(target, crate::storage::SCHEMA_VERSION);
    // 旧数据仍在。
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        tx_before
    );
    // v3/v4 表可写（新约束生效）。
    conn.execute(
        "INSERT INTO fund_asset_metadata(fund_id, nav_currency, risk_class, updated_at)
         SELECT id, 'CNY', 'equity', 'x' FROM funds WHERE code='110022'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO investment_goals(id, target_minor, target_date, updated_at) VALUES(1, 200000, '2027-12-31', 'x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO allocation_targets(risk_class, target_tenths, lower_tenths, upper_tenths, updated_at)
         VALUES('equity', 500, 400, 600, 'x')",
        [],
    )
    .unwrap();
    // 幂等重跑：再次 run_migrations 无变化、数据不丢。
    run_migrations(dir.path(), &conn, "2.5.0").unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        tx_before
    );
}

fn object_names(conn: &Connection, kind: &str) -> std::collections::BTreeSet<String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT name FROM sqlite_master WHERE type='{kind}' AND name NOT LIKE 'sqlite_%'"
        ))
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .unwrap()
}

/// 新库初始化与「空库逐版迁移到目标」必须得到同一套表——否则只会让旧库
/// 升级而新库漏表（方案 03 P1/P2 明文要求）。这是一个向前防护：将来新增
/// v5 表却只加进 MIGRATIONS 时，本测试失败。
#[test]
fn fresh_init_and_migration_ladder_produce_identical_tables() {
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh_store = Store::open(fresh_dir.path()).unwrap();
    assert_eq!(
        fresh_store.schema_version().unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    let fresh_objects = fresh_store
        .with_read(|conn| Ok::<_, rusqlite::Error>(object_names(conn, "table")))
        .unwrap();
    let fresh_indexes = fresh_store
        .with_read(|conn| Ok::<_, rusqlite::Error>(object_names(conn, "index")))
        .unwrap();

    let migrated_dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(migrated_dir.path());
    run_migrations(migrated_dir.path(), &conn, "0.1.0").unwrap();

    assert_eq!(
        fresh_objects,
        object_names(&conn, "table"),
        "新库初始化与迁移阶梯的表集合不一致"
    );
    assert_eq!(
        fresh_indexes,
        object_names(&conn, "index"),
        "新库初始化与迁移阶梯的索引集合不一致"
    );
    for table in [
        "manual_assets",
        "fund_asset_metadata",
        "investment_goals",
        "allocation_targets",
    ] {
        assert!(fresh_objects.contains(table), "新库缺少 {table}");
    }
    // 索引也必须一致（v2 升级路径曾漏掉 idx_drawings_symbol）。
    assert!(fresh_indexes.contains("idx_drawings_symbol"));
}

/// 空库（fund.db 存在但 user_version=0）必须与缺库区分，且 currentSchema
/// 不得与 readiness 矛盾（方案 02 §2 readiness 验收）。
#[test]
fn read_data_status_distinguishes_missing_empty_and_unreadable() {
    // 缺库。
    let missing = tempfile::tempdir().unwrap();
    let status = read_data_status(missing.path(), "0.1.0");
    assert_eq!(status.readiness, readiness::UNINITIALIZED);
    assert_eq!(status.current_schema, crate::storage::SCHEMA_VERSION);

    // 空库：文件存在、无 schema（建库中断也落在这一态）。
    let empty = tempfile::tempdir().unwrap();
    drop(open_fresh(empty.path()));
    let status = read_data_status(empty.path(), "0.1.0");
    assert_eq!(status.readiness, readiness::UNINITIALIZED);
    assert_eq!(
        status.current_schema, 0,
        "空库的 currentSchema 必须如实报 0"
    );

    // schema 低于目标。
    let older = tempfile::tempdir().unwrap();
    let conn = open_fresh(older.path());
    conn.pragma_update(None, "user_version", crate::storage::SCHEMA_VERSION - 1)
        .unwrap();
    drop(conn);
    let status = read_data_status(older.path(), "0.1.0");
    assert_eq!(status.readiness, readiness::MIGRATION_REQUIRED);
    assert_eq!(status.current_schema, crate::storage::SCHEMA_VERSION - 1);

    // schema 高于目标。
    let newer = tempfile::tempdir().unwrap();
    let conn = open_fresh(newer.path());
    conn.pragma_update(None, "user_version", crate::storage::SCHEMA_VERSION + 1)
        .unwrap();
    drop(conn);
    assert_eq!(
        read_data_status(newer.path(), "0.1.0").readiness,
        readiness::INCOMPATIBLE
    );

    // 不可读：不是 SQLite 文件，不得当成空库或已就绪。
    let broken = tempfile::tempdir().unwrap();
    std::fs::write(
        broken.path().join("fund.db"),
        b"not a sqlite database at all",
    )
    .unwrap();
    let status = read_data_status(broken.path(), "0.1.0");
    assert_eq!(status.readiness, readiness::UNREADABLE);
    assert_eq!(status.current_schema, 0);
}

/// committed journal 是迁移成功后的正常残留：readiness 与 health 都必须放行，
/// 不能因为文件存在就让健康状态永久为 not-ok（app.rs local_health）。
#[test]
fn committed_journal_is_not_a_health_failure() {
    let root = tempfile::tempdir().unwrap();
    // ModuleContext::for_app 约定：模块数据根 = <apps_root>/<app_id>/data。
    let data_dir = root.path().join("fund").join("data");
    let conn = open_fresh(&data_dir);
    run_migrations(&data_dir, &conn, "0.1.0").unwrap();
    drop(conn);
    assert_eq!(read_data_status(&data_dir, "0.1.0").readiness, "ready");

    let mut module = crate::FundModule::new();
    let ctx = app_runtime_core::ModuleContext::for_app(root.path(), "fund", "0.1.0", 1);
    module.initialize(&ctx).unwrap();
    let health = module.health().unwrap();
    assert!(
        health.ok,
        "committed journal 不得让 health 失败：ok={} detail={}",
        health.ok, health.detail
    );
    assert!(health.detail.contains("READY"), "detail={}", health.detail);
}

/// v19 账户元数据／幂等表：旧库（v18）升级结果必须与新库初始化逐列一致，
/// 既有账户行获得安全默认值，升级后领域操作可直接使用新列（R2 T03 / A17）。
#[test]
fn v18_to_v19_account_metadata_matches_fresh_init() {
    fn columns(
        conn: &Connection,
        table: &str,
    ) -> std::collections::BTreeSet<(String, String, i64, Option<String>, i64)> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }
    fn account_indexes(conn: &Connection) -> std::collections::BTreeSet<(String, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT name, sql FROM sqlite_master WHERE type='index' AND tbl_name='accounts'",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    // 旧库：阶梯迁移到 v18，并写入 v18 时代的既有账户。
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    for step in all_migrations().iter().filter(|m| m.version <= 18) {
        conn.execute_batch(step.sql).unwrap();
    }
    conn.pragma_update(None, "user_version", 18).unwrap();
    conn.execute(
        "INSERT INTO accounts(name, created_at, updated_at) VALUES('旧账户','x','x')",
        [],
    )
    .unwrap();

    let target = run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    assert_eq!(target, crate::storage::SCHEMA_VERSION);
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::storage::SCHEMA_VERSION
    );

    // 既有行获得安全默认值（version=1、order=0、未归档）。
    let row: (i64, i64, Option<String>) = conn
        .query_row(
            "SELECT version, sort_order, archived_at FROM accounts WHERE name='旧账户'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, (1, 0, None));

    // 与新库初始化逐列／逐索引一致。
    // v19 只新增 accounts 元数据三列；v20 之后的列（含 v30 的 channel/currency）
    // 由各自迁移负责，不属于本测试范围——全表全量一致性由
    // fresh_init_and_migration_ladder_produce_identical_tables 保障。
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh_store = crate::storage::Store::open(fresh_dir.path()).unwrap();
    fresh_store
        .with_read(|fconn| {
            let fresh_cols = columns(fconn, "accounts");
            let migrated_cols = columns(&conn, "accounts");
            for name in ["version", "sort_order", "archived_at"] {
                let expected = fresh_cols
                    .iter()
                    .find(|c| c.0 == name)
                    .unwrap_or_else(|| panic!("新库缺少 accounts.{name}"));
                let actual = migrated_cols
                    .iter()
                    .find(|c| c.0 == name)
                    .unwrap_or_else(|| panic!("v19 升级后缺少 accounts.{name}"));
                assert_eq!(expected, actual, "accounts.{name} 定义与新库不一致");
            }
            assert_eq!(
                columns(fconn, "account_ops"),
                columns(&conn, "account_ops"),
                "v19 account_ops 列集合与新库不一致"
            );
            assert_eq!(
                account_indexes(fconn),
                account_indexes(&conn),
                "v19 accounts 索引集合与新库不一致"
            );
            Ok::<(), rusqlite::Error>(())
        })
        .unwrap();
    drop(fresh_store);

    // 升级后领域操作直接可用：改名推进版本。
    let id: i64 = conn
        .query_row("SELECT id FROM accounts WHERE name='旧账户'", [], |r| {
            r.get(0)
        })
        .unwrap();
    crate::accounts::update_account(&conn, id, Some("改名账户"), None, 1, "u1").unwrap();
    let version: i64 = conn
        .query_row("SELECT version FROM accounts WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, 2);

    // 幂等重跑：不新增备份。
    drop(conn);
    let conn = open_fresh(dir.path());
    run_migrations(dir.path(), &conn, "0.1.0").unwrap();
    let backups = std::fs::read_dir(dir.path().join("backups"))
        .unwrap()
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn v21_observation_upgrade_preserves_rows_and_matches_new_schema() {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("fund.db")).unwrap();
    for step in crate::migration::all_migrations()
        .iter()
        .filter(|m| m.version <= 20)
    {
        conn.execute_batch(step.sql).unwrap();
        conn.pragma_update(None, "user_version", step.version)
            .unwrap();
    }
    conn.execute(
        "INSERT INTO opportunity_snapshots(snapshot_id,facts_key,data_revision,generated_at,projection_json) VALUES('old-v20','old-facts','9','1727900000','{\"snapshotId\":\"old-v20\",\"items\":[]}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO opportunity_feedback(snapshot_id,candidate_id,version,useful,reason,created_at) VALUES('old-v20','old-candidate',1,1,'kept','1727900001')",
        [],
    )
    .unwrap();
    assert_eq!(
        crate::migration::run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    assert_eq!(
        crate::migration::run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION,
        "v21 迁移幂等"
    );
    let kept: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM opportunity_snapshots WHERE snapshot_id='old-v20'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "旧 v20 数据保留");
    let schema = |c: &rusqlite::Connection| {
        c.prepare("SELECT sql FROM sqlite_master WHERE name='opportunity_observations'")
            .unwrap()
            .query_row([], |r| r.get::<_, String>(0))
            .unwrap()
    };
    let columns = |c: &rusqlite::Connection| {
        c.prepare("PRAGMA table_info(opportunity_observations)")
            .unwrap()
            .query_map([], |r| Ok(r.get::<_, String>(1)?))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let fresh = Store::open_in_memory().unwrap();
    assert_eq!(
        schema(&conn),
        fresh
            .with_read(|c| Ok::<_, FinanceError>(schema(c)))
            .unwrap()
    );
    assert_eq!(
        columns(&conn),
        fresh
            .with_read(|c| Ok::<_, FinanceError>(columns(c)))
            .unwrap()
    );
}

/// v30 新增的资产模型表清单（设计文档 docs/development/invest-v2-design.md）。
const V30_TABLES: &[&str] = &[
    "instruments",
    "flows",
    "holdings",
    "quotes",
    "fx_rates",
    "snapshots",
    "snapshot_items",
    "ai_evaluations",
    "ai_settings",
];

/// v30 必须同时满足：旧数据完好、旧表未被改动、新表可读、accounts 取到默认渠道、
/// 重复执行幂等、无 FK 违规。这是 P1 阶段的唯一验收依据。
#[test]
fn v30_asset_model_upgrade_preserves_v29_data_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    for step in all_migrations().iter().filter(|step| step.version <= 29) {
        conn.execute_batch(step.sql).unwrap();
        conn.pragma_update(None, "user_version", step.version)
            .unwrap();
    }

    // v30 之前新表不存在，确保本测试真的在验证迁移而不是空跑
    assert!(
        conn.query_row("SELECT COUNT(*) FROM instruments", [], |r| r
            .get::<_, i64>(0))
            .is_err(),
        "v29 时不应存在 instruments 表"
    );

    conn.execute(
        "INSERT INTO accounts(name,note,created_at,updated_at) VALUES('华泰证券','主力','0','0')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO funds(code,name,fund_type,updated_at) VALUES('000001','华夏成长','混合','0')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transactions(account_id,fund_id,type,quantity_raw,price_raw,amount_raw,trade_date,created_at)
         VALUES(1,1,'BUY',1000000,15000,1500000,'2026-01-02','0')",
        [],
    )
    .unwrap();

    assert_eq!(
        run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    // 幂等：重复执行不重复应用
    assert_eq!(
        run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION
    );

    // 旧数据逐项完好
    assert_eq!(
        conn.query_row("SELECT name FROM accounts WHERE id=1", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "华泰证券"
    );
    assert_eq!(
        conn.query_row("SELECT code FROM funds WHERE id=1", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "000001"
    );
    assert_eq!(
        conn.query_row(
            "SELECT quantity_raw FROM transactions WHERE account_id=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1000000
    );

    // accounts 新列对既有行取默认值，不破坏 CHECK
    let (channel, currency): (String, String) = conn
        .query_row(
            "SELECT channel,currency FROM accounts WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(channel, "domestic_broker");
    assert_eq!(currency, "CNY");

    // 新表全部可读且为空
    for table in V30_TABLES {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or_else(|e| panic!("{table} 不可读: {e}"));
        assert_eq!(count, 0, "{table} 新建时应为空");
    }

    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );

    // 新库走的是「SCHEMA_V1 + 逐版增量 SQL」的独立路径，不是迁移阶梯。
    // 两条路径必须同时含 v30 新表，否则全新用户会缺表。
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh = crate::storage::Store::open(fresh_dir.path()).unwrap();
    assert_eq!(
        fresh.schema_version().unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    fresh
        .with_read(|c| {
            for table in V30_TABLES {
                let count: i64 = c
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                    .unwrap_or_else(|e| panic!("新库缺少 {table}: {e}"));
                assert_eq!(count, 0, "新库 {table} 应为空");
            }
            Ok::<(), rusqlite::Error>(())
        })
        .unwrap();
}

/// v30 的业务约束必须真正生效：未知事件类型、缺比例的拆股、越界评分都要被拒绝。
/// 这些是回放正确性的前提，不能只靠应用层校验。
#[test]
fn v30_flow_and_evaluation_constraints_reject_invalid_rows() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_fresh(dir.path());
    run_migrations(dir.path(), &conn, "test").unwrap();

    conn.execute(
        "INSERT INTO accounts(name,channel,currency,created_at,updated_at)
         VALUES('币安','crypto_exchange','USDT','0','0')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO instruments(code,name,asset_class,market,currency,created_at,updated_at)
         VALUES('BTC','比特币','crypto','CRYPTO','USDT','0','0')",
        [],
    )
    .unwrap();

    // 合法买入：0.12 枚 × 65000 USDT
    conn.execute(
        "INSERT INTO flows(account_id,instrument_id,type,quantity_raw,price_raw,amount_raw,trade_date,created_at)
         VALUES(1,1,'BUY',1200,650000000,78000,'2026-01-02','0')",
        [],
    )
    .unwrap();

    // SPLIT 缺比例：数量无法回放，必须拒绝
    assert!(
        conn.execute(
            "INSERT INTO flows(account_id,instrument_id,type,quantity_raw,trade_date,created_at)
             VALUES(1,1,'SPLIT',1000,'2026-02-02','0')",
            [],
        )
        .is_err(),
        "缺 split_from/split_to 的拆股应被拒绝"
    );

    // 未知事件类型
    assert!(
        conn.execute(
            "INSERT INTO flows(account_id,instrument_id,type,trade_date,created_at)
             VALUES(1,1,'TRANSFER','2026-02-02','0')",
            [],
        )
        .is_err(),
        "未登记的事件类型应被拒绝"
    );

    // 合法 SPLIT：1 拆 2
    conn.execute(
        "INSERT INTO flows(account_id,instrument_id,type,split_from,split_to,trade_date,created_at)
         VALUES(1,1,'SPLIT',1,2,'2026-03-02','0')",
        [],
    )
    .unwrap();

    // 评分越界
    assert!(
        conn.execute(
            "INSERT INTO ai_evaluations(account_id,instrument_id,score,tone,label,summary,snapshot_json,created_at)
             VALUES(1,1,101,'steady','标签','摘要','{}','0')",
            [],
        )
        .is_err(),
        "评分超出 0-100 应被拒绝"
    );

    // 合法评估
    conn.execute(
        "INSERT INTO ai_evaluations(account_id,instrument_id,score,tone,label,summary,snapshot_json,created_at)
         VALUES(1,1,72,'steady','仓位偏重','单一标的占比偏高，注意分散。','{}','0')",
        [],
    )
    .unwrap();
}
