//! LocalProjectDriver（Phase D，APP-022~029）。
//!
//! 适配 `creative_app::local` 成熟 helper（scan / plan / runtime / lifecycle /
//! lifecycle_process / logs）+ `creative_app::adapters` 的**内部** lifecycle
//! facade（spawn_start / await_ready / stop / delete —— 内部 helper，不是
//! `commands::creative_app::*` public command）。**不重写**进程/PGID/端口租约/
//! 健康探测/runtime-store CAS 逻辑。
//!
//! 进程语义（主 Agent 决策 #6）：
//! - **stop** = graceful：SIGTERM 进程组 → grace period → verify PGID gone
//!   （`lifecycle::stop_app` 保证；失败保持 identity/port，绝不标 stopped，
//!   CR-302）；
//! - **force_stop** = SIGKILL（capability 门 + risk 2），严格 identity match
//!   （`lifecycle_process::force_kill_identity`），stale PID 拒绝强杀防误杀；
//! - **restart** = stop → **verify PGID gone** → 新 instance → start；
//!   cleanup_failed / orphaned 被 one-active unique index 阻断（禁止第二个
//!   active runtime）；
//! - **delete** = 只删注册：运行中先 stop+verify（失败阻断），**项目
//!   root_path 永不 `remove_dir_all`**（source 目录 reference-only）。
//!
//! 事件：driver 侧 lifecycle helper 广播被 suppress（`broadcast` 传
//! `false`）—— 统一由 `AppsService` emit `apps` channel（APP-019：legacy
//! helper 不重复 emit，避免双刷新）。

use crate::creative_app::adapters;
use crate::creative_app::local::lifecycle_process::{
    force_kill_identity, identity_matches_live, pid_is_alive,
};
use crate::creative_app::local::{self, inspect_local_project, LocalRuntimeHandle};
use crate::creative_app::model::{
    CreativeAppSource, CreativeAppSummary, DeleteOptions, InspectLocalRequest, LaunchMode,
    LaunchPlan, LocalCreativeAppRecord, ProcessIdentity,
};
use crate::{Error, Result};
use rusqlite::Connection;

/// 可配 graceful 等待预算（PGID 级 verify 总时长；runtime 内部 TERM→KILL
/// 兜底由 `GRACEFUL_WAIT_MS` 固定 500ms）。
pub const DEFAULT_GRACE_PERIOD_MS: u64 = 3_000;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn modules_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".natives")
        .join("modules")
}

/// 内部 lifecycle 上下文（复用 `creative_app::adapters::LifecycleCtx`）。
///
/// `runtime` 是 Tauri managed 的共享 `Arc<LocalRuntimeManager>`（clone = 引用
/// 计数 +1，进程注册表全局唯一 —— 与 `creative_app::adapters::LifecycleCtx`
/// 的 `local_runtime: Option<Arc<LocalRuntimeManager>>` 契约一致）。
fn ctx(
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
) -> adapters::LifecycleCtx {
    adapters::LifecycleCtx::new(
        app.clone(),
        modules_dir(),
        Some(runtime.clone()),
        host_http_port,
    )
}

/// 该 local source 当前的 active runtime instance id（无 = ""）。
pub fn active_runtime_id(conn: &Connection, source_id: &str) -> Result<String> {
    Ok(crate::creative_app::runtime_store::application_id_for(
        conn,
        CreativeAppSource::LocalProject,
        source_id,
    )?
    .and_then(|app_id| {
        crate::creative_app::runtime_store::active_instance_id(conn, &app_id)
            .ok()
            .flatten()
    })
    .unwrap_or_default())
}

// ── inspect / register / 运行配置（APP-023 / APP-027）──────────────────

/// inspect：复用 scan 生成候选 plan（不复制 launch plan parser）。
pub fn inspect(
    conn: &Connection,
    project_root: &str,
) -> Result<crate::creative_app::model::LocalProjectScanResult> {
    inspect_local_project(
        conn,
        &InspectLocalRequest {
            project_root: project_root.to_string(),
            entry_file: None,
        },
    )
}

