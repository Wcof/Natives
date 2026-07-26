//! commands/jobs.rs — 任务模块（Job Module）IPC 命令面（契约第 5 节，R-B5 归域）
//!
//! 全部返回 error.rs 的 Result（R-B1），JSON snake_case。
//! 错误码（前端可判定，均为返回串前缀）：
//! JOB_NOT_FOUND / JOB_INVALID_SCHEDULE / JOB_INVALID_PROJECT_PATH / JOB_DISPATCHER_NOT_WIRED。
//! 阻塞 SQLite 操作统一经 spawn_blocking 卸载（R-B6）。

use crate::jobs::dispatch::DispatchReceipt;
use crate::jobs::{global_dispatcher, runner, schedule, store};
use crate::{Error, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::json;

const PERMISSION_PROFILES: &[&str] = &["readonly", "ask", "full_access"];
const RECENT_RUNS_LIMIT: i64 = 10;
const DEFAULT_RUNS_PAGE: i64 = 50;
const MAX_RUNS_PAGE: i64 = 200;

// ── 出参类型（JSON snake_case，契约第 5 节） ──

#[derive(Debug, Serialize)]
pub struct JobSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub schedule_type: String,
    pub schedule_value: String,
    pub project_path: Option<String>,
    pub next_run: String,
    pub last_status: Option<String>,
    pub last_run_at: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobListResponse {
    pub jobs: Vec<JobSummary>,
    pub dispatcher_wired: bool,
}

#[derive(Debug, Serialize)]
pub struct JobDetail {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub description: Option<String>,
    pub project_path: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub key_id: Option<String>,
    pub agent_profile_id: Option<String>,
    pub capability_refs: Vec<String>,
    pub permission_profile: String,
    pub max_steps: Option<i64>,
    pub effort: Option<String>,
    pub runtime_id: Option<String>,
    pub schedule_type: String,
    pub schedule_value: String,
    pub next_run: String,
    pub last_status: Option<String>,
    pub last_run_at: Option<String>,
    pub consecutive_errors: i64,
    pub enabled: bool,
    pub created_at: String,
    pub expires_at: Option<String>,
    /// 最近 run 摘要（started_at 倒序，最多 10 条）。
    pub recent_runs: Vec<store::JobRunRecord>,
}

#[derive(Debug, Serialize)]
pub struct JobRunsResponse {
    pub runs: Vec<store::JobRunRecord>,
    pub total: i64,
}

// ── 校验辅助 ──

fn not_found(id: &str) -> Error {
    Error::Message(format!("JOB_NOT_FOUND: {id}"))
}

fn invalid_schedule(detail: &str) -> Error {
    Error::Message(format!("JOB_INVALID_SCHEDULE: {detail}"))
}

fn invalid_project_path(path: &str) -> Error {
    Error::Message(format!(
        "JOB_INVALID_PROJECT_PATH: '{path}' is not an existing absolute directory"
    ))
}

/// project_path 必须为存在的绝对路径目录（引擎硬性要求，禁止 cwd 兜底）。
fn validate_project_path(path: &str) -> Result<()> {
    let p = std::path::Path::new(path);
    if path.is_empty() || !p.is_absolute() || !p.is_dir() {
        return Err(invalid_project_path(path));
    }
    Ok(())
}

fn validate_permission_profile(value: &str) -> Result<()> {
    if PERMISSION_PROFILES.contains(&value) {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "permission_profile must be one of readonly|ask|full_access, got '{value}'"
        )))
    }
}

/// 校验 schedule 并计算首个 next_run（从 now 起算）。不可满足（如 once 已过期）
/// 一律 JOB_INVALID_SCHEDULE。
fn compute_next_run(schedule_type: &str, schedule_value: &str) -> Result<String> {
    let spec = schedule::parse(schedule_type, schedule_value).map_err(|e| invalid_schedule(&e))?;
    let next = schedule::next_run(&spec, Utc::now())
        .ok_or_else(|| invalid_schedule("schedule can never fire again (once in the past?)"))?;
    Ok(schedule::format_utc(&next))
}

fn validate_expires_at(value: &str) -> Result<()> {
    schedule::parse_utc(value)
        .map(|_| ())
        .ok_or_else(|| Error::InvalidInput(format!("expires_at is not ISO8601: '{value}'")))
}

fn summary_from(job: &store::JobDefinition) -> JobSummary {
    JobSummary {
        id: job.id.clone(),
        name: job.name.clone(),
        description: job.description.clone(),
        schedule_type: job.schedule_type.clone(),
        schedule_value: job.schedule_value.clone(),
        project_path: job.project_path.clone(),
        next_run: job.next_run.clone(),
        last_status: job.last_status.clone(),
        last_run_at: job.last_run_at.clone(),
        enabled: job.enabled,
        created_at: job.created_at.clone(),
        expires_at: job.expires_at.clone(),
    }
}

