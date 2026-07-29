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
                    let conn = crate::db::get_assistant_db_conn()?;
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
mod tests {
    use super::*;
    use crate::jobs::dispatch::{NativeJobDispatcher, NotWiredDispatcher};
    use assistant_protocol::v2::{RunStatusV2, RunV2};
    use chrono::TimeZone;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingDispatcher {
        calls: Mutex<Vec<(Trigger, DateTime<Utc>)>>,
    }

    struct StaticRunLookup {
        result: std::result::Result<Option<RunV2>, String>,
    }

    struct EngineErrorDispatcher;

    #[async_trait::async_trait]
    impl JobDispatcher for EngineErrorDispatcher {
        fn is_wired(&self) -> bool {
            true
        }

        async fn dispatch(
            &self,
            _job: &JobDefinition,
            _trigger: Trigger,
            _scheduled_at: DateTime<Utc>,
        ) -> std::result::Result<DispatchReceipt, DispatchError> {
            Err(DispatchError::Engine("uds unavailable".to_string()))
        }
    }

    #[async_trait::async_trait]
    impl JobRunLookup for StaticRunLookup {
        async fn get_run(&self, _run_id: &str) -> std::result::Result<Option<RunV2>, String> {
            self.result.clone()
        }
    }

    #[async_trait::async_trait]
    impl JobDispatcher for RecordingDispatcher {
        fn is_wired(&self) -> bool {
            true
        }

        async fn dispatch(
            &self,
            job: &JobDefinition,
            trigger: Trigger,
            scheduled_at: DateTime<Utc>,
        ) -> std::result::Result<DispatchReceipt, DispatchError> {
            self.calls
                .lock()
                .expect("recording dispatcher lock")
                .push((trigger, scheduled_at));
            Ok(DispatchReceipt {
                conversation_id: format!("job-{}", job.id),
                run_id: format!("run-{}", job.id),
                idempotency_key: super::super::dispatch::idempotency_key(
                    &job.id,
                    scheduled_at.timestamp(),
                ),
            })
        }
    }

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

    fn insert_job_run(conn: &Connection, status: &str) {
        let job = base_job("j1", "interval", "300", "2026-07-30T00:00:00Z");
        store::insert_job(conn, &job).expect("insert job");
        store::insert_run(
            conn,
            &JobRunRecord {
                id: "job-run-row".to_string(),
                job_id: job.id,
                started_at: "2026-07-29T03:00:00Z".to_string(),
                finished_at: None,
                status: Some(status.to_string()),
                result_summary: None,
                error: None,
                run_id: Some("daemon-run".to_string()),
                conversation_id: Some("job-j1".to_string()),
                trigger: Some("schedule".to_string()),
                error_code: None,
                detail: None,
            },
        )
        .expect("insert run");
    }

    fn daemon_run(status: RunStatusV2) -> RunV2 {
        RunV2 {
            id: "daemon-run".to_string(),
            conversation_id: "job-j1".to_string(),
            status,
            parent_run_id: None,
            agent_profile_id: None,
            provider_id: "provider-a".to_string(),
            key_id: None,
            model_id: "model-a".to_string(),
            permission_profile: "readonly".to_string(),
            trigger_message_id: None,
            started_at: None,
            finished_at: None,
            error_code: None,
            step_count: 0,
            max_steps: 50,
            project_path: Some("/tmp".to_string()),
            project_id: None,
            project_identity_version: None,
            retry_count: 0,
            created_at: None,
            last_event_sequence: 0,
            idempotency_key: None,
            effort: None,
            runtime_id: Some("native".to_string()),
            revision: 0,
            capability_snapshot: None,
        }
    }

    #[test]
    fn completed_daemon_run_projects_to_succeeded_with_real_finish_time() {
        let mut run = daemon_run(RunStatusV2::Completed);
        run.finished_at = Some(utc(2026, 7, 29, 3, 4, 5));

        let projected = reconcile_run_projection(&run);

        assert_eq!(projected.status, "succeeded");
        assert_eq!(
            projected.finished_at.as_deref(),
            Some("2026-07-29T03:04:05Z")
        );
        assert_eq!(projected.error_code, None);
    }

    #[test]
    fn daemon_lifecycle_projects_to_the_job_run_state_machine() {
        let cases = [
            (RunStatusV2::Created, "dispatched"),
            (RunStatusV2::Queued, "dispatched"),
            (RunStatusV2::Preparing, "running"),
            (RunStatusV2::Running, "running"),
            (RunStatusV2::WaitingPermission, "running"),
            (RunStatusV2::WaitingSubagent, "running"),
            (RunStatusV2::Cancelling, "running"),
            (RunStatusV2::Failed, "failed"),
            (RunStatusV2::Interrupted, "failed"),
            (RunStatusV2::Cancelled, "cancelled"),
        ];

        for (status, expected) in cases {
            let projected = reconcile_run_projection(&daemon_run(status));
            assert_eq!(projected.status, expected, "daemon status={status:?}");
        }
    }

