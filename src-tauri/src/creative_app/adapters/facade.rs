//! RuntimeDriver facade (batch 7 CR-702).
//!
//! A thin contract layer over the three existing source adapters. The facade
//! resolves the driver kind for a runtime and exposes the uniform lifecycle
//! (start / stop / probe / reconcile), delegating to the source adapter that
//! already owns each runtime. This gives every driver a single dispatch path
//! without creating a second runtime authority.

use super::LifecycleCtx;
use crate::creative_app::driver::{self, DriverCapabilities};
use crate::creative_app::local::LocalRuntimeManager;
use crate::creative_app::model::*;
use crate::creative_app::runtime_store;
use crate::Result;
use rusqlite::Connection;
use std::sync::Arc;
use tauri::AppHandle;

/// Driver handle: what a caller needs to drive one app's lifecycle.
pub struct DriverHandle {
    driver_kind: String,
    caps: DriverCapabilities,
}

impl DriverHandle {
    pub fn kind(&self) -> &str {
        &self.driver_kind
    }

    pub fn capabilities(&self) -> DriverCapabilities {
        self.caps
    }
}

/// Resolve the driver handle for an app id (driver kind + capabilities).
pub fn driver_for(conn: &Connection, id: &str) -> Result<DriverHandle> {
    let source = super::resolve(conn, id)?;
    let app_id = runtime_store::find_or_create_application(conn, source.as_source(), id)?;
    let plan = runtime_store::get_active_plan(conn, &app_id)?;
    let runtime = super::get_summary(conn, id)?.runtime;
    let driver_kind = driver::resolve_driver_kind(runtime, plan.as_ref());
    let caps = driver::capabilities_for(runtime);
    Ok(DriverHandle {
        driver_kind,
        caps,
    })
}

/// Uniform start: spawn phase 1 (lock held) then settle health (phase 2).
/// Delegates to the source adapter dispatch in `super`.
pub async fn start(conn: &Connection, ctx: &LifecycleCtx, id: &str) -> Result<CreativeAppSummary> {
    let _d = driver_for(conn, id)?;
    let spawned = super::spawn_start(conn, ctx, id).await?;
    super::await_ready(conn, ctx, id, &spawned).await
}

/// Uniform stop: release resources with verified postcondition.
pub async fn stop(conn: &Connection, ctx: &LifecycleCtx, id: &str) -> Result<CreativeAppSummary> {
    let _d = driver_for(conn, id)?;
    super::stop(conn, ctx, id).await
}

/// Uniform delete.
pub async fn delete(
    conn: &Connection,
    ctx: &LifecycleCtx,
    id: &str,
    opts: DeleteOptions,
) -> Result<DeleteResult> {
    let _d = driver_for(conn, id)?;
    super::delete(conn, ctx, id, opts).await
}

/// Probe the current state of an app's driver (uniform readiness check).
pub fn probe(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
    let _d = driver_for(conn, id)?;
    super::get_summary(conn, id)
}

/// Reconcile: verify live resources against DB state.
pub fn reconcile(
    conn: &Connection,
    app: &AppHandle,
    _runtime: &Arc<LocalRuntimeManager>,
) -> Result<u32> {
    crate::creative_app::local::lifecycle::reconcile_local_apps(conn, Some(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_kind_matches_runtime() {
        assert_eq!(
            driver::driver_kind_for_runtime(CreativeAppRuntime::NodeDevServer),
            "node_dev_server"
        );
        assert_eq!(
            driver::driver_kind_for_runtime(CreativeAppRuntime::DockerCompose),
            "docker_compose"
        );
    }

    #[test]
    fn capabilities_are_exposed() {
        let h = DriverHandle {
            driver_kind: driver::DRIVER_NODE_DEV.to_string(),
            caps: DriverCapabilities::LOCAL_PROCESS,
        };
        assert_eq!(h.kind(), "node_dev_server");
        assert!(h.capabilities().supports_verified_stop);
    }
}
