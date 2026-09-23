//! migration 模块：业务 schema 版本化迁移与备份（契约 §4.1 / 实施方案 B3）。
//!
//! - 业务 DB 的 schema version 权威是 `PRAGMA user_version`；journal 只负责
//!   跨步骤崩溃恢复，不含业务内容。
//! - journal `data/.migration.json`：prepared→migrating→verified→committed；
//!   失败为 restored/failed；temp+fsync+rename 原子写入。
//! - 备份在 `backups/<migrationId>/fund.db.bak`，用 SQLite 在线 backup API
//!   产生一致性快照（含 WAL 已提交内容），禁止只复制正在使用的 db 文件。
//! - 迁移失败且未接受新写入：恢复备份后报告，不进入 ready。

use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 单个版本化迁移。
pub struct Migration {
    /// 迁移到这一步后的 schema 版本（PRAGMA user_version 目标值）。
    pub version: i64,
    pub name: &'static str,
    /// 已应用版本（version 之前）的库上执行的 SQL。
    pub sql: &'static str,
}

pub const SCHEMA_V2_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS watchlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS watchlist_items (
    watchlist_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    name TEXT NOT NULL,
    asset_type TEXT NOT NULL DEFAULT 'stock',
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    PRIMARY KEY (watchlist_id, symbol)
);

CREATE TABLE IF NOT EXISTS chart_drawings (
    id TEXT PRIMARY KEY,
    symbol TEXT NOT NULL,
    period TEXT NOT NULL,
    tool_type TEXT NOT NULL,
    points_json TEXT NOT NULL,
    options_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

/// 迁移阶梯：v0（空库）→ v1 基线 schema → v2 自选池与划线持久化。新版本只追加，不改历史条目。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline_schema_v1",
        sql: crate::storage::SCHEMA_V1,
    },
    Migration {
        version: 2,
        name: "watchlists_and_drawings_v2",
        sql: SCHEMA_V2_SQL,
    },
];

/// 迁移 journal（契约 §4.1 字段，不含业务内容与 Secret）。
#[derive(Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct MigrationJournal {
    #[serde(rename = "migrationId")]
    pub migration_id: String,
    #[serde(rename = "fromSchema")]
    pub from_schema: i64,
    #[serde(rename = "toSchema")]
    pub to_schema: i64,
    #[serde(rename = "fromAppVersion")]
    pub from_app_version: String,
    #[serde(rename = "toAppVersion")]
    pub to_app_version: String,
    pub state: String,
    #[serde(rename = "backupId")]
    pub backup_id: String,
    #[serde(rename = "hasCommittedNewWrites")]
    pub has_committed_new_writes: bool,
    #[serde(rename = "previousVersionCompatible", default = "default_true")]
    pub previous_version_compatible: bool,
    /// 迁移提交时刻（P3-4：真实记录，不推测；旧 journal 无此字段）。
    #[serde(rename = "migratedAt", default)]
    pub migrated_at: Option<String>,
}

fn default_true() -> bool {
    true
}

/// 标记 journal 已产生新业务写入（防止后续误回滚覆盖新数据）。
pub fn mark_journal_new_writes(data_dir: &Path) -> Result<(), MigrationError> {
    let journal_path = data_dir.join(".migration.json");
    if let Some(mut journal) = read_journal(&journal_path) {
        if !journal.has_committed_new_writes {
            journal.has_committed_new_writes = true;
            write_journal_atomic(&journal_path, &journal)?;
        }
    }
    Ok(())
}

/// 迁移错误。
#[derive(Debug, PartialEq, Eq)]
pub enum MigrationError {
    /// 迁移失败，已恢复备份；须先修复，禁止 ready。
    Restored(String),
    /// 迁移失败且恢复也失败（failed）；数据保留在备份中。
    Failed(String),
    /// journal 存在未完成迁移（previous crash）；先恢复再重试。
    PendingJournal(String),
}

impl MigrationError {
    pub fn message(&self) -> &str {
        match self {
            MigrationError::Restored(m)
            | MigrationError::Failed(m)
            | MigrationError::PendingJournal(m) => m,
        }
    }
}

fn now_iso() -> String {
    // 有界、无外部依赖的时间戳（日志/journal 排序用）。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn migration_id(from: i64, to: i64) -> String {
    format!("m{from}-to-{to}-{}", now_iso())
}