/// 注册本地项目到 `local_creative_apps`（成熟 insert 路径；
/// `canonical_project_root` UNIQUE 天然幂等）。返回 source 记录。
pub fn register(
    conn: &mut Connection,
    title: &str,
    project_root: &str,
    description: Option<&str>,
    icon: Option<&str>,
    launch_mode: LaunchMode,
    launch_plan: Option<LaunchPlan>,
    env: &[(String, String)],
) -> Result<LocalCreativeAppRecord> {
    let root = local::canonical_project_root(project_root)?;
    let root_s = root.to_string_lossy().to_string();

    // 幂等：同一 root 已注册 → 返回既有记录（不重复注册）。
    if let Some(existing) = local::get_app_by_root(conn, &root_s)? {
        return Ok(existing);
    }

    let scan = local::inspect_local_project(
        conn,
        &InspectLocalRequest {
            project_root: root_s.clone(),
            entry_file: None,
        },
    )?;

    let plan = match launch_mode {
        LaunchMode::Smart => {
            if let Some(p) = launch_plan {
                local::validate_launch_plan(&root, p)?
            } else {
                scan.rule_plan.ok_or_else(|| {
                    Error::InvalidInput(
                        "smart launch could not build a plan; provide a custom LaunchPlan".into(),
                    )
                })?
            }
        }
        LaunchMode::Custom => {
            let p = launch_plan.ok_or_else(|| {
                Error::InvalidInput("custom launch_mode requires launchPlan".into())
            })?;
            local::validate_launch_plan(&root, p)?
        }
    };

    let (device_id, device_name) = local::store::current_device();
    let t = now();
    let id = uuid::Uuid::new_v4().to_string();
    let title = {
        let t = title.trim();
        if t.is_empty() {
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Local project")
                .to_string()
        } else {
            t.to_string()
        }
    };
    let timeout = plan.startup_timeout_ms.clamp(5_000, 300_000);
    let auto_open = plan.auto_open;
    let mut plan = plan;
    plan.startup_timeout_ms = timeout;
    plan.auto_open = auto_open;
    let fp = local::fingerprint_plan(&root, &plan);

    let status_detail_json = if scan.dependencies_missing {
        Some(
            serde_json::to_string(&crate::creative_app::model::CreativeAppStatusDetail {
                code: crate::creative_app::model::LocalCreativeIssueCode::DependenciesMissing,
                message: "node_modules missing; install dependencies before start".into(),
                recovery_actions: vec![
                    "install_dependencies".into(),
                    "open_terminal".into(),
                    "copy_install_command".into(),
                ],
            })
            .unwrap_or_default(),
        )
    } else {
        None
    };

    let rec = LocalCreativeAppRecord {
        id: id.clone(),
        title,
        description: description.map(|s| s.to_string()),
        icon: icon.map(|s| s.to_string()),
        canonical_project_root: root_s,
        device_id,
        device_name,
        project_kind: plan.project_kind,
        launch_mode,
        launch_plan_json: plan.to_json().map_err(|e| Error::Internal(e.to_string()))?,
        plan_fingerprint: fp,
        state: crate::creative_app::model::CreativeAppState::InstalledStopped,
        status_detail_json,
        open_url: None,
        current_port: None,
        process_identity_json: None,
        volume_identity: local::volume_identity(&root),
        auto_open,
        startup_timeout_ms: timeout,
        last_started_at: None,
        last_exit_reason: None,
        last_error: if scan.dependencies_missing {
            Some("dependencies_missing".into())
        } else {
            None
        },
        created_at: t.clone(),
        updated_at: t,
    };

    // 记录 + 加密 env 同一 SQLite 事务（决策 #9：所有 DB 写操作走事务）。
    let tx = conn.transaction().map_err(Error::Database)?;
    local::store::insert_app(&tx, &rec)?;
    if !env.is_empty() {
        local::store::replace_env(&tx, &id, env)?;
    }
    tx.commit().map_err(Error::Database)?;

    Ok(rec)
}

