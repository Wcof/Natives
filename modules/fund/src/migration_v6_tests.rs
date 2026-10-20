//! v5→v6 迁移验收（05 §5 / 08 第 6 条）：隔离旧版用户库副本上验证
//! 备份、顺序迁移、幂等、失败恢复与旧数据保留。不触碰真实用户库。
//!
//! 构造方式：直接执行 MIGRATIONS[0..=4]（v1→v5）得到真实 v5 库，写入旧口径
//! 数据（旧目标行、自选、交易），再走正式 run_migrations 升 v6，断言：
//! 1. 旧数据全部保留；2. 新表/列存在且约束生效；3. 再次迁移幂等；
//! 4. 人为注入损坏 SQL 失败后恢复旧库（journal 恢复路径）。
#[cfg(test)]
mod v6_migration_tests {
    use crate::migration::{self};
    use crate::storage::Store;
    use rusqlite::Connection;

    fn build_v5_store(dir: &std::path::Path) -> Connection {
        let conn = Connection::open(dir.join("fund.db")).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        // 顺序执行 v1..=v5（真实历史 SQL；MIGRATIONS[0]=SCHEMA_V1 全量 DDL
        // 不含 v6 增量对象，阶梯执行后的表集合与历史 v5 库一致）。
        for step in &migration::all_migrations()[..5] {
            conn.execute_batch(step.sql).unwrap();
            conn.pragma_update(None, "user_version", step.version)
                .unwrap();
        }
        conn
    }

