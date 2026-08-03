//! jobs/dispatch.rs — 派发接缝（接缝 A，契约第 3 节）
//!
//! NativeJobDispatcher 把 JobDefinition 映射为 CreateRunRequest /
//! StartRunRequest（assistant-protocol v2 run.*），经 Host daemon_authority
//! 进入 UDS Daemon 单执行主链；NotWiredDispatcher 仅保留 fail-closed 契约。

use super::store::JobDefinition;
use assistant_protocol::v2::{
    CapabilitySelection, CreateRunRequest, RunStatusV2, RunV2, StartRunRequest,
};
use chrono::{DateTime, Utc};
use serde::Serialize;

/// 派发回执：引擎侧会话 / run 标识 + 幂等键。
#[derive(Debug, Clone, Serialize)]
pub struct DispatchReceipt {
    pub conversation_id: String,
    pub run_id: String,
    pub idempotency_key: String,
}

/// 触发来源（契约第 2 节 trigger 列：schedule | manual）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Schedule,
    Manual,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Schedule => "schedule",
            Trigger::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// 执行链路未接线（fail-closed / compatibility adapter）。
    NotWired,
    /// Job 定义无法映射为引擎请求。
    InvalidJob(String),
    /// 引擎侧失败。
    Engine(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::NotWired => write!(f, "dispatcher not wired"),
            DispatchError::InvalidJob(m) => write!(f, "invalid job: {m}"),
            DispatchError::Engine(m) => write!(f, "engine error: {m}"),
        }
    }
}

/// 幂等键约定（契约第 3 节）：`job-{job_id}-{scheduled_at_unix}`。
/// P1 对接时随派发传给引擎。
pub fn idempotency_key(job_id: &str, scheduled_at_unix: i64) -> String {
    format!("job-{job_id}-{scheduled_at_unix}")
}

fn invalid_job(code: &str, detail: impl AsRef<str>) -> DispatchError {
    DispatchError::InvalidJob(format!("{code}: {}", detail.as_ref()))
}

fn required_job_field(
    value: &Option<String>,
    code: &str,
    field: &str,
) -> Result<String, DispatchError> {
    value
        .as_ref()
        .filter(|item| !item.trim().is_empty())
        .cloned()
        .ok_or_else(|| invalid_job(code, format!("{field} is required")))
}

fn parse_capability_selection(raw: &str) -> Result<CapabilitySelection, DispatchError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        invalid_job(
            "JOB_INVALID_CAPABILITY_SELECTION",
            format!("stored capability JSON is invalid: {error}"),
        )
    })?;
    if value.is_object() {
        let selection: CapabilitySelection = serde_json::from_value(value).map_err(|error| {
            invalid_job(
                "JOB_INVALID_CAPABILITY_SELECTION",
                format!("capability_selection is invalid: {error}"),
            )
        })?;
        selection
            .validate()
            .map_err(|error| invalid_job("JOB_INVALID_CAPABILITY_SELECTION", error))?;
        return Ok(selection);
    }
    if value.is_array() {
        let refs: Vec<String> = serde_json::from_value(value).map_err(|error| {
            invalid_job(
                "JOB_INVALID_CAPABILITY_SELECTION",
                format!("legacy capability_refs is invalid: {error}"),
            )
        })?;
        if refs.is_empty() {
            return Ok(CapabilitySelection {
                skills: Some(Vec::new()),
                mcp_servers: Some(Vec::new()),
                expert_id: None,
                team_id: None,
            });
        }
        return Err(invalid_job(
            "JOB_INVALID_CAPABILITY_SELECTION",
            "legacy capability_refs contains untyped ids",
        ));
    }
    Err(invalid_job(
        "JOB_INVALID_CAPABILITY_SELECTION",
        "capability binding must be an object or legacy array",
    ))
}