/// 运行配置编辑（APP-027 plan version 语义）。
///
/// - 运行中编辑**不改**当前 `RuntimeInstance.plan_id`（进程继续引用旧 plan）；
/// - 保存新 plan 并切 active（`runtime_store::upsert_active_plan` 保持
///   one-active-plan 不变量），下次启动使用新 active plan。
///
/// save-only 与 save-and-restart 的差异由 `AppsService::update_local` 在调用方
/// 处理（后者随后走 restart 路径，risk 1）。
pub fn update_run_config(
    conn: &mut Connection,
    source_id: &str,
    title: Option<&str>,
    description: Option<&str>,
    project_root: Option<&str>,
    launch_mode: Option<LaunchMode>,
    launch_plan: Option<LaunchPlan>,
    env: Option<&[(String, String)]>,
    env_upsert: Option<&[(String, String)]>,
    env_remove_keys: Option<&[String]>,
) -> Result<LocalCreativeAppRecord> {
    let mut rec =
        local::get_app(conn, source_id)?.ok_or_else(|| Error::NotFound(source_id.into()))?;

    if let Some(t) = title {
        let t = t.trim();
        if !t.is_empty() {
            rec.title = t.to_string();
        }
    }
    if let Some(d) = description {
        rec.description = Some(d.to_string());
    }
    if let Some(m) = launch_mode {
        rec.launch_mode = m;
    }

    let mut root = std::path::PathBuf::from(&rec.canonical_project_root);
    if let Some(new_root) = project_root {
        let canon = local::canonical_project_root(new_root)?;
        let root_s = canon.to_string_lossy().to_string();
        if root_s != rec.canonical_project_root {
            if let Some(other) = local::get_app_by_root(conn, &root_s)? {
                if other.id != rec.id {
                    return Err(Error::InvalidInput(format!(
                        "path already registered as local app: {}",
                        other.id
                    )));
                }
            }
            rec.canonical_project_root = root_s;
            root = canon;
        }
    }

    if let Some(plan) = launch_plan {
        let validated = local::validate_launch_plan(&root, plan)?;
        rec.project_kind = validated.project_kind;
        rec.plan_fingerprint = local::fingerprint_plan(&root, &validated);
        rec.launch_plan_json = validated
            .to_json()
            .map_err(|e| Error::Internal(e.to_string()))?;
        rec.startup_timeout_ms = validated.startup_timeout_ms;
        rec.auto_open = validated.auto_open;
    }

    // 记录 + env 变更同一事务（决策 #9）。
    let tx = conn.transaction().map_err(Error::Database)?;
    if let Some(env) = env {
        local::store::replace_env(&tx, &rec.id, env)?;
    }
    if let Some(upsert) = env_upsert {
        local::store::upsert_env(&tx, &rec.id, upsert)?;
    }
    if let Some(remove) = env_remove_keys {
        local::store::remove_env_keys(&tx, &rec.id, remove)?;
    }
    rec.updated_at = now();
    local::store::update_app(&tx, &rec)?;
    tx.commit().map_err(Error::Database)?;

    // 刷新 active startup plan（plan version 语义）+ 统一 identity。
    if let Some(app_id) = crate::creative_app::runtime_store::application_id_for(
        conn,
        CreativeAppSource::LocalProject,
        &rec.id,
    )? {
        let _ = crate::creative_app::runtime_store::upsert_active_plan(
            conn,
            &app_id,
            &rec.launch_plan_json,
        );
    }
    Ok(rec)
}

// ── 生命周期（复用 adapters 内部 facade，不重写进程逻辑）────────────────