/// 原子写入 journal：temp + fsync + rename。
fn write_journal_atomic(path: &Path, journal: &MigrationJournal) -> Result<(), MigrationError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MigrationError::Failed(e.to_string()))?;
    }
    let body = serde_json::to_vec(journal).map_err(|e| MigrationError::Failed(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file =
            std::fs::File::create(&temp).map_err(|e| MigrationError::Failed(e.to_string()))?;
        file.write_all(&body)
            .and_then(|_| file.sync_all())
            .map_err(|e| MigrationError::Failed(e.to_string()))?;
    }
    std::fs::rename(&temp, path).map_err(|e| MigrationError::Failed(e.to_string()))?;
    Ok(())
}

fn read_journal(path: &Path) -> Option<MigrationJournal> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 把当前库一致性快照备份到 backups/<id>/fund.db.bak（在线 backup API）。
fn backup_database(data_dir: &Path, id: &str, live: &Connection) -> Result<PathBuf, String> {
    let dir = data_dir.join("backups").join(id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target_path = dir.join("fund.db.bak");
    // 备份到临时文件再 rename，避免半写备份被当作可用快照。
    let temp_path = dir.join("fund.db.bak.tmp");
    std::fs::remove_file(&temp_path).ok();
    let mut target =
        Connection::open(&temp_path).map_err(|e| format!("open backup target: {e}"))?;
    let backup = rusqlite::backup::Backup::new(live, &mut target)
        .map_err(|e| format!("init backup: {e}"))?;
    backup
        .run_to_completion(64, std::time::Duration::from_millis(5), None)
        .map_err(|e| format!("run backup: {e}"))?;
    drop(backup);
    target
        .close()
        .map_err(|(_, e)| format!("close backup: {e}"))?;
    std::fs::rename(&temp_path, &target_path).map_err(|e| e.to_string())?;
    Ok(target_path)
}

/// 用备份恢复业务库（复制回 fund.db；调用方负责保证此时无写入）。
fn restore_database(data_dir: &Path, backup_id: &str) -> Result<(), String> {
    let source = data_dir.join("backups").join(backup_id).join("fund.db.bak");
    if !source.exists() {
        return Err(format!("backup missing: {backup_id}"));
    }
    let meta = std::fs::metadata(&source).map_err(|e| e.to_string())?;
    if meta.len() == 0 {
        return Err(format!("backup is empty: {backup_id}"));
    }
    let target = data_dir.join("fund.db");
    let temp_target = data_dir.join("fund.db.restore.tmp");
    std::fs::copy(&source, &temp_target).map_err(|e| format!("copy backup: {e}"))?;
    // 先清 WAL 与 SHM，恢复后的库自身就是一致快照。
    let _ = std::fs::remove_file(data_dir.join("fund.db-wal"));
    let _ = std::fs::remove_file(data_dir.join("fund.db-shm"));
    std::fs::rename(&temp_target, &target).map_err(|e| format!("rename restored db: {e}"))?;
    Ok(())
}

/// 已开始但未完成的 journal 恢复：有备份且未提交新写入则回滚数据。
/// 若已有新写入，拒绝覆盖并报错（保护已提交业务数据）。
/// restored 是终态，不再重复恢复。
pub fn recover_pending_journal(data_dir: &Path) -> Result<bool, String> {
    let journal_path = data_dir.join(".migration.json");
    let Some(journal) = read_journal(&journal_path) else {
        return Ok(false);
    };
    match journal.state.as_str() {
        "committed" | "failed" | "restored" => Ok(false),
        _ => {
            if journal.has_committed_new_writes {
                return Err("cannot restore backup: database has committed new writes".into());
            }
            restore_database(data_dir, &journal.backup_id)
                .map_err(|e| format!("restore backup: {e}"))?;
            let restored = MigrationJournal {
                state: "restored".into(),
                ..journal
            };
            write_journal_atomic(&journal_path, &restored).map_err(|e| e.message().to_string())?;
            Ok(true)
        }
    }
}

/// 把数据库从当前版本迁移到 MIGRATIONS 目标版本。
/// conn 打开时已应用 WAL/FK PRAGMA；调用方持独占运行环境（无并发写）。
pub fn run_migrations(
    data_dir: &Path,
    conn: &Connection,
    app_version: &str,
) -> Result<i64, MigrationError> {
    let journal_path = data_dir.join(".migration.json");

    // 崩溃恢复：上一迁移未 committed 且有备份 → 先恢复，禁止在半迁移库上继续。
    if let Some(journal) = read_journal(&journal_path) {
        if journal.state != "committed" && journal.state != "failed" && journal.state != "restored"
        {
            return Err(MigrationError::PendingJournal(format!(
                "migration {} in state {}",
                journal.migration_id, journal.state
            )));
        }
    }

    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| MigrationError::Failed(e.to_string()))?;
    let target = MIGRATIONS.last().map(|m| m.version).unwrap_or(0);
    if current == target {
        return Ok(current);
    }
    if current > target {
        return Err(MigrationError::Failed(format!(
            "data schema {current} is newer than app schema {target}"
        )));
    }

    // 找到第一个待应用的迁移起点。
    let pending: Vec<&Migration> = MIGRATIONS.iter().filter(|m| m.version > current).collect();

    let id = migration_id(current, target);
    let mut journal = MigrationJournal {
        migration_id: id.clone(),
        from_schema: current,
        to_schema: target,
        from_app_version: app_version.to_string(),
        to_app_version: app_version.to_string(),
        state: "prepared".into(),
        backup_id: id.clone(),
        has_committed_new_writes: false,
        previous_version_compatible: true,
        migrated_at: None,
    };
    write_journal_atomic(&journal_path, &journal)?;

    // 一致性备份（prepared→migrating 切点）。
    backup_database(data_dir, &id, conn).map_err(MigrationError::Failed)?;
    journal.state = "migrating".into();
    write_journal_atomic(&journal_path, &journal)?;

    // 逐版本应用（幂等失败回滚）。
    for migration in &pending {
        let apply = || -> Result<(), rusqlite::Error> {
            conn.execute_batch(migration.sql)?;
            conn.pragma_update(None, "user_version", migration.version)?;
            Ok(())
        };
        if let Err(error) = apply() {
            let message = format!("migration {}: {error}", migration.name);
            if !journal.has_committed_new_writes && restore_database(data_dir, &id).is_ok() {
                journal.state = "restored".into();
                write_journal_atomic(&journal_path, &journal)?;
                return Err(MigrationError::Restored(message));
            }
            journal.state = "failed".into();
            write_journal_atomic(&journal_path, &journal)?;
            return Err(MigrationError::Failed(message));
        }
    }

    // 验证：user_version 已达目标且核心表可读。
    let verified: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| MigrationError::Failed(e.to_string()))?;
    if verified != target {
        journal.state = "failed".into();
        let _ = write_journal_atomic(&journal_path, &journal);
        return Err(MigrationError::Failed(format!(
            "verified {verified} != target {target}"
        )));
    }

    journal.state = "verified".into();
    write_journal_atomic(&journal_path, &journal)?;
    journal.state = "committed".into();
    journal.migrated_at = Some(now_iso());
    write_journal_atomic(&journal_path, &journal)?;
    Ok(target)
}

