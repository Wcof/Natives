//! Public DTO and domain enums for Creative Apps.

use serde::{Deserialize, Serialize};

/// Unified read-only list projection for Personal Creations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppSummary {
    pub id: String,
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
    pub actions: CreativeAppActions,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeAppSource {
    Internal,
    ExternalGithub,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeAppRuntime {
    WorkshopStatic,
    DockerCompose,
    DockerRun,
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
            CreativeAppSource::ExternalGithub => match state {
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
                    can_start: false,
                    can_stop: true,
                    can_delete: true,
                    can_retry: true,
                },
                CreativeAppState::RuntimeUnavailable => Self {
                    can_open: false,
                    can_start: false,
                    can_stop: false,
                    can_delete: false, // need docker for full delete
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
}
