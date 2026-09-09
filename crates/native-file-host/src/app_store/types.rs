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

// ── ADR-0025 D7 budget constants (binary units, 1 MiB = 1,048,576 B) ──

/// Single package wire (download) hard limit: 5 MiB.
pub const PACKAGE_MAX_WIRE_BYTES: u64 = 5 * 1024 * 1024;

/// Single package decompressed payload hard limit: 20 MiB (decompression
/// bomb guard — the browser enforces it while decompressing, the Host
/// re-checks the staged bytes).
pub const PACKAGE_MAX_PAYLOAD_BYTES: u64 = 20 * 1024 * 1024;

/// Maximum number of required packages per app: 3 (no infinite
/// sub-packaging around the 5 MiB gate).
pub const REQUIRED_MAX_PACKAGES: u64 = 3;

/// Total wire size of the required package set: 15 MiB.
pub const REQUIRED_MAX_TOTAL_WIRE_BYTES: u64 = 15 * 1024 * 1024;

/// Total package count per app (required + optional): 16.
pub const MAX_TOTAL_PACKAGES: u64 = 16;
pub const APP_MAX_INSTALLED_BYTES: u64 = 50 * 1024 * 1024;

pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    }
}

pub fn architecture() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
}

/// Maximum base64 length of one `apps:install_package` payload parameter
/// (base64 of a ≤20 MiB payload, plus margin): 28 MiB.
pub const PACKAGE_DATA_MAX_BASE64_BYTES: usize = 28 * 1024 * 1024;

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
    /// Legacy migration-only field. V2 resource installs always leave it false.
    pub host_registered: bool,
    pub recovery_pending: bool,
    pub needs_migration: bool,
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    pub app: AppMeta,
    #[serde(default)]
    pub packages: Vec<PackageMeta>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub min_host_version: Option<String>,
}

/// Result of `apps:read_resource` (ADR-0026 D4).
#[derive(Serialize)]
pub struct ReadResourceResult {
    pub ok: bool,
    pub app_id: String,
    pub package_id: String,
    pub version: String,
    pub format: String,
    pub total_size: u64,
    pub offset: u64,
    pub length: usize,
    pub data: String,
}

impl InstallRequest {
    pub fn validate(&self) -> Result<(), AppError> {
        self.app.validate()?;
        if let Some(min_host) = &self.min_host_version {
            let req_ver = semver::Version::parse(min_host)
                .map_err(|_| AppError::InvalidState("invalid min_host_version".into()))?;
            let current_host = semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .map_err(|_| AppError::InvalidState("invalid host version".into()))?;
            if current_host < req_ver {
                return Err(AppError::InvalidState(
                    "host version below required min_host_version".into(),
                ));
            }
        }
        if self.packages.len() as u64 > MAX_TOTAL_PACKAGES {
            return Err(AppError::InvalidState("invalid package count".into()));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut required = 0;
        let mut wire = 0;
        let mut payload = 0;
        for package in &self.packages {
            crate::app_install::validate_identifier(&package.package_id, "package id")?;
            if !ids.insert(&package.package_id) || package.version != self.app.version {
                return Err(AppError::InvalidState(
                    "duplicate package or inconsistent version".into(),
                ));
            }
            if package.wire_size <= 0
                || package.wire_size as u64 > PACKAGE_MAX_WIRE_BYTES
                || package.payload_size <= 0
                || package.payload_size as u64 > PACKAGE_MAX_PAYLOAD_BYTES
            {
                return Err(AppError::InvalidState("package size outside budget".into()));
            }
            for hash in [&package.artifact_sha256, &package.payload_sha256] {
                if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(AppError::InvalidState("invalid package digest".into()));
                }
            }
            match package.kind.as_str() {
                "runtime" => {
                    return Err(AppError::InvalidState(
                        "runtime packages are deprecated; apps share native-file-host".into(),
                    ));
                }
                "data" | "resource"
                    if (package.platform == "any" || package.platform == platform())
                        && (package.arch == "any" || package.arch == architecture()) => {}
                _ => {
                    return Err(AppError::InvalidState(
                        "unsupported package kind or platform".into(),
                    ))
                }
            }
            if package.required {
                required += 1;
                wire += package.wire_size as u64;
            }
            payload += package.payload_size as u64;
        }
        if required > REQUIRED_MAX_PACKAGES
            || wire > REQUIRED_MAX_TOTAL_WIRE_BYTES
            || payload > APP_MAX_INSTALLED_BYTES
        {
            return Err(AppError::InvalidState(
                "app package set outside budget".into(),
            ));
        }
        let namespace = self.app.namespace();
        if self.permissions.len() > 16
            || self.permissions.iter().any(|permission| {
                permission != "app.lifecycle" && permission != &format!("keychain:{namespace}")
            })
        {
            return Err(AppError::InvalidState("unsupported app permission".into()));
        }
        Ok(())
    }
}

