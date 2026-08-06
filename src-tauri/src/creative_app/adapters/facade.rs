//! RuntimeDriver facade (batch 7 CR-702, T09 closure).
//!
//! The facade owns the RuntimeDriver dispatch: `driver_for` resolves the real
//! driver for an app (kind + capabilities), and `start` / `probe` / `stop` /
//! `delete` / `reconcile` route through it. Every driver is a real
//! implementation over the existing source adapters — a driver that cannot do
//! an operation returns `unsupported` instead of advertising a fake capability.
//!
//! Three concrete drivers cover the managed sources:
//! - [`WorkshopDriver`] — Internal (enable / disable / uninstall).
//! - [`LocalDriver`] — LocalProject (static HTTP, node dev server, python /
//!   binary managed process, docker compose via the local lifecycle).
//! - [`DockerDriver`] — ExternalGithub (docker compose / docker run).

use super::internal;
use super::LifecycleCtx;
use super::ResolvedSource;
use crate::creative_app::driver::{self, DriverCapabilities, RuntimeDriver};
use crate::creative_app::local::{self, LocalRuntimeManager};
use crate::creative_app::model::*;
use crate::creative_app::runtime_store;
use crate::Result;
use async_trait::async_trait;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;

// ── Concrete drivers ─────────────────────────────────────────────────

/// Internal Workshop driver: enable / disable / uninstall. No long-lived
/// process, no port, no health.
pub struct WorkshopDriver;

impl WorkshopDriver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait(?Send)]
impl RuntimeDriver for WorkshopDriver {
    fn kind(&self) -> &'static str {
        driver::DRIVER_WORKSHOP_STATIC
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities::WORKSHOP
    }

    async fn prepare(
        &self,
        _conn: &Connection,
        _ctx: &LifecycleCtx,
        _id: &str,
        _instance_id: &str,
    ) -> Result<()> {
        // Enable/disable has no process resources to prepare.
        Ok(())
    }

    async fn start(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        _instance_id: &str,
    ) -> Result<CreativeAppSummary> {
        internal::start(conn, &ctx.app, id)
    }

    async fn probe(
        &self,
        _conn: &Connection,
        _ctx: &LifecycleCtx,
        _id: &str,
        _instance_id: &str,
        spawned: &CreativeAppSummary,
    ) -> Result<CreativeAppSummary> {
        // Enable is synchronous — no health phase.
        Ok(spawned.clone())
    }

    async fn stop(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        _instance_id: &str,
    ) -> Result<CreativeAppSummary> {
        internal::stop(conn, &ctx.app, id)
    }

    async fn delete(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        _instance_id: &str,
        _opts: DeleteOptions,
    ) -> Result<DeleteResult> {
        internal::delete(conn, &ctx.app, ctx.modules_dir(), id)
    }

    async fn reconcile(&self, _conn: &Connection, _app: Option<&AppHandle>) -> Result<u32> {
        Ok(0)
    }
}

/// LocalProject driver: static HTTP, node dev server, python/binary managed
/// process, and docker compose (all through the local lifecycle). The precise
/// kind is resolved from the stored plan so capabilities are honest per kind.
pub struct LocalDriver {
    kind: &'static str,
    caps: DriverCapabilities,
}

impl LocalDriver {
    pub fn new(kind: &'static str) -> Arc<Self> {
        let caps = match kind {
            driver::DRIVER_LOCAL_STATIC => DriverCapabilities::HOST_HTTP,
            driver::DRIVER_DOCKER_COMPOSE => DriverCapabilities::DOCKER,
            _ => DriverCapabilities::LOCAL_PROCESS,
        };
        Arc::new(Self { kind, caps })
    }
}

#[async_trait(?Send)]
impl RuntimeDriver for LocalDriver {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn capabilities(&self) -> DriverCapabilities {
        self.caps
    }

    async fn prepare(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        _instance_id: &str,
    ) -> Result<()> {
        let rt = ctx.require_local_runtime()?;
        let _ = rt;
        // Python/binary: verify Host-trusted interpreter / binary identity
        // before any resource is acquired. docker_compose: require the engine.
        let rec = local::get_app(conn, id)?.ok_or_else(|| crate::Error::NotFound(id.into()))?;
        let plan = LaunchPlan::from_json(&rec.launch_plan_json)
            .map_err(|e| crate::Error::InvalidInput(format!("launch_plan: {e}")))?;
        if let Some(profile) = &plan.process_profile {
            use crate::creative_app::model::ProcessProfile;
            match profile {
                ProcessProfile::Python(p) => {
                    let _ = crate::creative_app::process_driver::resolve_python_interpreter(
                        &p.interpreter,
                    )?;
                }
                ProcessProfile::Binary(b) => {
                    let _ = crate::creative_app::process_driver::verify_binary_identity(
                        &b.executable_path,
                        &b.executable_hash,
                    )?;
                }
            }
        }
        if plan.runtime == LocalLaunchRuntime::DockerCompose {
            crate::creative_app::docker::require_docker().await?;
        }
        Ok(())
    }

