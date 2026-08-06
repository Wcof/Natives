//! RuntimeDriver contract (batch 7 CR-702, T09 closure).
//!
//! Every runtime driver implements one lifecycle: prepare / start / probe /
//! stop / delete / reconcile, plus an honest `capabilities()` descriptor.
//! Existing drivers (Workshop Static, Local Static, Node Dev, Python, Binary,
//! Docker Compose, Docker Run) implement this contract through the facade
//! registry (`adapters/facade.rs`); Attached / Remote are non-owned and expose
//! only probe/open/delete (never start/stop) — see `non_owned.rs`.
//!
//! A driver that cannot perform an operation must return `unsupported` rather
//! than advertise a fake capability (禁止假广告).

use super::model::{CreativeAppRuntime, CreativeAppSummary, DeleteOptions, DeleteResult};
use super::runtime_store::LaunchProfile;
use crate::creative_app::adapters::LifecycleCtx;
use crate::{Error, Result};
use async_trait::async_trait;
use rusqlite::Connection;
use tauri::AppHandle;

/// Driver kind identifiers (stable strings, persisted in plan/summary).
pub const DRIVER_WORKSHOP_STATIC: &str = "workshop_static";
pub const DRIVER_LOCAL_STATIC: &str = "local_static";
pub const DRIVER_NODE_DEV: &str = "node_dev_server";
pub const DRIVER_PYTHON: &str = "python_webui";
pub const DRIVER_BINARY: &str = "binary_webui";
pub const DRIVER_DOCKER_COMPOSE: &str = "docker_compose";
pub const DRIVER_DOCKER_RUN: &str = "docker_run";
pub const DRIVER_ATTACHED: &str = "attached";
pub const DRIVER_REMOTE: &str = "remote";

/// Map a runtime to its driver kind (stable string).
pub fn driver_kind_for_runtime(runtime: CreativeAppRuntime) -> &'static str {
    match runtime {
        CreativeAppRuntime::WorkshopStatic => DRIVER_WORKSHOP_STATIC,
        CreativeAppRuntime::LocalStatic => DRIVER_LOCAL_STATIC,
        CreativeAppRuntime::NodeDevServer => DRIVER_NODE_DEV,
        CreativeAppRuntime::DockerCompose => DRIVER_DOCKER_COMPOSE,
        CreativeAppRuntime::DockerRun => DRIVER_DOCKER_RUN,
    }
}

/// Driver capability descriptor — what a driver can and cannot do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverCapabilities {
    /// Whether start creates a long-lived process.
    pub supports_start: bool,
    /// Whether stop can verify resource release.
    pub supports_verified_stop: bool,
    /// Whether reconcile (find orphans) is supported.
    pub supports_reconcile: bool,
    /// Whether the driver performs a prepare step (port lease / identity).
    pub supports_prepare: bool,
}

impl DriverCapabilities {
    pub const LOCAL_PROCESS: Self = Self {
        supports_start: true,
        supports_verified_stop: true,
        supports_reconcile: true,
        supports_prepare: true,
    };
    pub const HOST_HTTP: Self = Self {
        supports_start: true,
        supports_verified_stop: true,
        supports_reconcile: false,
        supports_prepare: true,
    };
    pub const DOCKER: Self = Self {
        supports_start: true,
        supports_verified_stop: true,
        supports_reconcile: true,
        supports_prepare: true,
    };
    pub const WORKSHOP: Self = Self {
        supports_start: false, // enable/disable only — no long-lived process
        supports_verified_stop: false,
        supports_reconcile: false,
        supports_prepare: false,
    };
    pub const NON_OWNED: Self = Self {
        supports_start: false,
        supports_verified_stop: false,
        supports_reconcile: false,
        supports_prepare: false,
    };
}

/// Resolve driver capabilities from a runtime kind.
pub fn capabilities_for(runtime: CreativeAppRuntime) -> DriverCapabilities {
    match runtime {
        CreativeAppRuntime::WorkshopStatic => DriverCapabilities::WORKSHOP,
        CreativeAppRuntime::LocalStatic => DriverCapabilities::HOST_HTTP,
        CreativeAppRuntime::NodeDevServer => DriverCapabilities::LOCAL_PROCESS,
        CreativeAppRuntime::DockerCompose | CreativeAppRuntime::DockerRun => {
            DriverCapabilities::DOCKER
        }
    }
}

/// Resolve a driver kind from a plan's `driver_kind` column, falling back to
/// the runtime mapping for legacy plans (CR-702 compatibility). The plan JSON
/// is inspected for a managed-process profile (python/binary) because those are
/// carried under the `node_dev_server` runtime family.
pub fn resolve_driver_kind(runtime: CreativeAppRuntime, plan: Option<&LaunchProfile>) -> String {
    if let Some(p) = plan {
        if !p.driver_kind.is_empty() {
            // A stored driver kind wins, but a python/binary process profile
            // carried under a generic kind is upgraded to its precise kind.
            if let Some(precise) = process_profile_kind(&p.plan_json) {
                return precise.to_string();
            }
            return p.driver_kind.clone();
        }
    }
    driver_kind_for_runtime(runtime).to_string()
}

