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

fn base_job(id: &str, schedule_type: &str, schedule_value: &str, next_run: &str) -> JobDefinition {
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
        retry_of_run_id: None,
        retry_of_turn_id: None,
        continued_from_run_id: None,
        branch_id: None,
        branch_parent_message_id: None,
        checkpoint_id: None,
        resume_of_run_id: None,
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
    let err =
        run_job_manual(&conn, &NotWiredDispatcher, &job, now).expect_err("not wired must error");
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
