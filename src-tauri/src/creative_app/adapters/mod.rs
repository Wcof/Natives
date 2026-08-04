//! Three real Creative App source adapters under a small catalog / lifecycle seam.
//!
//! Not a plugin framework: only Internal / ExternalGithub / LocalProject.
//! Source differences (Workshop iframe, Docker CLI, Local Process) stay behind
//! these adapters; shared list/get/start/stop/delete/open semantics live here.

pub mod external;
pub mod internal;
pub mod local;

use super::model::*;
use crate::{Error, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Arc;
use tauri::AppHandle;

use super::local as local_mod;
use super::local::LocalRuntimeManager;
use super::store as external_store;

/// Resolved source for a creative-app id (catalog lookup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedSource {
    Internal,
    ExternalGithub,
    LocalProject,
}

impl ResolvedSource {
    pub fn as_source(self) -> CreativeAppSource {
        match self {
            Self::Internal => CreativeAppSource::Internal,
            Self::ExternalGithub => CreativeAppSource::ExternalGithub,
            Self::LocalProject => CreativeAppSource::LocalProject,
        }
    }
}

/// Runtime context required by lifecycle mutations.
///
/// Local Process needs supervisor + host HTTP port; Workshop needs modules_dir
/// on delete; Docker needs neither of those.
#[derive(Clone)]
pub struct LifecycleCtx {
    pub app: AppHandle,
    pub modules_dir: std::path::PathBuf,
    pub local_runtime: Option<Arc<LocalRuntimeManager>>,
    pub host_http_port: u16,
}

impl LifecycleCtx {
    pub fn new(
        app: AppHandle,
        modules_dir: impl Into<std::path::PathBuf>,
        local_runtime: Option<Arc<LocalRuntimeManager>>,
        host_http_port: u16,
    ) -> Self {
        Self {
            app,
            modules_dir: modules_dir.into(),
            local_runtime,
            host_http_port,
        }
    }

    pub fn modules_dir(&self) -> &Path {
        &self.modules_dir
    }

    pub fn require_local_runtime(&self) -> Result<&LocalRuntimeManager> {
        self.local_runtime
            .as_deref()
            .ok_or_else(|| Error::Internal("local runtime manager not available".into()))
    }
}

/// Resolve which real adapter owns `id`.
///
/// Lookup order is intentional and stable:
/// 1. external_creative_apps (GitHub container)
/// 2. local_creative_apps (Local Project)
/// 3. modules (Internal Workshop)
///
/// IDs are UUIDs / module ids and do not collide across tables by construction.
pub fn resolve(conn: &Connection, id: &str) -> Result<ResolvedSource> {
    if external_store::get_app(conn, id)?.is_some() {
        return Ok(ResolvedSource::ExternalGithub);
    }
    if local_mod::get_app(conn, id)?.is_some() {
        return Ok(ResolvedSource::LocalProject);
    }
    let modules = crate::module_manager::list_modules(conn)?;
    if modules.iter().any(|m| m.id == id) {
        return Ok(ResolvedSource::Internal);
    }
    Err(Error::NotFound(id.into()))
}

/// Unified catalog list: each adapter contributes its projections, then every
/// summary is bound to its unified application id and active runtime instance.
pub fn list_all(conn: &Connection) -> Result<Vec<CreativeAppSummary>> {
    let mut out = Vec::new();
    out.extend(internal::list(conn)?);
    out.extend(external::list(conn)?);
    out.extend(local::list(conn)?);
    let mut out = out
        .into_iter()
        .map(|s| super::runtime_store::attach_identity(conn, s))
        .collect::<Result<Vec<_>>>()?;
    sort_catalog(&mut out);
    Ok(out)
}

pub fn get_summary(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
    let summary = match resolve(conn, id)? {
        ResolvedSource::Internal => internal::get(conn, id),
        ResolvedSource::ExternalGithub => external::get(conn, id),
        ResolvedSource::LocalProject => local::get(conn, id),
    }?;
    super::runtime_store::attach_identity(conn, summary)
}

/// Shared instance CAS for process/container sources: create the instance in
/// `starting`, then mirror the driver outcome onto it. Internal Workshop apps
/// have no runtime and skip this (enable/disable only).
async fn begin_instance(conn: &Connection, source: ResolvedSource, id: &str) -> Result<String> {
    let app_id = super::runtime_store::find_or_create_application(conn, source.as_source(), id)?;
    if super::runtime_store::has_active_instance(conn, &app_id)? {
        return Err(Error::InvalidInput(
            "this app already has an active runtime instance; stop it first".into(),
        ));
    }
    let plan_id = super::runtime_store::active_plan_id(conn, &app_id)?;
    let owner_kind = match source {
        ResolvedSource::ExternalGithub => super::runtime_store::external_owner_kind(conn, id)?,
        ResolvedSource::LocalProject => super::runtime_store::local_owner_kind(conn, id)?,
        ResolvedSource::Internal => unreachable!("internal has no runtime instance"),
    };
    super::runtime_store::create_instance(conn, &app_id, plan_id.as_deref(), owner_kind)
}