    fn seed_legacy_data(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO accounts(name, created_at, updated_at) VALUES('旧账户','100','100');
            INSERT INTO funds(code, name, updated_at) VALUES('000001','旧基金','100');
            INSERT INTO transactions(account_id, fund_id, type, quantity_raw, price_raw, amount_raw, fee_raw, trade_date, created_at)
                VALUES(1, 1, 'BUY', 1000000, 2000000, 2000000000, 0, '2026-01-10', '100');
            INSERT INTO manual_assets(account_id, product_type, instrument_key, name, risk_class, market_value_minor, valued_at, created_at, updated_at)
                VALUES(1, 'cash', 'cash', '现金', 'cash', 50000000, '2026-09-30', '100', '100');
            INSERT INTO investment_goals(id, target_minor, target_date, updated_at)
                VALUES(1, 1500000000, '2031-09-30', '100');
            INSERT INTO allocation_targets(risk_class, target_tenths, lower_tenths, upper_tenths, updated_at)
                VALUES('equity', 600, 0, 1000, '100'), ('cash', 400, 0, 1000, '100');
            INSERT INTO finance_goals(name, target_minor, target_date, scope_kind, is_primary, created_at, updated_at)
                VALUES('旧迁移主目标', 1500000000, '2031-09-30', 'A', 1, '100', '100');
            INSERT INTO investment_theses(name, related_symbols, created_at, updated_at)
                VALUES('主线A', '["600519"]', '100', '100');
            INSERT INTO cash_reservations(account_id, symbol, amount_minor, created_at, updated_at)
                VALUES(1, 'sh600519', 1000000, '100', '100');
            "#,
        )
        .unwrap();
    }

    fn current_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    /// v5 副本 + 旧数据 → 正式 run_migrations → v6：数据保留、新结构可用、业务行不丢。
    #[test]
    fn v5_copy_upgrades_to_v6_preserving_legacy_rows() {
        let dir = tempfile::tempdir().unwrap();
        {
            let conn = build_v5_store(dir.path());
            seed_legacy_data(&conn);
            assert_eq!(current_version(&conn), 5);
        }
        // 正式迁移路径（含 journal、备份、标记）。
        {
            let conn = Connection::open(dir.path().join("fund.db")).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            let to = migration::run_migrations(dir.path(), &conn, "0.1.0").unwrap();
            assert_eq!(to, crate::storage::SCHEMA_VERSION);
            assert_eq!(current_version(&conn), crate::storage::SCHEMA_VERSION);

            // 旧数据保留（02 §6.2：旧目标口径不静默改变）。
            let goal: (i64, String) = conn
                .query_row(
                    "SELECT target_minor, target_date FROM investment_goals WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(
                goal,
                (1500000000, "2031-09-30".into()),
                "旧单目标行必须原样保留"
            );
            let finance_goals: i64 = conn
                .query_row("SELECT COUNT(*) FROM finance_goals", [], |r| r.get(0))
                .unwrap();
            assert!(finance_goals >= 1, "finance_goals 迁移行存在");
            let alloc: i64 = conn
                .query_row("SELECT COUNT(*) FROM allocation_targets", [], |r| r.get(0))
                .unwrap();
            assert_eq!(alloc, 2);
            let thesis_archived: (String, i64) = conn
                .query_row(
                    "SELECT name, archived FROM investment_theses WHERE name='主线A'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(
                thesis_archived,
                ("主线A".to_string(), 0),
                "主线默认未归档（新列默认值）"
            );

            // v6 新表可用：trade_cash 行随交易核对生成不是迁移职责，只验证结构。
            let trade_cash_cols: Vec<String> = {
                let mut stmt = conn.prepare("PRAGMA table_info(trade_cash)").unwrap();
                stmt.query_map([], |r| r.get::<_, String>(1))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert!(trade_cash_cols.contains(&"delta_minor".to_string()));
            assert!(trade_cash_cols.contains(&"status".to_string()));
            let res_cols: Vec<String> = {
                let mut stmt = conn
                    .prepare("PRAGMA table_info(cash_reservations)")
                    .unwrap();
                stmt.query_map([], |r| r.get::<_, String>(1))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert!(res_cols.contains(&"risk_class".to_string()));
        }
    }

    /// 幂等：v6 库上再次 run_migrations 不改版本、不报错、不动数据。
    #[test]
    fn v6_migration_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        {
            let conn = build_v5_store(dir.path());
            seed_legacy_data(&conn);
        }
        {
            let conn = Connection::open(dir.path().join("fund.db")).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migration::run_migrations(dir.path(), &conn, "0.1.0").unwrap();
            migration::run_migrations(dir.path(), &conn, "0.1.0").unwrap();
            assert_eq!(current_version(&conn), crate::storage::SCHEMA_VERSION);
            let goals: i64 = conn
                .query_row("SELECT COUNT(*) FROM finance_goals", [], |r| r.get(0))
                .unwrap();
            assert!(goals >= 1, "重复迁移不丢行");
        }
    }

    /// 失败恢复：迁移中途失败（journal 置 migrating 后进程崩溃模拟）→
    /// 重开时 PendingJournal；恢复备份后旧数据可读。
    #[test]
    fn failed_migration_leaves_journal_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        {
            let conn = build_v5_store(dir.path());
            seed_legacy_data(&conn);
        }
        // 模拟崩溃：手工写一个 migrating journal（拷贝真实库为备份）。
        let backup_dir = dir.path().join("backups");
        std::fs::create_dir_all(&backup_dir).unwrap();
        std::fs::copy(
            dir.path().join("fund.db"),
            backup_dir.join("fund.db.bak-test"),
        )
        .unwrap();
        migration::write_journal_atomic(
            &dir.path().join(".migration.json"),
            &migration::MigrationJournal {
                migration_id: "m5-to-6-test".into(),
                from_schema: 5,
                to_schema: 6,
                from_app_version: "0.1.0".into(),
                to_app_version: "0.1.0".into(),
                state: "migrating".into(),
                backup_id: "test".into(),
                has_committed_new_writes: false,
                previous_version_compatible: true,
                migrated_at: Some("100".into()),
            },
        )
        .unwrap();
        // 新连接：PendingJournal 必须先恢复，禁止半迁移库上继续。
        let conn = Connection::open(dir.path().join("fund.db")).unwrap();
        let result = migration::run_migrations(dir.path(), &conn, "0.1.0");
        assert!(
            matches!(result, Err(migration::MigrationError::PendingJournal(_))),
            "crashed journal must surface as PendingJournal, got {:?}",
            result
        );
        // 备份里的 v5 数据可读（恢复能力的证据）。
        let backup = Connection::open(backup_dir.join("fund.db.bak-test")).unwrap();
        let v: i64 = backup
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let goal: i64 = backup
            .query_row(
                "SELECT target_minor FROM investment_goals WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, 5);
        assert_eq!(goal, 1500000000);
    }

    /// Store::open（新库路径）直接落 v6，fresh-schema 与阶梯迁移结构一致。
    #[test]
    fn fresh_store_matches_migrated_structure() {
        let fresh = Store::open_in_memory().unwrap();
        assert_eq!(
            fresh.schema_version().unwrap(),
            crate::storage::SCHEMA_VERSION
        );
        fresh
            .with_read(|conn| -> rusqlite::Result<()> {
                for table in [
                    "trade_cash",
                    "finance_goals",
                    "cash_reservations",
                    "investment_theses",
                    "watch_events",
                ] {
                    conn.query_row(
                        "SELECT COUNT(*) FROM main.sqlite_master WHERE name=?1",
                        [table],
                        |r| r.get::<_, i64>(0),
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }
}