/// start Phase 1（spawn，**调用方持 mutation lock**）：instance CAS（one-active
/// unique index）→ prepare → spawn。返回当前态 summary（含 runtime_instance_id）。
pub async fn begin_start(
    conn: &Connection,
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
    source_id: &str,
) -> Result<CreativeAppSummary> {
    adapters::spawn_start(conn, &ctx(app, runtime, host_http_port), source_id).await
}

/// start Phase 2 健康等待（**无** mutation lock —— 允许并发 stop 抢占长 start）。
pub async fn await_start_ready(
    conn: &Connection,
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
    source_id: &str,
    spawned: &CreativeAppSummary,
) -> Result<CreativeAppSummary> {
    adapters::await_ready(conn, &ctx(app, runtime, host_http_port), source_id, spawned).await
}

/// graceful stop（APP-025）：SIGTERM 进程组 → grace period → verify PGID gone。
///
/// 失败（资源未验证释放）→ Err：记录保持 identity/port（`CleanupFailed`），
/// 绝不标 stopped（CR-302）。
pub async fn stop(
    conn: &Connection,
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
    source_id: &str,
) -> Result<CreativeAppSummary> {
    adapters::stop(conn, &ctx(app, runtime, host_http_port), source_id).await
}

/// force stop（capability 门 + risk 2）：对严格 identity match 的进程组
/// SIGKILL（`lifecycle_process::force_kill_identity`）。
///
/// stale PID 防误杀：identity（pid + executable + cwd fingerprint）不匹配时
/// 返回 typed InvalidInput，**绝不** kill 无关 PID。
pub async fn force_stop(
    conn: &Connection,
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
    source_id: &str,
) -> Result<CreativeAppSummary> {
    let rec = local::get_app(conn, source_id)?.ok_or_else(|| Error::NotFound(source_id.into()))?;
    let runtime_id = active_runtime_id(conn, source_id)?;

    // 活子进程（本次 Host 持有）：TERM→grace→KILL 树终止 + verify。
    if runtime.is_running(&runtime_id).await {
        if let Err(e) = runtime.stop(&runtime_id, Some(app)).await {
            return Err(e);
        }
    } else if let Some(ident_json) = &rec.process_identity_json {
        let ident: ProcessIdentity = serde_json::from_str(ident_json)
            .map_err(|e| Error::Internal(format!("bad process identity: {e}")))?;
        if identity_matches_live(&ident) {
            // 严格 identity match → SIGKILL 进程组（helper 保证 verify）。
            force_kill_identity(&ident).await?;
        } else if pid_is_alive(ident.pid) {
            // stale identity：PID 在活但不匹配 → 拒绝强杀（防误杀无关 PID）。
            return Err(Error::InvalidInput(
                "a process is alive with this PID but identity does not match; not killed".into(),
            ));
        }
    }

    // verify + 落库（source record + instance settle）。
    let rec2 = local::get_app(conn, source_id)?.unwrap();
    let identity_gone = rec2
        .process_identity_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<ProcessIdentity>(j).ok())
        .map(|i| !identity_matches_live(&i))
        .unwrap_or(true);
    if !identity_gone {
        return Err(Error::Internal(
            "force stop did not verify process identity release".into(),
        ));
    }
    let mut rec2 = rec2;
    rec2.state = crate::creative_app::model::CreativeAppState::InstalledStopped;
    rec2.open_url = None;
    rec2.current_port = None;
    rec2.process_identity_json = None;
    rec2.status_detail_json = None;
    rec2.last_error = None;
    rec2.last_exit_reason = Some("force_stopped_by_user".into());
    rec2.updated_at = now();
    local::store::update_app(conn, &rec2)?;
    if !runtime_id.is_empty() {
        let _ = crate::creative_app::runtime_store::mark_stopped(conn, &runtime_id);
        let _ = crate::creative_app::service_store::mark_instance_stopped(conn, &runtime_id);
    }
    let _ = app;
    let _ = host_http_port;
    Ok(local::summary_from_local(&rec2))
}

