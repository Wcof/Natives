//! Install orchestration for external GitHub creative apps.

use super::docker;
use super::github::{self, GhRelease};
use super::model::*;
use super::paths::{self, MAX_ASSET_BYTES, MAX_TOTAL_BYTES};
use super::compose;
use super::probe::{self, ProbeOutcome};
use super::state_machine;
use super::store;
use crate::{emit_db_state_changed, Error, Result};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn emit_progress(app: &AppHandle, app_id: &str, stage: ProgressStage, message: &str) {
    let ev = ProgressEvent {
        app_id: app_id.to_string(),
        stage,
        message: message.to_string(),
    };
    let _ = app.emit("creative-app-progress", &ev);
}

fn broadcast(app: &AppHandle, action: &str, id: &str) {
    emit_db_state_changed(
        app,
        "creative-app",
        serde_json::json!({ "action": action, "id": id }),
    );
}

pub fn resolve_token(conn: &Connection, request_token: Option<&str>) -> Result<Option<String>> {
    if let Some(t) = request_token.map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(Some(t.to_string()));
    }
    store::get_github_token_plaintext(conn)
}

pub fn inspect(conn: &Connection, req: &InspectGithubRequest) -> Result<InspectGithubResult> {
    let repo = github::parse_github_url(&req.repository_url)?;
    if req.save_token {
        if let Some(t) = req
            .token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            store::set_github_token(conn, t)?;
        }
    }
    let token = resolve_token(conn, req.token.as_deref())?;
    let token_ref = token.as_deref();

    let (release, available_tags) = if req.one_click && req.release_tag.is_none() {
        let rel = github::latest_stable_release(&repo.owner, &repo.repo, token_ref)?;
        let list = github::list_releases(&repo.owner, &repo.repo, token_ref).unwrap_or_default();
        (rel, github::manual_release_tags(&list))
    } else if let Some(tag) = &req.release_tag {
        let rel = github::get_release_by_tag(&repo.owner, &repo.repo, tag, token_ref)?;
        let list = github::list_releases(&repo.owner, &repo.repo, token_ref).unwrap_or_default();
        (rel, github::manual_release_tags(&list))
    } else {
        let list = github::list_releases(&repo.owner, &repo.repo, token_ref)?;
        let tags = github::manual_release_tags(&list);
        let rel = list.into_iter().find(|r| !r.draft && !r.prerelease).or({
            // allow first non-draft if only prereleases
            None
        });
        let rel = rel.ok_or_else(|| Error::NotFound("no installable release found".into()))?;
        (rel, tags)
    };

    // Probe without download first (asset names only); refine if we can peek manifest
    let outcome = probe::probe_release(&release, None)?;

    Ok(InspectGithubResult {
        repository_url: github::repository_https_url(&repo.owner, &repo.repo),
        owner: repo.owner,
        repo: repo.repo,
        release_tag: release.tag_name,
        release_id: Some(release.id),
        is_prerelease: release.prerelease,
        candidates: outcome.candidates,
        one_click_eligible: outcome.one_click_eligible,
        one_click_candidate_id: outcome.one_click_candidate_id,
        warnings: outcome.warnings,
        blockers: outcome.blockers,
        available_tags,
    })
}

/// Download assets needed for a candidate into release_dir; return refined probe.
pub fn download_and_probe(
    owner: &str,
    repo: &str,
    release: &GhRelease,
    candidate: &InstallCandidate,
    release_dir: &PathBuf,
    token: Option<&str>,
) -> Result<ProbeOutcome> {
    std::fs::create_dir_all(release_dir).map_err(Error::Io)?;
    let mut total: u64 = 0;

    // Always download manifest if present
    for asset in &release.assets {
        let need = github::is_manifest_asset_name(&asset.name)
            || asset.name == candidate.primary_asset
            || (candidate.runtime == CreativeAppRuntime::DockerCompose
                && github::is_compose_asset_name(&asset.name));
        if !need {
            continue;
        }
        if asset.size > MAX_ASSET_BYTES {
            return Err(Error::InvalidInput(format!(
                "asset {} too large",
                asset.name
            )));
        }
        let dest = release_dir.join(&asset.name);
        let n = github::download_asset_by_id(owner, repo, asset.id, &dest, token, MAX_ASSET_BYTES)?;
        total += n;
        if total > MAX_TOTAL_BYTES {
            return Err(Error::InvalidInput("total download exceeds 200 MiB".into()));
        }
        if asset.name == "natives.compose.zip" {
            let unpack = release_dir.join("compose_unpacked");
            // safe_extract_zip 已上移到共享模块 archive_ops（行为不变：符号链接条目报错）
            crate::archive_ops::safe_extract_zip(&dest, &unpack, MAX_TOTAL_BYTES)?;
        }
    }

    probe::probe_release(release, Some(release_dir))
}

