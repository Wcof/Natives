//! Public DTO and domain enums for Creative Apps.

use serde::{Deserialize, Serialize};

/// Unified read-only list projection for Personal Creations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppSummary {
    pub id: String,
    /// Unified identity shared across Catalog / assistant card / detail /
    /// preview for the same app, regardless of source (batch 1).
    #[serde(default)]
    pub application_id: String,
    /// Active runtime instance id, when the app currently has one (empty when
    /// not running). Buttons that operate on the runtime carry this id.
    #[serde(default)]
    pub runtime_instance_id: Option<String>,
    pub source: CreativeAppSource,
    pub runtime: CreativeAppRuntime,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub version: String,
    pub state: CreativeAppState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_detail: Option<CreativeAppStatusDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_project: Option<LocalProjectSummary>,
    pub actions: CreativeAppActions,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeAppSource {
    Internal,
    ExternalGithub,
    LocalProject,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeAppRuntime {
    WorkshopStatic,
    DockerCompose,
    DockerRun,
    LocalStatic,
    NodeDevServer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeAppState {
    Available,
    Disabled,
    Installing,
    InstalledStopped,
    Starting,
    Running,
    Stopping,
    RuntimeUnavailable,
    InstallFailed,
    StartFailed,
    Deleting,
    DeleteFailed,
    /// Stop could not verify that resources (process group / port) were released.
    /// Process identity, port and URL are preserved so a retry stop stays possible.
    CleanupFailed,
    /// A live process was found after restart / crash that the supervisor does not
    /// own. Identity is preserved; the user must resolve it (stop or restart).
    Orphaned,
}

impl CreativeAppState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Disabled => "disabled",
            Self::Installing => "installing",
            Self::InstalledStopped => "installed_stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::InstallFailed => "install_failed",
            Self::StartFailed => "start_failed",
            Self::Deleting => "deleting",
            Self::DeleteFailed => "delete_failed",
            Self::CleanupFailed => "cleanup_failed",
            Self::Orphaned => "orphaned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "available" => Self::Available,
            "disabled" => Self::Disabled,
            "installing" => Self::Installing,
            "installed_stopped" => Self::InstalledStopped,
            "starting" => Self::Starting,
            "running" => Self::Running,
            "stopping" => Self::Stopping,
            "runtime_unavailable" => Self::RuntimeUnavailable,
            "install_failed" => Self::InstallFailed,
            "start_failed" => Self::StartFailed,
            "deleting" => Self::Deleting,
            "delete_failed" => Self::DeleteFailed,
            "cleanup_failed" => Self::CleanupFailed,
            "orphaned" => Self::Orphaned,
            _ => return None,
        })
    }

    /// Transient states that must be reconciled against Docker on startup.
    pub fn is_transient(self) -> bool {
        matches!(
            self,
            Self::Installing | Self::Starting | Self::Stopping | Self::Deleting
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppActions {
    pub can_open: bool,
    pub can_start: bool,
    pub can_stop: bool,
    pub can_delete: bool,
    pub can_retry: bool,
}

impl CreativeAppActions {
    pub fn for_state(source: CreativeAppSource, state: CreativeAppState) -> Self {
        match source {
            CreativeAppSource::Internal => match state {
                CreativeAppState::Available => Self {
                    can_open: true,
                    can_start: false,
                    can_stop: true, // disable
                    can_delete: true,
                    can_retry: false,
                },
                CreativeAppState::Disabled => Self {
                    can_open: false,
                    can_start: true, // enable
                    can_stop: false,
                    can_delete: true,
                    can_retry: false,
                },
                _ => Self {
                    can_open: false,
                    can_start: false,
                    can_stop: false,
                    can_delete: true,
                    can_retry: false,
                },
            },
            CreativeAppSource::ExternalGithub | CreativeAppSource::LocalProject => match state {
                CreativeAppState::Running => Self {
                    can_open: true,
                    can_start: false,
                    can_stop: true,
                    can_delete: true,
                    can_retry: false,
                },
                CreativeAppState::InstalledStopped => Self {
                    can_open: false,
                    can_start: true,
                    can_stop: false,
                    can_delete: true,
                    can_retry: false,
                },
                CreativeAppState::InstallFailed | CreativeAppState::StartFailed => Self {
                    can_open: false,
                    can_start: true,
                    // external may still need stop to clean partial containers; local can stop too
                    can_stop: true,
                    can_delete: true,
                    can_retry: true,
                },
                CreativeAppState::RuntimeUnavailable => Self {
                    can_open: false,
                    can_start: source == CreativeAppSource::LocalProject,
                    can_stop: false,
                    // local project delete never needs docker
                    can_delete: source == CreativeAppSource::LocalProject,
                    can_retry: true,
                },
                CreativeAppState::Installing
                | CreativeAppState::Starting
                | CreativeAppState::Stopping
                | CreativeAppState::Deleting => Self {
                    can_open: false,
                    can_start: false,
                    can_stop: false,
                    can_delete: false,
                    can_retry: false,
                },
                CreativeAppState::DeleteFailed => Self {
                    can_open: false,
                    can_start: false,
                    can_stop: false,
                    can_delete: true,
                    can_retry: true,
                },
                // Stop failed to verify resource release, or a live process was found
                // after crash. Resources may still exist — keep stop + retry open, and
                // never offer "open" until resources are confirmed released.
                CreativeAppState::CleanupFailed | CreativeAppState::Orphaned => Self {
                    can_open: false,
                    can_start: true,
                    can_stop: true,
                    can_delete: true,
                    can_retry: true,
                },
                _ => Self {
                    can_open: false,
                    can_start: false,
                    can_stop: false,
                    can_delete: true,
                    can_retry: false,
                },
            },
        }
    }
}

// ── Runtime instance (batch 1: unified runtime ownership record) ──────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInstanceStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    CleanupFailed,
    Orphaned,
}

impl RuntimeInstanceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::CleanupFailed => "cleanup_failed",
            Self::Orphaned => "orphaned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "starting" => Self::Starting,
            "running" => Self::Running,
            "stopping" => Self::Stopping,
            "stopped" => Self::Stopped,
            "failed" => Self::Failed,
            "cleanup_failed" => Self::CleanupFailed,
            "orphaned" => Self::Orphaned,
            _ => return None,
        })
    }

    /// Non-terminal statuses that must be unique per application (CAS).
    pub fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

