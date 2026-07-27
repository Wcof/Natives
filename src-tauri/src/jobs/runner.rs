//! jobs/runner.rs — 30s tick 循环与到期处理（契约第 4 节）
//!
//! tick_once 是可注入时钟的纯逻辑 + store 调用，在 spawn_blocking 线程内
//! 执行（R-B6：阻塞 SQLite 不占 async worker）。未接线（NotWired）语义：
//! 不写 run 行，仅推进 next_run；once 任务一次性写一条 skipped run 行。

use super::dispatch::{DispatchError, DispatchReceipt, JobDispatcher, Trigger};
use super::schedule;
use super::store::{self, JobDefinition, JobRunRecord};
use crate::{Error, Result};
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

        // wired（P1 链路）：写 pending run 行 → dispatch → dispatched | dispatch_error
        match dispatch_due_job(conn, dispatcher, &job, &spec, now) {
            Ok(_) => report.dispatched += 1,
            Err(_) => report.dispatch_errors += 1,
        }
    }
    Ok(report)
}

/// 到期任务的 wired 派发（P1 链路；P0 无 wired 适配器，不会走到这里）。
fn dispatch_due_job(
    conn: &Connection,
    dispatcher: &dyn JobDispatcher,
    job: &JobDefinition,
    spec: &schedule::ScheduleSpec,
    now: DateTime<Utc>,
) -> Result<DispatchReceipt> {
    let now_iso = schedule::format_utc(&now);
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

    match block_on_dispatch(dispatcher, job, Trigger::Schedule) {
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
            store::update_run_result(
                conn,
                &row_id,
                "dispatch_error",
                None,
                None,
                Some(code),
                Some(&err.to_string()),
                Some(&now_iso),
            )?;
            store::mark_dispatch_attempt(conn, &job.id, status, &now_iso, next_iso.as_deref())?;
            Err(Error::Message(code.to_string()))
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
    match block_on_dispatch(dispatcher, job, Trigger::Manual) {
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
                Some(code),
                Some(&err.to_string()),
                Some(&now_iso),
            )?;
            store::mark_dispatch_attempt(conn, &job.id, status, &now_iso, Some(&job.next_run))?;
            Err(Error::Message(code.to_string()))
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

fn dispatch_error_meta(err: &DispatchError) -> (&'static str, &'static str) {
    match err {
        DispatchError::NotWired => (NOT_WIRED_CODE, NOT_WIRED_STATUS),
        DispatchError::InvalidJob(_) => ("invalid_job", "dispatch_error:invalid_job"),
        DispatchError::Engine(_) => ("engine", "dispatch_error:engine"),
    }
}

/// 在阻塞线程上驱动 async dispatch。tick 循环 / 命令体都运行在 spawn_blocking
/// 线程（有 runtime 上下文，Handle::block_on 合法）；单测无 runtime 时临时建
/// current_thread runtime。
fn block_on_dispatch(
    dispatcher: &dyn JobDispatcher,
    job: &JobDefinition,
    trigger: Trigger,
) -> std::result::Result<DispatchReceipt, DispatchError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(dispatcher.dispatch(job, trigger)),
        Err(_) => match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(dispatcher.dispatch(job, trigger)),
            Err(e) => Err(DispatchError::Engine(format!("no async runtime: {e}"))),
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
                    let conn = crate::db::get_assistant_db_conn()?;
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
mod tests {
    use super::*;
    use crate::jobs::dispatch::NotWiredDispatcher;
    use chrono::TimeZone;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma");
        store::ensure_schema(&conn).expect("ensure schema");
        conn
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s)
            .single()
            .expect("valid utc")
    }

    fn base_job(
        id: &str,
        schedule_type: &str,
        schedule_value: &str,
        next_run: &str,
    ) -> JobDefinition {
        JobDefinition {
            id: id.to_string(),
            name: format!("job {id}"),
            prompt: "do the thing".to_string(),
            description: None,
            project_path: Some("/tmp".to_string()),
            provider_id: None,
            model_id: None,
            key_id: None,
            agent_profile_id: None,
            capability_refs: "[]".to_string(),
            permission_profile: "readonly".to_string(),
            max_steps: None,
            effort: None,
            runtime_id: None,
            schedule_type: schedule_type.to_string(),
            schedule_value: schedule_value.to_string(),
            next_run: next_run.to_string(),
            last_status: None,
            last_run_at: None,
            consecutive_errors: 0,
            enabled: true,
            created_at: "2026-07-01T00:00:00Z".to_string(),
            expires_at: None,
        }
    }

    fn run_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM task_runs", [], |r| r.get(0))
            .expect("count runs")
    }

    #[test]
    fn not_wired_interval_advances_next_run_without_run_row() {
        let conn = test_conn();
        let job = base_job("j1", "interval", "300", "2026-07-26T11:00:00Z");
        store::insert_job(&conn, &job).expect("insert");

        let now = utc(2026, 7, 26, 12, 0, 0);
        let report = tick_once(&conn, &NotWiredDispatcher, now).expect("tick");

        assert_eq!(report.deferred_not_wired, 1);
        assert_eq!(report.skipped_once, 0);
        assert_eq!(
            run_count(&conn),
            0,
            "not-wired interval must not write run rows"
        );

        let stored = store::get_job(&conn, "j1").expect("get").expect("exists");
        assert_eq!(
            stored.last_status.as_deref(),
            Some("dispatch_error:not_wired")
        );
        assert_eq!(stored.next_run, "2026-07-26T12:05:00Z"); // now + 300s
        assert!(stored.enabled);
        assert!(stored.last_run_at.is_none(), "no run happened");

        // 推进后不再到期：下一次 tick 是空转
        let report2 = tick_once(&conn, &NotWiredDispatcher, now).expect("tick2");
        assert_eq!(report2, TickReport::default());
    }

    #[test]
    fn not_wired_once_writes_single_skipped_run_and_disables() {
        let conn = test_conn();
        let job = base_job("j2", "once", "2026-07-26T11:30:00Z", "2026-07-26T11:30:00Z");
        store::insert_job(&conn, &job).expect("insert");

        let now = utc(2026, 7, 26, 12, 0, 0);
        let report = tick_once(&conn, &NotWiredDispatcher, now).expect("tick");

        assert_eq!(report.skipped_once, 1);
        assert_eq!(run_count(&conn), 1, "once miss writes exactly one run row");

        let (runs, total) = store::list_runs(&conn, Some("j2"), 10, 0).expect("runs");
        assert_eq!(total, 1);
        assert_eq!(runs[0].status.as_deref(), Some("skipped"));
        assert_eq!(runs[0].trigger.as_deref(), Some("schedule"));
        assert_eq!(
            runs[0].error_code.as_deref(),
            Some("JOB_DISPATCHER_NOT_WIRED")
        );
        assert!(
            runs[0].run_id.is_none(),
            "never dispatched, no engine run id"
        );

        let stored = store::get_job(&conn, "j2").expect("get").expect("exists");
        assert!(!stored.enabled, "consumed once job must be disabled");
        assert_eq!(
            stored.last_status.as_deref(),
            Some("dispatch_error:not_wired")
        );
        assert_eq!(stored.last_run_at.as_deref(), Some("2026-07-26T12:00:00Z"));

        // 幂等：再 tick 不追加 run 行
        let report2 = tick_once(&conn, &NotWiredDispatcher, now).expect("tick2");
        assert_eq!(report2, TickReport::default());
        assert_eq!(run_count(&conn), 1);
    }

    #[test]
    fn expired_job_disabled_without_run_row() {
        let conn = test_conn();
        let mut job = base_job("j3", "interval", "3600", "2026-07-26T11:00:00Z");
        job.expires_at = Some("2026-07-26T11:59:00Z".to_string());
        store::insert_job(&conn, &job).expect("insert");

        let now = utc(2026, 7, 26, 12, 0, 0);
        let report = tick_once(&conn, &NotWiredDispatcher, now).expect("tick");

        assert_eq!(report.expired, 1);
        assert_eq!(
            report.deferred_not_wired, 0,
            "expired job must not be re-scheduled"
        );
        assert_eq!(run_count(&conn), 0);

        let stored = store::get_job(&conn, "j3").expect("get").expect("exists");
        assert!(!stored.enabled);
        assert_eq!(stored.last_status.as_deref(), Some("expired"));
    }

    #[test]
    fn manual_run_not_wired_returns_error_without_run_row() {
        let conn = test_conn();
        let job = base_job("j4", "interval", "300", "2026-07-27T00:00:00Z");
        store::insert_job(&conn, &job).expect("insert");

        let now = utc(2026, 7, 26, 12, 0, 0);
        let err = run_job_manual(&conn, &NotWiredDispatcher, &job, now)
            .expect_err("not wired must error");
        assert!(err.to_string().starts_with("JOB_DISPATCHER_NOT_WIRED"));
        assert_eq!(run_count(&conn), 0);
    }
}