/// Phase 1 of start (caller holds the mutation lock): create the runtime
/// instance and spawn the runtime. Fast for local (spawn only); external /
/// internal finish entirely here. Returns the summary in its current state with
/// `runtime_instance_id` bound, so phase 2 knows which instance to settle.
pub async fn spawn_start(
    conn: &Connection,
    ctx: &LifecycleCtx,
    id: &str,
) -> Result<CreativeAppSummary> {
    let source = resolve(conn, id)?;
    if source == ResolvedSource::Internal {
        let summary = internal::start(conn, &ctx.app, id)?;
        return super::runtime_store::attach_identity(conn, summary);
    }
    let instance_id = begin_instance(conn, source, id).await?;
    let result = match source {
        ResolvedSource::ExternalGithub => external::start(conn, &ctx.app, id).await,
        ResolvedSource::LocalProject => {
            let rt = ctx.require_local_runtime()?;
            local::start(conn, &ctx.app, rt, ctx.host_http_port, id, &instance_id).await
        }
        ResolvedSource::Internal => unreachable!(),
    };
    match result {
        Ok(mut summary) => {
            summary.runtime_instance_id = Some(instance_id.clone());
            if source == ResolvedSource::ExternalGithub {
                // External start includes its own health pass; settle the instance now.
                let hint = super::runtime_store::external_instance_hint(conn, id)
                    .unwrap_or((None, None, None));
                let urls = summary.open_url.iter().cloned().collect::<Vec<_>>();
                let _ = super::runtime_store::mark_running(
                    conn,
                    &instance_id,
                    &urls,
                    hint.0,
                    hint.1,
                    hint.2,
                );
            }
            Ok(super::runtime_store::attach_identity(conn, summary)?)
        }
        Err(e) => {
            let _ = super::runtime_store::mark_failed(conn, &instance_id, &e.to_string());
            Err(e)
        }
    }
}

/// Phase 2 of start (NO mutation lock): settle the local health wait. External /
/// internal are already settled by phase 1. A concurrent stop cancels the wait;
/// the instance then stays owned by the stop path.
pub async fn await_ready(
    conn: &Connection,
    ctx: &LifecycleCtx,
    id: &str,
    spawned: &CreativeAppSummary,
) -> Result<CreativeAppSummary> {
    match resolve(conn, id)? {
        ResolvedSource::Internal | ResolvedSource::ExternalGithub => Ok(spawned.clone()),
        ResolvedSource::LocalProject => {
            let rt = ctx.require_local_runtime()?;
            let instance_id = spawned.runtime_instance_id.clone();
            let rt_ref = instance_id.as_deref().unwrap_or_default();
            let result = local::await_start_ready(conn, &ctx.app, rt, id, rt_ref).await;
            match result {
                Ok(summary) => {
                    if summary.state == CreativeAppState::Running {
                        if let Some(iid) = &instance_id {
                            let hint = super::runtime_store::local_instance_hint(conn, id)
                                .unwrap_or((None, None, None));
                            let urls = summary.open_url.iter().cloned().collect::<Vec<_>>();
                            let _ = super::runtime_store::mark_running(
                                conn, iid, &urls, hint.0, hint.1, hint.2,
                            );
                        }
                    }
                    // Not running here means stop preempted the start; the instance
                    // is owned by the stop path and is left untouched.
                    Ok(super::runtime_store::attach_identity(conn, summary)?)
                }
                Err(e) => {
                    if let Some(iid) = &instance_id {
                        let _ = super::runtime_store::mark_failed(conn, iid, &e.to_string());
                    }
                    Err(e)
                }
            }
        }
    }
}

pub async fn stop(conn: &Connection, ctx: &LifecycleCtx, id: &str) -> Result<CreativeAppSummary> {
    let source = resolve(conn, id)?;
    if source == ResolvedSource::Internal {
        let summary = internal::stop(conn, &ctx.app, id)?;
        return super::runtime_store::attach_identity(conn, summary);
    }
    let app_id = super::runtime_store::find_or_create_application(conn, source.as_source(), id)?;
    let instance_id = super::runtime_store::active_instance_id(conn, &app_id)?;
    if let Some(iid) = &instance_id {
        let _ = super::runtime_store::mark_stopping(conn, iid);
    }
    let result = match source {
        ResolvedSource::ExternalGithub => external::stop(conn, &ctx.app, id).await,
        ResolvedSource::LocalProject => {
            let rt = ctx.require_local_runtime()?;
            let rt_id = instance_id.as_deref().unwrap_or_default();
            local::stop(conn, &ctx.app, rt, id, rt_id).await
        }
        ResolvedSource::Internal => unreachable!(),
    };
    match result {
        Ok(summary) => {
            if let Some(iid) = &instance_id {
                super::runtime_store::mark_stopped(conn, iid)?;
            }
            Ok(super::runtime_store::attach_identity(conn, summary)?)
        }
        Err(e) => {
            if let Some(iid) = &instance_id {
                let _ = super::runtime_store::mark_cleanup_failed(conn, iid, &e.to_string());
            }
            Err(e)
        }
    }
}