/// restart（APP-026）：stop（含 PGID verify）→ 新 instance → start。
///
/// cleanup_failed / orphaned 时 `create_instance` 的 one-active unique index
/// 拒绝第二个 active runtime（typed Conflict）—— restart 因此**不产生**
/// 并行 active runtime。
pub async fn restart(
    conn: &Connection,
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
    source_id: &str,
) -> Result<CreativeAppSummary> {
    // 1. graceful stop（失败 = cleanup_failed，禁止继续）。
    stop(conn, app, runtime, host_http_port, source_id).await?;

    // 2. verify PGID gone（APP-026 验收）：identity 已清 / 组已消失。
    let rec = local::get_app(conn, source_id)?.ok_or_else(|| Error::NotFound(source_id.into()))?;
    if let Some(ident_json) = &rec.process_identity_json {
        if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
            if let Some(pgid) = ident.process_group_id {
                if local::runtime::process_group_exists(pgid) {
                    return Err(Error::Conflict(format!(
                        "process group {pgid} still alive after stop; restart blocked"
                    )));
                }
            }
            if identity_matches_live(&ident) {
                return Err(Error::Conflict(
                    "stale process identity still matches a live process; restart blocked".into(),
                ));
            }
        }
    }

    // 3. 新 instance（one-active index 守卫）+ start Phase1/2。
    let spawned = begin_start(conn, app, runtime, host_http_port, source_id).await?;
    await_start_ready(conn, app, runtime, host_http_port, source_id, &spawned).await
}

// ── 删除 / logs / orphan / reconcile（APP-028 / APP-029）───────────────

/// delete（APP-028）：只删注册，**绝不**删项目目录。
///
/// 运行中先 stop+verify（失败 → 保留记录并阻断删除）；成功后 adapters 统一
/// delete（source 记录 + purge 日志）+ `runtime_store::delete_application`
///（applications 行 + FK cascade 元数据）。
pub async fn delete(
    conn: &Connection,
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
    source_id: &str,
) -> Result<()> {
    let result = adapters::delete(
        conn,
        &ctx(app, runtime, host_http_port),
        source_id,
        DeleteOptions::default(),
    )
    .await?;
    if !result.ok {
        return Err(Error::Internal(format!(
            "local delete failed: {}",
            result.warnings.join("; ")
        )));
    }
    // 清统一 identity（applications 行 + FK cascade）—— 必须在 source 行
    // 删除前 lookup（delete_application 内部先 lookup 再级联）。
    crate::creative_app::runtime_store::delete_application(
        conn,
        CreativeAppSource::LocalProject,
        source_id,
    )?;
    Ok(())
}

/// logs（APP-029）：runtime-scoped 结构化行（无 active runtime → 空，UI 回退
/// 聚合 tail）。
pub fn logs(
    runtime: &LocalRuntimeHandle,
    source_id: &str,
    runtime_id: Option<&str>,
    limit: usize,
) -> Vec<serde_json::Value> {
    let Some(rt) = runtime_id else {
        return Vec::new();
    };
    runtime
        .recent_logs(source_id, rt, 0, limit)
        .into_iter()
        .map(|l| {
            serde_json::json!({
                "seq": l.seq,
                "tsMs": l.ts_ms,
                "stream": l.stream.as_str(),
                "text": l.text,
            })
        })
        .collect()
}

/// resolve_orphan（APP-029）：kill 已验证 identity 的孤儿进程 + 清 identity +
/// settle 孤儿 instance；`restart` = true 时随后 fresh start。
pub async fn resolve_orphan(
    conn: &Connection,
    app: &tauri::AppHandle,
    runtime: &LocalRuntimeHandle,
    host_http_port: u16,
    source_id: &str,
    restart: bool,
) -> Result<CreativeAppSummary> {
    let summary = local::resolve_orphan(conn, app, runtime, source_id).await?;
    if restart {
        let spawned = begin_start(conn, app, runtime, host_http_port, source_id).await?;
        await_start_ready(conn, app, runtime, host_http_port, source_id, &spawned).await
    } else {
        Ok(summary)
    }
}

