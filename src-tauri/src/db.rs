#![allow(dead_code)]
use crate::{Error, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::path::Path;

/// Current host schema version after all incremental migrations. Kept in sync
/// with the last `_schema_version` write in `apply_migrations`; tests assert
/// against it so a future migration does not leave a stale literal behind.
pub const SCHEMA_VERSION: &str = "24";

/// Map a source-table `state` string to a runtime_instances.status for the
/// v12 backfill. Terminal / unknown states produce no instance.
pub(super) fn instance_status_for_state(state: &str) -> Option<&'static str> {
    Some(match state {
        "running" => "running",
        "starting" => "starting",
        "stopping" => "stopping",
        "start_failed" => "failed",
        "cleanup_failed" => "cleanup_failed",
        "orphaned" => "orphaned",
        _ => return None,
    })
}

/// Extract pgid / pid from a local process_identity_json during backfill.
pub(crate) fn parse_identity(json: Option<&str>) -> (Option<i32>, Option<u32>) {
    let Some(s) = json else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(s) else {
        return (None, None);
    };
    let pgid = v
        .get("processGroupId")
        .and_then(|x| x.as_i64())
        .map(|x| x as i32);
    let pid = v.get("pid").and_then(|x| x.as_i64()).map(|x| x as u32);
    (pgid, pid)
}

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