/// Per-package staged state, one row per `app_package_stages` entry
/// (ADR-0025 D11: download/verify/stage states are tracked per package;
/// the transaction row tracks the app-level chain).
#[derive(Serialize)]
pub struct PackageStage {
    pub install_id: String,
    pub package_id: String,
    pub state: String,
    pub staged_path: String,
    pub payload_size: i64,
    pub payload_sha256: String,
}

/// Result of `apps:install_package`: the package advanced to
/// `staged`, or the transaction failed with a code the UI surfaces.
#[derive(Serialize)]
pub struct InstallPackageResult {
    pub install_id: String,
    pub package_id: String,
    pub state: String,
    pub payload_size: u64,
    pub payload_sha256: String,
    /// True when the package set is complete and the transaction is
    /// ready for `apps:install_commit`.
    pub ready: bool,
}

/// Catalog-derived app metadata.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub fn namespace(&self) -> String {
        if self.app_id.starts_with("com.natives.app.") {
            self.app_id.clone()
        } else {
            format!("com.natives.app.{}", self.app_id)
        }
    }
    /// Core-side validation before anything is persisted (ADR-0025 D13:
    /// `kind` V1 only allows `extension_app`; catalog must not smuggle
    /// paths or scripts).
    pub fn validate(&self) -> Result<(), AppError> {
        crate::app_install::validate_identifier(&self.app_id, "app id")?;
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
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(AppError::InvalidState("app name must not be empty".into()));
        }
        crate::app_install::validate_identifier(&self.version, "app version")?;
        semver::Version::parse(&self.version)
            .map_err(|_| AppError::InvalidState("app version must be SemVer".into()))?;
        for (value, allowed) in [
            (&self.runtime_spec, &["version"][..]),
            (&self.surface, &["route", "icon"][..]),
            (&self.manifest, &["permissions", "schemaVersion"][..]),
        ] {
            let object = value
                .as_object()
                .ok_or_else(|| AppError::InvalidState("app metadata must be an object".into()))?;
            if object.keys().any(|key| !allowed.contains(&key.as_str())) {
                return Err(AppError::InvalidState(
                    "unsupported app metadata field".into(),
                ));
            }
        }
        if self
            .surface
            .get("route")
            .and_then(serde_json::Value::as_str)
            != Some(format!("app.html?app={}", self.app_id).as_str())
        {
            return Err(AppError::InvalidState(
                "surface route must belong to the app".into(),
            ));
        }
        if self
            .runtime_spec
            .get("version")
            .is_some_and(|version| version.as_str() != Some(self.version.as_str()))
        {
            return Err(AppError::InvalidState("runtime version mismatch".into()));
        }
        Ok(())
    }
}

/// Catalog-derived package descriptor (ADR-0025 D7). Describes the target;
/// the install path is decided by Core from `kind`, never by the catalog.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(default = "required_by_default")]
    pub required: bool,
}

fn required_by_default() -> bool {
    true
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
