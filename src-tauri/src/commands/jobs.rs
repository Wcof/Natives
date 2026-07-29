//! commands/jobs.rs — 任务模块（Job Module）IPC 命令面（契约第 5 节，R-B5 归域）
//!
//! 全部返回 error.rs 的 Result（R-B1），JSON snake_case。
//! 错误码（前端可判定，均为返回串前缀）：
//! JOB_NOT_FOUND / JOB_INVALID_SCHEDULE / JOB_INVALID_PROJECT_PATH / JOB_DISPATCHER_NOT_WIRED。
//! 阻塞 SQLite 操作统一经 spawn_blocking 卸载（R-B6）。

use crate::jobs::dispatch::DispatchReceipt;
use crate::jobs::{global_dispatcher, runner, schedule, store};
use crate::{Error, Result};
use assistant_protocol::v2::CapabilitySelection;
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
    pub capability_selection: Option<CapabilitySelection>,
    /// One-release compatibility view for legacy untyped arrays.
    pub capability_refs: Option<Vec<String>>,
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

fn invalid_capability_selection(detail: impl AsRef<str>) -> Error {
    Error::Message(format!(
        "JOB_INVALID_CAPABILITY_SELECTION: {}",
        detail.as_ref()
    ))
}

fn encode_capability_binding(
    selection: Option<CapabilitySelection>,
    legacy_refs: Option<Vec<String>>,
    default_empty: bool,
) -> Result<Option<String>> {
    if selection.is_some() && legacy_refs.is_some() {
        return Err(invalid_capability_selection(
            "capability_selection and capability_refs cannot be submitted together",
        ));
    }
    if let Some(selection) = selection {
        selection.validate().map_err(invalid_capability_selection)?;
        return serde_json::to_string(&selection)
            .map(Some)
            .map_err(Error::Json);
    }
    if let Some(legacy_refs) = legacy_refs {
        return serde_json::to_string(&legacy_refs)
            .map(Some)
            .map_err(Error::Json);
    }
    if default_empty {
        return serde_json::to_string(&CapabilitySelection {
            skills: Some(Vec::new()),
            mcp_servers: Some(Vec::new()),
            expert_id: None,
            team_id: None,
        })
        .map(Some)
        .map_err(Error::Json);
    }
    Ok(None)
}

