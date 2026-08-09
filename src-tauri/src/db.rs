#![allow(dead_code)]
use crate::{Error, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::Path;

/// Current host schema version after all incremental migrations. Kept in sync
/// with the last `_schema_version` write in `apply_migrations`; tests assert
/// against it so a future migration does not leave a stale literal behind.
pub const SCHEMA_VERSION: &str = "24";

/// Database connection pool type alias
pub type DbPool = Pool<SqliteConnectionManager>;

use lazy_static::lazy_static;
use std::sync::Mutex;
mod db_migrations;

lazy_static! {
    static ref ASSISTANT_DB_POOL: Mutex<Option<DbPool>> = Mutex::new(None);
    /// 主 natives.db pool（全局持有，供 runtime 等无法 access AppState 的模块使用）
    static ref MAIN_DB_POOL: Mutex<Option<DbPool>> = Mutex::new(None);
}

/// 注册主 natives.db pool（lib.rs setup 钩子调用）
pub fn register_main_pool(pool: DbPool) {
    let mut guard = MAIN_DB_POOL.lock().unwrap();
    *guard = Some(pool);
}

/// 清空主 natives.db pool(仅测试用,集成测试也使用):让依赖 natives.db SoT
/// 的 host-owned 方法(如 provider.list)在单测中确定性回退到 :memory:
/// DataStore,避免测试之间通过全局 pool 互相污染导致 flaky。
/// 说明:不使用 `#[cfg(test)]`——src-tauri/tests/* 集成测试链接的是非 test
/// 编译产物,cfg(test) 函数对它们不可见;本函数不参与生产逻辑。
#[doc(hidden)]
pub fn clear_main_pool_for_tests() {
    let mut guard = MAIN_DB_POOL.lock().unwrap();
    *guard = None;
}

/// 为测试注册一个指向临时路径的 assistant.db pool(仅测试用)。让依赖
/// `get_assistant_db_conn()` 的 host-owned 方法(如 provider.list 的模型缓存
/// 富化)在单测中命中临时库,而不会惰性初始化写入真实 `~/.natives`。
/// 不使用 `#[cfg(test)]`——集成测试链接非 test 编译产物。
#[doc(hidden)]
pub fn set_assistant_pool_for_tests(pool: DbPool) {
    let mut guard = ASSISTANT_DB_POOL.lock().unwrap();
    *guard = Some(pool);
}

/// 清空 assistant.db pool(仅测试用),避免单测之间通过全局 pool 互相污染。
#[doc(hidden)]
pub fn clear_assistant_pool_for_tests() {
    let mut guard = ASSISTANT_DB_POOL.lock().unwrap();
    *guard = None;
}

/// 获取主 natives.db pool 的连接（runtime 等无 State 上下文场景）
pub fn get_main_conn() -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
    let guard = MAIN_DB_POOL.lock().unwrap();
    match guard.as_ref() {
        Some(pool) => pool
            .get()
            .map_err(|e| Error::Internal(format!("Failed to get main DB connection: {e}"))),
        None => Err(Error::Internal("main DB pool not initialized".into())),
    }
}

/// Initialize the assistant database pool at ~/.natives/assistant.db.
/// This is a separate SQLite database isolated from the core natives.db.
///
/// D2-01 (MIG-004/DATA-002): the historical `assistant_sessions` /
/// session-based `assistant_messages` DDL is retired — the Host no longer
/// maintains an active `assistant_*` runtime schema. Old databases keep those
/// tables and the startup-only legacy migration service
/// (`daemon::data::LegacyMigrationService`) converts them one-way. This pool
/// exists only for the Host-owned tables (jobs `scheduled_tasks`/`task_runs`
/// and the provider mirror written by `commands/provider.rs`).
pub fn init_assistant_db() -> Result<()> {
    let data_dir = dirs::home_dir()
        .ok_or_else(|| Error::Internal("Cannot find home dir".to_string()))?
        .join(".natives");
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| Error::Internal(format!("Cannot create .natives dir: {e}")))?;
    let db_path = data_dir.join("assistant.db");
    let pool = init_db_pool(&db_path)?;

    // scheduled_tasks + task_runs 表（Job 任务模块复用扩展；
    // DDL 与条件补列的单一来源在 jobs::store::ensure_schema）
    let conn = pool
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    crate::jobs::store::ensure_schema(&conn)?;

    let mut guard = ASSISTANT_DB_POOL.lock().unwrap();
    *guard = Some(pool);
    Ok(())
}

/// Get a connection from the assistant database pool.
pub fn get_assistant_db_conn() -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
    let guard = ASSISTANT_DB_POOL.lock().unwrap();
    match guard.as_ref() {
        Some(pool) => pool
            .get()
            .map_err(|e| Error::Internal(format!("Failed to get assistant DB connection: {e}"))),
        None => {
            drop(guard);
            init_assistant_db()?;
            let guard = ASSISTANT_DB_POOL.lock().unwrap();
            match guard.as_ref() {
                Some(pool) => pool.get().map_err(|e| {
                    Error::Internal(format!("Failed to get assistant DB connection: {e}"))
                }),
                None => Err(Error::Internal(
                    "Failed to initialize assistant DB".to_string(),
                )),
            }
        }
    }
}

/// Initialize the SQLite database with WAL mode, foreign keys, and all tables.
/// Kept for standalone DB initialization (e.g., tests, CLI tools).
#[allow(dead_code)]
pub fn init_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    create_tables(&conn)?;
    apply_migrations(&conn)?;
    Ok(conn)
}

/// Initialize a connection pool (size 4, idle timeout 30s).
/// Each connection gets WAL mode, foreign keys, and busy_timeout set.
pub fn init_db_pool(path: &Path) -> Result<DbPool> {
    let manager = SqliteConnectionManager::file(path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    });
    let pool = Pool::builder()
        .max_size(4)
        .idle_timeout(Some(std::time::Duration::from_secs(30)))
        .build(manager)
        .map_err(|e| Error::Internal(format!("failed to create DB pool: {e}")))?;

    // Run schema creation/migration on one connection
    let conn = pool
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    create_tables(&conn)?;
    apply_migrations(&conn)?;

    Ok(pool)
}

mod backfill;
mod catalog;
mod kv;
mod schema;
mod settings;

pub(crate) use backfill::*;
pub use catalog::*;
pub use kv::*;
pub use schema::*;
pub use settings::*;

#[cfg(test)]
#[path = "db_tests.rs"]
mod db_tests;