fn build_run_requests(
    job: &JobDefinition,
    scheduled_at: DateTime<Utc>,
) -> Result<(CreateRunRequest, StartRunRequest, String), DispatchError> {
    let project_path = required_job_field(
        &job.project_path,
        "JOB_INVALID_PROJECT_PATH",
        "project_path",
    )?;
    let provider_id = required_job_field(&job.provider_id, "JOB_INVALID_PROVIDER", "provider_id")?;
    let model_id = required_job_field(&job.model_id, "JOB_INVALID_MODEL", "model_id")?;
    let max_steps = match job.max_steps {
        None => 50,
        Some(value) if value > 0 => u32::try_from(value).map_err(|_| {
            invalid_job("JOB_INVALID_MAX_STEPS", "max_steps must fit a positive u32")
        })?,
        Some(_) => {
            return Err(invalid_job(
                "JOB_INVALID_MAX_STEPS",
                "max_steps must be positive",
            ))
        }
    };
    let capability_selection = parse_capability_selection(&job.capability_refs)?;
    let conversation_id = format!("job-{}", job.id);
    let key = idempotency_key(&job.id, scheduled_at.timestamp());
    let create = CreateRunRequest {
        conversation_id: conversation_id.clone(),
        provider_id: provider_id.clone(),
        model_id: model_id.clone(),
        key_id: job.key_id.clone(),
        agent_profile_id: job.agent_profile_id.clone(),
        permission_profile: Some(job.permission_profile.clone()),
        content: Some(job.prompt.clone()),
        attachments: None,
        max_steps: Some(max_steps),
        parent_run_id: None,
        project_path: Some(project_path.clone()),
        idempotency_key: Some(key.clone()),
        effort: job.effort.clone(),
        runtime_id: job.runtime_id.clone(),
        capability_selection: Some(capability_selection.clone()),
    };
    let start = StartRunRequest {
        run_id: None,
        conversation_id: Some(conversation_id),
        provider_id: Some(provider_id),
        model_id: Some(model_id),
        key_id: job.key_id.clone(),
        content: Some(job.prompt.clone()),
        attachments: None,
        trigger_message_id: None,
        permission_profile: Some(job.permission_profile.clone()),
        max_steps: Some(max_steps),
        project_path: Some(project_path),
        idempotency_key: None,
        effort: job.effort.clone(),
        runtime_id: job.runtime_id.clone(),
        agent_profile_id: job.agent_profile_id.clone(),
        capability_selection: Some(capability_selection),
    };
    Ok((create, start, key))
}

#[async_trait::async_trait]
pub trait JobDispatcher: Send + Sync {
    fn is_wired(&self) -> bool;
    async fn dispatch(
        &self,
        job: &JobDefinition,
        trigger: Trigger,
        scheduled_at: DateTime<Utc>,
    ) -> Result<DispatchReceipt, DispatchError>;
}

/// Fail-closed 适配器：执行链路未接线时诚实报告 NotWired（无假绿，R-F2）。
pub struct NotWiredDispatcher;

#[async_trait::async_trait]
impl JobDispatcher for NotWiredDispatcher {
    fn is_wired(&self) -> bool {
        false
    }

    async fn dispatch(
        &self,
        _job: &JobDefinition,
        _trigger: Trigger,
        _scheduled_at: DateTime<Utc>,
    ) -> Result<DispatchReceipt, DispatchError> {
        Err(DispatchError::NotWired)
    }
}

#[async_trait::async_trait]
trait RunDispatchGateway: Send + Sync {
    async fn create_run(&self, request: CreateRunRequest) -> Result<RunV2, String>;
    async fn start_run(&self, request: StartRunRequest) -> Result<RunV2, String>;
}

struct DaemonAuthorityGateway;

#[async_trait::async_trait]
impl RunDispatchGateway for DaemonAuthorityGateway {
    async fn create_run(&self, request: CreateRunRequest) -> Result<RunV2, String> {
        crate::daemon_authority::create_run(request).await
    }

    async fn start_run(&self, request: StartRunRequest) -> Result<RunV2, String> {
        crate::daemon_authority::start_run(request).await
    }
}

/// Host-owned adapter from persisted jobs to the single Daemon run authority.
pub struct NativeJobDispatcher {
    gateway: std::sync::Arc<dyn RunDispatchGateway>,
}

impl NativeJobDispatcher {
    pub fn new() -> Self {
        Self {
            gateway: std::sync::Arc::new(DaemonAuthorityGateway),
        }
    }

    #[cfg(test)]
    fn with_gateway(gateway: std::sync::Arc<dyn RunDispatchGateway>) -> Self {
        Self { gateway }
    }
}

impl Default for NativeJobDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl JobDispatcher for NativeJobDispatcher {
    fn is_wired(&self) -> bool {
        true
    }