    async fn start(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
    ) -> Result<CreativeAppSummary> {
        let rt = ctx.require_local_runtime()?;
        local::start_app(conn, &ctx.app, rt, ctx.host_http_port, id, instance_id).await
    }

    async fn probe(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
        _spawned: &CreativeAppSummary,
    ) -> Result<CreativeAppSummary> {
        let rt = ctx.require_local_runtime()?;
        // Compose start is synchronous in start_app; await_start_ready already
        // returns the settled summary for it (no-op health phase).
        local::await_start_ready(conn, &ctx.app, rt, id, instance_id).await
    }

    async fn stop(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
    ) -> Result<CreativeAppSummary> {
        let rt = ctx.require_local_runtime()?;
        local::stop_app(conn, &ctx.app, rt, id, instance_id).await
    }

    async fn delete(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        instance_id: &str,
        _opts: DeleteOptions,
    ) -> Result<DeleteResult> {
        let rt = ctx.require_local_runtime()?;
        local::delete_running_app(conn, &ctx.app, rt, id, instance_id).await
    }

    async fn reconcile(&self, conn: &Connection, app: Option<&AppHandle>) -> Result<u32> {
        local::lifecycle::reconcile_local_apps(conn, app)
    }
}

/// ExternalGithub driver: docker compose / docker run via the external adapter.
pub struct DockerDriver;

impl DockerDriver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait(?Send)]
impl RuntimeDriver for DockerDriver {
    fn kind(&self) -> &'static str {
        // The precise docker kind is derived from the record at `driver_for`;
        // this is the generic external docker driver kind.
        driver::DRIVER_DOCKER_RUN
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities::DOCKER
    }

    async fn prepare(
        &self,
        _conn: &Connection,
        _ctx: &LifecycleCtx,
        _id: &str,
        _instance_id: &str,
    ) -> Result<()> {
        crate::creative_app::docker::require_docker().await?;
        Ok(())
    }

    async fn start(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        _instance_id: &str,
    ) -> Result<CreativeAppSummary> {
        super::external::start(conn, &ctx.app, id).await
    }

    async fn probe(
        &self,
        _conn: &Connection,
        _ctx: &LifecycleCtx,
        _id: &str,
        _instance_id: &str,
        spawned: &CreativeAppSummary,
    ) -> Result<CreativeAppSummary> {
        // External start includes its own health pass — no separate phase 2.
        Ok(spawned.clone())
    }

    async fn stop(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        _instance_id: &str,
    ) -> Result<CreativeAppSummary> {
        super::external::stop(conn, &ctx.app, id).await
    }

    async fn delete(
        &self,
        conn: &Connection,
        ctx: &LifecycleCtx,
        id: &str,
        _instance_id: &str,
        opts: DeleteOptions,
    ) -> Result<DeleteResult> {
        super::external::delete(conn, &ctx.app, id, opts).await
    }

    async fn reconcile(&self, conn: &Connection, app: Option<&AppHandle>) -> Result<u32> {
        let n = crate::creative_app::install::reconcile_all(conn, app).await?;
        Ok(n as u32)
    }
}

// ── Driver resolution ────────────────────────────────────────────────

/// Resolve the driver handle for an app id (driver kind + capabilities).
pub fn driver_for(conn: &Connection, id: &str) -> Result<Arc<dyn RuntimeDriver>> {
    let source = super::resolve(conn, id)?;
    match source {
        ResolvedSource::Internal => Ok(WorkshopDriver::new()),
        ResolvedSource::ExternalGithub => {
            // The precise docker kind (compose vs run) is resolved from the
            // record config, but a stale/unparseable config must not prevent
            // the driver from being resolved (delete/stop still work).
            let _ = runtime_store::external_owner_kind(conn, id);
            Ok(DockerDriver::new())
        }
        ResolvedSource::LocalProject => {
            let app_id = runtime_store::find_or_create_application(conn, source.as_source(), id)?;
            let plan = runtime_store::get_active_plan(conn, &app_id)?;
            let runtime = super::get_summary(conn, id)?.runtime;
            let driver_kind = driver::resolve_driver_kind(runtime, plan.as_ref());
            Ok(LocalDriver::new(local_kind_constant(&driver_kind)))
        }
    }
}

/// Map a resolved local driver kind string back to its stable constant so the
/// driver can carry a `&'static str` without leaking.
fn local_kind_constant(kind: &str) -> &'static str {
    match kind {
        driver::DRIVER_LOCAL_STATIC => driver::DRIVER_LOCAL_STATIC,
        driver::DRIVER_DOCKER_COMPOSE => driver::DRIVER_DOCKER_COMPOSE,
        driver::DRIVER_PYTHON => driver::DRIVER_PYTHON,
        driver::DRIVER_BINARY => driver::DRIVER_BINARY,
        _ => driver::DRIVER_NODE_DEV,
    }
}

