#![allow(dead_code)]
use crate::{Error, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::Path;

/// Current host schema version after all incremental migrations. Kept in sync
/// with the last `_schema_version` write in `apply_migrations`; tests assert
/// against it so a future migration does not leave a stale literal behind.
pub const SCHEMA_VERSION: &str = "27";

/// Database connection pool type alias
pub type DbPool = Pool<SqliteConnectionManager>;

use lazy_static::lazy_static;
use std::sync::Mutex;
mod db_migrations;
mod migrations_steps;

lazy_static! {
    /// 主 natives.db pool（全局持有，供 runtime 等无法 access AppState 的模块使用）
    static ref MAIN_DB_POOL: Mutex<Option<DbPool>> = Mutex::new(None);
    /// Global test lock to serialize tests that mutate MAIN_DB_POOL
    pub static ref DB_TEST_LOCK: Mutex<()> = Mutex::new(());
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

/// Ensure the Host-owned auxiliary schema on the natives.db main pool:
/// jobs (`scheduled_tasks` / `task_runs`) and the provider mirror tables
/// (`assistant_provider_configs` / `assistant_provider_keys` /
/// `assistant_model_cache`).
///
/// W1 (modular remediation): these tables are Host authority and live in the
/// Host's own natives.db — the Host no longer opens the Daemon's assistant.db.
pub fn ensure_host_owned_tables(conn: &Connection) -> Result<()> {
    crate::jobs::store::ensure_schema(conn)?;
    crate::daemon::data::ensure_provider_mirror_schema(conn)
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
    ensure_host_owned_tables(&conn)?;

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