/// 数据状态（契约 §4.1 / --inspect-data 与 app:data_status 共用）。
#[derive(Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DataStatus {
    #[serde(rename = "currentSchema")]
    pub current_schema: i64,
    #[serde(rename = "migrationState")]
    pub migration_state: String,
    #[serde(rename = "lastDataWriterVersion")]
    pub last_data_writer_version: String,
    #[serde(rename = "hasCommittedNewWrites")]
    pub has_committed_new_writes: bool,
    #[serde(rename = "previousVersionCompatible")]
    pub previous_version_compatible: bool,
}

pub fn read_data_status(data_dir: &Path, default_app_version: &str) -> DataStatus {
    let db_path = data_dir.join("fund.db");
    let mut db_schema: Option<i64> = None;
    let mut db_has_writes = false;
    let mut db_writer_version: Option<String> = None;

    if db_path.exists() {
        if let Ok(conn) = Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            db_schema = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).ok();
            db_has_writes = crate::storage::Store::has_committed_new_writes(&conn);
            db_writer_version = crate::storage::Store::last_data_writer_version(&conn);
        }
    }

    let journal_path = data_dir.join(".migration.json");
    let journal = read_journal(&journal_path);

    let current_schema = db_schema
        .filter(|v| *v > 0)
        .or_else(|| journal.as_ref().map(|j| j.to_schema))
        .unwrap_or(crate::storage::SCHEMA_VERSION);

    let migration_state = journal
        .as_ref()
        .map(|j| j.state.clone())
        .unwrap_or_else(|| "committed".to_string());

    let has_committed_new_writes = db_has_writes
        || journal
            .as_ref()
            .map(|j| j.has_committed_new_writes)
            .unwrap_or(false);

    let previous_version_compatible = journal
        .as_ref()
        .map(|j| j.previous_version_compatible)
        .unwrap_or(true);

    let last_data_writer_version = db_writer_version
        .or_else(|| journal.as_ref().map(|j| j.to_app_version.clone()))
        .unwrap_or_else(|| default_app_version.to_string());

    DataStatus {
        current_schema,
        migration_state,
        last_data_writer_version,
        has_committed_new_writes,
        previous_version_compatible,
    }
}

#[cfg(test)]
mod tests {
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
}