    #[test]
    fn reconciliation_updates_nonterminal_job_run_from_daemon_truth() {
        let conn = test_conn();
        insert_job_run(&conn, "dispatched");
        let mut run = daemon_run(RunStatusV2::Completed);
        run.finished_at = Some(utc(2026, 7, 29, 3, 4, 5));
        let lookup = StaticRunLookup {
            result: Ok(Some(run)),
        };

        let updated = reconcile_nonterminal_runs(&conn, &lookup).expect("reconcile");

        assert_eq!(updated, 1);
        let (runs, _) = store::list_runs(&conn, Some("j1"), 10, 0).expect("runs");
        assert_eq!(runs[0].status.as_deref(), Some("succeeded"));
        assert_eq!(runs[0].finished_at.as_deref(), Some("2026-07-29T03:04:05Z"));
    }

    #[test]
    fn reconciliation_keeps_local_state_when_daemon_is_temporarily_unreachable() {
        let conn = test_conn();
        insert_job_run(&conn, "dispatched");
        let lookup = StaticRunLookup {
            result: Err("uds unavailable".to_string()),
        };

        let updated = reconcile_nonterminal_runs(&conn, &lookup).expect("best-effort reconcile");

        assert_eq!(updated, 0);
        let (runs, _) = store::list_runs(&conn, Some("j1"), 10, 0).expect("runs");
        assert_eq!(runs[0].status.as_deref(), Some("dispatched"));
        assert_eq!(runs[0].finished_at, None);
        assert_eq!(runs[0].error_code, None);
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

    #[test]
    fn scheduled_dispatch_uses_the_planned_due_time_not_the_late_tick_time() {
        let conn = test_conn();
        let job = base_job("j5", "interval", "300", "2026-07-26T11:00:00Z");
        store::insert_job(&conn, &job).expect("insert");
        let dispatcher = RecordingDispatcher::default();
        let tick_time = utc(2026, 7, 26, 12, 0, 0);

        let report = tick_once(&conn, &dispatcher, tick_time).expect("tick");

        assert_eq!(report.dispatched, 1);
        let calls = dispatcher.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, Trigger::Schedule);
        assert_eq!(calls[0].1, utc(2026, 7, 26, 11, 0, 0));
    }

    #[test]
    fn transient_engine_error_keeps_the_original_due_time_for_idempotent_retry() {
        let conn = test_conn();
        let job = base_job("j6", "interval", "300", "2026-07-26T11:00:00Z");
        store::insert_job(&conn, &job).expect("insert");

        let report =
            tick_once(&conn, &EngineErrorDispatcher, utc(2026, 7, 26, 12, 0, 0)).expect("tick");

        assert_eq!(report.dispatch_errors, 1);
        let stored = store::get_job(&conn, "j6").expect("get").expect("job");
        assert_eq!(
            stored.next_run, "2026-07-26T11:00:00Z",
            "the next tick must retry with the same scheduled_at/idempotency key"
        );
        assert!(stored.enabled);
    }

    #[test]
    fn engine_dispatch_failure_uses_a_structured_job_error_code() {
        let conn = test_conn();
        let job = base_job("j-engine", "interval", "300", "2026-07-27T00:00:00Z");
        store::insert_job(&conn, &job).expect("insert");

        let error = run_job_manual(
            &conn,
            &EngineErrorDispatcher,
            &job,
            utc(2026, 7, 26, 12, 0, 0),
        )
        .expect_err("engine failure");

        assert!(error.to_string().starts_with("JOB_ENGINE_ERROR"));
        let (runs, _) = store::list_runs(&conn, Some("j-engine"), 10, 0).expect("runs");
        assert_eq!(runs[0].error_code.as_deref(), Some("JOB_ENGINE_ERROR"));
    }

    #[test]
    fn invalid_job_dispatch_preserves_the_specific_structured_error_code() {
        let conn = test_conn();
        let job = base_job("j7", "interval", "300", "2026-07-27T00:00:00Z");
        store::insert_job(&conn, &job).expect("insert");

        let error = run_job_manual(
            &conn,
            &NativeJobDispatcher::new(),
            &job,
            utc(2026, 7, 26, 12, 0, 0),
        )
        .expect_err("provider is missing");

        assert!(error.to_string().starts_with("JOB_INVALID_PROVIDER"));
        let (runs, _) = store::list_runs(&conn, Some("j7"), 10, 0).expect("runs");
        assert_eq!(runs[0].error_code.as_deref(), Some("JOB_INVALID_PROVIDER"));
    }
}