fn detail_from(conn: &rusqlite::Connection, job: store::JobDefinition) -> Result<JobDetail> {
    let (recent_runs, _total) = store::list_runs(conn, Some(&job.id), RECENT_RUNS_LIMIT, 0)?;
    let capability_refs: Vec<String> =
        serde_json::from_str(&job.capability_refs).unwrap_or_default();
    Ok(JobDetail {
        id: job.id,
        name: job.name,
        prompt: job.prompt,
        description: job.description,
        project_path: job.project_path,
        provider_id: job.provider_id,
        model_id: job.model_id,
        key_id: job.key_id,
        agent_profile_id: job.agent_profile_id,
        capability_refs,
        permission_profile: job.permission_profile,
        max_steps: job.max_steps,
        effort: job.effort,
        runtime_id: job.runtime_id,
        schedule_type: job.schedule_type,
        schedule_value: job.schedule_value,
        next_run: job.next_run,
        last_status: job.last_status,
        last_run_at: job.last_run_at,
        consecutive_errors: job.consecutive_errors,
        enabled: job.enabled,
        created_at: job.created_at,
        expires_at: job.expires_at,
        recent_runs,
    })
}

/// R-B6：阻塞 SQLite 工作统一卸载到 blocking 线程。
async fn with_conn<T, F>(work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = crate::db::get_assistant_db_conn()?;
        work(&conn)
    })
    .await
    .map_err(|e| Error::Internal(format!("jobs blocking task join: {e}")))?
}

// ── 命令（契约第 5 节的 8 个） ──