pub async fn install_github(
    conn: &Connection,
    app: &AppHandle,
    req: InstallGithubRequest,
) -> Result<CreativeAppSummary> {
    // Docker hard requirement — no "register only"
    let engine = docker::require_docker().await?;
    let repo = github::parse_github_url(&req.repository_url)?;
    let token = resolve_token(conn, req.token.as_deref())?;
    let token_ref = token.as_deref();

    let release = github::get_release_by_tag(&repo.owner, &repo.repo, &req.release_tag, token_ref)?;

    // Initial probe by names
    let initial = probe::probe_release(&release, None)?;
    if !initial.blockers.is_empty() && initial.candidates.is_empty() {
        return Err(Error::InvalidInput(initial.blockers.join("; ")));
    }
    let candidate = initial
        .candidates
        .iter()
        .find(|c| c.id == req.candidate_id)
        .cloned()
        .ok_or_else(|| Error::InvalidInput("candidate not found in release".into()))?;

    if !candidate.hard_blockers.is_empty() {
        return Err(Error::InvalidInput(format!(
            "candidate blocked: {}",
            candidate.hard_blockers.join("; ")
        )));
    }

    // Compose needs compose plugin
    if candidate.runtime == CreativeAppRuntime::DockerCompose && !engine.compose_available {
        return Err(Error::Internal(
            "Docker Compose plugin is required for this candidate".into(),
        ));
    }

    let app_id = uuid::Uuid::new_v4().to_string();
    let title = if candidate.title.is_empty() {
        format!("{}/{}", repo.owner, repo.repo)
    } else {
        candidate.title.clone()
    };

    let host_port = req
        .host_port
        .or(candidate.suggested_host_port)
        .ok_or_else(|| Error::InvalidInput("host port is required".into()))?;
    let container_port = candidate.container_port.unwrap_or(host_port);
    let open_path = req
        .open_path
        .clone()
        .unwrap_or_else(|| candidate.open_path.clone());
    let health_path = req
        .health_path
        .clone()
        .or_else(|| candidate.health_path.clone());
    let service = req
        .service
        .clone()
        .or_else(|| candidate.service.clone())
        .unwrap_or_else(|| "web".into());

    // Port conflict
    if docker::host_port_in_use(host_port).await.unwrap_or(false) {
        return Err(Error::InvalidInput(format!(
            "host port {host_port} is already in use; choose another port"
        )));
    }

    let ts = now();
    let open_url = format!("http://127.0.0.1:{host_port}{open_path}");
    let health_url = health_path
        .as_ref()
        .map(|h| format!("http://127.0.0.1:{host_port}{h}"))
        .or_else(|| Some(open_url.clone()));

    // Placeholder runtime config — refined after download
    let runtime_config = match candidate.runtime {
        CreativeAppRuntime::DockerCompose => RuntimeConfig::DockerCompose {
            project_name: paths::compose_project_name(&app_id),
            compose_file: "runtime/docker-compose.yml".into(),
            service: service.clone(),
            container_port,
            host_port,
            open_path: open_path.clone(),
            health_path: health_path.clone(),
            env_keys: req.env.iter().map(|e| e.key.clone()).collect(),
        },
        CreativeAppRuntime::DockerRun => RuntimeConfig::DockerRun {
            container_name: paths::run_container_name(&app_id),
            image: candidate.image.clone().unwrap_or_default(),
            container_port,
            host_port,
            open_path: open_path.clone(),
            health_path: health_path.clone(),
            env_keys: req.env.iter().map(|e| e.key.clone()).collect(),
        },
        CreativeAppRuntime::WorkshopStatic
        | CreativeAppRuntime::LocalStatic
        | CreativeAppRuntime::NodeDevServer => {
            return Err(Error::InvalidInput(
                "runtime is not an external install runtime".into(),
            ));
        }
    };

    let rec = ExternalCreativeAppRecord {
        id: app_id.clone(),
        title: title.clone(),
        description: Some(candidate.description.clone()),
        icon: None,
        version: release.tag_name.clone(),
        owner: repo.owner.clone(),
        repo: repo.repo.clone(),
        repository_url: github::repository_https_url(&repo.owner, &repo.repo),
        release_tag: release.tag_name.clone(),
        release_id: Some(release.id),
        runtime: candidate.runtime,
        state: CreativeAppState::Installing,
        open_url: Some(open_url.clone()),
        health_url: health_url.clone(),
        host_port: Some(host_port),
        runtime_config_json: runtime_config
            .to_json()
            .map_err(|e| Error::Internal(e.to_string()))?,
        last_error: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };
    store::insert_app(conn, &rec)?;
    for e in &req.env {
        store::set_env(conn, &app_id, &e.key, &e.value)?;
    }
    broadcast(app, "install_start", &app_id);
    emit_progress(
        app,
        &app_id,
        ProgressStage::InspectingRelease,
        "Downloading release assets",
    );

    let release_dir = paths::release_dir(&app_id);
    let runtime_dir = paths::runtime_dir(&app_id);
    std::fs::create_dir_all(&runtime_dir).map_err(Error::Io)?;

    let download_result = download_and_probe(
        &repo.owner,
        &repo.repo,
        &release,
        &candidate,
        &release_dir,
        token_ref,
    );

    let outcome = match download_result {
        Ok(o) => o,
        Err(e) => {
            fail_install(conn, app, &app_id, &e.to_string())?;
            return Err(e);
        }
    };

    let refined = outcome
        .candidates
        .iter()
        .find(|c| c.id == candidate.id || c.primary_asset == candidate.primary_asset)
        .cloned()
        .unwrap_or(candidate.clone());

    if !refined.hard_blockers.is_empty() && !req.confirm_bind_mounts {
        // hard blockers always fail; bind mount risks need confirm
        if refined.hard_blockers.iter().any(|b| !b.contains("manual")) {
            fail_install(conn, app, &app_id, &refined.hard_blockers.join("; "))?;
            return Err(Error::InvalidInput(refined.hard_blockers.join("; ")));
        }
    }
    // Absolute mount risks
    if refined
        .risk_summary
        .iter()
        .any(|r| r.contains("absolute host mount"))
        && !req.confirm_bind_mounts
    {
        fail_install(
            conn,
            app,
            &app_id,
            "absolute host mounts require confirmBindMounts",
        )?;
        return Err(Error::InvalidInput(
            "absolute host mounts require explicit confirmation".into(),
        ));
    }

    emit_progress(
        app,
        &app_id,
        ProgressStage::DownloadingAssets,
        "Assets ready",
    );

    let env_map: HashMap<String, String> = store::get_env_map(conn, &app_id)?.into_iter().collect();
    let env_pairs: Vec<(String, String)> = env_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let install_result = match refined.runtime {
        CreativeAppRuntime::DockerCompose => {
            install_compose(
                app,
                &app_id,
                &release_dir,
                &runtime_dir,
                &refined,
                &service,
                host_port,
                container_port,
                &open_path,
                &health_path,
                &env_pairs,
            )
            .await
        }
        CreativeAppRuntime::DockerRun => {
            let image = refined
                .image
                .clone()
                .ok_or_else(|| Error::InvalidInput("missing image".into()))?;
            install_run(
                app,
                &app_id,
                &image,
                host_port,
                container_port,
                &open_path,
                &health_path,
                &env_pairs,
            )
            .await
        }
        CreativeAppRuntime::WorkshopStatic
        | CreativeAppRuntime::LocalStatic
        | CreativeAppRuntime::NodeDevServer => Err(Error::InvalidInput("invalid runtime".into())),
    };

    match install_result {
        Ok(cfg) => {
            let ts = now();
            let mut rec =
                store::get_app(conn, &app_id)?.ok_or_else(|| Error::NotFound(app_id.clone()))?;
            rec.state = CreativeAppState::Running;
            rec.runtime_config_json = cfg.to_json().map_err(|e| Error::Internal(e.to_string()))?;
            rec.open_url = Some(open_url.clone());
            rec.health_url = health_url.clone();
            rec.host_port = Some(host_port);
            rec.last_error = None;
            rec.updated_at = ts;
            store::update_app(conn, &rec)?;
            emit_progress(app, &app_id, ProgressStage::Ready, "Running");
            broadcast(app, "install_ok", &app_id);
            Ok(summary_from_external(&rec))
        }
        Err(e) => {
            // Best-effort cleanup of containers created for this app
            let _ = cleanup_partial(&app_id, &refined.runtime).await;
            fail_install(conn, app, &app_id, &e.to_string())?;
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)] // pre-existing parameter list
async fn install_compose(
    app: &AppHandle,
    app_id: &str,
    release_dir: &Path,
    runtime_dir: &Path,
    candidate: &InstallCandidate,
    service: &str,
    host_port: u16,
    container_port: u16,
    open_path: &str,
    health_path: &Option<String>,
    env_pairs: &[(String, String)],
) -> Result<RuntimeConfig> {
    // Locate compose file
    let mut compose_src = release_dir.join(&candidate.primary_asset);
    if candidate.primary_asset == "natives.compose.zip" {
        let unpacked = release_dir.join("compose_unpacked");
        compose_src = find_compose_in_dir(&unpacked).ok_or_else(|| {
            Error::InvalidInput("natives.compose.zip contains no compose file".into())
        })?;
    }
    if !compose_src.exists() {
        // try any compose name in release dir
        compose_src = find_compose_in_dir(release_dir)
            .ok_or_else(|| Error::InvalidInput("compose file not found after download".into()))?;
    }

    // Re-analyze for hard blockers
    let analysis = compose::analyze_compose_file(&compose_src)?;
    if analysis.build_only || !analysis.hard_blockers.is_empty() {
        return Err(Error::InvalidInput(format!(
            "compose blocked: {}",
            analysis.hard_blockers.join("; ")
        )));
    }

    let dest = runtime_dir.join("docker-compose.yml");
    compose::normalize_compose_to_localhost(&compose_src, &dest, service, host_port, container_port)?;

    let project = paths::compose_project_name(app_id);
    emit_progress(
        app,
        app_id,
        ProgressStage::PullingImage,
        "docker compose pull",
    );
    docker::compose_pull(&project, &dest).await?;
    emit_progress(app, app_id, ProgressStage::Creating, "docker compose up");
    docker::compose_up(&project, &dest, env_pairs).await?;
    emit_progress(
        app,
        app_id,
        ProgressStage::HealthCheck,
        "waiting for readiness",
    );

    let health = health_path
        .as_ref()
        .map(|h| format!("http://127.0.0.1:{host_port}{h}"))
        .unwrap_or_else(|| format!("http://127.0.0.1:{host_port}{open_path}"));
    docker::wait_ready(&health, Duration::from_secs(60)).await?;

    Ok(RuntimeConfig::DockerCompose {
        project_name: project,
        compose_file: dest.to_string_lossy().to_string(),
        service: service.to_string(),
        container_port,
        host_port,
        open_path: open_path.to_string(),
        health_path: health_path.clone(),
        env_keys: env_pairs.iter().map(|(k, _)| k.clone()).collect(),
    })
}

#[allow(clippy::too_many_arguments)] // pre-existing parameter list
async fn install_run(
    app: &AppHandle,
    app_id: &str,
    image: &str,
    host_port: u16,
    container_port: u16,
    open_path: &str,
    health_path: &Option<String>,
    env_pairs: &[(String, String)],
) -> Result<RuntimeConfig> {
    if image.is_empty() {
        return Err(Error::InvalidInput("image is empty".into()));
    }
    let name = paths::run_container_name(app_id);
    emit_progress(app, app_id, ProgressStage::PullingImage, "docker pull");
    docker::docker_pull(image).await?;
    emit_progress(app, app_id, ProgressStage::Creating, "docker create");
    let env_keys: Vec<String> = env_pairs.iter().map(|(k, _)| k.clone()).collect();
    let env_map: HashMap<String, String> = env_pairs.iter().cloned().collect();
    // Remove existing with same name if any
    if docker::docker_exists(&name).await.unwrap_or(false) {
        let _ = docker::docker_rm(&name, true).await;
    }
    docker::docker_create(
        &name,
        image,
        host_port,
        container_port,
        app_id,
        &env_keys,
        &env_map,
    )
    .await?;
    emit_progress(app, app_id, ProgressStage::Starting, "docker start");
    docker::docker_start(&name).await?;
    emit_progress(
        app,
        app_id,
        ProgressStage::HealthCheck,
        "waiting for readiness",
    );
    let health = health_path
        .as_ref()
        .map(|h| format!("http://127.0.0.1:{host_port}{h}"))
        .unwrap_or_else(|| format!("http://127.0.0.1:{host_port}{open_path}"));
    docker::wait_ready(&health, Duration::from_secs(60)).await?;
    Ok(RuntimeConfig::DockerRun {
        container_name: name,
        image: image.to_string(),
        container_port,
        host_port,
        open_path: open_path.to_string(),
        health_path: health_path.clone(),
        env_keys,
    })
}

fn find_compose_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    // shallow search
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(f) = find_compose_in_dir(&p) {
                    return Some(f);
                }
            }
        }
    }
    None
}