pub fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        -- 1. Installed plugin registry
        CREATE TABLE IF NOT EXISTS modules (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            entry TEXT NOT NULL,
            type TEXT NOT NULL,
            description TEXT,
            author TEXT,
            icon TEXT,
            enabled INTEGER DEFAULT 1,
            min_natives_version TEXT,
            state TEXT DEFAULT 'installed',
            created_at TEXT,
            updated_at TEXT
        );

        -- 2. Per-module permission grants
        CREATE TABLE IF NOT EXISTS module_permissions (
            module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
            permission TEXT NOT NULL,
            granted INTEGER DEFAULT 0,
            PRIMARY KEY (module_id, permission)
        );

        -- 3. Key-value app settings
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT
        );

        -- 4. Per-module key-value storage
        CREATE TABLE IF NOT EXISTS module_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT,
            UNIQUE(module_id, key)
        );

        -- 5. Workshop/marketplace cache
        CREATE TABLE IF NOT EXISTS workshop_cache (
            id TEXT PRIMARY KEY,
            name TEXT,
            version TEXT,
            description TEXT,
            author TEXT,
            icon TEXT,
            permissions TEXT,
            installed INTEGER DEFAULT 0
        );

        -- 6. Environment configuration profiles
        CREATE TABLE IF NOT EXISTS env_profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            is_default INTEGER DEFAULT 0,
            created_at TEXT
        );

        -- 7. Encrypted env vars per profile
        CREATE TABLE IF NOT EXISTS env_variables (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            profile_id INTEGER NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value_encrypted TEXT NOT NULL,
            UNIQUE(profile_id, key)
        );

        -- 8. Notification queue
        CREATE TABLE IF NOT EXISTS notifications (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT,
            title TEXT,
            body TEXT,
            level TEXT DEFAULT 'info',
            read INTEGER DEFAULT 0,
            created_at TEXT
        );

        -- 9. Sidebar ordering
        CREATE TABLE IF NOT EXISTS module_order (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE UNIQUE,
            sort_order INTEGER NOT NULL
        );

        -- 10. Permission audit trail
        CREATE TABLE IF NOT EXISTS permission_audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL,
            permission TEXT NOT NULL,
            action TEXT NOT NULL CHECK(action IN ('grant','revoke','deny','approve')),
            granted INTEGER NOT NULL,
            reason TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- 11. Kernel-owned module contracts (KI-1 audit tree)
        CREATE TABLE IF NOT EXISTS module_contracts (
            module_id TEXT PRIMARY KEY,
            contract_id TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- 12. User-configured LLM providers (native_runtime / provider commands)
        CREATE TABLE IF NOT EXISTS user_providers (
            id TEXT PRIMARY KEY,
            preset_name TEXT NOT NULL,
            api_protocol TEXT NOT NULL DEFAULT 'openai_chat_completions',
            name TEXT NOT NULL,
            website_url TEXT NOT NULL DEFAULT '',
            base_url TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- 13. Provider API keys (envelope-encrypted, KEK-DEK)
        CREATE TABLE IF NOT EXISTS provider_api_keys (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            label TEXT NOT NULL DEFAULT '',
            api_key_encrypted TEXT NOT NULL DEFAULT '',
            dek_encrypted TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );

        -- 14. Imported Sub2API account pools. Credentials and proxy passwords
        -- are envelope-encrypted and never projected to the renderer.
        CREATE TABLE IF NOT EXISTS provider_account_proxies (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            proxy_key_hash TEXT NOT NULL,
            config_encrypted TEXT NOT NULL,
            dek_encrypted TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(provider_id, proxy_key_hash)
        );
        CREATE TABLE IF NOT EXISTS provider_accounts (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            name TEXT NOT NULL DEFAULT '',
            platform TEXT NOT NULL,
            account_type TEXT NOT NULL,
            credentials_encrypted TEXT NOT NULL,
            dek_encrypted TEXT NOT NULL,
            extra_json TEXT NOT NULL DEFAULT '{}',
            proxy_id TEXT REFERENCES provider_account_proxies(id) ON DELETE SET NULL,
            concurrency INTEGER NOT NULL DEFAULT 1,
            priority INTEGER NOT NULL DEFAULT 0,
            expires_at TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            identity_fingerprint TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(provider_id, identity_fingerprint)
        );
        CREATE INDEX IF NOT EXISTS idx_provider_accounts_pool
            ON provider_accounts(provider_id, status, priority);

        -- 15. Host-owned routing configuration. Runtime health remains in assistant.db.
        CREATE TABLE IF NOT EXISTS provider_routing_settings (
            id INTEGER PRIMARY KEY CHECK(id=1),
            enabled INTEGER NOT NULL DEFAULT 0,
            local_enabled INTEGER NOT NULL DEFAULT 0,
            local_port INTEGER NOT NULL DEFAULT 15721,
            local_token_encrypted TEXT,
            local_token_dek_encrypted TEXT,
            rectifier_json TEXT NOT NULL DEFAULT '{}',
            global_proxy_json TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_route_bindings (
            id TEXT PRIMARY KEY,
            position INTEGER NOT NULL DEFAULT 0,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            credential_kind TEXT NOT NULL CHECK(credential_kind IN ('api_key', 'sub2api_pool')),
            credential_id TEXT,
            model_id TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_provider_route_bindings_position
            ON provider_route_bindings(position);

        -- Indexes
        CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read, created_at);
        CREATE INDEX IF NOT EXISTS idx_audit_log_module ON permission_audit_log(module_id, created_at);
        ",
    )?;
    Ok(())
}

pub fn apply_migrations(conn: &Connection) -> Result<()> {
    // T214: migration registry split into db_migrations (per-version
    // migrate_vN steps + legacy repairs preserved in original order).
    db_migrations::apply(conn)
}

/// Idempotent backfill of the three existing sources into the unified
/// `applications` identity, `startup_plans`, and `runtime_instances` for rows
/// that are currently non-terminal. Safe to run repeatedly (INSERT OR IGNORE +
/// deterministic ids); extracted so tests can prove idempotency.
pub(crate) fn backfill_creative_identity(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-internal-' || id, 'internal', id, COALESCE(name, id), description, icon,
               COALESCE(NULLIF(version, ''), '1'),
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM modules;

        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-external-' || id, 'external_github', id, COALESCE(title, id), description, icon,
               COALESCE(NULLIF(version, ''), '1'),
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM external_creative_apps;

        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-local-' || id, 'local_project', id, COALESCE(title, id), description, icon, '1',
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM local_creative_apps;

        INSERT OR IGNORE INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
        SELECT 'plan-local-' || a.id, a.id, 1, l.launch_plan_json, 1,
               COALESCE(l.created_at, datetime('now')), COALESCE(l.updated_at, datetime('now'))
        FROM local_creative_apps l
        JOIN applications a ON a.source = 'local_project' AND a.source_id = l.id;

        INSERT OR IGNORE INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
        SELECT 'plan-external-' || a.id, a.id, 1, e.runtime_config_json, 1,
               COALESCE(e.created_at, datetime('now')), COALESCE(e.updated_at, datetime('now'))
        FROM external_creative_apps e
        JOIN applications a ON a.source = 'external_github' AND a.source_id = e.id;
        ",
    )
    .map_err(Error::Database)?;

    // Backfill runtime_instances for rows that are currently non-terminal, so
    // every active resource is traceable to one instance even after migration.
    {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT l.id, l.state, l.current_port, l.open_url, l.process_identity_json,
                        l.launch_plan_json
                 FROM local_creative_apps l
                 JOIN applications a ON a.source = 'local_project' AND a.source_id = l.id",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(Error::Database)?;
        for r in rows {
            let (source_id, state, port, open_url, ident_json, plan_json) =
                r.map_err(Error::Database)?;
            let Some(status) = instance_status_for_state(&state) else {
                continue;
            };
            let application_id = conn
                .query_row(
                    "SELECT id FROM applications WHERE source = 'local_project' AND source_id = ?1",
                    params![source_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(Error::Database)?;
            let (pgid, pid) = parse_identity(ident_json.as_deref());
            let plan_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1 LIMIT 1",
                    params![application_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Error::Database)?;
            let runtime = serde_json::from_str::<serde_json::Value>(&plan_json)
                .ok()
                .and_then(|v| {
                    v.get("runtime")
                        .and_then(|r| r.as_str())
                        .map(str::to_string)
                });
            let owner_kind = match runtime.as_deref() {
                Some("static_http") => "host_http",
                // Local Compose plans own a docker_compose project, NOT a local
                // process (batch 1 CR-103 fixes the earlier misclassification).
                Some("docker_compose") => "docker_compose",
                _ => "local_process",
            };
            let urls = open_url
                .as_deref()
                .map(|u| serde_json::json!([u]).to_string());
            conn.execute(
                "INSERT OR IGNORE INTO runtime_instances
                    (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
                     resolved_urls_json, current_port, pid, failure, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, NULL, ?7, ?8, ?9, NULL, ?10, ?10)",
                params![
                    format!("ri-backfill-{application_id}"),
                    application_id,
                    plan_id,
                    status,
                    owner_kind,
                    pgid,
                    urls,
                    port,
                    pid,
                    now,
                ],
            )
            .map_err(Error::Database)?;
        }
    }
    {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.state, e.host_port, e.open_url, e.runtime_config_json
                 FROM external_creative_apps e
                 JOIN applications a ON a.source = 'external_github' AND a.source_id = e.id",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(Error::Database)?;
        for r in rows {
            let (source_id, state, port, open_url, cfg_json) = r.map_err(Error::Database)?;
            let Some(status) = instance_status_for_state(&state) else {
                continue;
            };
            let application_id = conn
                .query_row(
                    "SELECT id FROM applications WHERE source = 'external_github' AND source_id = ?1",
                    params![source_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(Error::Database)?;
            let plan_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1 LIMIT 1",
                    params![application_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Error::Database)?;
            let owner_kind = if serde_json::from_str::<serde_json::Value>(&cfg_json)
                .ok()
                .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
                .as_deref()
                == Some("docker_compose")
            {
                "docker_compose"
            } else {
                "docker_run"
            };
            let urls = open_url
                .as_deref()
                .map(|u| serde_json::json!([u]).to_string());
            conn.execute(
                "INSERT OR IGNORE INTO runtime_instances
                    (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
                     resolved_urls_json, current_port, pid, failure, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL, NULL, ?6, ?7, NULL, NULL, ?8, ?8)",
                params![
                    format!("ri-backfill-{application_id}"),
                    application_id,
                    plan_id,
                    status,
                    owner_kind,
                    urls,
                    port,
                    now,
                ],
            )
            .map_err(Error::Database)?;
        }
    }
    Ok(())
}

/// Insert an immutable audit/report row for a creative identity repair.
/// Deterministic `id` keeps re-runs idempotent (INSERT OR IGNORE).
#[allow(clippy::too_many_arguments)] // pre-existing parameter list
fn insert_identity_report(
    conn: &Connection,
    id: &str,
    kind: &str,
    application_id: Option<&str>,
    source: Option<&str>,
    source_id: Option<&str>,
    action: &str,
    payload_json: &str,
    ts: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO creative_identity_reports
            (id, kind, application_id, source, source_id, action, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            kind,
            application_id,
            source,
            source_id,
            action,
            payload_json,
            ts
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Batch 1 CR-101: delete confirmed ghost `applications` rows and audit the rest.
///
/// A ghost is an applications row whose source detail row is gone (no matching
/// row in `modules` / `external_creative_apps` / `local_creative_apps`). We only
/// delete ghosts with no dependent `startup_plans` / `runtime_instances` /
/// `preview_targets` (double gate per upgrade plan T01); every deletion backs
/// up the full row JSON into `creative_identity_reports`. Ghosts that still
/// carry dependent data are quarantined (reported, kept), and a ghost whose
/// source_id collides with a real row of another source is also reported.
/// Idempotent: after the first run the deletable set is empty.
pub(crate) fn repair_creative_identity_ghosts(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;
    let ts = chrono::Utc::now().to_rfc3339();

    let candidates: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.source, a.source_id
                 FROM applications a
                 WHERE NOT EXISTS (SELECT 1 FROM modules m WHERE m.id = a.source_id AND a.source = 'internal')
                   AND NOT EXISTS (SELECT 1 FROM external_creative_apps e WHERE e.id = a.source_id AND a.source = 'external_github')
                   AND NOT EXISTS (SELECT 1 FROM local_creative_apps l WHERE l.id = a.source_id AND a.source = 'local_project')",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };

    let mut deleted = 0usize;
    for (id, source, source_id) in candidates {
        let row_json: String = conn
            .query_row(
                "SELECT json_object('id', id, 'source', source, 'source_id', source_id,
                                    'title', title, 'version', version,
                                    'created_at', created_at, 'updated_at', updated_at)
                 FROM applications WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        // Double gate: dependent rows must be empty before we may delete.
        let has_deps: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM startup_plans sp WHERE sp.application_id = ?1)
                  + (SELECT COUNT(*) FROM runtime_instances ri WHERE ri.application_id = ?1)
                  + (SELECT COUNT(*) FROM preview_targets pt
                        JOIN runtime_instances ri2 ON ri2.id = pt.runtime_instance_id
                     WHERE ri2.application_id = ?1)",
                params![id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        if has_deps > 0 {
            insert_identity_report(
                conn,
                &format!("ghost-quarantine-{id}"),
                "ghost_application_with_dependencies",
                Some(&id),
                Some(&source),
                Some(&source_id),
                "quarantined",
                &row_json,
                &ts,
            )?;
            continue;
        }
        // Cross-source collision: the same source_id exists as a REAL row in a
        // different source table. The real row lives in its own table, so
        // deletion is still safe, but the collision is worth an audit record.
        let cross: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM modules m WHERE m.id = ?2 AND ?1 <> 'internal')
                  + (SELECT COUNT(*) FROM external_creative_apps e WHERE e.id = ?2 AND ?1 <> 'external_github')
                  + (SELECT COUNT(*) FROM local_creative_apps l WHERE l.id = ?2 AND ?1 <> 'local_project')",
                params![source, source_id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        if cross > 0 {
            insert_identity_report(
                conn,
                &format!("collision-{id}"),
                "cross_source_collision",
                Some(&id),
                Some(&source),
                Some(&source_id),
                "reported",
                &row_json,
                &ts,
            )?;
        }
        insert_identity_report(
            conn,
            &format!("ghost-{id}"),
            "ghost_application",
            Some(&id),
            Some(&source),
            Some(&source_id),
            "deleted",
            &row_json,
            &ts,
        )?;
        conn.execute("DELETE FROM applications WHERE id = ?1", params![id])
            .map_err(Error::Database)?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Batch 1 CR-102: reconcile duplicate active runtime/plan rows, then create the
/// partial unique indexes that make "one active per application" a DB invariant.
///
/// Duplicates are NOT deleted and NOT silently marked stopped (invariant #8):
/// the newest active row is kept and older runtime duplicates are demoted to
/// `orphaned` (outside the index scope), while older plan duplicates get
/// `is_active=0`. Every demotion is audited in `creative_identity_reports`.
/// Idempotent: after the first run there are no duplicates left to demote.
pub(crate) fn repair_creative_active_invariants(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;
    let ts = chrono::Utc::now().to_rfc3339();
    let mut fixed = 0usize;

    // 1) One active startup_plan per application.
    let dup_plan_apps: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT application_id FROM startup_plans WHERE is_active = 1
                 GROUP BY application_id HAVING COUNT(*) > 1",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    for app in dup_plan_apps {
        let ids: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM startup_plans
                     WHERE application_id = ?1 AND is_active = 1
                     ORDER BY updated_at DESC, id",
                )
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![app], |r| r.get::<_, String>(0))
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)?
        };
        for id in ids.into_iter().skip(1) {
            let row_json: String = conn
                .query_row(
                    "SELECT json_object('id', id, 'application_id', application_id,
                                        'plan_version', plan_version, 'is_active', is_active,
                                        'updated_at', updated_at)
                     FROM startup_plans WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(Error::Database)?;
            conn.execute(
                "UPDATE startup_plans SET is_active = 0, updated_at = ?2 WHERE id = ?1",
                params![id, ts],
            )
            .map_err(Error::Database)?;
            insert_identity_report(
                conn,
                &format!("plan-dedup-{id}"),
                "duplicate_active_plan",
                Some(&app),
                None,
                None,
                "demoted",
                &row_json,
                &ts,
            )?;
            fixed += 1;
        }
    }

    // 2) One active runtime_instance per application (over the index scope).
    let dup_runtime_apps: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT application_id FROM runtime_instances
                 WHERE status IN ('starting','running','stopping')
                 GROUP BY application_id HAVING COUNT(*) > 1",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    for app in dup_runtime_apps {
        let ids: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM runtime_instances
                     WHERE application_id = ?1 AND status IN ('starting','running','stopping')
                     ORDER BY updated_at DESC, id",
                )
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![app], |r| r.get::<_, String>(0))
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)?
        };
        for id in ids.into_iter().skip(1) {
            let row_json: String = conn
                .query_row(
                    "SELECT json_object('id', id, 'application_id', application_id,
                                        'status', status, 'owner_kind', owner_kind,
                                        'updated_at', updated_at)
                     FROM runtime_instances WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(Error::Database)?;
            // Demote to orphaned (NOT stopped — the resource is unproven; see
            // invariant #8). Orphaned is outside the index scope so the partial
            // unique index below remains satisfiable.
            conn.execute(
                "UPDATE runtime_instances
                 SET status = 'orphaned', cleanup_status = NULL, updated_at = ?2
                 WHERE id = ?1",
                params![id, ts],
            )
            .map_err(Error::Database)?;
            insert_identity_report(
                conn,
                &format!("runtime-dedup-{id}"),
                "duplicate_active_runtime",
                Some(&app),
                None,
                None,
                "orphaned",
                &row_json,
                &ts,
            )?;
            fixed += 1;
        }
    }

    // 3) Partial unique indexes — safe now that duplicates are gone.
    conn.execute_batch(
        "
        CREATE UNIQUE INDEX IF NOT EXISTS idx_runtime_instances_one_active
            ON runtime_instances(application_id)
            WHERE status IN ('starting','running','stopping');
        CREATE UNIQUE INDEX IF NOT EXISTS idx_startup_plans_one_active
            ON startup_plans(application_id) WHERE is_active = 1;
        ",
    )
    .map_err(Error::Database)?;

    Ok(fixed)
}

