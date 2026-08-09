//! jobs/runner.rs — 30s tick 循环与到期处理（契约第 4 节）
//!
//! tick_once 是可注入时钟的纯逻辑 + store 调用，在 spawn_blocking 线程内
//! 执行（R-B6：阻塞 SQLite 不占 async worker）。生产使用 NativeJobDispatcher；
//! NotWired 语义仅用于 fail-closed：不写伪 run，once 记录 skipped。

use super::dispatch::{DispatchError, DispatchReceipt, JobDispatcher, Trigger};
use super::schedule;
use super::store::{self, JobDefinition, JobRunRecord};
use crate::{Error, Result};
use assistant_protocol::v2::{RunStatusV2, RunV2};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::path::Path;

const NOT_WIRED_STATUS: &str = "dispatch_error:not_wired";
const NOT_WIRED_CODE: &str = "JOB_DISPATCHER_NOT_WIRED";

/// 单次 tick 的处理统计（可观测、可断言）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    pub expired: usize,
    /// 未接线且非 once：仅推进 next_run，未写 run 行。
    pub deferred_not_wired: usize,
    /// 未接线的 once 任务：写一条 skipped run 行并停用。
    pub skipped_once: usize,
    pub dispatched: usize,
    pub dispatch_errors: usize,
    /// 存量行 schedule 解析失败，已停用。
    pub invalid_schedule: usize,
}

fn new_run_row_id() -> String {
    format!("run_{}", uuid::Uuid::new_v4())
}

#[derive(Debug, PartialEq, Eq)]
struct ReconciledRunProjection {
    status: &'static str,
    finished_at: Option<String>,
    error_code: Option<String>,
}

fn reconcile_run_projection(run: &RunV2) -> ReconciledRunProjection {
    let status = match run.status {
        RunStatusV2::Created | RunStatusV2::Queued => "dispatched",
        RunStatusV2::Preparing
        | RunStatusV2::Running
        | RunStatusV2::WaitingPermission
        | RunStatusV2::WaitingSubagent
        | RunStatusV2::Cancelling => "running",
        RunStatusV2::Completed => "succeeded",
        RunStatusV2::Failed | RunStatusV2::Interrupted => "failed",
        RunStatusV2::Cancelled => "cancelled",
    };
    ReconciledRunProjection {
        status,
        finished_at: run.finished_at.as_ref().map(schedule::format_utc),
        error_code: run.error_code.clone(),
    }
}

#[async_trait::async_trait]
trait JobRunLookup: Send + Sync {
    async fn get_run(&self, run_id: &str) -> std::result::Result<Option<RunV2>, String>;
}

struct DaemonRunLookup;

#[async_trait::async_trait]
impl JobRunLookup for DaemonRunLookup {
    async fn get_run(&self, run_id: &str) -> std::result::Result<Option<RunV2>, String> {
        crate::daemon_authority::get_run(run_id).await
    }
}

fn reconcile_nonterminal_runs(conn: &Connection, lookup: &dyn JobRunLookup) -> Result<usize> {
    let mut updated = 0;
    for row in store::list_reconcilable_runs(conn)? {
        let Some(run_id) = row.run_id.as_deref() else {
            continue;
        };
        let run = match block_on_run_lookup(lookup, run_id) {
            Ok(Some(run)) => run,
            Ok(None) => continue,
            Err(_) => continue,
        };
        let projected = reconcile_run_projection(&run);
        store::update_run_result(
            conn,
            &row.id,
            projected.status,
            row.run_id.as_deref(),
            row.conversation_id.as_deref(),
            projected.error_code.as_deref(),
            row.detail.as_deref(),
            projected.finished_at.as_deref(),
        )?;
        updated += 1;
    }
    Ok(updated)
}