pub async fn delete(
    conn: &Connection,
    ctx: &LifecycleCtx,
    id: &str,
    opts: DeleteOptions,
) -> Result<DeleteResult> {
    let source = resolve(conn, id)?;
    let result = match source {
        ResolvedSource::Internal => internal::delete(conn, &ctx.app, ctx.modules_dir(), id),
        ResolvedSource::ExternalGithub => external::delete(conn, &ctx.app, id, opts).await,
        ResolvedSource::LocalProject => {
            let rt = ctx.require_local_runtime()?;
            // The active runtime id (may be empty when the app is not running).
            let app_id =
                super::runtime_store::find_or_create_application(conn, source.as_source(), id)?;
            let rt_id = super::runtime_store::active_instance_id(conn, &app_id)?
                .unwrap_or_default();
            local::delete(conn, &ctx.app, rt, id, &rt_id).await
        }
    };
    if result.is_ok() {
        super::runtime_store::delete_application(conn, source.as_source(), id)?;
    }
    result
}

pub fn open_target(conn: &Connection, id: &str) -> Result<OpenTarget> {
    match resolve(conn, id)? {
        ResolvedSource::Internal => internal::open_target(conn, id),
        ResolvedSource::ExternalGithub => external::open_target(conn, id),
        ResolvedSource::LocalProject => local::open_target(conn, id),
    }
}

/// Restart is only meaningful for process/container sources.
pub async fn restart(
    conn: &Connection,
    ctx: &LifecycleCtx,
    id: &str,
) -> Result<CreativeAppSummary> {
    match resolve(conn, id)? {
        ResolvedSource::Internal => Err(Error::InvalidInput(
            "restart is only supported for local/external creative apps".into(),
        )),
        ResolvedSource::ExternalGithub | ResolvedSource::LocalProject => {
            // Stop must fully release (mark old instance stopped) before start
            // creates a new instance — the instance CAS enforces this.
            stop(conn, ctx, id).await?;
            let spawned = spawn_start(conn, ctx, id).await?;
            await_ready(conn, ctx, id, &spawned).await
        }
    }
}

fn sort_catalog(out: &mut [CreativeAppSummary]) {
    out.sort_by(|a, b| {
        let rank = |s: &CreativeAppSummary| match s.state {
            CreativeAppState::Running => 0,
            CreativeAppState::Available => 1,
            CreativeAppState::InstalledStopped => 2,
            CreativeAppState::StartFailed | CreativeAppState::InstallFailed => 3,
            _ => 4,
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
}

#[cfg(test)]
#[path = "lifecycle_matrix_tests.rs"]
mod lifecycle_matrix_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creative_app::model::{CreativeAppActions, CreativeAppRuntime, CreativeAppState};

    #[test]
    fn sort_puts_running_first() {
        let mut apps = vec![
            CreativeAppSummary {
                id: "a".into(),
                application_id: String::new(),
                runtime_instance_id: None,
                source: CreativeAppSource::Internal,
                runtime: CreativeAppRuntime::WorkshopStatic,
                title: "B".into(),
                description: None,
                icon: None,
                version: "1".into(),
                state: CreativeAppState::Available,
                open_url: None,
                repository_url: None,
                last_error: None,
                status_detail: None,
                local_project: None,
                actions: CreativeAppActions::default(),
            },
            CreativeAppSummary {
                id: "b".into(),
                application_id: String::new(),
                runtime_instance_id: None,
                source: CreativeAppSource::ExternalGithub,
                runtime: CreativeAppRuntime::DockerRun,
                title: "A".into(),
                description: None,
                icon: None,
                version: "1".into(),
                state: CreativeAppState::Running,
                open_url: None,
                repository_url: None,
                last_error: None,
                status_detail: None,
                local_project: None,
                actions: CreativeAppActions::default(),
            },
        ];
        sort_catalog(&mut apps);
        assert_eq!(apps[0].id, "b");
    }

    #[test]
    fn resolved_source_maps_to_public_enum() {
        assert_eq!(
            ResolvedSource::Internal.as_source(),
            CreativeAppSource::Internal
        );
        assert_eq!(
            ResolvedSource::ExternalGithub.as_source(),
            CreativeAppSource::ExternalGithub
        );
        assert_eq!(
            ResolvedSource::LocalProject.as_source(),
            CreativeAppSource::LocalProject
        );
    }
}
