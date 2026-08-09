use super::*;

/// Validate an agent proposal through the Host gate. Returns the proposal
/// (if valid) with a redacted journal snapshot. Never registers anything.
#[tauri::command]
pub fn creative_app_proposal_validate(
    proposal: crate::creative_app::proposal::AgentProposal,
) -> Result<crate::creative_app::proposal::ValidatedProposal> {
    proposal.validate()?;
    Ok(crate::creative_app::proposal::ValidatedProposal {
        redacted: crate::creative_app::proposal::redacted_proposal_input(&proposal),
        proposal,
    })
}

/// Reject an agent proposal by its stable proposal id. The Host looks up the
/// persisted inbox row (never a Renderer-supplied proposal body), CASes
/// pending → rejected, and returns a tagged result. Rejecting an already-
/// decided proposal is an idempotent no-op (`already_decided`).
#[tauri::command]
pub async fn creative_app_proposal_reject(
    proposal_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::creative_app::proposal_inbox::ProposalRejectResult> {
    use crate::creative_app::proposal_inbox::{self, ProposalRejectResult};
    let pool = state.db.clone();
    // Best-effort sync: the proposal may have been produced after the Renderer's
    // last list. A sync failure must not block a reject of an already-known row.
    if let Err(e) = proposal_inbox::sync_pending_from_daemon(&pool).await {
        eprintln!("[proposal] sync before reject failed (continuing): {e}");
    }
    let app = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        let Some(stored) = proposal_inbox::get_stored(&c, &proposal_id)? else {
            return Err(Error::InvalidInput(format!(
                "proposal not found or not pending: {proposal_id}"
            )));
        };
        match stored.status.as_str() {
            proposal_inbox::STATUS_PENDING => {}
            other => {
                return Ok(ProposalRejectResult::AlreadyDecided {
                    proposal_id: proposal_id.clone(),
                    current_status: other.to_string(),
                });
            }
        }
        let redacted = crate::creative_app::proposal::redacted_proposal_input(
            &crate::creative_app::proposal::validate_protocol_proposal(&stored.envelope.payload)?
                .proposal,
        );
        let op_id = op::create_operation(&c, None, "proposal_reject", "user", Some(&redacted))?;
        if !proposal_inbox::cas_status(
            &c,
            &proposal_id,
            proposal_inbox::STATUS_PENDING,
            proposal_inbox::STATUS_REJECTED,
        )? {
            op::finish_failure(
                &c,
                op_id,
                Some("proposal_already_decided"),
                "concurrent decision",
            )?;
            emit_operation(&app, &c, op_id)?;
            return Ok(ProposalRejectResult::AlreadyDecided {
                proposal_id: proposal_id.clone(),
                current_status: stored.status.clone(),
            });
        }
        op::finish_success(&c, op_id)?;
        emit_operation(&app, &c, op_id)?;
        crate::emit_db_state_changed(
            &app,
            "creative-app",
            serde_json::json!({ "action": "proposal_rejected", "id": proposal_id }),
        );
        Ok(ProposalRejectResult::Rejected { proposal_id })
    })
    .await
    .map_err(|e| Error::Internal(format!("proposal_reject join: {e}")))?
}