fn decode_capability_binding(
    raw: &str,
) -> Result<(Option<CapabilitySelection>, Option<Vec<String>>)> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        invalid_capability_selection(format!("stored capability JSON is invalid: {error}"))
    })?;
    if value.is_array() {
        let legacy_refs: Vec<String> = serde_json::from_value(value).map_err(|error| {
            invalid_capability_selection(format!("legacy capability_refs is invalid: {error}"))
        })?;
        let selection = legacy_refs.is_empty().then(|| CapabilitySelection {
            skills: Some(Vec::new()),
            mcp_servers: Some(Vec::new()),
            expert_id: None,
            team_id: None,
        });
        return Ok((selection, Some(legacy_refs)));
    }
    if value.is_object() {
        let selection: CapabilitySelection = serde_json::from_value(value).map_err(|error| {
            invalid_capability_selection(format!("stored capability_selection is invalid: {error}"))
        })?;
        selection.validate().map_err(invalid_capability_selection)?;
        return Ok((Some(selection), None));
    }
    Err(invalid_capability_selection(
        "stored capability binding must be an object or legacy array",
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

fn apply_nullable_text_patch(target: &mut Option<String>, patch: Option<String>) {
    let Some(value) = patch else {
        return;
    };
    let value = value.trim();
    *target = (!value.is_empty()).then(|| value.to_string());
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
    let (capability_selection, capability_refs) = decode_capability_binding(&job.capability_refs)?;
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
        capability_selection,
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
    capability_selection: Option<CapabilitySelection>,
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
            encode_capability_binding(capability_selection, capability_refs, true)?.ok_or_else(
                || Error::Internal("new job capability binding was not encoded".to_string()),
            )?;
        let mut normalized_description = None;
        apply_nullable_text_patch(&mut normalized_description, description);
        let mut normalized_key_id = None;
        apply_nullable_text_patch(&mut normalized_key_id, key_id);

        let now_iso = schedule::format_utc(&Utc::now());
        let job = store::JobDefinition {
            id: format!("job_{}", uuid::Uuid::new_v4()),
            name,
            prompt,
            description: normalized_description,
            project_path: Some(project_path),
            provider_id,
            model_id,
            key_id: normalized_key_id,
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
    capability_selection: Option<CapabilitySelection>,
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
        apply_nullable_text_patch(&mut job.description, description);
        if let Some(v) = provider_id {
            job.provider_id = Some(v);
        }
        if let Some(v) = model_id {
            job.model_id = Some(v);
        }
        apply_nullable_text_patch(&mut job.key_id, key_id);
        if let Some(v) = agent_profile_id {
            job.agent_profile_id = Some(v);
        }
        if let Some(binding) =
            encode_capability_binding(capability_selection, capability_refs, false)?
        {
            job.capability_refs = binding;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_job(capability_refs: &str) -> store::JobDefinition {
        store::JobDefinition {
            id: "job-a".to_string(),
            name: "Job A".to_string(),
            prompt: "Do work".to_string(),
            description: None,
            project_path: Some("/tmp".to_string()),
            provider_id: Some("provider-a".to_string()),
            model_id: Some("model-a".to_string()),
            key_id: None,
            agent_profile_id: None,
            capability_refs: capability_refs.to_string(),
            permission_profile: "readonly".to_string(),
            max_steps: Some(50),
            effort: None,
            runtime_id: Some("native".to_string()),
            schedule_type: "interval".to_string(),
            schedule_value: "60".to_string(),
            next_run: "2026-07-29T01:00:00Z".to_string(),
            last_status: None,
            last_run_at: None,
            consecutive_errors: 0,
            enabled: true,
            created_at: "2026-07-29T00:00:00Z".to_string(),
            expires_at: None,
        }
    }

    #[test]
    fn capability_binding_rejects_new_and_legacy_fields_together() {
        let error = encode_capability_binding(
            Some(CapabilitySelection::default()),
            Some(Vec::new()),
            false,
        )
        .expect_err("mixed capability fields must fail");

        assert!(error
            .to_string()
            .contains("JOB_INVALID_CAPABILITY_SELECTION"));
    }

    #[test]
    fn capability_binding_serializes_typed_selection_as_an_object() {
        let raw = encode_capability_binding(
            Some(CapabilitySelection {
                skills: Some(vec!["skill-a".to_string()]),
                mcp_servers: Some(vec!["mcp-a".to_string()]),
                expert_id: Some("expert-a".to_string()),
                team_id: None,
            }),
            None,
            false,
        )
        .expect("valid selection")
        .expect("encoded selection");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("json");

        assert!(value.is_object());
        assert_eq!(value["skills"], serde_json::json!(["skill-a"]));
        assert_eq!(value["mcp_servers"], serde_json::json!(["mcp-a"]));
        assert_eq!(value["expert_id"], "expert-a");
    }

    #[test]
    fn capability_binding_preserves_legacy_arrays_for_compatibility() {
        let raw = encode_capability_binding(None, Some(vec!["legacy-id".to_string()]), false)
            .expect("legacy input remains accepted")
            .expect("encoded legacy refs");

        assert_eq!(raw, r#"["legacy-id"]"#);
    }

    #[test]
    fn capability_binding_defaults_new_jobs_to_an_explicit_empty_selection() {
        let raw = encode_capability_binding(None, None, true)
            .expect("default selection")
            .expect("encoded default");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("json");

        assert_eq!(value["skills"], serde_json::json!([]));
        assert_eq!(value["mcp_servers"], serde_json::json!([]));
    }

    #[test]
    fn capability_binding_reads_legacy_empty_array_as_explicit_empty_selection() {
        let (selection, legacy_refs) =
            decode_capability_binding("[]").expect("legacy empty binding");
        let selection = selection.expect("explicit empty selection");

        assert_eq!(selection.skills, Some(Vec::new()));
        assert_eq!(selection.mcp_servers, Some(Vec::new()));
        assert_eq!(legacy_refs, Some(Vec::new()));
    }

    #[test]
    fn capability_binding_reads_typed_object_without_inventing_legacy_refs() {
        let (selection, legacy_refs) = decode_capability_binding(
            r#"{"skills":["skill-a"],"mcp_servers":[],"team_id":"team-a"}"#,
        )
        .expect("typed binding");
        let selection = selection.expect("typed selection");

        assert_eq!(selection.skills, Some(vec!["skill-a".to_string()]));
        assert_eq!(selection.mcp_servers, Some(Vec::new()));
        assert_eq!(selection.team_id.as_deref(), Some("team-a"));
        assert_eq!(legacy_refs, None);
    }

    #[test]
    fn malformed_stored_capability_json_uses_the_structured_job_error() {
        let error = decode_capability_binding("{not-json").expect_err("invalid json");

        assert!(error
            .to_string()
            .contains("JOB_INVALID_CAPABILITY_SELECTION"));
    }

    #[test]
    fn explicit_empty_optional_text_clears_a_stored_provider_key() {
        let mut key_id = Some("old-provider-key".to_string());

        apply_nullable_text_patch(&mut key_id, Some("  ".to_string()));

        assert_eq!(key_id, None);

        apply_nullable_text_patch(&mut key_id, Some(" new-provider-key ".to_string()));
        assert_eq!(key_id.as_deref(), Some("new-provider-key"));

        apply_nullable_text_patch(&mut key_id, None);
        assert_eq!(
            key_id.as_deref(),
            Some("new-provider-key"),
            "an omitted update field must preserve the stored value"
        );
    }

    #[test]
    fn job_detail_exposes_typed_capability_selection_from_the_reused_text_column() {
        let conn = rusqlite::Connection::open_in_memory().expect("db");
        store::ensure_schema(&conn).expect("schema");
        let detail = detail_from(
            &conn,
            stored_job(r#"{"skills":["skill-a"],"mcp_servers":[]}"#),
        )
        .expect("detail");

        assert_eq!(
            detail
                .capability_selection
                .and_then(|selection| selection.skills),
            Some(vec!["skill-a".to_string()])
        );
        assert_eq!(detail.capability_refs, None);
    }
}
