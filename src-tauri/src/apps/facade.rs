//! Apps facade —— 现有表 read-through 与真实生命周期管理（APP-001 ~ APP-018）。
//!
//! 复用 > 新建：只读投影 `applications` / `runtime_instances` / `startup_plans` /
//! `application_surfaces` 四张成熟表；
//! 运行（RuntimeInstance）与呈现（Surface）分离，Surface close ≠ Runtime stop。

use rusqlite::Connection as DbConn;
use serde_json::Value;

use super::model::{App, RuntimeInstance, RuntimeSpec, Surface};
use crate::{Error, Result};

/// 列出全部 App（按更新时间倒序）。
pub fn list_apps(conn: &DbConn) -> Result<Vec<App>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, source, source_id, title, description, icon, version, created_at, updated_at
             FROM applications ORDER BY updated_at DESC",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(App {
                id: row.get(0)?,
                source: row.get(1)?,
                source_id: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                icon: row.get(5)?,
                version: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(Error::Database)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

/// 单个 App。
pub fn get_app(conn: &DbConn, id: &str) -> Result<Option<App>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, source, source_id, title, description, icon, version, created_at, updated_at
             FROM applications WHERE id = ?1",
        )
        .map_err(Error::Database)?;
    let mut rows = stmt
        .query_map([id], |row| {
            Ok(App {
                id: row.get(0)?,
                source: row.get(1)?,
                source_id: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                icon: row.get(5)?,
                version: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(Error::Database)?;
    rows.next().transpose().map_err(Error::Database)
}

/// 创建或注册新 App。
pub fn create_app(
    conn: &DbConn,
    title: &str,
    source: &str,
    source_id: &str,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<App> {
    let id = format!("app-{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().to_rfc3339();
    let version = "1.0.0".to_string();

    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        [&id, source, source_id, title, description.unwrap_or(""), icon.unwrap_or(""), &version, &now, &now],
    )
    .map_err(Error::Database)?;

    Ok(App {
        id,
        source: source.to_string(),
        source_id: source_id.to_string(),
        title: title.to_string(),
        description: description.map(|s| s.to_string()),
        icon: icon.map(|s| s.to_string()),
        version,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// 删除 App。
pub fn delete_app(conn: &DbConn, id: &str) -> Result<bool> {
    let count = conn
        .execute("DELETE FROM applications WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(count > 0)
}

/// App 的当前有效 RuntimeSpec（`startup_plans.plan_json` 反序列化）。
pub fn active_runtime_spec(conn: &DbConn, application_id: &str) -> Result<Option<RuntimeSpec>> {
    let plan_json: Option<String> = conn
        .query_row(
            "SELECT plan_json FROM startup_plans
             WHERE application_id = ?1 AND is_active = 1
             ORDER BY plan_version DESC LIMIT 1",
            [application_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::Database)?;
    let Some(json) = plan_json else {
        return Ok(None);
    };
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|e| Error::Internal(format!("invalid RuntimeSpec JSON: {e}")))
}

/// 某 App 的运行实例（最新在前）。
pub fn list_runtime_instances(conn: &DbConn, application_id: &str) -> Result<Vec<RuntimeInstance>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, application_id, plan_id, status, cleanup_status, owner_kind,
                    pgid, current_port, pid, failure, created_at, updated_at
             FROM runtime_instances WHERE application_id = ?1 ORDER BY updated_at DESC",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([application_id], |row| {
            Ok(RuntimeInstance {
                id: row.get(0)?,
                application_id: row.get(1)?,
                plan_id: row.get(2)?,
                status: row.get(3)?,
                cleanup_status: row.get(4)?,
                owner_kind: row.get(5)?,
                pgid: row.get(6)?,
                current_port: row.get(7)?,
                pid: row.get(8)?,
                failure: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(Error::Database)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

/// App 的 Surfaces（呈现层，与 Runtime 分离）。
pub fn list_surfaces(conn: &DbConn, application_id: &str) -> Result<Vec<Surface>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, application_id, kind, label, title, url, bounds_json, created_at, updated_at
             FROM application_surfaces WHERE application_id = ?1 ORDER BY created_at",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([application_id], |row| {
            Ok(Surface {
                id: row.get(0)?,
                application_id: row.get(1)?,
                kind: row.get(2)?,
                label: row.get(3)?,
                title: row.get(4)?,
                url: row.get(5)?,
                bounds_json: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(Error::Database)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

/// 演示 read-through：`Surface` 的 URL 投影（不落库，纯派生）。
pub fn surface_urls(conn: &DbConn, application_id: &str) -> Result<Vec<Value>> {
    let surfaces = list_surfaces(conn, application_id)?;
    Ok(surfaces
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "kind": s.kind,
                "label": s.label,
                "url": s.url,
            })
        })
        .collect())
}

trait OptionalExt<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> DbConn {
        let conn = DbConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE applications (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                source_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                icon TEXT,
                version TEXT NOT NULL DEFAULT '1.0.0',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE startup_plans (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL,
                plan_version INTEGER NOT NULL DEFAULT 1,
                plan_json TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE runtime_instances (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL,
                plan_id TEXT,
                status TEXT NOT NULL,
                cleanup_status TEXT,
                owner_kind TEXT NOT NULL,
                pgid INTEGER,
                current_port INTEGER,
                pid INTEGER,
                failure TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE application_surfaces (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                label TEXT NOT NULL,
                title TEXT,
                url TEXT,
                bounds_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_app_crud_and_query() {
        let conn = test_conn();
        let app = create_app(
            &conn,
            "Test React App",
            "local",
            "~/projects/react-test",
            Some("A test app"),
            None,
        )
        .unwrap();

        assert_eq!(app.title, "Test React App");
        assert_eq!(app.source, "local");

        let listed = list_apps(&conn).unwrap();
        assert_eq!(listed.len(), 1);

        let retrieved = get_app(&conn, &app.id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().title, "Test React App");

        let deleted = delete_app(&conn, &app.id).unwrap();
        assert!(deleted);

        let empty = list_apps(&conn).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_runtime_instances_and_surfaces() {
        let conn = test_conn();
        let app = create_app(&conn, "Vite App", "local", "~/vite", None, None).unwrap();

        conn.execute(
            "INSERT INTO runtime_instances VALUES ('inst-1', ?1, 'plan-1', 'running', NULL, 'local', 1234, 5173, 5678, NULL, 't0', 't1')",
            [&app.id],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO application_surfaces VALUES ('surf-1', ?1, 'main', 'Main Window', 'Vite App', 'http://localhost:5173', NULL, 't0', 't1')",
            [&app.id],
        )
        .unwrap();

        let instances = list_runtime_instances(&conn, &app.id).unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].status, "running");
        assert_eq!(instances[0].current_port, Some(5173));

        let surfaces = list_surfaces(&conn, &app.id).unwrap();
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0].url, Some("http://localhost:5173".into()));

        let urls = surface_urls(&conn, &app.id).unwrap();
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0]["url"], "http://localhost:5173");
    }
}