/// Approve an agent proposal by its stable proposal id.
///
/// The Host looks up the persisted, Host-validated inbox row, re-resolves the
/// executable/interpreter identity (canonical path + Host-recomputed SHA-256),
/// records the executable approval, CASes pending → approved, and registers
/// the application. The Renderer never supplies executable paths or hashes.
///
/// Failures keep the proposal visible to the user: a verification failure
/// leaves the status `pending`; a registration failure CASes it to `failed`
/// and returns the error — never a success value.
#[tauri::command]
pub async fn creative_app_proposal_approve(
    proposal_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<crate::creative_app::proposal_inbox::ProposalApproveResult> {
    use crate::creative_app::proposal_inbox::{self, ProposalApproveResult};
    let pool = state.db.clone();
    let host_port = host_http_port(&state);
    let local_runtime = local_runtime.inner().clone();
    if let Err(e) = proposal_inbox::sync_pending_from_daemon(&pool).await {
        eprintln!("[proposal] sync before approve failed (continuing): {e}");
    }
    let lock = lock.inner().clone();
    let _guard = lock.acquire_app("__registration__").await;

    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = conn(&pool)?;
        let Some(stored) = proposal_inbox::get_stored(&c, &proposal_id)? else {
            return Err(Error::InvalidInput(format!(
                "proposal not found or not pending: {proposal_id}"
            )));
        };
        match stored.status.as_str() {
            proposal_inbox::STATUS_PENDING => {}
            other => {
                return Ok(ProposalApproveResult::AlreadyDecided {
                    proposal_id: proposal_id.clone(),
                    current_status: other.to_string(),
                });
            }
        }

        // Re-validate through the Host gate (defense in depth) and resolve the
        // executable/interpreter identity. A shell pseudo-python, a missing
        // binary, or a fake agent hash never gets here: the profile that was
        // stored at merge time already passed the structural gate, and the
        // identity resolution below re-checks existence + content.
        let validated =
            crate::creative_app::proposal::validate_protocol_proposal(&stored.envelope.payload)?;
        let proposal = validated.proposal;
        let (canonical, identity, verified_proposal) =
            proposal_inbox::resolve_proposal_identity(&proposal)?;

        let redacted = crate::creative_app::proposal::redacted_proposal_input(&proposal);
        let op_id = op::create_operation(&c, None, "proposal_approve", "user", Some(&redacted))?;
        op::transition(&c, op_id, &[op::PHASE_PENDING], op::PHASE_RUNNING)?;

        // Everything from here on is a single decision transaction: a failure
        // settles the journal op and (when we already CASed to approved) marks
        // the proposal failed — the card never sees a silent success.
        let decision: crate::Result<ProposalApproveResult> = (|| {
            // Pin the executable approval record BEFORE the CAS so a
            // verification failure leaves the proposal pending (the user sees
            // the error and can still reject).
            if !canonical.is_empty() {
                proposal_inbox::record_executable_approval(
                    &c,
                    &canonical,
                    &identity,
                    &format!("proposal:{proposal_id}"),
                    "user",
                    &proposal_id,
                )?;
            }

            if !proposal_inbox::cas_status(
                &c,
                &proposal_id,
                proposal_inbox::STATUS_PENDING,
                proposal_inbox::STATUS_APPROVED,
            )? {
                return Ok(ProposalApproveResult::AlreadyDecided {
                    proposal_id: proposal_id.clone(),
                    current_status: stored.status.clone(),
                });
            }

            // T09: kind=create registers only; kind=start also launches the app
            // through start→health→endpoint. A kind=start on an already
            // registered root reuses the existing app instead of duplicating.
            let register_only = !proposal_inbox::proposal_should_start(&verified_proposal);
            // Registration failure (including a duplicate path for kind=create)
            // CASes approved → failed (terminal) so the card never sees a
            // silent success.
            let app_id = (|| -> crate::Result<String> {
                match proposal_inbox::start_target_for_proposal(&c, &verified_proposal)? {
                    Some(existing) => Ok(existing),
                    None => register_proposal_app(&mut c, &verified_proposal).map(|s| s.id),
                }
            })()
            .inspect_err(|_| {
                let _ = proposal_inbox::cas_status(
                    &c,
                    &proposal_id,
                    proposal_inbox::STATUS_APPROVED,
                    proposal_inbox::STATUS_FAILED,
                );
            })?;
            if register_only {
                let summary = crate::creative_app::adapters::get_summary(&c, &app_id)?;
                return Ok(ProposalApproveResult::Approved {
                    proposal_id: proposal_id.clone(),
                    app: Box::new(summary),
                });
            }
            // Launch start→health→endpoint. A start failure keeps the proposal
            // approved and the app retryable (its state honestly reflects the
            // outcome); only a registration failure is terminal for the card.
            let summary =
                match start_approved_app(&handle, &local_runtime, host_port, &pool, &app_id, &lock)
                {
                    Ok(s) => s,
                    Err(e) => {
                        // The app is registered; return its honest current state
                        // (StartFailed) so the approval card reflects reality and
                        // the user can retry from the catalog.
                        crate::creative_app::adapters::get_summary(&c, &app_id).map_err(|_| e)?
                    }
                };
            Ok(ProposalApproveResult::Approved {
                proposal_id: proposal_id.clone(),
                app: Box::new(summary),
            })
        })();

        match decision {
            Ok(ProposalApproveResult::Approved {
                proposal_id: pid,
                app,
            }) => {
                op::finish_success(&c, op_id)?;
                emit_operation(&handle, &c, op_id)?;
                crate::emit_db_state_changed(
                    &handle,
                    "creative-app",
                    serde_json::json!({ "action": "proposal_approved", "id": app.id }),
                );
                Ok(ProposalApproveResult::Approved {
                    proposal_id: pid,
                    app,
                })
            }
            Ok(result @ ProposalApproveResult::AlreadyDecided { .. }) => {
                op::finish_failure(
                    &c,
                    op_id,
                    Some("proposal_already_decided"),
                    "concurrent decision",
                )?;
                emit_operation(&handle, &c, op_id)?;
                Ok(result)
            }
            Err(e) => {
                op::finish_failure(&c, op_id, Some("proposal_approve_failed"), &e.to_string())?;
                emit_operation(&handle, &c, op_id)?;
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("proposal_approve join: {e}")))?
}

/// List pending proposals awaiting user approval. The Host first pulls fresh
/// proposal facts from the daemon (reconnect-recoverable), then returns the
/// pending inbox rows — each flattened with its validated proposal intent.
#[tauri::command]
pub async fn creative_app_proposal_list(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<crate::creative_app::proposal_inbox::ProposalInboxEntry>> {
    use crate::creative_app::proposal_inbox;
    let pool = state.db.clone();
    proposal_inbox::sync_pending_from_daemon(&pool).await?;
    let _app = app_handle;
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        proposal_inbox::list_pending(&c)
    })
    .await
    .map_err(|e| Error::Internal(format!("proposal_list join: {e}")))?
}