// ── Local project domain (third source) ────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalProjectKind {
    Html,
    Vite,
    Vue,
    VueVite,
    ViteOther,
    Unknown,
}

impl LocalProjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Vite => "vite",
            Self::Vue => "vue",
            Self::VueVite => "vue_vite",
            Self::ViteOther => "vite_other",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "html" => Self::Html,
            "vite" => Self::Vite,
            "vue" => Self::Vue,
            "vue_vite" => Self::VueVite,
            "vite_other" => Self::ViteOther,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Smart,
    Custom,
}

impl LaunchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smart => "smart",
            Self::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "smart" => Self::Smart,
            "custom" => Self::Custom,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}

impl PackageManager {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "npm" => Self::Npm,
            "pnpm" => Self::Pnpm,
            "yarn" => Self::Yarn,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalCreativeIssueCode {
    PathMissing,
    EnvironmentMissing,
    DependenciesMissing,
    PortConflict,
    ConfigInvalid,
    AiError,
    StartUnhealthy,
    OrphanedProcess,
    /// Stop could not verify resource release (process group / port still present).
    StopFailed,
}

impl LocalCreativeIssueCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PathMissing => "path_missing",
            Self::EnvironmentMissing => "environment_missing",
            Self::DependenciesMissing => "dependencies_missing",
            Self::PortConflict => "port_conflict",
            Self::ConfigInvalid => "config_invalid",
            Self::AiError => "ai_error",
            Self::StartUnhealthy => "start_unhealthy",
            Self::OrphanedProcess => "orphaned_process",
            Self::StopFailed => "stop_failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "path_missing" => Self::PathMissing,
            "environment_missing" => Self::EnvironmentMissing,
            "dependencies_missing" => Self::DependenciesMissing,
            "port_conflict" => Self::PortConflict,
            "config_invalid" => Self::ConfigInvalid,
            "ai_error" => Self::AiError,
            "start_unhealthy" => Self::StartUnhealthy,
            "orphaned_process" => Self::OrphanedProcess,
            "stop_failed" => Self::StopFailed,
            _ => return None,
        })
    }
}

