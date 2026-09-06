//! Authoritative types for the App Store registry (ADR-0025 D13).
//!
//! The App Store owns exactly one Apps Domain model:
//! `App` / `RuntimeSpec` (json) / `Surface` (json) / `PackageSpec`
//! (`PackageMeta`). It never models fund business entities (ADR-0025 D15:
//! Core knows `app_id = fund` exists, not what fund means).

use serde::{Deserialize, Serialize};

/// V1 only allows `extension_app` (ADR-0025 D13).
pub const KIND_EXTENSION_APP: &str = "extension_app";

/// Maximum app_id length (catalog-derived, Core-validated).
pub const APP_ID_MAX_LEN: usize = 64;

/// Complete App Registry row (`apps` table).
#[derive(Serialize)]
pub struct App {
    pub app_id: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub show_in_sidebar: bool,
    pub sidebar_order: i64,
    pub runtime_spec_json: String,
    pub surface_json: String,
    pub manifest_json: String,
    pub installed_at: i64,
    pub updated_at: i64,
    pub revision: i64,
}

/// Installed package receipt row (`app_packages` table, ADR-0025 D7/D13).
#[derive(Serialize)]
pub struct AppPackage {
    pub app_id: String,
    pub package_id: String,
    pub kind: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub wire_size: i64,
    pub payload_size: i64,
    pub artifact_sha256: String,
    pub payload_sha256: String,
    pub installed_path: String,
    pub installed_at: i64,
}

/// Permission grant row (`app_permissions` table).
#[derive(Serialize)]
pub struct AppPermission {
    pub app_id: String,
    pub permission: String,
    pub granted: bool,
    pub granted_at: i64,
}

/// Install lifecycle states (ADR-0025 D11).
pub mod install_state {
    pub const CATALOG_RESOLVED: &str = "catalog_resolved";
    pub const DOWNLOADING: &str = "downloading";
    pub const ARTIFACT_VERIFIED: &str = "artifact_verified";
    pub const DECOMPRESSING: &str = "decompressing";
    pub const PAYLOAD_VERIFIED: &str = "payload_verified";
    pub const STAGING: &str = "staging";
    pub const RUNTIME_REGISTERING: &str = "runtime_registering";
    pub const HEALTH_CHECK: &str = "health_check";
    pub const COMMITTING: &str = "committing";
    pub const INSTALLED: &str = "installed";
    pub const ROLLING_BACK: &str = "rolling_back";
    pub const FAILED: &str = "failed";

    /// Canonical forward chain (ADR-0025 D11).
    pub const FORWARD: &[&str] = &[
        CATALOG_RESOLVED,
        DOWNLOADING,
        ARTIFACT_VERIFIED,
        DECOMPRESSING,
        PAYLOAD_VERIFIED,
        STAGING,
        RUNTIME_REGISTERING,
        HEALTH_CHECK,
        COMMITTING,
        INSTALLED,
    ];
}

/// Install transaction row (`app_install_transactions` table).
#[derive(Serialize)]
pub struct InstallTransaction {
    pub install_id: String,
    pub app_id: String,
    pub from_version: String,
    pub to_version: String,
    /// Catalog-resolved request snapshot; `apps:install_commit` is
    /// self-contained from this column (ADR-0025 D11).
    pub request_json: String,
    pub state: String,
    pub staging_path: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

/// Payload of `apps:install_begin`. A catalog entry only becomes an App
/// after `apps:install_commit` (ADR-0025 D11/D42). Package bytes are not
/// part of this call; they arrive via `apps:install_package` (Phase A5).
#[derive(Serialize, Deserialize)]
pub struct InstallRequest {
    pub app: AppMeta,
    #[serde(default)]
    pub packages: Vec<PackageMeta>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// Catalog-derived app metadata.
#[derive(Clone, Serialize, Deserialize)]
pub struct AppMeta {
    pub app_id: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub show_in_sidebar: bool,
    #[serde(default)]
    pub sidebar_order: i64,
    #[serde(default)]
    pub runtime_spec: serde_json::Value,
    #[serde(default)]
    pub surface: serde_json::Value,
    #[serde(default)]
    pub manifest: serde_json::Value,
}

impl AppMeta {
    /// Core-side validation before anything is persisted (ADR-0025 D13:
    /// `kind` V1 only allows `extension_app`; catalog must not smuggle
    /// paths or scripts).
    pub fn validate(&self) -> Result<(), AppError> {
        if self.app_id.is_empty() || self.app_id.len() > APP_ID_MAX_LEN {
            return Err(AppError::InvalidState(format!(
                "invalid app_id: {:?}",
                &self.app_id
            )));
        }
        if !self.app_id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'_'
        }) {
            return Err(AppError::InvalidState(format!(
                "app_id must be [a-z0-9._-]: {:?}",
                self.app_id
            )));
        }
        if self.kind != KIND_EXTENSION_APP {
            return Err(AppError::InvalidState(format!(
                "kind {} not allowed in V1 (only {})",
                self.kind, KIND_EXTENSION_APP
            )));
        }
        if self.name.trim().is_empty() {
            return Err(AppError::InvalidState("app name must not be empty".into()));
        }
        if self.version.trim().is_empty() {
            return Err(AppError::InvalidState(
                "app version must not be empty".into(),
            ));
        }
        Ok(())
    }
}

/// Catalog-derived package descriptor (ADR-0025 D7). Describes the target;
/// the install path is decided by Core from `kind`, never by the catalog.
#[derive(Serialize, Deserialize)]
pub struct PackageMeta {
    pub package_id: String,
    pub kind: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub wire_size: i64,
    pub payload_size: i64,
    pub artifact_sha256: String,
    pub payload_sha256: String,
}

// NOTE (ADR-0025 D8): the commit-time package receipt (PackageReceipt) is
// introduced together with real package handling in Phase A5; Phase A2
// persists receipts directly from the validated PackageMeta.

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    InvalidState(String),
    Conflict(String),
    Sql(rusqlite::Error),
    Io(std::io::Error),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "APP_NOT_FOUND",
            Self::InvalidState(_) => "APP_INVALID_STATE",
            Self::Conflict(_) => "APP_CONFLICT",
            Self::Sql(_) => "APP_INTERNAL",
            Self::Io(_) => "APP_IO",
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(target) => write!(f, "app not found: {target}"),
            Self::InvalidState(message) => write!(f, "invalid app state: {message}"),
            Self::Conflict(message) => write!(f, "conflicting app state: {message}"),
            Self::Sql(error) => write!(f, "sql error: {error}"),
            Self::Io(error) => write!(f, "io error: {error}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound("row not found".into()),
            other => Self::Sql(other),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