/// Register an approved proposal as a real application. Only drivers with a
/// concrete registration path are supported; others return a clear error.
fn register_proposal_app(
    c: &mut rusqlite::Connection,
    proposal: &crate::creative_app::proposal::AgentProposal,
) -> Result<CreativeAppSummary> {
    use crate::creative_app::local;
    use crate::creative_app::model::{
        LaunchMode, LaunchPlanSource, LaunchProgram, LocalLaunchRuntime, LocalProjectKind,
    };
    use crate::creative_app::proposal::ProposedDriver;

    match &proposal.driver {
        ProposedDriver::StaticHttp => {
            // Static HTTP app inside the proposed project root.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            let kind = if root.join("index.html").is_file() {
                LocalProjectKind::Html
            } else {
                LocalProjectKind::Unknown
            };
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: kind,
                runtime: LocalLaunchRuntime::StaticHttp,
                program: LaunchProgram::Internal,
                cwd_relative: ".".into(),
                script: None,
                entry_file: Some("index.html".into()),
                script_runner: None,
                args: vec![],
                environment_keys: proposal.environment_keys.clone(),
                port: proposal_port(proposal),
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: 60_000,
                auto_open: true,
                confidence: Some(0.9),
                reason: "agent proposal approved".into(),
                compose: None,
                process_profile: None,
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(60_000),
                },
            )?;
            Ok(summary)
        }
        ProposedDriver::Python(p) => {
            // Python WebUI: build a managed-process plan carrying the profile.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            crate::creative_app::process_driver::validate_python_profile(p)?;
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: LocalProjectKind::Unknown,
                runtime: LocalLaunchRuntime::NodeDevServer, // managed-process family
                program: LaunchProgram::Node,
                cwd_relative: p.cwd_relative.clone(),
                script: Some(p.entry.clone()),
                entry_file: Some(p.entry.clone()),
                script_runner: None,
                args: p.args.clone(),
                environment_keys: proposal.environment_keys.clone(),
                port: p.port.clone(),
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: p.startup_timeout_ms,
                auto_open: true,
                confidence: Some(0.85),
                reason: "agent proposal: python webui".into(),
                compose: None,
                process_profile: Some(crate::creative_app::model::ProcessProfile::Python(
                    p.clone(),
                )),
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(p.startup_timeout_ms),
                },
            )?;
            Ok(summary)
        }
        ProposedDriver::Binary(b) => {
            // Binary WebUI: build a managed-process plan carrying the profile.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            crate::creative_app::process_driver::validate_binary_profile(b)?;
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: LocalProjectKind::Unknown,
                runtime: LocalLaunchRuntime::NodeDevServer, // managed-process family
                program: LaunchProgram::Node,
                cwd_relative: b.cwd_relative.clone(),
                script: None,
                entry_file: None,
                script_runner: None,
                args: b.args.clone(),
                environment_keys: proposal.environment_keys.clone(),
                port: b.port.clone(),
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: b.startup_timeout_ms,
                auto_open: true,
                confidence: Some(0.85),
                reason: "agent proposal: binary webui".into(),
                compose: None,
                process_profile: Some(crate::creative_app::model::ProcessProfile::Binary(
                    b.clone(),
                )),
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(b.startup_timeout_ms),
                },
            )?;
            Ok(summary)
        }
        ProposedDriver::Compose {
            command,
            privileged: _,
        } => {
            // Compose app: derive the compose file from the project root.
            // The proposal gate already rejected privileged containers and
            // command overrides (validate), so `command` here is empty.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            let compose_file = [
                "docker-compose.yml",
                "docker-compose.yaml",
                "compose.yml",
                "compose.yaml",
            ]
            .iter()
            .find(|f| root.join(f).is_file())
            .map(|f| f.to_string())
            .ok_or_else(|| {
                Error::InvalidInput(
                    "no docker-compose.yml / compose.yml found in project root".into(),
                )
            })?;
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: LocalProjectKind::Unknown,
                runtime: LocalLaunchRuntime::DockerCompose,
                program: LaunchProgram::Internal,
                cwd_relative: ".".into(),
                script: None,
                entry_file: None,
                script_runner: None,
                args: vec![],
                environment_keys: proposal.environment_keys.clone(),
                port: crate::creative_app::model::LaunchPort {
                    mode: crate::creative_app::model::LaunchPortMode::Auto,
                    value: None,
                },
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: 60_000,
                auto_open: true,
                confidence: Some(0.8),
                reason: "agent proposal: docker compose".into(),
                compose: Some(crate::creative_app::model::ComposePlanDetail {
                    compose_file,
                    project_seed: "agent".into(),
                    service: None,
                    command: command.clone().unwrap_or_default(),
                    health_path: proposal.health_path.clone(),
                    host_port: None,
                }),
                process_profile: None,
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(60_000),
                },
            )?;
            Ok(summary)
        }
    }
}