fn fail_install(conn: &Connection, app: &AppHandle, app_id: &str, err: &str) -> Result<()> {
    let ts = now();
    store::set_state(
        conn,
        app_id,
        CreativeAppState::InstallFailed,
        Some(err),
        &ts,
    )?;
    emit_progress(app, app_id, ProgressStage::Failed, err);
    broadcast(app, "install_failed", app_id);
    Ok(())
}

async fn cleanup_partial(app_id: &str, runtime: &CreativeAppRuntime) -> Result<()> {
    match runtime {
        CreativeAppRuntime::DockerRun => {
            let name = paths::run_container_name(app_id);
            let _ = docker::docker_rm(&name, true).await;
        }
        CreativeAppRuntime::DockerCompose => {
            let project = paths::compose_project_name(app_id);
            let compose = paths::runtime_dir(app_id).join("docker-compose.yml");
            if compose.exists() {
                let _ = docker::compose_down(&project, &compose, false, false).await;
            }
        }
        _ => {}
    }
    // Also try label-based cleanup
    if let Ok(ids) = docker::containers_by_app_id(app_id).await {
        for id in ids {
            let _ = docker::docker_rm(&id, true).await;
        }
    }
    Ok(())
}

pub async fn start_app(conn: &Connection, app: &AppHandle, id: &str) -> Result<CreativeAppSummary> {
    docker::require_docker().await?;
    let mut rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let next = state_machine::transition(rec.state, CreativeAppState::Starting)?;
    let ts = now();
    store::set_state(conn, id, next, None, &ts)?;
    broadcast(app, "starting", id);

    let cfg = store::parse_runtime_config(&rec.runtime_config_json)?;
    let env_map: HashMap<String, String> = store::get_env_map(conn, id)?.into_iter().collect();
    let env_pairs: Vec<(String, String)> = env_map.into_iter().collect();

    let result = match cfg {
        RuntimeConfig::DockerCompose {
            ref project_name,
            ref compose_file,
            ..
        } => {
            let path = PathBuf::from(compose_file);
            docker::compose_up(project_name, &path, &env_pairs).await
        }
        RuntimeConfig::DockerRun {
            ref container_name, ..
        } => docker::docker_start(container_name).await,
    };

    match result {
        Ok(()) => {
            let health = rec
                .health_url
                .clone()
                .or_else(|| rec.open_url.clone())
                .unwrap_or_default();
            if !health.is_empty() {
                if let Err(e) = docker::wait_ready(&health, Duration::from_secs(60)).await {
                    let ts = now();
                    store::set_state(
                        conn,
                        id,
                        CreativeAppState::StartFailed,
                        Some(&e.to_string()),
                        &ts,
                    )?;
                    broadcast(app, "start_failed", id);
                    rec = store::get_app(conn, id)?.unwrap();
                    return Ok(summary_from_external(&rec));
                }
            }
            let ts = now();
            store::set_state(conn, id, CreativeAppState::Running, None, &ts)?;
            broadcast(app, "started", id);
        }
        Err(e) => {
            let ts = now();
            let msg = e.to_string();
            let st = if msg.to_lowercase().contains("docker") {
                CreativeAppState::RuntimeUnavailable
            } else {
                CreativeAppState::StartFailed
            };
            store::set_state(conn, id, st, Some(&msg), &ts)?;
            broadcast(app, "start_failed", id);
        }
    }
    rec = store::get_app(conn, id)?.unwrap();
    Ok(summary_from_external(&rec))
}