#[tauri::command(rename_all = "snake_case")]
pub async fn job_list() -> Result<JobListResponse> {
    with_conn(|conn| {
        let jobs = store::list_jobs(conn)?.iter().map(summary_from).collect();
        Ok(JobListResponse {
            jobs,
            dispatcher_wired: global_dispatcher().is_wired(),
        })
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn job_get(id: String) -> Result<JobDetail> {
    with_conn(move |conn| {
        let job = store::get_job(conn, &id)?.ok_or_else(|| not_found(&id))?;
        detail_from(conn, job)
    })
    .await
}

/// 创建任务。name/schedule_type/schedule_value/project_path/prompt 必填，其余可选。
#[allow(clippy::too_many_arguments)] // 入参形状由契约第 5 节冻结（invoke 键即字段名）
#[tauri::command(rename_all = "snake_case")]
pub async fn job_create(
    name: String,
    schedule_type: String,
    schedule_value: String,
    project_path: String,
    prompt: String,
    description: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    key_id: Option<String>,
    agent_profile_id: Option<String>,
    capability_refs: Option<Vec<String>>,
    permission_profile: Option<String>,
    max_steps: Option<i64>,
    effort: Option<String>,
    runtime_id: Option<String>,
    expires_at: Option<String>,
) -> Result<JobDetail> {
    with_conn(move |conn| {
        if name.trim().is_empty() {
            return Err(Error::InvalidInput("name must not be empty".to_string()));
        }
        if prompt.trim().is_empty() {
            return Err(Error::InvalidInput("prompt must not be empty".to_string()));
        }
        validate_project_path(&project_path)?;
        let next_run = compute_next_run(&schedule_type, &schedule_value)?;
        let permission_profile = permission_profile.unwrap_or_else(|| "readonly".to_string());
        validate_permission_profile(&permission_profile)?;
        if let Some(exp) = expires_at.as_deref() {
            validate_expires_at(exp)?;
        }
        let capability_refs =
            serde_json::to_string(&capability_refs.unwrap_or_default()).map_err(Error::Json)?;

        let now_iso = schedule::format_utc(&Utc::now());
        let job = store::JobDefinition {
            id: format!("job_{}", uuid::Uuid::new_v4()),
            name,
            prompt,
            description,
            project_path: Some(project_path),
            provider_id,
            model_id,
            key_id,
            agent_profile_id,
            capability_refs,
            permission_profile,
            max_steps,
            effort,
            runtime_id,
            schedule_type,
            schedule_value,
            next_run,
            last_status: None,
            last_run_at: None,
            consecutive_errors: 0,
            enabled: true,
            created_at: now_iso,
            expires_at,
        };
        store::insert_job(conn, &job)?;
        detail_from(conn, job)
    })
    .await
}

/// 部分更新。缺省字段保持不变；schedule 字段变更后按合并值重算 next_run。
#[allow(clippy::too_many_arguments)] // 入参形状由契约第 5 节冻结（invoke 键即字段名）
#[tauri::command(rename_all = "snake_case")]
pub async fn job_update(
    id: String,
    name: Option<String>,
    schedule_type: Option<String>,
    schedule_value: Option<String>,
    project_path: Option<String>,
    prompt: Option<String>,
    description: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    key_id: Option<String>,
    agent_profile_id: Option<String>,
    capability_refs: Option<Vec<String>>,
    permission_profile: Option<String>,
    max_steps: Option<i64>,
    effort: Option<String>,
    runtime_id: Option<String>,
    expires_at: Option<String>,
) -> Result<JobDetail> {
    with_conn(move |conn| {
        let mut job = store::get_job(conn, &id)?.ok_or_else(|| not_found(&id))?;

        if let Some(v) = name {
            if v.trim().is_empty() {
                return Err(Error::InvalidInput("name must not be empty".to_string()));
            }
            job.name = v;
        }
        if let Some(v) = prompt {
            if v.trim().is_empty() {
                return Err(Error::InvalidInput("prompt must not be empty".to_string()));
            }
            job.prompt = v;
        }
        if let Some(v) = project_path {
            validate_project_path(&v)?;
            job.project_path = Some(v);
        }
        if let Some(v) = description {
            job.description = Some(v);
        }
        if let Some(v) = provider_id {
            job.provider_id = Some(v);
        }
        if let Some(v) = model_id {
            job.model_id = Some(v);
        }
        if let Some(v) = key_id {
            job.key_id = Some(v);
        }
        if let Some(v) = agent_profile_id {
            job.agent_profile_id = Some(v);
        }
        if let Some(v) = capability_refs {
            job.capability_refs = serde_json::to_string(&v).map_err(Error::Json)?;
        }
        if let Some(v) = permission_profile {
            validate_permission_profile(&v)?;
            job.permission_profile = v;
        }
        if let Some(v) = max_steps {
            job.max_steps = Some(v);
        }
        if let Some(v) = effort {
            job.effort = Some(v);
        }
        if let Some(v) = runtime_id {
            job.runtime_id = Some(v);
        }
        if let Some(v) = expires_at {
            validate_expires_at(&v)?;
            job.expires_at = Some(v);
        }

        let schedule_changed = schedule_type.is_some() || schedule_value.is_some();
        if let Some(v) = schedule_type {
            job.schedule_type = v;
        }
        if let Some(v) = schedule_value {
            job.schedule_value = v;
        }
        if schedule_changed {
            job.next_run = compute_next_run(&job.schedule_type, &job.schedule_value)?;
        }

        if !store::update_job(conn, &job)? {
            return Err(not_found(&job.id));
        }
        detail_from(conn, job)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn job_delete(id: String) -> Result<serde_json::Value> {
    with_conn(move |conn| {
        if !store::delete_job(conn, &id)? {
            return Err(not_found(&id));
        }
        Ok(json!({ "ok": true }))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn job_set_enabled(id: String, enabled: bool) -> Result<JobDetail> {
    with_conn(move |conn| {
        let job = store::get_job(conn, &id)?.ok_or_else(|| not_found(&id))?;
        // 重新启用时从 now 重算 next_run，避免陈旧 next_run 触发补跑洪峰；
        // once 已过期的任务无法再启用（JOB_INVALID_SCHEDULE）。
        let next_run = if enabled {
            Some(compute_next_run(&job.schedule_type, &job.schedule_value)?)
        } else {
            None
        };
        store::set_enabled(conn, &id, enabled, next_run.as_deref())?;
        let job = store::get_job(conn, &id)?.ok_or_else(|| not_found(&id))?;
        detail_from(conn, job)
    })
    .await
}

/// 手动触发。成功返回 DispatchReceipt；未接线返回错误码 JOB_DISPATCHER_NOT_WIRED。
#[tauri::command(rename_all = "snake_case")]
pub async fn job_run_now(id: String) -> Result<DispatchReceipt> {
    with_conn(move |conn| {
        let job = store::get_job(conn, &id)?.ok_or_else(|| not_found(&id))?;
        let dispatcher = global_dispatcher();
        runner::run_job_manual(conn, dispatcher.as_ref(), &job, Utc::now())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn job_runs_list(
    job_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<JobRunsResponse> {
    with_conn(move |conn| {
        let limit = limit.unwrap_or(DEFAULT_RUNS_PAGE).clamp(1, MAX_RUNS_PAGE);
        let offset = offset.unwrap_or(0).max(0);
        let (runs, total) = store::list_runs(conn, job_id.as_deref(), limit, offset)?;
        Ok(JobRunsResponse { runs, total })
    })
    .await
}