/// Derive a LaunchPort from a proposal's intended open path (auto port).
fn proposal_port(
    _proposal: &crate::creative_app::proposal::AgentProposal,
) -> crate::creative_app::model::LaunchPort {
    crate::creative_app::model::LaunchPort {
        mode: crate::creative_app::model::LaunchPortMode::Auto,
        value: None,
    }
}

/// Launch a proposal-approved app through start→health→endpoint (T09).
///
/// Runs under the per-app mutation lock. A start failure returns Err; the
/// caller keeps the proposal approved and the app's honest state (StartFailed)
/// so a retry stays possible from the catalog.
fn start_approved_app(
    handle: &tauri::AppHandle,
    local_runtime: &LocalRuntimeHandle,
    host_port: u16,
    pool: &DbPool,
    app_id: &str,
    lock: &MutationLock,
) -> Result<CreativeAppSummary> {
    let ctx = lifecycle_ctx(handle.clone(), local_runtime.clone(), host_port);
    let rt = tokio::runtime::Handle::current();
    let c = conn(pool)?;
    let _guard = rt.block_on(lock.acquire_app(app_id));
    let summary = rt.block_on(adapters::facade::start(&c, &ctx, app_id))?;
    runtime_store::attach_identity(&c, summary)
}

#[cfg(test)]
mod proposal_tests {
    use super::*;
    use crate::creative_app::model::OwnershipMode;
    use crate::creative_app::proposal::{AgentProposal, ProposalKind, ProposedDriver};