pub async fn stop_app(conn: &Connection, app: &AppHandle, id: &str) -> Result<CreativeAppSummary> {
    docker::require_docker().await?;
    let mut rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let next = state_machine::transition(rec.state, CreativeAppState::Stopping)?;
    let ts = now();
    store::set_state(conn, id, next, None, &ts)?;
    broadcast(app, "stopping", id);

    let cfg = store::parse_runtime_config(&rec.runtime_config_json)?;
    let result = match cfg {
        RuntimeConfig::DockerCompose {
            ref project_name,
            ref compose_file,
            ..
        } => docker::compose_stop(project_name, &PathBuf::from(compose_file)).await,
        RuntimeConfig::DockerRun {
            ref container_name, ..
        } => docker::docker_stop(container_name).await,
    };

    match result {
        Ok(()) => {
            let ts = now();
            store::set_state(conn, id, CreativeAppState::InstalledStopped, None, &ts)?;
            broadcast(app, "stopped", id);
            rec = store::get_app(conn, id)?.unwrap();
            Ok(summary_from_external(&rec))
        }
        Err(e) => {
            let ts = now();
            let msg = e.to_string();
            store::set_state(conn, id, CreativeAppState::StartFailed, Some(&msg), &ts)?;
            broadcast(app, "stop_failed", id);
            // Fail closed: restart must not start a new container after stop failure.
            Err(Error::Internal(format!("stop failed: {msg}")))
        }
    }
}