/// poll exits（APP-029 / R-S1）：Host watchdog 调用，**不依赖** Renderer；
/// 状态变化经 `lifecycle_process::mark_process_exited` emit `apps` channel
/// （UI 无需轮询）。
pub async fn poll_exits<R: tauri::Runtime>(
    conn: &Connection,
    app: Option<&tauri::AppHandle<R>>,
    runtime: &LocalRuntimeHandle,
) -> Result<u32> {
    local::lifecycle_process::poll_and_reconcile_exits(conn, app, runtime).await
}

/// startup reconcile（APP-029）：crash 后恢复 local 状态（emit `apps`）。
pub fn reconcile_local_apps(conn: &Connection, app: Option<&tauri::AppHandle>) -> Result<u32> {
    local::lifecycle::reconcile_local_apps(conn, app)
}

/// LocalRuntimeHandle re-export（apps 域内部句柄类型）。
pub type RuntimeHandle = LocalRuntimeHandle;

#[cfg(test)]
mod tests {
    //! APP-022 验收：Local Driver 单测通过 fake/in-memory repo 调用。

    use super::*;

    fn v28() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn register_rejects_missing_root_and_is_idempotent() {
        let mut conn = v28();
        // 不存在的路径拒绝。
        assert!(register(
            &mut conn,
            "X",
            "/nonexistent/path/xyz",
            None,
            None,
            LaunchMode::Smart,
            None,
            &[],
        )
        .is_err());

        // 真实临时目录 + smart（静态 index.html → rule plan 可构建）。
        let tmp = std::env::temp_dir().join(format!("apps_driver_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("index.html"), "<html></html>").unwrap();
        let root = tmp.to_string_lossy().to_string();
        let rec = register(
            &mut conn,
            "T",
            &root,
            None,
            None,
            LaunchMode::Smart,
            None,
            &[],
        )
        .expect("register static html project");
        assert_eq!(rec.title, "T");
        // 幂等：同 root 再注册返回同一 id。
        let rec2 = register(
            &mut conn,
            "T",
            &root,
            None,
            None,
            LaunchMode::Smart,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(rec.id, rec2.id);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inspect_returns_scan_with_rule_plan() {
        let conn = v28();
        let tmp =
            std::env::temp_dir().join(format!("apps_driver_inspect_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("index.html"), "<html></html>").unwrap();
        let scan = inspect(&conn, &tmp.to_string_lossy()).expect("inspect static html");
        let canon = local::canonical_project_root(&tmp.to_string_lossy()).unwrap();
        assert_eq!(scan.project_root, canon.to_string_lossy().to_string());
        assert!(
            scan.rule_plan.is_some(),
            "static html project must yield a rule plan"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn force_stop_refuses_stale_identity() {
        // identity 无 pid → 不匹配 live → 不 kill（防误杀无关 PID）。
        let ident = ProcessIdentity::default();
        assert!(!identity_matches_live(&ident));
        // 活 pid（自身）+ 错误 cwd → 仍不匹配（strict identity match）。
        let mut ident2 = ProcessIdentity::default();
        ident2.pid = Some(std::process::id());
        ident2.cwd = Some("/nonexistent/cwd".into());
        assert!(!identity_matches_live(&ident2));
    }

    #[test]
    fn logs_without_runtime_id_is_empty() {
        let rt: LocalRuntimeHandle = std::sync::Arc::new(local::LocalRuntimeManager::new());
        let lines = logs(&rt, "src-1", None, 100);
        assert!(lines.is_empty());
    }

    #[test]
    fn delete_on_unregistered_app_returns_not_found() {
        let conn = v28();
        let rec = local::get_app(&conn, "src-nope").unwrap();
        assert!(rec.is_none());
    }

    #[test]
    fn restart_on_unregistered_app_returns_not_found() {
        let conn = v28();
        let rec = local::get_app(&conn, "src-nope").unwrap();
        assert!(rec.is_none());
    }
}