    async fn dispatch(
        &self,
        job: &JobDefinition,
        _trigger: Trigger,
        scheduled_at: DateTime<Utc>,
    ) -> Result<DispatchReceipt, DispatchError> {
        let (create, mut start, key) = build_run_requests(job, scheduled_at)?;
        let created = self
            .gateway
            .create_run(create)
            .await
            .map_err(DispatchError::Engine)?;
        if created.status == RunStatusV2::Queued {
            start.run_id = Some(created.id.clone());
            self.gateway
                .start_run(start)
                .await
                .map_err(DispatchError::Engine)?;
        }
        Ok(DispatchReceipt {
            conversation_id: created.conversation_id,
            run_id: created.id,
            idempotency_key: key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_protocol::v2::{RunStatusV2, RunV2};
    use chrono::{TimeZone, Utc};
    use std::sync::{Arc, Mutex};

    struct RecordingRunGateway {
        created: Mutex<Vec<CreateRunRequest>>,
        started: Mutex<Vec<StartRunRequest>>,
        create_result: RunV2,
    }

    #[async_trait::async_trait]
    impl RunDispatchGateway for RecordingRunGateway {
        async fn create_run(&self, request: CreateRunRequest) -> Result<RunV2, String> {
            self.created.lock().expect("created lock").push(request);
            Ok(self.create_result.clone())
        }

        async fn start_run(&self, request: StartRunRequest) -> Result<RunV2, String> {
            self.started.lock().expect("started lock").push(request);
            let mut run = self.create_result.clone();
            run.status = RunStatusV2::Preparing;
            Ok(run)
        }
    }

    fn run(status: RunStatusV2) -> RunV2 {
        RunV2 {
            retry_of_run_id: None,
            retry_of_turn_id: None,
            continued_from_run_id: None,
            branch_id: None,
            branch_parent_message_id: None,
            checkpoint_id: None,
            resume_of_run_id: None,
            id: "real-run-id".to_string(),
            conversation_id: "job-daily-review".to_string(),
            status,
            parent_run_id: None,
            agent_profile_id: None,
            provider_id: "provider-a".to_string(),
            key_id: Some("key-a".to_string()),
            model_id: "model-a".to_string(),
            permission_profile: "ask".to_string(),
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
            idempotency_key: Some("job-daily-review-1785286800".to_string()),
            effort: Some("high".to_string()),
            runtime_id: Some("native".to_string()),
            revision: 0,
            capability_snapshot: None,
        }
    }

    fn job() -> JobDefinition {
        JobDefinition {
            id: "daily-review".to_string(),
            name: "Daily review".to_string(),
            prompt: "review the project".to_string(),
            description: None,
            project_path: Some("/tmp".to_string()),
            provider_id: Some("provider-a".to_string()),
            model_id: Some("model-a".to_string()),
            key_id: Some("key-a".to_string()),
            agent_profile_id: None,
            capability_refs:
                r#"{"skills":["skill-a"],"mcp_servers":["mcp-a"],"expert_id":"expert-a"}"#
                    .to_string(),
            permission_profile: "ask".to_string(),
            max_steps: Some(50),
            effort: Some("high".to_string()),
            runtime_id: Some("native".to_string()),
            schedule_type: "interval".to_string(),
            schedule_value: "3600".to_string(),
            next_run: "2026-07-29T01:00:00Z".to_string(),
            last_status: None,
            last_run_at: None,
            consecutive_errors: 0,
            enabled: true,
            created_at: "2026-07-29T00:00:00Z".to_string(),
            expires_at: None,
        }
    }

    #[tokio::test]
    async fn dispatcher_contract_carries_the_explicit_scheduled_at() {
        let scheduled_at = Utc
            .with_ymd_and_hms(2026, 7, 29, 1, 0, 0)
            .single()
            .expect("valid timestamp");
        let error = NotWiredDispatcher
            .dispatch(&job(), Trigger::Schedule, scheduled_at)
            .await
            .expect_err("not-wired remains fail-closed");
        assert_eq!(error, DispatchError::NotWired);
    }

    #[tokio::test]
    async fn native_dispatcher_starts_the_created_queued_run_by_its_real_id() {
        let scheduled_at = Utc
            .with_ymd_and_hms(2026, 7, 29, 1, 0, 0)
            .single()
            .expect("valid timestamp");
        let gateway = Arc::new(RecordingRunGateway {
            created: Mutex::new(Vec::new()),
            started: Mutex::new(Vec::new()),
            create_result: run(RunStatusV2::Queued),
        });
        let dispatcher = NativeJobDispatcher::with_gateway(gateway.clone());

        let receipt = dispatcher
            .dispatch(&job(), Trigger::Schedule, scheduled_at)
            .await
            .expect("dispatch");

        assert_eq!(receipt.run_id, "real-run-id");
        assert_eq!(receipt.conversation_id, "job-daily-review");
        assert_eq!(receipt.idempotency_key, "job-daily-review-1785286800");
        assert_eq!(gateway.created.lock().expect("created").len(), 1);
        let started = gateway.started.lock().expect("started");
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].run_id.as_deref(), Some("real-run-id"));
    }

    #[tokio::test]
    async fn native_dispatcher_does_not_restart_an_idempotently_returned_active_run() {
        let scheduled_at = Utc
            .with_ymd_and_hms(2026, 7, 29, 1, 0, 0)
            .single()
            .expect("valid timestamp");
        let gateway = Arc::new(RecordingRunGateway {
            created: Mutex::new(Vec::new()),
            started: Mutex::new(Vec::new()),
            create_result: run(RunStatusV2::Running),
        });
        let dispatcher = NativeJobDispatcher::with_gateway(gateway.clone());

        let receipt = dispatcher
            .dispatch(&job(), Trigger::Schedule, scheduled_at)
            .await
            .expect("idempotent dispatch");

        assert_eq!(receipt.run_id, "real-run-id");
        assert_eq!(gateway.created.lock().expect("created").len(), 1);
        assert!(
            gateway.started.lock().expect("started").is_empty(),
            "only queued runs may be started"
        );
    }

    #[test]
    fn native_request_mapping_preserves_job_fields_and_stable_identity() {
        let scheduled_at = Utc
            .with_ymd_and_hms(2026, 7, 29, 1, 0, 0)
            .single()
            .expect("valid timestamp");
        let (create, start, key) =
            build_run_requests(&job(), scheduled_at).expect("valid job mapping");

        assert_eq!(create.conversation_id, "job-daily-review");
        assert_eq!(key, "job-daily-review-1785286800");
        assert_eq!(create.idempotency_key.as_deref(), Some(key.as_str()));
        assert_eq!(create.provider_id, "provider-a");
        assert_eq!(create.model_id, "model-a");
        assert_eq!(create.key_id.as_deref(), Some("key-a"));
        assert_eq!(create.permission_profile.as_deref(), Some("ask"));
        assert_eq!(create.content.as_deref(), Some("review the project"));
        assert_eq!(create.project_path.as_deref(), Some("/tmp"));
        assert_eq!(create.max_steps, Some(50));
        assert_eq!(create.effort.as_deref(), Some("high"));
        assert_eq!(create.runtime_id.as_deref(), Some("native"));
        assert_eq!(
            create
                .capability_selection
                .as_ref()
                .and_then(|selection| selection.skills.as_ref()),
            Some(&vec!["skill-a".to_string()])
        );
        assert_eq!(
            start
                .capability_selection
                .as_ref()
                .and_then(|selection| selection.mcp_servers.as_ref()),
            Some(&vec!["mcp-a".to_string()])
        );
        assert_eq!(
            start
                .capability_selection
                .as_ref()
                .and_then(|selection| selection.expert_id.as_deref()),
            Some("expert-a")
        );
        assert_eq!(start.conversation_id.as_deref(), Some("job-daily-review"));
        assert_eq!(start.provider_id.as_deref(), Some("provider-a"));
        assert_eq!(start.model_id.as_deref(), Some("model-a"));
        assert_eq!(start.idempotency_key, None);
    }

    #[test]
    fn native_request_mapping_rejects_missing_required_fields_and_invalid_steps() {
        let scheduled_at = Utc::now();

        let mut missing_project = job();
        missing_project.project_path = None;
        assert_invalid_code(
            build_run_requests(&missing_project, scheduled_at),
            "JOB_INVALID_PROJECT_PATH",
        );

        let mut missing_provider = job();
        missing_provider.provider_id = None;
        assert_invalid_code(
            build_run_requests(&missing_provider, scheduled_at),
            "JOB_INVALID_PROVIDER",
        );

        let mut missing_model = job();
        missing_model.model_id = Some(" ".to_string());
        assert_invalid_code(
            build_run_requests(&missing_model, scheduled_at),
            "JOB_INVALID_MODEL",
        );

        for invalid in [0, -1, i64::from(u32::MAX) + 1] {
            let mut invalid_steps = job();
            invalid_steps.max_steps = Some(invalid);
            assert_invalid_code(
                build_run_requests(&invalid_steps, scheduled_at),
                "JOB_INVALID_MAX_STEPS",
            );
        }
    }

    #[test]
    fn legacy_capability_arrays_are_only_compatible_when_empty() {
        let scheduled_at = Utc::now();
        let mut empty = job();
        empty.capability_refs = "[]".to_string();
        let (create, _, _) =
            build_run_requests(&empty, scheduled_at).expect("legacy empty maps to empty selection");
        let selection = create
            .capability_selection
            .expect("explicit empty selection");
        assert_eq!(selection.skills, Some(Vec::new()));
        assert_eq!(selection.mcp_servers, Some(Vec::new()));

        let mut ambiguous = job();
        ambiguous.capability_refs = r#"["skill-or-mcp"]"#.to_string();
        assert_invalid_code(
            build_run_requests(&ambiguous, scheduled_at),
            "JOB_INVALID_CAPABILITY_SELECTION",
        );
    }

    fn assert_invalid_code(
        result: Result<
            (
                assistant_protocol::v2::CreateRunRequest,
                assistant_protocol::v2::StartRunRequest,
                String,
            ),
            DispatchError,
        >,
        code: &str,
    ) {
        let error = result.expect_err("mapping must fail");
        assert!(
            error.to_string().contains(code),
            "expected {code}, got {error}"
        );
    }
}