/// Detect a python/binary managed-process profile inside a plan JSON and return
/// its precise driver kind, if any.
pub fn process_profile_kind(plan_json: &str) -> Option<&'static str> {
    let v = serde_json::from_str::<serde_json::Value>(plan_json).ok()?;
    let profile = v.get("processProfile")?;
    let kind = profile.get("kind")?.as_str()?;
    match kind {
        "python" => Some(DRIVER_PYTHON),
        "binary" => Some(DRIVER_BINARY),
        _ => None,
    }
}

/// RuntimeDriver lifecycle contract (T09).
///
/// `prepare` runs phase 1 under the app mutation lock before `start`; it is
/// where port leases are acquired and interpreter/binary identity is verified.
/// `start` spawns (or enables) and returns the summary in its current state.
/// `probe` is phase 2 (health) and runs without the lock so a concurrent stop
/// can cancel it. `stop` must verify resource release — on failure the caller
/// keeps the identity/port so a retry stop stays possible.
#[async_trait(?Send)]
pub trait RuntimeDriver: Send + Sync {
    fn kind(&self) -> &'static str;

    fn capabilities(&self) -> DriverCapabilities;

    /// Prepare phase-1 resources. `prepare` must be honest: a driver that
    /// cannot prepare must return a typed error, never a fake success.
    async fn prepare(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
    ) -> Result<()>;

    /// Spawn phase 1 (caller holds the mutation lock).
    async fn start(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
    ) -> Result<CreativeAppSummary>;

    /// Phase 2 health/probe (NO mutation lock).
    async fn probe(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
        spawned: &CreativeAppSummary,
    ) -> Result<CreativeAppSummary>;

    /// Stop + verify release.
    async fn stop(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
    ) -> Result<CreativeAppSummary>;

    /// Delete the app record + owned resources.
    async fn delete(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
        opts: DeleteOptions,
    ) -> Result<DeleteResult>;

    /// Reconcile live resources against DB state (orphan recovery).
    async fn reconcile(&self, conn: &Connection, app: Option<&AppHandle>) -> Result<u32>;
}

/// Helper: honest `unsupported` error for a driver that cannot perform an op.
pub fn unsupported(driver: &str, op: &str) -> Error {
    Error::InvalidInput(format!("driver '{driver}' does not support '{op}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_kind_mapping_stable() {
        assert_eq!(
            driver_kind_for_runtime(CreativeAppRuntime::WorkshopStatic),
            "workshop_static"
        );
        assert_eq!(
            driver_kind_for_runtime(CreativeAppRuntime::LocalStatic),
            "local_static"
        );
        assert_eq!(
            driver_kind_for_runtime(CreativeAppRuntime::NodeDevServer),
            "node_dev_server"
        );
        assert_eq!(
            driver_kind_for_runtime(CreativeAppRuntime::DockerCompose),
            "docker_compose"
        );
        assert_eq!(
            driver_kind_for_runtime(CreativeAppRuntime::DockerRun),
            "docker_run"
        );
    }

    #[test]
    #[allow(clippy::assertions_on_constants)] // 常量能力矩阵回归检查
    fn capabilities_are_driver_specific() {
        assert!(DriverCapabilities::LOCAL_PROCESS.supports_verified_stop);
        assert!(DriverCapabilities::LOCAL_PROCESS.supports_prepare);
        assert!(!DriverCapabilities::HOST_HTTP.supports_reconcile);
        assert!(DriverCapabilities::DOCKER.supports_reconcile);
        // Non-owned drivers never advertise start/stop authority.
        assert!(!DriverCapabilities::NON_OWNED.supports_start);
        assert!(!DriverCapabilities::NON_OWNED.supports_verified_stop);
        assert!(!DriverCapabilities::WORKSHOP.supports_start);
    }

    #[test]
    fn process_profile_kind_detects_python_and_binary() {
        let py = serde_json::json!({ "processProfile": { "kind": "python" } }).to_string();
        assert_eq!(process_profile_kind(&py), Some("python_webui"));
        let bin = serde_json::json!({ "processProfile": { "kind": "binary" } }).to_string();
        assert_eq!(process_profile_kind(&bin), Some("binary_webui"));
        let none = serde_json::json!({ "runtime": "node_dev_server" }).to_string();
        assert_eq!(process_profile_kind(&none), None);
    }

    #[test]
    fn resolve_driver_kind_upgrades_process_profile() {
        let plan = LaunchProfile {
            id: "p".into(),
            plan_version: 1,
            schema_version: 1,
            driver_kind: "node_dev_server".into(),
            ownership_mode: "managed".into(),
            is_active: true,
            plan_json: serde_json::json!({
                "schemaVersion": 1,
                "processProfile": { "kind": "python", "interpreter": "/x/python" },
                "runtime": "node_dev_server"
            })
            .to_string(),
        };
        assert_eq!(
            resolve_driver_kind(CreativeAppRuntime::NodeDevServer, Some(&plan)),
            "python_webui"
        );
    }
}