/// Coarse status detail for local creative issues (not every transient stage).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppStatusDetail {
    pub code: LocalCreativeIssueCode,
    pub message: String,
    #[serde(default)]
    pub recovery_actions: Vec<String>,
}

/// Local-project projection nested under CreativeAppSummary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalProjectSummary {
    pub project_root: String,
    pub project_kind: LocalProjectKind,
    pub launch_mode: LaunchMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManager>,
    pub device_id: String,
    pub device_name: String,
    /// Whether the host should open the app GUI after a successful start.
    pub auto_open: bool,
}

/// Canonical LaunchPlan produced by rule / user / AI; always re-validated locally.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub schema_version: u32,
    pub source: LaunchPlanSource,
    pub project_kind: LocalProjectKind,
    pub runtime: LocalLaunchRuntime,
    pub program: LaunchProgram,
    pub cwd_relative: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_file: Option<String>,
    /// Detected runner from the real package.json script body (or node entry).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_runner: Option<ScriptRunner>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment_keys: Vec<String>,
    pub port: LaunchPort,
    pub open_path: String,
    pub health_path: String,
    pub startup_timeout_ms: u32,
    pub auto_open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub reason: String,
    /// Compose detail when `runtime` is `docker_compose` (batch 5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compose: Option<ComposePlanDetail>,
    /// Explicit user authorization to run a compose default that the P0 risk
    /// classifier would otherwise block (batch 8). The assistant can never set
    /// this — only a user action on a Host plan. Never stores user config content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_approval: Option<TradeApproval>,
}

/// The only modes that may relax the trade gate, both non-real-funds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TradeApproval {
    /// Run the engine's non-trading webserver mode.
    Webserver,
    /// Run dry-run — only allowed after the config projection proves dry_run.
    DryRun,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchPlanSource {
    Rule,
    User,
    Ai,
}

/// Docker Compose plan detail (batch 5). Paths are project-relative; absolute
/// resolution happens at start against the canonical project root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComposePlanDetail {
    /// Relative compose file (e.g. `docker-compose.yml`).
    pub compose_file: String,
    /// Seed for the unique Compose project name (`natives-{seed}-{suffix}`).
    pub project_seed: String,
    /// Optional service filter — only this service is started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Effective command override, when the compose default is not acceptable.
    #[serde(default)]
    pub command: Vec<String>,
    /// Health URL path on the resolved host port.
    pub health_path: String,
    /// Optional explicit host port override (else derived from inspect).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalLaunchRuntime {
    StaticHttp,
    NodeDevServer,
    /// Docker Compose project (batch 5). The compose file is resolved relative to
    /// the project root at start; the actual Compose project name is derived from
    /// `project_seed` + a unique suffix so two Natives apps never share a project.
    DockerCompose,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptRunner {
    Vite,
    VueCli,
    Node,
}

impl ScriptRunner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vite => "vite",
            Self::VueCli => "vue_cli",
            Self::Node => "node",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "vite" => Self::Vite,
            "vue_cli" | "vue-cli" | "vue_cli_service" => Self::VueCli,
            "node" => Self::Node,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchProgram {
    Internal,
    Npm,
    Pnpm,
    Yarn,
    Node,
}