/// 单次 tick：过期回收 → 扫描到期任务 → 未接线推迟 / 派发。
/// `now` 由调用方注入（生产为 Utc::now()，单测为固定时钟）。
pub fn tick_once(
    conn: &Connection,
    dispatcher: &dyn JobDispatcher,
    now: DateTime<Utc>,
) -> Result<TickReport> {
    let mut report = TickReport::default();
    let now_iso = schedule::format_utc(&now);

    // 契约第 4 节第 4 条：过期任务置 enabled=0、last_status='expired'
    report.expired = store::expire_overdue(conn, &now_iso)?;

    for job in store::list_due(conn, &now_iso)? {
        let spec = match schedule::parse(&job.schedule_type, &job.schedule_value) {
            Ok(spec) => spec,
            Err(_) => {
                // 存量坏行：停用，避免每 tick 重复扫描
                store::disable_with_status(conn, &job.id, "failed:JOB_INVALID_SCHEDULE", None)?;
                report.invalid_schedule += 1;
                continue;
            }
        };

        if !dispatcher.is_wired() {
            if job.schedule_type == "once" {
                // once 错过窗口：一次性写一条 skipped run 行并停用
                store::insert_run(
                    conn,
                    &JobRunRecord {
                        id: new_run_row_id(),
                        job_id: job.id.clone(),
                        started_at: now_iso.clone(),
                        finished_at: Some(now_iso.clone()),
                        status: Some("skipped".to_string()),
                        result_summary: None,
                        error: None,
                        run_id: None,
                        conversation_id: None,
                        trigger: Some(Trigger::Schedule.as_str().to_string()),
                        error_code: Some(NOT_WIRED_CODE.to_string()),
                        detail: Some(
                            "dispatcher not wired; once job missed its window".to_string(),
                        ),
                    },
                )?;
                store::disable_with_status(conn, &job.id, NOT_WIRED_STATUS, Some(&now_iso))?;
                report.skipped_once += 1;
            } else {
                // interval / cron：不写 run 行，仅记录状态并推进 next_run
                match schedule::next_run(&spec, now) {
                    Some(next) => store::mark_deferred(
                        conn,
                        &job.id,
                        NOT_WIRED_STATUS,
                        &schedule::format_utc(&next),
                    )?,
                    // cron 表达式在可见窗口内不可满足：停用，避免死循环扫描
                    None => store::disable_with_status(conn, &job.id, NOT_WIRED_STATUS, None)?,
                }
                report.deferred_not_wired += 1;
            }
            continue;
        }

        // wired 链路：写 pending run 行 → dispatch → dispatched | dispatch_error
        match dispatch_due_job(conn, dispatcher, &job, &spec, now) {
            Ok(_) => report.dispatched += 1,
            Err(_) => report.dispatch_errors += 1,
        }
    }
    Ok(report)
}

/// 到期任务的 wired 派发。
fn dispatch_due_job(
    conn: &Connection,
    dispatcher: &dyn JobDispatcher,
    job: &JobDefinition,
    spec: &schedule::ScheduleSpec,
    now: DateTime<Utc>,
) -> Result<DispatchReceipt> {
    let now_iso = schedule::format_utc(&now);
    let scheduled_at = schedule::parse_utc(&job.next_run)
        .ok_or_else(|| Error::Message("JOB_INVALID_SCHEDULE: invalid next_run".to_string()))?;
    let next_iso = schedule::next_run(spec, now).map(|t| schedule::format_utc(&t));
    let row_id = new_run_row_id();
    store::insert_run(
        conn,
        &JobRunRecord {
            id: row_id.clone(),
            job_id: job.id.clone(),
            started_at: now_iso.clone(),
            finished_at: None,
            status: Some("pending".to_string()),
            result_summary: None,
            error: None,
            run_id: None,
            conversation_id: None,
            trigger: Some(Trigger::Schedule.as_str().to_string()),
            error_code: None,
            detail: None,
        },
    )?;

    // project_path 派发时校验（契约第 1 节：老行可空，禁止 cwd 兜底）
    if !project_path_valid(job) {
        store::update_run_result(
            conn,
            &row_id,
            "dispatch_error",
            None,
            None,
            Some("JOB_INVALID_PROJECT_PATH"),
            Some("project_path missing or not an existing absolute directory"),
            Some(&now_iso),
        )?;
        store::mark_dispatch_attempt(
            conn,
            &job.id,
            "dispatch_error:invalid_project_path",
            &now_iso,
            next_iso.as_deref(),
        )?;
        return Err(Error::Message("JOB_INVALID_PROJECT_PATH".to_string()));
    }

    match block_on_dispatch(dispatcher, job, Trigger::Schedule, scheduled_at) {
        Ok(receipt) => {
            store::update_run_result(
                conn,
                &row_id,
                "dispatched",
                Some(&receipt.run_id),
                Some(&receipt.conversation_id),
                None,
                None,
                None,
            )?;
            store::mark_dispatch_attempt(
                conn,
                &job.id,
                "dispatched",
                &now_iso,
                next_iso.as_deref(),
            )?;
            Ok(receipt)
        }
        Err(err) => {
            let (code, status) = dispatch_error_meta(&err);
            let retry_next = if matches!(&err, DispatchError::Engine(_)) {
                Some(job.next_run.as_str())
            } else {
                next_iso.as_deref()
            };
            store::update_run_result(
                conn,
                &row_id,
                "dispatch_error",
                None,
                None,
                Some(&code),
                Some(&err.to_string()),
                Some(&now_iso),
            )?;
            store::mark_dispatch_attempt(conn, &job.id, &status, &now_iso, retry_next)?;
            Err(Error::Message(code))
        }
    }
}