/// Derive a LaunchProfile `driver_kind` from a stored plan JSON. Handles the
/// local LaunchPlan shape (`runtime`, camelCase) and the external RuntimeConfig
/// shape (`kind`, snake_case). Kept self-contained because migrations must run
/// against historical schemas without depending on domain modules.
fn plan_driver_kind_from_json(json: &str) -> &'static str {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return "unknown";
    };
    if let Some(r) = v.get("runtime").and_then(|r| r.as_str()) {
        return match r {
            "static_http" => "local_static",
            "docker_compose" => "docker_compose",
            _ => "node_dev_server",
        };
    }
    if let Some(k) = v.get("kind").and_then(|k| k.as_str()) {
        return if k == "docker_compose" {
            "docker_compose"
        } else {
            "docker_run"
        };
    }
    "unknown"
}

/// Batch 1 CR-103: promote `startup_plans` to the versioned LaunchProfile base.
///
/// Adds nullable `schema_version` / `driver_kind` / `ownership_mode` columns
/// (read-upgrader derives them from plan_json when NULL), backfills existing
/// rows, and repairs the Compose backfill that misclassified a local
/// docker_compose instance as `local_process`. Additive and idempotent: the
/// column adds are PRAGMA-guarded and the backfill only touches NULL rows.
pub(crate) fn upgrade_startup_plans_v1(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;

    // 1) Add the versioned columns (guarded so re-running is safe).
    let plan_cols: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(startup_plans)")
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    if !plan_cols.iter().any(|c| c == "schema_version") {
        conn.execute(
            "ALTER TABLE startup_plans ADD COLUMN schema_version INTEGER",
            [],
        )
        .map_err(Error::Database)?;
    }
    if !plan_cols.iter().any(|c| c == "driver_kind") {
        conn.execute("ALTER TABLE startup_plans ADD COLUMN driver_kind TEXT", [])
            .map_err(Error::Database)?;
    }
    if !plan_cols.iter().any(|c| c == "ownership_mode") {
        conn.execute(
            "ALTER TABLE startup_plans ADD COLUMN ownership_mode TEXT",
            [],
        )
        .map_err(Error::Database)?;
    }

    // 2) Backfill columns for legacy rows (only those still missing them).
    let ts = chrono::Utc::now().to_rfc3339();
    let mut fixed = 0usize;
    let missing: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, plan_json FROM startup_plans
                 WHERE schema_version IS NULL OR driver_kind IS NULL OR ownership_mode IS NULL",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };
    for (id, plan_json) in missing {
        let dk = plan_driver_kind_from_json(&plan_json);
        let sv = serde_json::from_str::<serde_json::Value>(&plan_json)
            .ok()
            .and_then(|v| v.get("schemaVersion").and_then(|s| s.as_u64()))
            .unwrap_or(1);
        conn.execute(
            "UPDATE startup_plans
             SET schema_version = ?2, driver_kind = ?3, ownership_mode = 'managed', updated_at = ?4
             WHERE id = ?1",
            params![id, sv as i64, dk, ts],
        )
        .map_err(Error::Database)?;
        insert_identity_report(
            conn,
            &format!("plan-upgrade-{id}"),
            "plan_versioned_columns",
            None,
            None,
            None,
            "backfilled",
            &serde_json::json!({ "driverKind": dk, "schemaVersion": sv }).to_string(),
            &ts,
        )?;
        fixed += 1;
    }

    // 3) Repair the earlier Compose backfill that classified local docker_compose
    //    instances as `local_process` (#34).
    let misclassified: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT ri.id, ri.application_id, l.launch_plan_json
                 FROM runtime_instances ri
                 JOIN applications a ON a.id = ri.application_id
                 JOIN local_creative_apps l ON l.id = a.source_id AND a.source = 'local_project'
                 WHERE ri.owner_kind = 'local_process'
                   AND json_extract(l.launch_plan_json, '$.runtime') = 'docker_compose'",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };
    for (rid, app_id, plan_json) in misclassified {
        conn.execute(
            "UPDATE runtime_instances SET owner_kind = 'docker_compose', updated_at = ?2 WHERE id = ?1",
            params![rid, ts],
        )
        .map_err(Error::Database)?;
        insert_identity_report(
            conn,
            &format!("owner-repair-{rid}"),
            "plan_owner_repaired",
            Some(&app_id),
            None,
            None,
            "repaired",
            &serde_json::json!({ "instanceId": rid, "driverKind": plan_driver_kind_from_json(&plan_json) })
                .to_string(),
            &ts,
        )?;
        fixed += 1;
    }

    Ok(fixed)
}