pub async fn delete_app(
    conn: &Connection,
    app: &AppHandle,
    id: &str,
    opts: DeleteOptions,
) -> Result<DeleteResult> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let ts = now();
    store::set_state(conn, id, CreativeAppState::Deleting, None, &ts)?;
    broadcast(app, "deleting", id);

    let mut warnings = Vec::new();
    let cfg = store::parse_runtime_config(&rec.runtime_config_json).ok();

    // Docker resources first
    if let Ok(engine) = docker::require_docker().await {
        let _ = engine;
        if let Some(cfg) = &cfg {
            let r = match cfg {
                RuntimeConfig::DockerCompose {
                    project_name,
                    compose_file,
                    ..
                } => {
                    let path = PathBuf::from(compose_file);
                    if path.exists() {
                        docker::compose_down(
                            project_name,
                            &path,
                            opts.remove_volumes,
                            opts.remove_images,
                        )
                        .await
                    } else {
                        Ok(())
                    }
                }
                RuntimeConfig::DockerRun { container_name, .. } => {
                    let _ = docker::docker_stop(container_name).await;
                    docker::docker_rm(container_name, true).await
                }
            };
            if let Err(e) = r {
                let ts = now();
                store::set_state(
                    conn,
                    id,
                    CreativeAppState::DeleteFailed,
                    Some(&e.to_string()),
                    &ts,
                )?;
                broadcast(app, "delete_failed", id);
                return Ok(DeleteResult {
                    ok: false,
                    warnings: vec![e.to_string()],
                });
            }
        }
        // optional image cleanup is best-effort already in compose_down
        if opts.remove_images {
            if let Err(e) = cleanup_partial(id, &rec.runtime).await {
                warnings.push(format!("optional image cleanup: {e}"));
            }
        }
    } else {
        let ts = now();
        store::set_state(
            conn,
            id,
            CreativeAppState::DeleteFailed,
            Some("Docker unavailable; cannot complete delete"),
            &ts,
        )?;
        broadcast(app, "delete_failed", id);
        return Ok(DeleteResult {
            ok: false,
            warnings: vec!["Docker unavailable".into()],
        });
    }

    // Directory
    let dir = paths::app_dir(id);
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            let ts = now();
            store::set_state(
                conn,
                id,
                CreativeAppState::DeleteFailed,
                Some(&format!("remove dir: {e}")),
                &ts,
            )?;
            broadcast(app, "delete_failed", id);
            return Ok(DeleteResult {
                ok: false,
                warnings,
            });
        }
    }

    // DB last (CASCADE env)
    store::delete_app(conn, id)?;
    broadcast(app, "deleted", id);
    Ok(DeleteResult { ok: true, warnings })
}

