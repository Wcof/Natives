//! AppRepository（APP-014 / APP-015）—— Apps 域唯一 SQL 层。
//!
//! 直接 SQL 读写 v28 schema（`applications` + 三类 spec 表 + `runtime_instances`），
//! 强类型读自 [`super::model`]。所有写操作：
//! - **事务化**（单语句写依赖 SQLite autocommit 原子性；多语句写显式 `transaction()`）；
//! - **幂等**（register 按 `(source, source_id)` 唯一键 upsert；spec upsert 按 PK）；
//! - **错误用 `crate::Error`**（0 行更新 = typed `Conflict`，不静默成功）。
//!
//! 删除语义（02 架构 §删除）：`remove_application` **只删注册**——applications
//! 行 + FK cascade 的 spec/surface/window/instance/plan 元数据；**不碰**用户项目
//! 目录、不碰 `.app`、不调用 BrowserProfile delete（清 Web 数据是独立高风险操作）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::model::{
    App, AppKind, AppView, RegistrationOrigin, RuntimeInstance, RuntimeSpec, Surface,
    UpdateAppMetadataInput,
};
use crate::{Error, Result};

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 本地项目 spec 行（无独立 spec 表 —— 复用 `local_creative_apps` 成熟表，
/// `canonical_project_root` 等字段由 local driver 维护；这里只投影注册侧需要的）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalProjectSpec {
    pub application_id: String,
    pub project_root: String,
    pub project_kind: String,
    pub plan_fingerprint: String,
}

/// 系统应用 spec 行（`system_application_specs`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemApplicationSpec {
    pub application_id: String,
    pub application_path: String,
    pub bundle_identifier: Option<String>,
    pub platform: String,
    pub launch_policy: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Web 应用 spec 行（`web_application_specs`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WebApplicationSpec {
    pub application_id: String,
    pub url: String,
    pub approved_origins: Vec<String>,
    pub open_behavior: String,
    pub keep_alive: bool,
    pub created_at: String,
    pub updated_at: String,
}

const APP_COLUMNS: &str =
    "id, source, source_id, title, description, icon, version, created_at, updated_at";