/// 手动触发（job_run_now，契约第 5 节）。未接线直接返回 JOB_DISPATCHER_NOT_WIRED，
/// 不写 run 行（避免把用户的探测点击刷进历史）。
pub fn run_job_manual(
    conn: &Connection,
    dispatcher: &dyn JobDispatcher,
    job: &JobDefinition,
    now: DateTime<Utc>,
) -> Result<DispatchReceipt> {
    if !dispatcher.is_wired() {
        return Err(Error::Message(NOT_WIRED_CODE.to_string()));
    }
    if !project_path_valid(job) {
        return Err(Error::Message("JOB_INVALID_PROJECT_PATH".to_string()));
    }
    let now_iso = schedule::format_utc(&now);
    let row_id = new_run_row_id();
    store::insert_run(
        conn,
        &JobRunRecord {
            id: row_id.clone(),
            job_id: job.id.clone(),
            started_at: now_iso.clone(),
            finished_at: None,
            status: Some("pending".to_string()),
            result_summary: None,
            error: None,
            run_id: None,
            conversation_id: None,
            trigger: Some(Trigger::Manual.as_str().to_string()),
            error_code: None,
            detail: None,
        },
    )?;
    match block_on_dispatch(dispatcher, job, Trigger::Manual, now) {
        Ok(receipt) => {
            store::update_run_result(
                conn,
                &row_id,
                "dispatched",
                Some(&receipt.run_id),
                Some(&receipt.conversation_id),
                None,
                None,
                None,
            )?;
            // 手动触发不推进 next_run，只刷新最近状态
            store::mark_dispatch_attempt(
                conn,
                &job.id,
                "dispatched",
                &now_iso,
                Some(&job.next_run),
            )?;
            Ok(receipt)
        }
        Err(err) => {
            let (code, status) = dispatch_error_meta(&err);
            store::update_run_result(
                conn,
                &row_id,
                "dispatch_error",
                None,
                None,
                Some(&code),
                Some(&err.to_string()),
                Some(&now_iso),
            )?;
            store::mark_dispatch_attempt(conn, &job.id, &status, &now_iso, Some(&job.next_run))?;
            Err(Error::Message(code))
        }
    }
}

fn project_path_valid(job: &JobDefinition) -> bool {
    match job.project_path.as_deref() {
        Some(p) if !p.is_empty() => {
            let path = Path::new(p);
            path.is_absolute() && path.is_dir()
        }
        _ => false,
    }
}

fn dispatch_error_meta(err: &DispatchError) -> (String, String) {
    match err {
        DispatchError::NotWired => (NOT_WIRED_CODE.to_string(), NOT_WIRED_STATUS.to_string()),
        DispatchError::InvalidJob(message) => {
            let code = message
                .split(':')
                .next()
                .filter(|value| value.starts_with("JOB_"))
                .unwrap_or("JOB_INVALID_JOB")
                .to_string();
            (
                code.clone(),
                format!("dispatch_error:{}", code.to_ascii_lowercase()),
            )
        }
        DispatchError::Engine(_) => (
            "JOB_ENGINE_ERROR".to_string(),
            "dispatch_error:job_engine_error".to_string(),
        ),
    }
}

/// 在阻塞线程上驱动 async dispatch。tick 循环 / 命令体都运行在 spawn_blocking
/// 线程（有 runtime 上下文，Handle::block_on 合法）；单测无 runtime 时临时建
/// current_thread runtime。
fn block_on_dispatch(
    dispatcher: &dyn JobDispatcher,
    job: &JobDefinition,
    trigger: Trigger,
    scheduled_at: DateTime<Utc>,
) -> std::result::Result<DispatchReceipt, DispatchError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(dispatcher.dispatch(job, trigger, scheduled_at)),
        Err(_) => match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(dispatcher.dispatch(job, trigger, scheduled_at)),
            Err(e) => Err(DispatchError::Engine(format!("no async runtime: {e}"))),
        },
    }
}

fn block_on_run_lookup(
    lookup: &dyn JobRunLookup,
    run_id: &str,
) -> std::result::Result<Option<RunV2>, String> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(lookup.get_run(run_id)),
        Err(_) => match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(lookup.get_run(run_id)),
            Err(error) => Err(format!("no async runtime: {error}")),
        },
    }
}

/// lib.rs setup 拉起的常驻 tick 循环（Once 幂等，30s 间隔）。
pub fn start() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let joined = tokio::task::spawn_blocking(|| -> Result<TickReport> {
                    let conn = crate::db::get_main_conn()?;
                    let _ = reconcile_nonterminal_runs(&conn, &DaemonRunLookup)?;
                    let dispatcher = super::global_dispatcher();
                    tick_once(&conn, dispatcher.as_ref(), Utc::now())
                })
                .await;
                match joined {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => eprintln!(
                        "[jobs] tick failed: {}",
                        crate::log_sanitizer::sanitize(&e.to_string())
                    ),
                    Err(e) => eprintln!("[jobs] tick join failed: {e}"),
                }
            }
        });
    });
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;