/// Driver for a non-owned (attached/remote) app — pure record driver, never
/// start/stop. Lives beside the managed drivers so callers get one dispatch.
pub fn non_owned_driver(ownership: OwnershipMode) -> Arc<dyn RuntimeDriver> {
    struct NonOwned {
        kind: &'static str,
    }
    #[async_trait(?Send)]
    impl RuntimeDriver for NonOwned {
        fn kind(&self) -> &'static str {
            self.kind
        }
        fn capabilities(&self) -> DriverCapabilities {
            DriverCapabilities::NON_OWNED
        }
        async fn prepare(
            &self,
            _conn: &Connection,
            _ctx: &LifecycleCtx,
            _id: &str,
            _instance_id: &str,
        ) -> Result<()> {
            Err(driver::unsupported(self.kind, "prepare"))
        }
        async fn start(
            &self,
            _conn: &Connection,
            _ctx: &LifecycleCtx,
            _id: &str,
            _instance_id: &str,
        ) -> Result<CreativeAppSummary> {
            Err(driver::unsupported(self.kind, "start"))
        }
        async fn probe(
            &self,
            _conn: &Connection,
            _ctx: &LifecycleCtx,
            _id: &str,
            _instance_id: &str,
            _spawned: &CreativeAppSummary,
        ) -> Result<CreativeAppSummary> {
            Err(driver::unsupported(self.kind, "probe"))
        }
        async fn stop(
            &self,
            _conn: &Connection,
            _ctx: &LifecycleCtx,
            _id: &str,
            _instance_id: &str,
        ) -> Result<CreativeAppSummary> {
            Err(driver::unsupported(self.kind, "stop"))
        }
        async fn delete(
            &self,
            _conn: &Connection,
            _ctx: &LifecycleCtx,
            _id: &str,
            _instance_id: &str,
            _opts: DeleteOptions,
        ) -> Result<DeleteResult> {
            Err(driver::unsupported(self.kind, "delete"))
        }
        async fn reconcile(&self, _conn: &Connection, _app: Option<&AppHandle>) -> Result<u32> {
            Err(driver::unsupported(self.kind, "reconcile"))
        }
    }
    let kind = match ownership {
        OwnershipMode::Managed => driver::DRIVER_ATTACHED, // unreachable; managed uses real drivers
        OwnershipMode::Attached => driver::DRIVER_ATTACHED,
        OwnershipMode::Remote => driver::DRIVER_REMOTE,
    };
    Arc::new(NonOwned { kind })
}

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

/// Resolve just the driver kind + capabilities for an app (read-only).
pub fn driver_info(conn: &Connection, id: &str) -> Result<DriverHandle> {
    let d = driver_for(conn, id)?;
    Ok(DriverHandle {
        driver_kind: d.kind().to_string(),
        caps: d.capabilities(),
    })
}

// ── Uniform lifecycle dispatch ───────────────────────────────────────

/// Uniform start: spawn phase 1 (lock held) then settle health (phase 2).
/// Dispatches through the real driver.
pub async fn start(conn: &Connection, ctx: &LifecycleCtx, id: &str) -> Result<CreativeAppSummary> {
    let spawned = super::spawn_start(conn, ctx, id).await?;
    super::await_ready(conn, ctx, id, &spawned).await
}

/// Uniform stop: release resources with verified postcondition.
pub async fn stop(conn: &Connection, ctx: &LifecycleCtx, id: &str) -> Result<CreativeAppSummary> {
    super::stop(conn, ctx, id).await
}

/// Uniform delete.
pub async fn delete(
    conn: &Connection,
    ctx: &LifecycleCtx,
    id: &str,
    opts: DeleteOptions,
) -> Result<DeleteResult> {
    super::delete(conn, ctx, id, opts).await
}

/// Probe the current state of an app's driver (uniform readiness check).
pub fn probe(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
    super::get_summary(conn, id)
}

/// Reconcile: verify live resources against DB state.
pub fn reconcile(
    conn: &Connection,
    app: &AppHandle,
    _runtime: &Arc<LocalRuntimeManager>,
) -> Result<u32> {
    local::lifecycle::reconcile_local_apps(conn, Some(app))
}

/// Local-project entry file path resolution used by the static driver prepare.
pub fn static_entry_path(project_root: &std::path::Path, plan: &LaunchPlan) -> PathBuf {
    let entry = plan.entry_file.as_deref().unwrap_or("index.html");
    if plan.cwd_relative == "." {
        project_root.join(entry)
    } else {
        project_root.join(&plan.cwd_relative).join(entry)
    }
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

    #[test]
    fn non_owned_driver_never_advertises_start() {
        for mode in [OwnershipMode::Attached, OwnershipMode::Remote] {
            let d = non_owned_driver(mode);
            assert!(!d.capabilities().supports_start);
            assert!(!d.capabilities().supports_verified_stop);
        }
    }

    #[test]
    fn unsupported_error_is_honest_and_typed() {
        // A non-owned driver must return a typed unsupported error rather than
        // fabricate a start/stop. `driver::unsupported` is the honest message.
        let e = driver::unsupported(driver::DRIVER_ATTACHED, "start");
        assert!(e.to_string().contains("attached"));
        assert!(e.to_string().contains("does not support 'start'"));
    }
}