fn map_app_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<App> {
    Ok(App {
        id: r.get(0)?,
        source: r.get(1)?,
        source_id: r.get(2)?,
        title: r.get(3)?,
        description: r.get(4)?,
        icon: r.get(5)?,
        version: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

pub struct AppRepository;

impl AppRepository {
    // ── 查询 ───────────────────────────────────────────────────────────

    /// 全部 App（sidebar 优先、其余按更新时间倒序）。
    pub fn list(conn: &Connection) -> Result<Vec<App>> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {APP_COLUMNS} FROM applications
                 ORDER BY show_in_sidebar DESC, COALESCE(sidebar_order, 0) ASC, updated_at DESC"
            ))
            .map_err(Error::Database)?;
        let rows = stmt.query_map([], map_app_row).map_err(Error::Database)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Error::Database)
    }

    /// 单个 App。
    pub fn get(conn: &Connection, id: &str) -> Result<Option<App>> {
        conn.query_row(
            &format!("SELECT {APP_COLUMNS} FROM applications WHERE id = ?1"),
            params![id],
            map_app_row,
        )
        .optional()
        .map_err(Error::Database)
    }

    /// 按 (source, source_id) 反查统一 id（read-only，永不 fabricate 行）。
    pub fn find_by_source(
        conn: &Connection,
        source: &str,
        source_id: &str,
    ) -> Result<Option<String>> {
        conn.query_row(
            "SELECT id FROM applications WHERE source = ?1 AND source_id = ?2",
            params![source, source_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(Error::Database)
    }

    /// kind / origin / sidebar / metadata 投影（v28 新列）。
    pub fn identity(
        conn: &Connection,
        id: &str,
    ) -> Result<(
        Option<AppKind>,
        Option<RegistrationOrigin>,
        bool,
        Option<i64>,
    )> {
        let row: (Option<String>, Option<String>, i64, Option<i64>) = conn
            .query_row(
                "SELECT kind, registration_origin, show_in_sidebar, sidebar_order
                 FROM applications WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(Error::Database)?
            .ok_or_else(|| Error::NotFound(id.into()))?;
        Ok((
            row.0.as_deref().and_then(AppKind::parse),
            row.1.as_deref().and_then(RegistrationOrigin::parse),
            row.2 != 0,
            row.3,
        ))
    }

    /// 某 App 的活动实例（最新一条非终态 + 全量按时间倒序由 service 组合）。
    pub fn active_instance(
        conn: &Connection,
        application_id: &str,
    ) -> Result<Option<RuntimeInstance>> {
        conn.query_row(
            "SELECT id, application_id, plan_id, status, cleanup_status, owner_kind,
                    pgid, current_port, pid, failure, created_at, updated_at
             FROM runtime_instances
             WHERE application_id = ?1
               AND status IN ('starting', 'running', 'stopping', 'cleanup_failed', 'orphaned')
             ORDER BY updated_at DESC, id DESC LIMIT 1",
            params![application_id],
            map_instance_row,
        )
        .optional()
        .map_err(Error::Database)
    }

    /// 某 App 全部实例（最新在前）。
    pub fn list_instances(conn: &Connection, application_id: &str) -> Result<Vec<RuntimeInstance>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, application_id, plan_id, status, cleanup_status, owner_kind,
                        pgid, current_port, pid, failure, created_at, updated_at
                 FROM runtime_instances WHERE application_id = ?1
                 ORDER BY updated_at DESC, id DESC",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map(params![application_id], map_instance_row)
            .map_err(Error::Database)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Error::Database)
    }

    /// App 的 Surfaces（呈现层）。
    pub fn list_surfaces(conn: &Connection, application_id: &str) -> Result<Vec<Surface>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, application_id, kind, label, title, url, bounds_json, created_at, updated_at
                 FROM application_surfaces WHERE application_id = ?1 ORDER BY created_at",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map(params![application_id], map_surface_row)
            .map_err(Error::Database)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Error::Database)
    }

    /// App 的当前有效 RuntimeSpec（`startup_plans` 活动 plan 反序列化）。
    pub fn active_spec(conn: &Connection, application_id: &str) -> Result<Option<RuntimeSpec>> {
        let plan_json: Option<String> = conn
            .query_row(
                "SELECT plan_json FROM startup_plans
                 WHERE application_id = ?1 AND is_active = 1
                 ORDER BY plan_version DESC, id DESC LIMIT 1",
                params![application_id],
                |r| r.get(0),
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

    // ── 注册（幂等，按 kind 分派 source 命名空间）───────────────────────

    /// 注册本地项目：applications(kind=local_project) 行，幂等 upsert。
    /// 返回统一 application id。
    pub fn register_local(
        conn: &Connection,
        title: &str,
        project_root: &str,
        description: Option<&str>,
        icon: Option<&str>,
        origin: RegistrationOrigin,
    ) -> Result<String> {
        Self::upsert_application(
            conn,
            AppKind::LocalProject,
            "local_project",
            project_root,
            title,
            description,
            icon,
            origin,
        )
    }

    /// 注册系统应用：applications(kind=system_application) + spec 行，幂等。
    pub fn register_system(
        conn: &Connection,
        title: &str,
        application_path: &str,
        bundle_identifier: Option<&str>,
        platform: &str,
        launch_policy: Option<&str>,
        origin: RegistrationOrigin,
    ) -> Result<String> {
        if application_path.trim().is_empty() {
            return Err(Error::InvalidInput("applicationPath is required".into()));
        }
        let launch_policy = launch_policy
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("activate_existing");
        let app_id = Self::upsert_application(
            conn,
            AppKind::SystemApplication,
            "system_application",
            application_path,
            title,
            None,
            None,
            origin,
        )?;
        let t = now();
        conn.execute(
            "INSERT INTO system_application_specs
                (application_id, application_path, bundle_identifier, platform, launch_policy,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(application_id) DO UPDATE SET
                application_path = excluded.application_path,
                bundle_identifier = excluded.bundle_identifier,
                platform = excluded.platform,
                launch_policy = excluded.launch_policy,
                updated_at = excluded.updated_at",
            params![
                app_id,
                application_path,
                bundle_identifier,
                platform,
                launch_policy,
                t
            ],
        )
        .map_err(Error::Database)?;
        Ok(app_id)
    }

    /// 注册 Web 应用：applications(kind=web_application) + spec 行，幂等。
    pub fn register_web(
        conn: &Connection,
        title: &str,
        url: &str,
        approved_origins: &[String],
        open_behavior: Option<&str>,
        keep_alive: bool,
    ) -> Result<String> {
        // APPV2-T01：共享校验层统一入口 —— 无 scheme 输入规范化为 https 后持久化，
        // 公网必须 https、显式 http 仅 loopback（见 `web_url` 模块）。
        let normalized = crate::apps::web_url::normalize_web_url(url)?;
        let open_behavior = open_behavior
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("native_webview");
        let app_id = Self::upsert_application(
            conn,
            AppKind::WebApplication,
            "web_application",
            &normalized,
            title,
            None,
            None,
            RegistrationOrigin::Manual,
        )?;
        let t = now();
        conn.execute(
            "INSERT INTO web_application_specs
                (application_id, url, approved_origins_json, open_behavior, keep_alive,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(application_id) DO UPDATE SET
                url = excluded.url,
                approved_origins_json = excluded.approved_origins_json,
                open_behavior = excluded.open_behavior,
                keep_alive = excluded.keep_alive,
                updated_at = excluded.updated_at",
            params![
                app_id,
                normalized,
                serde_json::to_string(approved_origins).map_err(|e| Error::Json(e))?,
                open_behavior,
                keep_alive as i64,
                t
            ],
        )
        .map_err(Error::Database)?;
        Ok(app_id)
    }

    /// 通用幂等注册：`(source, source_id)` 唯一键存在 → 仅刷新 kind/origin，
    /// 返回既有 id；不存在 → 插入新行。
    fn upsert_application(
        conn: &Connection,
        kind: AppKind,
        source: &str,
        source_id: &str,
        title: &str,
        description: Option<&str>,
        icon: Option<&str>,
        origin: RegistrationOrigin,
    ) -> Result<String> {
        if let Some(id) = Self::find_by_source(conn, source, source_id)? {
            // 幂等：刷新权威身份列（kind/origin 成为权威，source 保留兼容期）。
            let n = conn
                .execute(
                    "UPDATE applications SET kind = ?2, registration_origin = ?3, updated_at = ?4
                     WHERE id = ?1",
                    params![id, kind.as_str(), origin.as_str(), now()],
                )
                .map_err(Error::Database)?;
            if n == 0 {
                return Err(Error::Conflict(format!(
                    "application vanished during register: {id}"
                )));
            }
            return Ok(id);
        }
        let id = format!("app-{}", Uuid::new_v4());
        let t = now();
        conn.execute(
            "INSERT INTO applications
                (id, source, source_id, title, description, icon, version,
                 kind, registration_origin, show_in_sidebar, sidebar_order,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '1', ?7, ?8, 0, NULL, ?9, ?9)",
            params![
                id,
                source,
                source_id,
                title,
                description,
                icon,
                kind.as_str(),
                origin.as_str(),
                t
            ],
        )
        .map_err(Error::Database)?;
        Ok(id)
    }

    // ── 编辑（事务化 / 幂等 / 0 行 = Conflict）─────────────────────────

    /// 通用 metadata 更新（title/description/icon/sidebar）。
    pub fn update_metadata(
        conn: &mut Connection,
        input: &UpdateAppMetadataInput,
    ) -> Result<AppView> {
        if Self::get(conn, &input.app_id)?.is_none() {
            return Err(Error::NotFound(input.app_id.clone()));
        }
        let tx = conn.transaction().map_err(Error::Database)?;
        if let Some(t) = &input.title {
            let t = t.trim();
            if t.is_empty() {
                return Err(Error::InvalidInput("title must not be empty".into()));
            }
            tx.execute(
                "UPDATE applications SET title = ?2, updated_at = ?3 WHERE id = ?1",
                params![input.app_id, t, now()],
            )
            .map_err(Error::Database)?;
        }
        if let Some(d) = &input.description {
            tx.execute(
                "UPDATE applications SET description = ?2, updated_at = ?3 WHERE id = ?1",
                params![input.app_id, d, now()],
            )
            .map_err(Error::Database)?;
        }
        if let Some(i) = &input.icon {
            tx.execute(
                "UPDATE applications SET icon = ?2, updated_at = ?3 WHERE id = ?1",
                params![input.app_id, i, now()],
            )
            .map_err(Error::Database)?;
        }
        if let Some(s) = input.show_in_sidebar {
            tx.execute(
                "UPDATE applications SET show_in_sidebar = ?2, updated_at = ?3 WHERE id = ?1",
                params![input.app_id, s as i64, now()],
            )
            .map_err(Error::Database)?;
        }
        if let Some(o) = input.sidebar_order {
            tx.execute(
                "UPDATE applications SET sidebar_order = ?2, updated_at = ?3 WHERE id = ?1",
                params![input.app_id, o, now()],
            )
            .map_err(Error::Database)?;
        }
        tx.commit().map_err(Error::Database)?;
        Self::view(conn, &input.app_id)
    }

    /// 系统应用 spec 更新（全字段可选 = 部分更新）。
    pub fn update_system_spec(
        conn: &mut Connection,
        app_id: &str,
        application_path: Option<&str>,
        bundle_identifier: Option<Option<&str>>,
        platform: Option<&str>,
        launch_policy: Option<&str>,
    ) -> Result<SystemApplicationSpec> {
        let t = now();
        let n = match (application_path, platform, launch_policy, bundle_identifier) {
            (Some(p), Some(pl), Some(lp), Some(bi)) => conn
                .execute(
                    "UPDATE system_application_specs
                     SET application_path = ?2, platform = ?3, launch_policy = ?4,
                         bundle_identifier = ?5, updated_at = ?6
                     WHERE application_id = ?1",
                    params![app_id, p, pl, lp, bi, t],
                )
                .map_err(Error::Database)?,
            (None, None, None, Some(bi)) => conn
                .execute(
                    "UPDATE system_application_specs
                     SET bundle_identifier = ?2, updated_at = ?3
                     WHERE application_id = ?1",
                    params![app_id, bi, t],
                )
                .map_err(Error::Database)?,
            (Some(p), None, None, None) => conn
                .execute(
                    "UPDATE system_application_specs
                     SET application_path = ?2, updated_at = ?3
                     WHERE application_id = ?1",
                    params![app_id, p, t],
                )
                .map_err(Error::Database)?,
            (None, Some(pl), None, None) => conn
                .execute(
                    "UPDATE system_application_specs
                     SET platform = ?2, updated_at = ?3
                     WHERE application_id = ?1",
                    params![app_id, pl, t],
                )
                .map_err(Error::Database)?,
            (None, None, Some(lp), None) => conn
                .execute(
                    "UPDATE system_application_specs
                     SET launch_policy = ?2, updated_at = ?3
                     WHERE application_id = ?1",
                    params![app_id, lp, t],
                )
                .map_err(Error::Database)?,
            (None, None, None, None) => 0, // no-op（幂等：全 None 不写）
            (Some(p), None, Some(lp), None) => conn
                .execute(
                    "UPDATE system_application_specs
                     SET application_path = ?2, launch_policy = ?3, updated_at = ?4
                     WHERE application_id = ?1",
                    params![app_id, p, lp, t],
                )
                .map_err(Error::Database)?,
            _ => {
                // 组合缺省：path/platform/launch_policy 任意子集 + bundle 已覆盖上面；
                // 其余组合按非 None 字段拼 UPDATE。
                let mut sets: Vec<&str> = vec![];
                let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![];
                if let Some(p) = application_path {
                    sets.push("application_path = ?");
                    args.push(Box::new(p.to_string()));
                }
                if let Some(pl) = platform {
                    sets.push("platform = ?");
                    args.push(Box::new(pl.to_string()));
                }
                if let Some(lp) = launch_policy {
                    sets.push("launch_policy = ?");
                    args.push(Box::new(lp.to_string()));
                }
                if let Some(bi) = bundle_identifier {
                    sets.push("bundle_identifier = ?");
                    args.push(Box::new(bi.map(|s| s.to_string())));
                }
                if sets.is_empty() {
                    0
                } else {
                    sets.push("updated_at = ?");
                    args.push(Box::new(t));
                    args.push(Box::new(app_id.to_string()));
                    let sql = format!(
                        "UPDATE system_application_specs SET {} WHERE application_id = ?",
                        sets.join(", ")
                    );
                    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
                    conn.execute(&sql, refs.as_slice())
                        .map_err(Error::Database)?
                }
            }
        };
        if n == 0
            && (application_path.is_some()
                || platform.is_some()
                || launch_policy.is_some()
                || bundle_identifier.is_some())
        {
            // spec 行不存在 = 未注册为 system app（或已被删）→ typed NotFound。
            return Err(Error::NotFound(format!("system spec for {app_id}")));
        }
        Self::load_system_spec(conn, app_id)
    }

    /// Web 应用 spec 更新（全字段可选 = 部分更新）。
    pub fn update_web_spec(
        conn: &mut Connection,
        app_id: &str,
        url: Option<&str>,
        approved_origins: Option<&[String]>,
        open_behavior: Option<&str>,
        keep_alive: Option<bool>,
    ) -> Result<WebApplicationSpec> {
        // APPV2-T01：与 register 同一共享校验层（规范化 + 策略）。
        let normalized_url = match url {
            Some(u) => Some(crate::apps::web_url::normalize_web_url(u)?),
            None => None,
        };
        let t = now();
        let mut sets: Vec<&str> = vec![];
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![];
        if let Some(u) = &normalized_url {
            sets.push("url = ?");
            args.push(Box::new(u.clone()));
        }
        if let Some(o) = approved_origins {
            sets.push("approved_origins_json = ?");
            args.push(Box::new(
                serde_json::to_string(o).map_err(|e| Error::Json(e))?,
            ));
        }
        if let Some(b) = open_behavior {
            sets.push("open_behavior = ?");
            args.push(Box::new(b.to_string()));
        }
        if let Some(k) = keep_alive {
            sets.push("keep_alive = ?");
            args.push(Box::new(k as i64));
        }
        if sets.is_empty() {
            return Self::load_web_spec(conn, app_id); // no-op（幂等）
        }
        sets.push("updated_at = ?");
        args.push(Box::new(t));
        args.push(Box::new(app_id.to_string()));
        let sql = format!(
            "UPDATE web_application_specs SET {} WHERE application_id = ?",
            sets.join(", ")
        );
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let n = conn
            .execute(&sql, refs.as_slice())
            .map_err(Error::Database)?;
        if n == 0 {
            return Err(Error::NotFound(format!("web spec for {app_id}")));
        }
        Self::load_web_spec(conn, app_id)
    }

    /// 侧边栏显隐 + 排序（独立 IPC：apps_set_sidebar_visibility / _order）。
    pub fn set_sidebar(
        conn: &mut Connection,
        app_id: &str,
        show: Option<bool>,
        order: Option<i64>,
    ) -> Result<AppView> {
        if Self::get(conn, app_id)?.is_none() {
            return Err(Error::NotFound(app_id.into()));
        }
        let t = now();
        if let Some(s) = show {
            let n = conn
                .execute(
                    "UPDATE applications SET show_in_sidebar = ?2, updated_at = ?3 WHERE id = ?1",
                    params![app_id, s as i64, t],
                )
                .map_err(Error::Database)?;
            if n == 0 {
                return Err(Error::Conflict(format!(
                    "application vanished during sidebar update: {app_id}"
                )));
            }
        }
        if let Some(o) = order {
            let n = conn
                .execute(
                    "UPDATE applications SET sidebar_order = ?2, updated_at = ?3 WHERE id = ?1",
                    params![app_id, o, t],
                )
                .map_err(Error::Database)?;
            if n == 0 {
                return Err(Error::Conflict(format!(
                    "application vanished during sidebar update: {app_id}"
                )));
            }
        }
        Self::view(conn, app_id)
    }

    // ── Spec 读 ────────────────────────────────────────────────────────

    /// 本地项目 spec 投影（复用 `local_creative_apps` 成熟表，read-only）。
    pub fn load_local_spec(
        conn: &Connection,
        application_id: &str,
    ) -> Result<Option<LocalProjectSpec>> {
        let id = application_id.to_string();
        let source_id = Self::find_local_by_app_id(conn, application_id)?
            .ok_or_else(|| Error::NotFound(id.clone()))?;
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT l.canonical_project_root, l.project_kind, l.plan_fingerprint
                 FROM local_creative_apps l
                 WHERE l.id = ?1",
                params![source_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(Error::Database)?;
        Ok(Some(match row {
            Some((root, kind, fp)) => LocalProjectSpec {
                application_id: id,
                project_root: root,
                project_kind: kind,
                plan_fingerprint: fp,
            },
            // 无 local 明细行的 local app（如 legacy internal backfill）也投影注册侧 id。
            None => LocalProjectSpec {
                application_id: id,
                project_root: source_id,
                project_kind: "unknown".into(),
                plan_fingerprint: String::new(),
            },
        }))
    }

    fn find_local_by_app_id(conn: &Connection, application_id: &str) -> Result<Option<String>> {
        conn.query_row(
            "SELECT source_id FROM applications WHERE id = ?1 AND kind = 'local_project'",
            params![application_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(Error::Database)
    }

    /// 系统应用 spec。
    pub fn load_system_spec(
        conn: &Connection,
        application_id: &str,
    ) -> Result<SystemApplicationSpec> {
        let row: (String, Option<String>, String, String, String, String) = conn
            .query_row(
                "SELECT application_path, bundle_identifier, platform, launch_policy, created_at, updated_at
                 FROM system_application_specs WHERE application_id = ?1",
                params![application_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()
            .map_err(Error::Database)?
            .ok_or_else(|| Error::NotFound(format!("system spec for {application_id}")))?;
        Ok(SystemApplicationSpec {
            application_id: application_id.into(),
            application_path: row.0,
            bundle_identifier: row.1,
            platform: row.2,
            launch_policy: row.3,
            created_at: row.4,
            updated_at: row.5,
        })
    }

    /// Web 应用 spec。
    pub fn load_web_spec(conn: &Connection, application_id: &str) -> Result<WebApplicationSpec> {
        let row: (String, String, String, bool, String, String) = conn
            .query_row(
                "SELECT url, approved_origins_json, open_behavior, keep_alive, created_at, updated_at
                 FROM web_application_specs WHERE application_id = ?1",
                params![application_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()
            .map_err(Error::Database)?
            .ok_or_else(|| Error::NotFound(format!("web spec for {application_id}")))?;
        let approved_origins: Vec<String> = serde_json::from_str(&row.1).unwrap_or_default();
        Ok(WebApplicationSpec {
            application_id: application_id.into(),
            url: row.0,
            approved_origins,
            open_behavior: row.2,
            keep_alive: row.3,
            created_at: row.4,
            updated_at: row.5,
        })
    }

    // ── 删除（只删注册，不碰外部资产）─────────────────────────────────

    /// 删除应用注册：applications 行 + FK cascade（specs / surfaces / windows /
    /// runtime_instances / startup_plans / operations 引用自动清理）。
    ///
    /// **绝不**删用户项目目录、`.app`、BrowserProfile（02 架构 §删除 /
    /// APP-015 step 4）。返回是否删除了行（幂等：重复删除 = false）。
    pub fn remove_application(conn: &Connection, application_id: &str) -> Result<bool> {
        // FK cascade 覆盖 spec/surface/window/instance/plan；显式先清
        // preview_targets（其 FK 指向 runtime_instances，由 cascade 级联，
        // 但显式删除让「删除后无悬空 preview」在 FK off 的环境也成立）。
        conn.execute(
            "DELETE FROM preview_targets WHERE runtime_instance_id IN
             (SELECT id FROM runtime_instances WHERE application_id = ?1)",
            params![application_id],
        )
        .map_err(Error::Database)?;
        let n = conn
            .execute(
                "DELETE FROM applications WHERE id = ?1",
                params![application_id],
            )
            .map_err(Error::Database)?;
        Ok(n > 0)
    }

    // ── 视图投影 ───────────────────────────────────────────────────────

    /// AppView 基础投影（capabilities 由 service 用 CapabilityResolver 填充）。
    pub fn view(conn: &Connection, app_id: &str) -> Result<AppView> {
        let app = Self::get(conn, app_id)?.ok_or_else(|| Error::NotFound(app_id.into()))?;
        let (kind, origin, show, order) = Self::identity(conn, app_id)?;
        Ok(AppView {
            app_id: app.id,
            title: app.title,
            kind: kind.map(|k| k.as_str().to_string()).unwrap_or_default(),
            registration_origin: origin.map(|o| o.as_str().to_string()).unwrap_or_default(),
            description: app.description,
            show_in_sidebar: show,
            sidebar_order: order,
            capabilities: Default::default(),
            runtime_state: crate::apps::model::AppRuntimeState::Stopped,
            updated_at: app.updated_at,
        })
    }
}

fn map_instance_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RuntimeInstance> {
    Ok(RuntimeInstance {
        id: r.get(0)?,
        application_id: r.get(1)?,
        plan_id: r.get(2)?,
        status: r.get(3)?,
        cleanup_status: r.get(4)?,
        owner_kind: r.get(5)?,
        pgid: r.get(6)?,
        current_port: r.get(7)?,
        pid: r.get(8)?,
        failure: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

fn map_surface_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Surface> {
    Ok(Surface {
        id: r.get(0)?,
        application_id: r.get(1)?,
        kind: r.get(2)?,
        label: r.get(3)?,
        title: r.get(4)?,
        url: r.get(5)?,
        bounds_json: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

/// Web URL 校验（APP-013 step 5 / 06：后端先验 URL）。
/// APPV2-T01：实现收敛到 [`crate::apps::web_url`]（注册/编辑/导航共用单一策略，
/// 修复此前「注册允许 https、导航只放行 http」的规则不一致）。
pub fn validate_web_url(url: &str) -> Result<()> {
    crate::apps::web_url::validate_web_url(url)
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