pub async fn logs(conn: &Connection, id: &str, tail: usize) -> Result<String> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let cfg = store::parse_runtime_config(&rec.runtime_config_json)?;
    match cfg {
        RuntimeConfig::DockerCompose {
            project_name,
            compose_file,
            ..
        } => docker::compose_logs(&project_name, &PathBuf::from(compose_file), tail).await,
        RuntimeConfig::DockerRun { container_name, .. } => {
            docker::docker_logs(&container_name, tail).await
        }
    }
}

pub async fn reconcile_all(conn: &Connection, app: Option<&AppHandle>) -> Result<usize> {
    let docker_ok = docker::engine_status().await.available;
    let apps = store::list_apps(conn)?;
    let mut n = 0;
    for rec in apps {
        let ts = now();
        if !docker_ok {
            if !matches!(
                rec.state,
                CreativeAppState::RuntimeUnavailable
                    | CreativeAppState::InstallFailed
                    | CreativeAppState::DeleteFailed
            ) {
                store::set_state(
                    conn,
                    &rec.id,
                    CreativeAppState::RuntimeUnavailable,
                    Some("Docker Engine unavailable"),
                    &ts,
                )?;
                n += 1;
                if let Some(a) = app {
                    broadcast(a, "reconcile", &rec.id);
                }
            }
            continue;
        }

        let cfg = match store::parse_runtime_config(&rec.runtime_config_json) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let running = match &cfg {
            RuntimeConfig::DockerCompose {
                project_name,
                compose_file,
                ..
            } => {
                let p = PathBuf::from(compose_file);
                if p.exists() {
                    docker::compose_ps_running(project_name, &p)
                        .await
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            RuntimeConfig::DockerRun { container_name, .. } => {
                docker::docker_inspect_running(container_name)
                    .await
                    .unwrap_or(false)
            }
        };

        let target = if rec.state.is_transient() {
            if running {
                CreativeAppState::Running
            } else {
                state_machine::converge_orphan(rec.state)
            }
        } else if running {
            CreativeAppState::Running
        } else if matches!(
            rec.state,
            CreativeAppState::Running | CreativeAppState::StartFailed
        ) {
            CreativeAppState::InstalledStopped
        } else {
            rec.state
        };

        if target != rec.state && state_machine::transition(rec.state, target).is_ok() {
            store::set_state(conn, &rec.id, target, rec.last_error.as_deref(), &ts)?;
            n += 1;
            if let Some(a) = app {
                broadcast(a, "reconcile", &rec.id);
            }
        }
    }
    Ok(n)
}

pub fn summary_from_external(rec: &ExternalCreativeAppRecord) -> CreativeAppSummary {
    CreativeAppSummary {
        id: rec.id.clone(),
        application_id: String::new(),
        runtime_instance_id: None,
        source: CreativeAppSource::ExternalGithub,
        runtime: rec.runtime,
        title: rec.title.clone(),
        description: rec.description.clone(),
        icon: rec.icon.clone(),
        version: rec.version.clone(),
        state: rec.state,
        open_url: rec.open_url.clone(),
        repository_url: Some(rec.repository_url.clone()),
        last_error: rec.last_error.clone(),
        status_detail: None,
        local_project: None,
        actions: CreativeAppActions::for_state(CreativeAppSource::ExternalGithub, rec.state),
    }
}

pub fn summary_from_internal(
    id: &str,
    name: &str,
    version: &str,
    enabled: i32,
    description: Option<String>,
    icon: Option<String>,
) -> CreativeAppSummary {
    let state = if enabled != 0 {
        CreativeAppState::Available
    } else {
        CreativeAppState::Disabled
    };
    CreativeAppSummary {
        id: id.to_string(),
        application_id: String::new(),
        runtime_instance_id: None,
        source: CreativeAppSource::Internal,
        runtime: CreativeAppRuntime::WorkshopStatic,
        title: name.to_string(),
        description,
        icon,
        version: version.to_string(),
        state,
        open_url: None,
        repository_url: None,
        last_error: None,
        status_detail: None,
        local_project: None,
        actions: CreativeAppActions::for_state(CreativeAppSource::Internal, state),
    }
}