impl LaunchProgram {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Node => "node",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "internal" => Self::Internal,
            "npm" => Self::Npm,
            "pnpm" => Self::Pnpm,
            "yarn" => Self::Yarn,
            "node" => Self::Node,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPort {
    pub mode: LaunchPortMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchPortMode {
    Auto,
    Fixed,
}

impl LaunchPlan {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    pub fn creative_runtime(&self) -> CreativeAppRuntime {
        match self.runtime {
            LocalLaunchRuntime::StaticHttp => CreativeAppRuntime::LocalStatic,
            LocalLaunchRuntime::NodeDevServer => CreativeAppRuntime::NodeDevServer,
            LocalLaunchRuntime::DockerCompose => CreativeAppRuntime::DockerCompose,
        }
    }
}

/// Process identity saved for orphan recovery (phase 2 uses full match).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcessIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_unix: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_group_id: Option<i32>,
}

/// DB row for local creative apps (source directory is reference-only).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCreativeAppRecord {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub canonical_project_root: String,
    pub device_id: String,
    pub device_name: String,
    pub project_kind: LocalProjectKind,
    pub launch_mode: LaunchMode,
    pub launch_plan_json: String,
    pub plan_fingerprint: String,
    pub state: CreativeAppState,
    pub status_detail_json: Option<String>,
    pub open_url: Option<String>,
    pub current_port: Option<u16>,
    pub process_identity_json: Option<String>,
    /// Stable volume identity (mount point) for the project root (batch 4).
    #[serde(default)]
    pub volume_identity: String,
    pub auto_open: bool,
    pub startup_timeout_ms: u32,
    pub last_started_at: Option<String>,
    pub last_exit_reason: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Result of scanning a local project directory (not persisted as state).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalProjectScanResult {
    pub project_root: String,
    pub project_kind: LocalProjectKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManager>,
    /// When multiple lockfiles exist, user must choose.
    #[serde(default)]
    pub package_manager_choices: Vec<PackageManager>,
    #[serde(default)]
    pub scripts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_script: Option<String>,
    pub has_node_modules: bool,
    pub dependencies_missing: bool,
    #[serde(default)]
    pub tool_versions: LocalToolVersions,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_plan: Option<LaunchPlan>,
    /// Non-web manifests detected (docker-compose, dockerfile, python, makefile).
    /// Evidence only — these runtimes are not yet available as drivers (batch 4).
    #[serde(default)]
    pub extra_manifests: Vec<String>,
    /// Relative tree sample (virtual root /project).
    #[serde(default)]
    pub tree_sample: Vec<String>,
    /// Existing local creative id if path already registered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalToolVersions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnpm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yarn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectLocalRequest {
    pub project_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLocalRequest {
    pub project_root: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub launch_mode: LaunchMode,
    /// Required when launch_mode is custom, or when overriding smart plan.
    #[serde(default)]
    pub launch_plan: Option<LaunchPlan>,
    #[serde(default)]
    pub env: Vec<EnvPair>,
    #[serde(default)]
    pub auto_open: Option<bool>,
    #[serde(default)]
    pub startup_timeout_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLocalRequest {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub launch_mode: Option<LaunchMode>,
    #[serde(default)]
    pub launch_plan: Option<LaunchPlan>,
    /// Legacy full replace of env map (prefer env_upsert / env_remove_keys).
    #[serde(default)]
    pub env: Option<Vec<EnvPair>>,
    #[serde(default)]
    pub env_upsert: Option<Vec<EnvPair>>,
    #[serde(default)]
    pub env_remove_keys: Option<Vec<String>>,
    #[serde(default)]
    pub auto_open: Option<bool>,
    #[serde(default)]
    pub startup_timeout_ms: Option<u32>,
    /// Changing directory requires rescan confirmation on the client.
    #[serde(default)]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCreativeConfig {
    pub summary: CreativeAppSummary,
    pub launch_plan: LaunchPlan,
    pub env_keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_install: Option<DependencyInstallPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInstallPreview {
    pub program: String,
    pub args: Vec<String>,
    pub package_manager: PackageManager,
    pub requires_confirmation: bool,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalScriptCandidate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner: Option<ScriptRunner>,
    pub executable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// DB row for external creative apps.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalCreativeAppRecord {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub version: String,
    pub owner: String,
    pub repo: String,
    pub repository_url: String,
    pub release_tag: String,
    pub release_id: Option<i64>,
    pub runtime: CreativeAppRuntime,
    pub state: CreativeAppState,
    pub open_url: Option<String>,
    pub health_url: Option<String>,
    pub host_port: Option<u16>,
    pub runtime_config_json: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Runtime-specific config stored as validated JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeConfig {
    DockerCompose {
        project_name: String,
        compose_file: String,
        service: String,
        container_port: u16,
        host_port: u16,
        open_path: String,
        health_path: Option<String>,
        env_keys: Vec<String>,
    },
    DockerRun {
        container_name: String,
        image: String,
        container_port: u16,
        host_port: u16,
        open_path: String,
        health_path: Option<String>,
        env_keys: Vec<String>,
    },
}

impl RuntimeConfig {
    pub fn host_port(&self) -> u16 {
        match self {
            Self::DockerCompose { host_port, .. } => *host_port,
            Self::DockerRun { host_port, .. } => *host_port,
        }
    }

    pub fn open_path(&self) -> &str {
        match self {
            Self::DockerCompose { open_path, .. } => open_path,
            Self::DockerRun { open_path, .. } => open_path,
        }
    }

    pub fn health_path(&self) -> Option<&str> {
        match self {
            Self::DockerCompose { health_path, .. } => health_path.as_deref(),
            Self::DockerRun { health_path, .. } => health_path.as_deref(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Options for deleting an external app.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOptions {
    /// Remove app-owned named volumes (default false).
    #[serde(default)]
    pub remove_volumes: bool,
    /// Remove images no longer referenced by other containers (default false).
    #[serde(default)]
    pub remove_images: bool,
}

/// Delete result with optional non-fatal warnings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub ok: bool,
    pub warnings: Vec<String>,
}

/// Result of a lifecycle mutation that keeps the app: the journaled operation
/// id plus the current summary projection. The summary stays in the response so
/// existing consumers keep working during the one-version compatibility window
/// (batch 2 CR-201).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub operation_id: i64,
    pub summary: CreativeAppSummary,
}

/// Result of a delete mutation: the journaled operation id plus the delete
/// outcome (the app is gone, so there is no summary to project).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMutationResult {
    pub operation_id: i64,
    pub result: DeleteResult,
}

/// Open target for list "open" action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenTarget {
    WorkshopModule { module_id: String },
    LocalUrl { url: String, app_id: String },
}

/// Discrete install/start progress stages (no fake percentages).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    InspectingRelease,
    DownloadingAssets,
    PullingImage,
    Creating,
    Starting,
    HealthCheck,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub app_id: String,
    pub stage: ProgressStage,
    pub message: String,
}

/// GitHub inspect request (strong types only).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectGithubRequest {
    pub repository_url: String,
    /// Optional one-shot token (not persisted unless saveToken).
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub save_token: bool,
    /// When true, inspect latest stable only (one-click path).
    #[serde(default = "default_true")]
    pub one_click: bool,
    /// Manual: specific tag (including prerelease).
    #[serde(default)]
    pub release_tag: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Install request after user confirms a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallGithubRequest {
    pub repository_url: String,
    pub release_tag: String,
    pub release_id: Option<i64>,
    pub candidate_id: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub host_port: Option<u16>,
    #[serde(default)]
    pub open_path: Option<String>,
    #[serde(default)]
    pub health_path: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    /// User-provided env values for this install (plaintext once; encrypted at rest).
    #[serde(default)]
    pub env: Vec<EnvPair>,
    /// Explicit confirmation for absolute bind mounts in manual mode.
    #[serde(default)]
    pub confirm_bind_mounts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvPair {
    pub key: String,
    pub value: String,
}

/// Result of inspecting a GitHub repository for install candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectGithubResult {
    pub repository_url: String,
    pub owner: String,
    pub repo: String,
    pub release_tag: String,
    pub release_id: Option<i64>,
    pub is_prerelease: bool,
    pub candidates: Vec<InstallCandidate>,
    pub one_click_eligible: bool,
    pub one_click_candidate_id: Option<String>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    /// Available tags for manual mode (non-draft).
    #[serde(default)]
    pub available_tags: Vec<ReleaseTagInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseTagInfo {
    pub tag: String,
    pub release_id: i64,
    pub is_prerelease: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCandidate {
    pub id: String,
    pub runtime: CreativeAppRuntime,
    pub confidence: u8,
    pub title: String,
    pub description: String,
    /// Compose asset name or image ref.
    pub primary_asset: String,
    pub service: Option<String>,
    pub image: Option<String>,
    pub suggested_host_port: Option<u16>,
    pub container_port: Option<u16>,
    pub open_path: String,
    pub health_path: Option<String>,
    pub env_requirements: Vec<EnvRequirement>,
    pub risk_summary: Vec<String>,
    pub hard_blockers: Vec<String>,
    pub requires_manual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvRequirement {
    pub key: String,
    pub required: bool,
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubTokenStatus {
    pub configured: bool,
    pub masked: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerEngineStatus {
    pub available: bool,
    pub version: Option<String>,
    pub compose_available: bool,
    pub compose_version: Option<String>,
    pub error: Option<String>,
}

// ── Surface / Endpoint / Window (batch 5 CR-501) ──────────────────────

/// A surface is a visual presentation of an application — either the main
/// window surface or an embed surface (child WebView). Each application has
/// exactly one main surface; additional embed surfaces are optional.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSurface {
    pub id: String,
    pub application_id: String,
    /// `main` for the primary surface, `embed` for child WebView surfaces.
    pub kind: String,
    /// Human-readable label (e.g. "Main Window", "Preview").
    pub label: String,
    /// Optional title overridden by the surface content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// URL the surface is currently showing (or should show).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A runtime endpoint is a network address where a runtime instance serves
/// content. Each runtime instance can have multiple endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEndpoint {
    pub id: String,
    pub runtime_instance_id: String,
    /// `preview` for the primary preview URL, `api` for backend API, `health`.
    pub kind: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub created_at: String,
    pub updated_at: String,
}

/// A window instance is a Tauri WebView (or WebviewWindow) that displays
/// a surface. Each window maps 1:1 to a Tauri WebView object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowInstance {
    pub id: String,
    pub application_id: String,
    pub surface_id: String,
    /// The runtime instance this window is bound to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_instance_id: Option<String>,
    /// Tauri WebView label, e.g. "creative-app-my-app".
    pub label: String,
    /// Window state: open, minimized, closed, background.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl WindowInstance {
    pub const STATE_OPEN: &'static str = "open";
    pub const STATE_MINIMIZED: &'static str = "minimized";
    pub const STATE_CLOSED: &'static str = "closed";
    pub const STATE_BACKGROUND: &'static str = "background";
}

// ── BrowserProfile (batch 6 CR-601) ──────────────────────────────────

/// A browser profile is a named session isolation boundary. Profiles store
/// metadata only — never cookie content, which is managed by the platform's
/// WebKit data store. On macOS 26.5, all WKWebView instances share the same
/// data store, so per-profile isolation is not available (see ADR-0017).
/// Profiles are recorded for future use when platform isolation becomes
/// possible, and for audit / clear operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    pub id: String,
    /// Human-readable name (e.g. "Default", "Work").
    pub name: String,
    /// Platform-specific data store key (e.g. WKWebsiteDataStore identifier).
    /// Never stores cookie content.
    pub platform_store_key: String,
    /// Whether this is the default profile for new apps.
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

// ── OAuth allowlist (batch 6 CR-602) ─────────────────────────────────

/// Per-app domain allowlist entry for OAuth popup windows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAllowlistEntry {
    pub id: String,
    pub application_id: String,
    /// Allowed domain (e.g. "accounts.google.com").
    pub domain: String,
    pub created_at: String,
}

// ── App grants (batch 6 CR-603) ──────────────────────────────────────

/// Per-app capability grants for upload, download, clipboard, and window.open.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppGrant {
    pub id: String,
    pub application_id: String,
    /// Grant kind: "upload", "download", "clipboard", "window_open".
    pub kind: String,
    /// "default_deny", "one_time", "persistent".
    pub policy: String,
    /// Optional path restriction (for upload/download).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl AppGrant {
    pub const KIND_UPLOAD: &'static str = "upload";
    pub const KIND_DOWNLOAD: &'static str = "download";
    pub const KIND_CLIPBOARD: &'static str = "clipboard";
    pub const KIND_WINDOW_OPEN: &'static str = "window_open";
    pub const POLICY_DEFAULT_DENY: &'static str = "default_deny";
    pub const POLICY_ONE_TIME: &'static str = "one_time";
    pub const POLICY_PERSISTENT: &'static str = "persistent";
}

// ── Service Instance (batch 7 CR-701) ────────────────────────────────

/// A service instance is a single running unit inside a runtime instance.
/// A Compose runtime can expose multiple services; other runtimes have exactly
/// one service (the "main" service).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstance {
    pub id: String,
    pub runtime_instance_id: String,
    /// Service name within the runtime (e.g. "web", "db", "main").
    pub name: String,
    /// Readiness state: "starting", "ready", "unhealthy", "degraded", "stopped".
    pub readiness: String,
    /// Whether this service is required for the app to be considered healthy.
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ServiceInstance {
    pub const READY_STARTING: &'static str = "starting";
    pub const READY_READY: &'static str = "ready";
    pub const READY_UNHEALTHY: &'static str = "unhealthy";
    pub const READY_DEGRADED: &'static str = "degraded";
    pub const READY_STOPPED: &'static str = "stopped";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        for s in [
            CreativeAppState::Available,
            CreativeAppState::InstallFailed,
            CreativeAppState::RuntimeUnavailable,
            CreativeAppState::CleanupFailed,
            CreativeAppState::Orphaned,
        ] {
            assert_eq!(CreativeAppState::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn actions_running_external_can_open() {
        let a = CreativeAppActions::for_state(
            CreativeAppSource::ExternalGithub,
            CreativeAppState::Running,
        );
        assert!(a.can_open && a.can_stop && !a.can_start);
    }

    #[test]
    fn runtime_config_json_roundtrip() {
        let cfg = RuntimeConfig::DockerRun {
            container_name: "natives-ca-x".into(),
            image: "nginx:alpine".into(),
            container_port: 80,
            host_port: 18080,
            open_path: "/".into(),
            health_path: Some("/health".into()),
            env_keys: vec!["TOKEN".into()],
        };
        let j = cfg.to_json().unwrap();
        let back = RuntimeConfig::from_json(&j).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn local_project_actions_and_launch_plan_roundtrip() {
        let a = CreativeAppActions::for_state(
            CreativeAppSource::LocalProject,
            CreativeAppState::InstalledStopped,
        );
        assert!(a.can_start && a.can_delete && !a.can_open);

        let plan = LaunchPlan {
            schema_version: 1,
            source: LaunchPlanSource::Rule,
            project_kind: LocalProjectKind::Html,
            runtime: LocalLaunchRuntime::StaticHttp,
            program: LaunchProgram::Internal,
            cwd_relative: ".".into(),
            script: None,
            entry_file: Some("index.html".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            auto_open: true,
            confidence: Some(0.9),
            reason: "index.html present".into(),
            compose: None,
            trade_approval: None,
        };
        let j = plan.to_json().unwrap();
        let back = LaunchPlan::from_json(&j).unwrap();
        assert_eq!(plan, back);
        assert_eq!(back.creative_runtime(), CreativeAppRuntime::LocalStatic);
    }

    #[test]
    fn local_issue_code_roundtrip() {
        assert_eq!(
            LocalCreativeIssueCode::parse("dependencies_missing"),
            Some(LocalCreativeIssueCode::DependenciesMissing)
        );
        assert_eq!(
            LocalCreativeIssueCode::OrphanedProcess.as_str(),
            "orphaned_process"
        );
        assert_eq!(
            LocalCreativeIssueCode::parse("stop_failed"),
            Some(LocalCreativeIssueCode::StopFailed)
        );
    }
}
