//! External app / surface / window / grant / process DTOs (W3 split from
//! `model.rs`). Kept as a sibling module so `model` can re-export everything
//! for backwards compatibility.

use serde::{Deserialize, Serialize};

use super::model::{
    CreativeAppActions, CreativeAppRuntime, CreativeAppSource, CreativeAppState,
    CreativeAppSummary, LaunchPort,
};

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
/// a surface. Each window maps 1:1 to a Tauri WebView object; the WebView
/// label is derived from the window id (`creative-window-{id}`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowInstance {
    pub id: String,
    pub application_id: String,
    pub surface_id: String,
    /// The runtime instance this window is bound to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_instance_id: Option<String>,
    /// Tauri WebView label, e.g. "creative-window-{windowId}".
    pub label: String,
    /// Window state: open, minimized, closed, background.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds_json: Option<String>,
    /// Content URL the window is currently showing (NULL when closed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Reconcile/operation detail so the UI can show why a window is closed
    /// (honest state — never a fabricated value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Reconcile outcome: "ok" | "missing" | "orphaned".
    #[serde(default)]
    pub reconcile_state: String,
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

/// A grant lifecycle event (set / consumed / revoked) for the permission
/// history UI. `policy`/`path` snapshot the values at the time of the event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrantEvent {
    pub id: String,
    pub application_id: String,
    pub kind: String,
    /// "set" | "consumed" | "revoked"
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub created_at: String,
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

// ── Python WebUI profile (batch 8 CR-801) ────────────────────────────

/// Versioned Python WebUI launch profile. Never stores secret values — only
/// interpreter reference, module/args, cwd, and env KEY names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PythonLaunchProfile {
    pub schema_version: u32,
    /// Interpreter reference: absolute venv path or system python name.
    /// Examples: "/project/.venv/bin/python", "python3".
    pub interpreter: String,
    /// Entry module or script (relative to cwd). Examples: "app.py", "main.py".
    pub entry: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory relative to the project root (default ".").
    #[serde(default)]
    pub cwd_relative: String,
    /// Env KEY names only — values never stored here.
    #[serde(default)]
    pub environment_keys: Vec<String>,
    pub port: LaunchPort,
    pub open_path: String,
    pub health_path: String,
    pub startup_timeout_ms: u32,
    /// Whether the interpreter is a venv (true) or system python (false).
    pub is_venv: bool,
}

/// Result of detecting a Python WebUI candidate in a project scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PythonScanCandidate {
    /// Relative entry script (e.g. "app.py").
    pub entry: String,
    /// Detected interpreter hint (venv path or system).
    pub interpreter_hint: String,
    /// Relative project root where the app lives (subdirectory or ".").
    pub project_root_relative: String,
    #[serde(default)]
    pub risks: Vec<String>,
    /// Whether the profile is fully specified (no missing env requirements).
    pub complete: bool,
}

// ── Binary WebUI profile (batch 8 CR-802) ────────────────────────────

/// Versioned Binary WebUI launch profile. The executable is identified by its
/// canonical absolute path and content hash — a hash change requires re-approval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BinaryLaunchProfile {
    pub schema_version: u32,
    /// Canonical absolute path to the executable.
    pub executable_path: String,
    /// SHA-256 of the executable content at approval time.
    pub executable_hash: String,
    /// Whether the user approved this exact executable (hash matched).
    pub approved: bool,
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory relative to the project root.
    #[serde(default)]
    pub cwd_relative: String,
    #[serde(default)]
    pub environment_keys: Vec<String>,
    pub port: LaunchPort,
    pub open_path: String,
    pub health_path: String,
    pub startup_timeout_ms: u32,
}

/// Result of detecting a Binary WebUI candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BinaryScanCandidate {
    /// Executable path relative to the project root.
    pub executable_relative: String,
    /// Whether the file is executable (has exec permission).
    pub executable: bool,
    /// Whether a prior approval record matches this file's hash.
    pub previously_approved: bool,
    #[serde(default)]
    pub risks: Vec<String>,
}

// ── Non-owned apps (batch 9 CR-901/902) ──────────────────────────────

/// Ownership mode for a creative app — "managed" (Natives owns resources) or
/// "attached"/"remote" (Natives can inspect/open but does not own stop/kill).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipMode {
    /// Natives fully owns the runtime resources (local process, Docker, HTTP).
    Managed,
    /// A local service Natives did not start; inspect/open only, no stop/kill.
    Attached,
    /// A remote URL; approved origins only, never Tauri capability.
    Remote,
}

impl OwnershipMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Attached => "attached",
            Self::Remote => "remote",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "managed" => Self::Managed,
            "attached" => Self::Attached,
            "remote" => Self::Remote,
            _ => return None,
        })
    }
}

/// A non-owned app record: a URL/origin Natives can inspect and open, but whose
/// lifecycle Natives does not own (attached local service or remote web app).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NonOwnedApp {
    pub id: String,
    pub ownership: OwnershipMode,
    /// Base URL / origin to open (validated to loopback for attached).
    pub url: String,
    /// Approved origins for remote navigation (empty for attached).
    #[serde(default)]
    pub approved_origins: Vec<String>,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Catalog projection for a non-owned app (T09). Carries the record plus an
/// honest action matrix — `can_open` / `can_delete` only, never start/stop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NonOwnedAppSummary {
    #[serde(flatten)]
    pub app: NonOwnedApp,
    pub actions: CreativeAppActions,
}

impl NonOwnedAppSummary {
    pub fn project(app: NonOwnedApp) -> Self {
        let actions = CreativeAppActions::for_non_owned(app.ownership);
        Self { app, actions }
    }
}

/// Probe result for a non-owned app — does the origin currently respond?
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NonOwnedProbe {
    pub reachable: bool,
    /// HTTP status code when reachable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// True when the origin does not answer (service disappeared).
    pub unreachable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