// ──────────────────────────────────────────────
// Module data CRUD (used by db:get/set/delete/list)
// Maps to the "module_data" table with a fixed module_id
// The Electron preload's db.get/set uses a single flat namespace,
// stored in module_data with module_id = '_app'.
// ──────────────────────────────────────────────

const APP_MODULE_ID: &str = "_app";

/// Generic module_data read (used by state persistence)
pub fn db_get_module_data(
    conn: &Connection,
    module_id: &str,
    key: &str,
) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT value FROM module_data WHERE module_id = ?1 AND key = ?2")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![module_id, key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(Error::Database)?;
    match result {
        Some(s) => {
            let v: serde_json::Value =
                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
            Ok(Some(v))
        }
        None => Ok(None),
    }
}

/// Generic module_data write (used by state persistence)
pub fn db_set_module_data(
    conn: &Connection,
    module_id: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<()> {
    let serialized = serde_json::to_string(value).map_err(Error::Json)?;
    conn.execute(
        "INSERT INTO module_data (module_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
        rusqlite::params![module_id, key, serialized],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Generic module_data delete (used by state persistence)
pub fn db_delete_module_data(conn: &Connection, module_id: &str, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM module_data WHERE module_id = ?1 AND key = ?2",
        rusqlite::params![module_id, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:get — read a value from module_data
pub fn db_get(conn: &Connection, key: &str) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT value FROM module_data WHERE module_id = ?1 AND key = ?2")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![APP_MODULE_ID, key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(Error::Database)?;

    match result {
        Some(s) => {
            let v: serde_json::Value =
                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
            Ok(Some(v))
        }
        None => Ok(None),
    }
}

/// db:set — upsert a value into module_data
pub fn db_set(conn: &Connection, key: &str, value: &serde_json::Value) -> Result<()> {
    let serialized = serde_json::to_string(value).map_err(Error::Json)?;
    conn.execute(
        "INSERT INTO module_data (module_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
        rusqlite::params![APP_MODULE_ID, key, serialized],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:delete — remove a key from module_data
pub fn db_delete(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM module_data WHERE module_id = ?1 AND key = ?2",
        rusqlite::params![APP_MODULE_ID, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:list — list keys in module_data, optionally filtered by prefix
pub fn db_list(conn: &Connection, prefix: Option<&str>) -> Result<Vec<String>> {
    let stmt = match prefix {
        Some(_) => {
            let mut s = conn
                .prepare("SELECT key FROM module_data WHERE module_id = ?1 AND key LIKE ?2")
                .map_err(Error::Database)?;
            let pattern = format!("{}%", prefix.unwrap());
            let rows = s
                .query_map(rusqlite::params![APP_MODULE_ID, pattern], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)
        }
        None => {
            let mut s = conn
                .prepare("SELECT key FROM module_data WHERE module_id = ?1")
                .map_err(Error::Database)?;
            let rows = s
                .query_map(rusqlite::params![APP_MODULE_ID], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)
        }
    }?;
    Ok(stmt)
}

// ──────────────────────────────────────────────
// Settings helpers (used by theme, locale, etc.)
// ──────────────────────────────────────────────

/// Read a setting value by key
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![key], |row| row.get::<_, String>(0))
        .optional()
        .map_err(Error::Database)?;
    Ok(result)
}

/// Write a setting value (upsert)
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Delete a setting value by key
pub fn delete_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM settings WHERE key = ?1",
        rusqlite::params![key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Notifications
// ──────────────────────────────────────────────

/// Create a notification and return its ID
pub fn create_notification(
    conn: &Connection,
    title: &str,
    body: &str,
    level: &str,
    module_id: Option<&str>,
) -> Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO notifications (module_id, title, body, level, read, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
        rusqlite::params![module_id, title, body, level, now],
    )
    .map_err(Error::Database)?;
    Ok(conn.last_insert_rowid())
}

/// List notifications, optionally filtered by unread-only
pub fn list_notifications(conn: &Connection, unread_only: bool) -> Result<Vec<serde_json::Value>> {
    let sql = if unread_only {
        "SELECT id, module_id, title, body, level, read, created_at FROM notifications WHERE read = 0 ORDER BY created_at DESC"
    } else {
        "SELECT id, module_id, title, body, level, read, created_at FROM notifications ORDER BY created_at DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "moduleId": row.get::<_, Option<String>>(1)?,
                "title": row.get::<_, String>(2)?,
                "body": row.get::<_, String>(3)?,
                "level": row.get::<_, String>(4)?,
                "read": row.get::<_, i64>(5)? != 0,
                "createdAt": row.get::<_, String>(6)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

/// Mark a notification as read
pub fn mark_notification_read(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE notifications SET read = 1 WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Mark all notifications as read
pub fn mark_all_notifications_read(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE notifications SET read = 1 WHERE read = 0", [])
        .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Builtin tools CRUD (extensible tool registry)
// ──────────────────────────────────────────────

/// List all builtin tools from DB
pub fn list_builtin_tools(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT id, enabled, driver FROM builtin_tools")
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "enabled": row.get::<_, i64>(1)? != 0,
                "driver": row.get::<_, String>(2)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

/// Update a builtin tool's enabled/driver state
pub fn update_builtin_tool(conn: &Connection, id: &str, enabled: bool, driver: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO builtin_tools (id, enabled, driver, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
        rusqlite::params![id, enabled as i64, driver],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Ensure a builtin tool row exists (seed from frontend registry)
pub fn seed_builtin_tool(conn: &Connection, id: &str, driver: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO builtin_tools (id, enabled, driver) VALUES (?1, 0, ?2)",
        rusqlite::params![id, driver],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Usage stats CRUD (structured per-model daily stats)
// ──────────────────────────────────────────────

/// Upsert a daily model usage stat row with source breadcrumb.
#[allow(clippy::too_many_arguments)] // pre-existing parameter list
pub fn upsert_usage_stat(
    conn: &Connection,
    date: &str,
    source: &str,
    source_path: Option<&str>,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    request_count: u64,
    cost_usd: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_stats (date, source, source_path, model, input_tokens, output_tokens,
            cache_creation_tokens, cache_read_tokens, request_count, cost_usd)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(date, source, model) DO UPDATE SET
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            request_count = excluded.request_count,
            cost_usd = excluded.cost_usd,
            source_path = excluded.source_path",
        rusqlite::params![
            date,
            source,
            source_path,
            model,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            request_count,
            cost_usd
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Query usage stats, optionally filtered by source and date range.
/// Returns array of JSON objects with all columns.
#[allow(dead_code)]
pub fn query_usage_stats(
    conn: &Connection,
    source: Option<&str>,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut sql = "SELECT date, source, source_path, model, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, request_count, cost_usd FROM usage_stats WHERE 1=1".to_string();
    if source.is_some() {
        sql.push_str(" AND source = ?");
    }
    if from_date.is_some() {
        sql.push_str(" AND date >= ?");
    }
    if to_date.is_some() {
        sql.push_str(" AND date <= ?");
    }
    sql.push_str(" ORDER BY date DESC, model");

    let mut stmt = conn.prepare(&sql).map_err(Error::Database)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(s) = source {
        params.push(Box::new(s.to_string()));
    }
    if let Some(d) = from_date {
        params.push(Box::new(d.to_string()));
    }
    if let Some(d) = to_date {
        params.push(Box::new(d.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "source": row.get::<_, String>(1)?,
                "sourcePath": row.get::<_, Option<String>>(2)?,
                "model": row.get::<_, String>(3)?,
                "inputTokens": row.get::<_, u64>(4)?,
                "outputTokens": row.get::<_, u64>(5)?,
                "cacheCreationTokens": row.get::<_, u64>(6)?,
                "cacheReadTokens": row.get::<_, u64>(7)?,
                "requestCount": row.get::<_, u64>(8)?,
                "costUsd": row.get::<_, f64>(9)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

// ──────────────────────────────────────────────
// Skill usage CRUD (skill invocation tracking)
// ──────────────────────────────────────────────

/// Upsert a daily skill usage row with source breadcrumb.
pub fn upsert_skill_usage(
    conn: &Connection,
    date: &str,
    source: &str,
    source_path: Option<&str>,
    skill_name: &str,
    trigger_count: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO skill_usage (date, source, source_path, skill_name, trigger_count)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date, source, skill_name) DO UPDATE SET
            trigger_count = excluded.trigger_count,
            source_path = excluded.source_path",
        rusqlite::params![date, source, source_path, skill_name, trigger_count],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Query skill usage stats, optionally filtered by skill name and date range.
#[allow(dead_code)]
pub fn query_skill_usage(
    conn: &Connection,
    skill_name: Option<&str>,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut sql =
        "SELECT date, source, source_path, skill_name, trigger_count FROM skill_usage WHERE 1=1"
            .to_string();
    if skill_name.is_some() {
        sql.push_str(" AND skill_name = ?");
    }
    if from_date.is_some() {
        sql.push_str(" AND date >= ?");
    }
    if to_date.is_some() {
        sql.push_str(" AND date <= ?");
    }
    sql.push_str(" ORDER BY date DESC, skill_name");

    let mut stmt = conn.prepare(&sql).map_err(Error::Database)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(s) = skill_name {
        params.push(Box::new(s.to_string()));
    }
    if let Some(d) = from_date {
        params.push(Box::new(d.to_string()));
    }
    if let Some(d) = to_date {
        params.push(Box::new(d.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "source": row.get::<_, String>(1)?,
                "sourcePath": row.get::<_, Option<String>>(2)?,
                "skillName": row.get::<_, String>(3)?,
                "triggerCount": row.get::<_, u64>(4)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

#[cfg(test)]
#[path = "db_tests.rs"]
mod db_tests;
