//! RuntimeDriver contract (batch 7 CR-702).
//!
//! The minimal contract every runtime driver implements: inspect, start,
//! probe, stop, reconcile. Existing drivers (Workshop Static, Local Static,
//! Node, Docker Compose, Docker Run) are migrated to this contract so the
//! runtime lifecycle is uniform. Driver-specific preparation and log streams
//! stay as independent helpers — the driver does not own prepare/log.
//!
//! This module defines the trait and a registry. The actual per-driver
//! implementations live in `adapters/` (local, docker) and are exposed here
//! through the facade for lifecycle dispatch.

use super::model::CreativeAppRuntime;
use super::runtime_store::LaunchProfile;

/// Driver kind identifiers (stable strings, persisted in plan/summary).
pub const DRIVER_WORKSHOP_STATIC: &str = "workshop_static";
pub const DRIVER_LOCAL_STATIC: &str = "local_static";
pub const DRIVER_NODE_DEV: &str = "node_dev_server";
pub const DRIVER_DOCKER_COMPOSE: &str = "docker_compose";
pub const DRIVER_DOCKER_RUN: &str = "docker_run";

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
}

impl DriverCapabilities {
    pub const LOCAL_PROCESS: Self = Self {
        supports_start: true,
        supports_verified_stop: true,
        supports_reconcile: true,
    };
    pub const HOST_HTTP: Self = Self {
        supports_start: true,
        supports_verified_stop: true,
        supports_reconcile: false,
    };
    pub const DOCKER: Self = Self {
        supports_start: true,
        supports_verified_stop: true,
        supports_reconcile: true,
    };
}

/// Resolve driver capabilities from a runtime kind.
pub fn capabilities_for(runtime: CreativeAppRuntime) -> DriverCapabilities {
    match runtime {
        CreativeAppRuntime::WorkshopStatic | CreativeAppRuntime::LocalStatic => {
            DriverCapabilities::HOST_HTTP
        }
        CreativeAppRuntime::NodeDevServer => DriverCapabilities::LOCAL_PROCESS,
        CreativeAppRuntime::DockerCompose | CreativeAppRuntime::DockerRun => {
            DriverCapabilities::DOCKER
        }
    }
}

/// Resolve a driver kind from a plan's `driver_kind` column, falling back to
/// the runtime mapping for legacy plans (CR-702 compatibility).
pub fn resolve_driver_kind(runtime: CreativeAppRuntime, plan: Option<&LaunchProfile>) -> String {
    if let Some(p) = plan {
        if !p.driver_kind.is_empty() {
            return p.driver_kind.clone();
        }
    }
    driver_kind_for_runtime(runtime).to_string()
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
    fn capabilities_are_driver_specific() {
        assert_eq!(
            DriverCapabilities::LOCAL_PROCESS.supports_verified_stop,
            true
        );
        assert_eq!(DriverCapabilities::HOST_HTTP.supports_reconcile, false);
        assert_eq!(DriverCapabilities::DOCKER.supports_reconcile, true);
    }
}