    fn static_proposal(root: &str) -> AgentProposal {
        AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Approved App".into(),
            project_root: root.into(),
            driver: ProposedDriver::StaticHttp,
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        }
    }

    #[test]
    fn static_proposal_registers_local_app() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<html>hi</html>").unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = static_proposal(&root_s);
        proposal.validate().unwrap();

        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Approved App");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
        assert_eq!(summary.runtime, CreativeAppRuntime::LocalStatic);
    }

    #[test]
    fn duplicate_proposal_path_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<html>hi</html>").unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = static_proposal(&root_s);
        proposal.validate().unwrap();
        let _first = register_proposal_app(&mut conn, &proposal).unwrap();

        // Registering the same path again must fail.
        let err = register_proposal_app(&mut conn, &proposal).unwrap_err();
        assert!(err.to_string().contains("already registered"));
    }

    #[test]
    fn compose_proposal_registers_local_app() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("composeproj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("compose.yml"),
            "services:\n  web:\n    image: nginx\n",
        )
        .unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Compose".into(),
            project_root: root_s,
            driver: ProposedDriver::Compose {
                command: None,
                privileged: false,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        proposal.validate().unwrap();
        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Compose");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
    }

    #[test]
    fn compose_proposal_without_compose_file_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("nocompose");
        std::fs::create_dir_all(&root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Compose".into(),
            project_root: root_s,
            driver: ProposedDriver::Compose {
                command: None,
                privileged: false,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        proposal.validate().unwrap();
        let err = register_proposal_app(&mut conn, &proposal).unwrap_err();
        assert!(err.to_string().contains("compose.yml"), "got: {err}");
    }

    #[test]
    fn python_proposal_registers_local_app() {
        use crate::creative_app::model::{LaunchPort, LaunchPortMode, PythonLaunchProfile};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pyproj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("app.py"), "from flask import Flask\n").unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let py = PythonLaunchProfile {
            schema_version: 1,
            interpreter: "/usr/bin/python3".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec!["PORT".into()],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            is_venv: false,
        };
        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Python App".into(),
            project_root: root_s,
            driver: ProposedDriver::Python(py),
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        };
        proposal.validate().unwrap();
        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Python App");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
    }

    #[test]
    fn binary_proposal_registers_local_app() {
        use crate::creative_app::model::{BinaryLaunchProfile, LaunchPort, LaunchPortMode};
        use crate::creative_app::process_driver::sha256_hex;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("binproj");
        std::fs::create_dir_all(&root).unwrap();
        // A real executable file so the hash is computed from actual content.
        let bin_path = root.join("myapp");
        std::fs::write(&bin_path, "#!/bin/sh\necho hi\n").unwrap();
        let hash = sha256_hex(&bin_path).unwrap();
        let bin_s = bin_path.to_string_lossy().to_string();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let b = BinaryLaunchProfile {
            schema_version: 1,
            executable_path: bin_s,
            executable_hash: hash,
            approved: true,
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
        };
        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Binary App".into(),
            project_root: root_s,
            driver: ProposedDriver::Binary(b),
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        proposal.validate().unwrap();
        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Binary App");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
    }
}
