use super::*;

    fn mem() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();
        conn
    }

    fn sample_rec() -> LocalCreativeAppRecord {
        let t = now();
        LocalCreativeAppRecord {
            id: "loc1".into(),
            title: "Local".into(),
            description: None,
            icon: None,
            canonical_project_root: "/tmp/proj".into(),
            device_id: "d".into(),
            device_name: "n".into(),
            project_kind: LocalProjectKind::Vite,
            launch_mode: LaunchMode::Smart,
            launch_plan_json: "{}".into(),
            plan_fingerprint: "fp".into(),
            state: CreativeAppState::Running,
            status_detail_json: None,
            open_url: Some("http://127.0.0.1:5173/".into()),
            current_port: Some(5173),
            process_identity_json: Some(r#"{"pid":123,"processGroupId":123}"#.into()),
            volume_identity: String::new(),
            auto_open: true,
            startup_timeout_ms: 60_000,
            last_started_at: Some(t.clone()),
            last_exit_reason: None,
            last_error: None,
            created_at: t.clone(),
            updated_at: t,
        }
    }

    #[test]
    fn identity_without_pid_not_orphan() {
        let id = ProcessIdentity::default();
        assert!(!identity_matches_live(&id));
    }

    /// Batch 3 CR-301 (#22): a late exit from a superseded run must settle only
    /// that instance — a newer active run and the source record stay untouched.
    #[test]
    fn stale_run_exit_does_not_stop_newer_run() {
        let conn = mem();
        let app = crate::creative_app::runtime_store::find_or_create_application(
            &conn,
            CreativeAppSource::LocalProject,
            "loc1",
        )
        .unwrap();
        // The source record mirrors the CURRENT (run 2) running state.
        let mut rec = sample_rec();
        rec.process_identity_json = None;
        store::insert_app(&conn, &rec).unwrap();

        let i1 =
            crate::creative_app::runtime_store::create_instance(&conn, &app, None, "local_process")
                .unwrap();
        crate::creative_app::runtime_store::mark_running(&conn, &i1, &[], None, None, None)
            .unwrap();
        // Restart: run 1 is settled stopped, run 2 becomes active.
        crate::creative_app::runtime_store::mark_stopping(&conn, &i1).unwrap();
        crate::creative_app::runtime_store::mark_stopped(&conn, &i1).unwrap();
        let i2 =
            crate::creative_app::runtime_store::create_instance(&conn, &app, None, "local_process")
                .unwrap();
        crate::creative_app::runtime_store::mark_running(
            &conn,
            &i2,
            &["http://127.0.0.1:5173/".into()],
            Some(5173),
            None,
            None,
        )
        .unwrap();

        // Run 1's exit arrives late (after run 2 is active).
        mark_process_exited::<tauri::Wry>(&conn, None, &i1, 3).unwrap();

        let (s1, s2): (String, String) = conn
            .query_row(
                "SELECT (SELECT status FROM runtime_instances WHERE id = ?1),
                        (SELECT status FROM runtime_instances WHERE id = ?2)",
                [&i1, &i2],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(s1, "stopped", "run 1's instance settles with its exit");
        assert_eq!(s2, "running", "run 2 must stay untouched");
        let src = store::get_app(&conn, "loc1").unwrap().unwrap();
        assert_eq!(
            src.state,
            CreativeAppState::Running,
            "source record must stay running (it mirrors run 2)"
        );
    }

    /// P0: a failed stop must never write installed_stopped and must preserve the
    /// identity/port/url so a retry stop stays possible.
    #[test]
    fn stop_failure_keeps_identity_and_non_stopped_state() {
        let rec = sample_rec();
        let failed = record_stop_outcome(rec, false, Some("process group 123 still alive".into()));
        assert_eq!(failed.state, CreativeAppState::CleanupFailed);
        assert_ne!(failed.state.as_str(), "installed_stopped");
        assert!(
            failed.process_identity_json.is_some(),
            "identity must be preserved on stop failure"
        );
        assert_eq!(failed.current_port, Some(5173), "port must be preserved");
        assert_eq!(
            failed.open_url.as_deref(),
            Some("http://127.0.0.1:5173/"),
            "url must be preserved"
        );
        assert!(failed.last_error.is_some());
        let detail: CreativeAppStatusDetail =
            serde_json::from_str(failed.status_detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail.code, LocalCreativeIssueCode::StopFailed);
    }

    #[test]
    fn stop_success_clears_runtime_fields() {
        let rec = sample_rec();
        let ok = record_stop_outcome(rec, true, None);
        assert_eq!(ok.state, CreativeAppState::InstalledStopped);
        assert!(ok.process_identity_json.is_none());
        assert!(ok.current_port.is_none());
        assert!(ok.open_url.is_none());
        assert_eq!(ok.last_exit_reason.as_deref(), Some("stopped_by_user"));
    }

    /// Batch 9, scenario 9 (Host/Daemon crash): a Running record whose live
    /// process survived must reconcile to orphaned — never to a false stopped —
    /// and the runtime instance settles to orphaned.
    #[tokio::test]
    async fn reconcile_marks_live_leftover_as_orphaned() {
        use std::process::Stdio;
        use tokio::process::Command;

        let Ok(_) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available");
            return;
        };

        let conn = mem();
        let cwd = std::env::temp_dir().join(format!("natives-orphan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("package.json"), "{}").unwrap();

        let port = super::runtime::pick_free_port();
        let js =
            format!("require('http').createServer((q,s)=>s.end('ok')).listen({port},'127.0.0.1');");
        let mut cmd = Command::new("node");
        cmd.arg("-e")
            .arg(&js)
            .current_dir(&cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn node");
        let pid = child.id().expect("pid");
        // Wait until the port is bound so the identity is unquestionably live.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !super::runtime::port_listening(port) {
            assert!(
                std::time::Instant::now() < deadline,
                "node never bound port"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let mut rec = sample_rec();
        rec.canonical_project_root = cwd.to_string_lossy().to_string();
        rec.current_port = Some(port);
        rec.open_url = Some(format!("http://127.0.0.1:{port}/"));
        rec.process_identity_json =
            Some(serde_json::to_string(&build_live_identity(pid, &cwd, "fp")).unwrap());
        store::insert_app(&conn, &rec).unwrap();
        // Unified identity + a running instance so settle_instance can mirror.
        let app = crate::creative_app::runtime_store::find_or_create_application(
            &conn,
            CreativeAppSource::LocalProject,
            "loc1",
        )
        .unwrap();
        let iid =
            crate::creative_app::runtime_store::create_instance(&conn, &app, None, "local_process")
                .unwrap();
        crate::creative_app::runtime_store::mark_running(
            &conn,
            &iid,
            &[rec.open_url.clone().unwrap()],
            Some(port),
            Some(pid as i32),
            Some(pid),
        )
        .unwrap();

        // The Host "crashed": reconcile sees a Running record with a live process.
        reconcile_local_apps(&conn, None).unwrap();

        let got = store::get_app(&conn, "loc1").unwrap().unwrap();
        assert_eq!(
            got.state,
            CreativeAppState::Orphaned,
            "a live leftover must be orphaned, never a false stopped"
        );
        assert!(
            got.process_identity_json.is_some(),
            "identity must be preserved for a retry stop"
        );
        let inst_status: String = conn
            .query_row(
                "SELECT status FROM runtime_instances WHERE id = ?1",
                [&iid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(inst_status, "orphaned", "instance must settle to orphaned");

        // The node server is still serving — kill the group then reap.
        unsafe {
            let _ = libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let _ = child.wait().await;
        let _ = std::fs::remove_dir_all(&cwd);
    }

    fn build_live_identity(pid: u32, cwd: &std::path::Path, fp: &str) -> ProcessIdentity {
        use sysinfo::{Pid, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
        let p = sys.process(Pid::from_u32(pid)).expect("live process");
        ProcessIdentity {
            pid: Some(pid),
            started_at_unix: Some(p.start_time() as i64),
            executable: p.exe().map(|e| e.to_string_lossy().to_string()),
            cwd: Some(cwd.to_string_lossy().to_string()),
            plan_fingerprint: Some(fp.to_string()),
            process_group_id: Some(pid as i32),
        }
    }

    /// T09 acceptance: a real fixture E2E through the local process driver.
    ///
    /// A real node HTTP server is spawned (hermetic temp dir, random port);
    /// start→health→endpoint must write REAL ServiceInstance + RuntimeEndpoint
    /// rows; stop must verify release and leave zero live resources (process,
    /// port, reader/health/log tasks).
    #[tokio::test]
    async fn local_process_driver_e2e_writes_services_and_releases_resources() {
        let Ok(_) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available");
            return;
        };

        let conn = mem();
        let rt = new_runtime_manager();
        let cwd = std::env::temp_dir().join(format!("natives-t09-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            cwd.join("server.js"),
            "require('http').createServer((q,s)=>s.end('ok')).listen(process.env.PORT,'127.0.0.1');",
        )
        .unwrap();

        let plan = LaunchPlan {
            schema_version: 1,
            source: LaunchPlanSource::Rule,
            project_kind: LocalProjectKind::Vite,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: LaunchProgram::Node,
            cwd_relative: ".".into(),
            script: Some("server.js".into()),
            entry_file: Some("server.js".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 15_000,
            auto_open: false,
            confidence: None,
            reason: "t09 e2e".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        };
        let t = now();
        let rec = LocalCreativeAppRecord {
            id: "loc-e2e".into(),
            title: "E2E".into(),
            description: None,
            icon: None,
            canonical_project_root: cwd.to_string_lossy().to_string(),
            device_id: "d".into(),
            device_name: "n".into(),
            project_kind: LocalProjectKind::Vite,
            launch_mode: LaunchMode::Smart,
            launch_plan_json: plan.to_json().unwrap(),
            plan_fingerprint: "fp-e2e".into(),
            state: CreativeAppState::InstalledStopped,
            status_detail_json: None,
            open_url: None,
            current_port: None,
            process_identity_json: None,
            volume_identity: String::new(),
            auto_open: false,
            startup_timeout_ms: 15_000,
            last_started_at: None,
            last_exit_reason: None,
            last_error: None,
            created_at: t.clone(),
            updated_at: t,
        };
        store::insert_app(&conn, &rec).unwrap();
        let app_id = crate::creative_app::runtime_store::find_or_create_application(
            &conn,
            CreativeAppSource::LocalProject,
            "loc-e2e",
        )
        .unwrap();
        let instance_id = crate::creative_app::runtime_store::create_instance(
            &conn,
            &app_id,
            None,
            "local_process",
        )
        .unwrap();
        // The driver begins the lifecycle with a real service row (T09).
        crate::creative_app::service_store::upsert_main_service(&conn, &instance_id).unwrap();

        let mock = tauri::test::mock_app();
        let handle = mock.handle().clone();

        // Phase 1: spawn (port lease held until the child binds).
        let _spawned = start_app(&conn, &handle, &rt, 18080, "loc-e2e", &instance_id)
            .await
            .expect("spawn");
        // Phase 2: health → Running + endpoint write.
        let summary = await_start_ready(&conn, &handle, &rt, "loc-e2e", &instance_id)
            .await
            .expect("healthy");
        assert_eq!(summary.state, CreativeAppState::Running);
        let port = summary
            .local_project
            .as_ref()
            .and_then(|_| {
                store::get_app(&conn, "loc-e2e")
                    .unwrap()
                    .unwrap()
                    .current_port
            })
            .expect("real port from health pass");

        // ServiceInstance + RuntimeEndpoint rows are REAL, not store-only.
        let services =
            crate::creative_app::service_store::list_services(&conn, &instance_id).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "main");
        assert_eq!(services[0].readiness, "ready");
        let endpoints =
            crate::creative_app::surface_store::list_endpoints(&conn, &instance_id).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].kind, "preview");
        assert!(endpoints[0].url.contains(&port.to_string()));
        assert_eq!(
            services[0].endpoint_id.as_deref(),
            Some(endpoints[0].id.as_str())
        );

        // The child is live on the real port.
        assert!(rt.is_running(&instance_id).await);
        assert!(super::runtime::port_listening(port));

        // Stop: verified release → zero live resources.
        let stopped = stop_app(&conn, &handle, &rt, "loc-e2e", &instance_id)
            .await
            .expect("stop");
        assert_eq!(stopped.state, CreativeAppState::InstalledStopped);
        assert!(!rt.is_running(&instance_id).await);
        assert!(
            !super::runtime::port_listening(port),
            "port must be released after a verified stop"
        );
        assert!(
            rt.live_runtime_ids().await.is_empty(),
            "no live process slot may remain after stop"
        );
        let services_after =
            crate::creative_app::service_store::list_services(&conn, &instance_id).unwrap();
        assert_eq!(
            services_after[0].readiness,
            crate::creative_app::model::ServiceInstance::READY_STOPPED
        );
        let endpoints_after =
            crate::creative_app::surface_store::list_endpoints(&conn, &instance_id).unwrap();
        assert!(endpoints_after.is_empty(), "endpoints cleared after stop");

        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// T09: the LocalStatic driver completes synchronously (no process) and
    /// still writes a real ServiceInstance + RuntimeEndpoint projection.
    #[tokio::test]
    async fn local_static_driver_e2e_writes_services_and_endpoint() {
        let conn = mem();
        let rt = new_runtime_manager();
        let cwd = std::env::temp_dir().join(format!("natives-t09-static-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("index.html"), "<html><body>hi</body></html>").unwrap();

        let plan = LaunchPlan {
            schema_version: 1,
            source: LaunchPlanSource::Rule,
            project_kind: LocalProjectKind::Html,
            runtime: LocalLaunchRuntime::StaticHttp,
            program: LaunchProgram::Internal,
            cwd_relative: ".".into(),
            script: None,
            entry_file: Some("index.html".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 15_000,
            auto_open: false,
            confidence: None,
            reason: "t09 static".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        };
        let t = now();
        let rec = LocalCreativeAppRecord {
            id: "loc-static".into(),
            title: "Static".into(),
            description: None,
            icon: None,
            canonical_project_root: cwd.to_string_lossy().to_string(),
            device_id: "d".into(),
            device_name: "n".into(),
            project_kind: LocalProjectKind::Html,
            launch_mode: LaunchMode::Smart,
            launch_plan_json: plan.to_json().unwrap(),
            plan_fingerprint: "fp-static".into(),
            state: CreativeAppState::InstalledStopped,
            status_detail_json: None,
            open_url: None,
            current_port: None,
            process_identity_json: None,
            volume_identity: String::new(),
            auto_open: false,
            startup_timeout_ms: 15_000,
            last_started_at: None,
            last_exit_reason: None,
            last_error: None,
            created_at: t.clone(),
            updated_at: t,
        };
        store::insert_app(&conn, &rec).unwrap();
        let app_id = crate::creative_app::runtime_store::find_or_create_application(
            &conn,
            CreativeAppSource::LocalProject,
            "loc-static",
        )
        .unwrap();
        let instance_id =
            crate::creative_app::runtime_store::create_instance(&conn, &app_id, None, "host_http")
                .unwrap();
        crate::creative_app::service_store::upsert_main_service(&conn, &instance_id).unwrap();

        let mock = tauri::test::mock_app();
        let handle = mock.handle().clone();

        let spawned = start_app(&conn, &handle, &rt, 18081, "loc-static", &instance_id)
            .await
            .expect("static spawn");
        assert_eq!(spawned.state, CreativeAppState::Running);
        let summary = await_start_ready(&conn, &handle, &rt, "loc-static", &instance_id)
            .await
            .expect("static ready");
        assert_eq!(summary.state, CreativeAppState::Running);

        // Service + endpoint rows are real.
        let services =
            crate::creative_app::service_store::list_services(&conn, &instance_id).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].readiness, "ready");
        let endpoints =
            crate::creative_app::surface_store::list_endpoints(&conn, &instance_id).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert!(
            endpoints[0].url.contains("local-projects"),
            "static endpoint must point at the host HTTP local-projects route: {}",
            endpoints[0].url
        );

        let stopped = stop_app(&conn, &handle, &rt, "loc-static", &instance_id)
            .await
            .expect("static stop");
        assert_eq!(stopped.state, CreativeAppState::InstalledStopped);
        let services_after =
            crate::creative_app::service_store::list_services(&conn, &instance_id).unwrap();
        assert_eq!(services_after[0].readiness, "stopped");
        assert!(
            crate::creative_app::surface_store::list_endpoints(&conn, &instance_id)
                .unwrap()
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// T09: the Python driver runs a REAL python HTTP server through the
    /// Host-trusted interpreter (T06) and releases everything on stop.
    #[tokio::test]
    async fn python_driver_e2e_runs_real_server_via_trusted_interpreter() {
        let Ok(_) = std::process::Command::new("python3")
            .arg("--version")
            .output()
        else {
            eprintln!("[skip] python3 not available");
            return;
        };
        // The trusted interpreter must resolve (real python, not a shell).
        let interpreter =
            match crate::creative_app::process_driver::resolve_python_interpreter("python3") {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[skip] python3 not resolvable: {e}");
                    return;
                }
            };

        let conn = mem();
        let rt = new_runtime_manager();
        let cwd = std::env::temp_dir().join(format!("natives-t09-py-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            cwd.join("server.py"),
            "from http.server import HTTPServer, BaseHTTPRequestHandler\n\
             import os\n\
             class H(BaseHTTPRequestHandler):\n\
             \x20   def do_GET(self):\n\
             \x20       self.send_response(200); self.end_headers(); self.wfile.write(b'ok')\n\
             \x20   def log_message(self, *a): pass\n\
             HTTPServer(('127.0.0.1', int(os.environ['PORT'])), H).serve_forever()\n",
        )
        .unwrap();

        let plan = LaunchPlan {
            schema_version: 1,
            source: LaunchPlanSource::Ai,
            project_kind: LocalProjectKind::Unknown,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: LaunchProgram::Node,
            cwd_relative: ".".into(),
            script: Some("server.py".into()),
            entry_file: Some("server.py".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 15_000,
            auto_open: false,
            confidence: None,
            reason: "t09 python".into(),
            compose: None,
            trade_approval: None,
            process_profile: Some(crate::creative_app::model::ProcessProfile::Python(
                crate::creative_app::model::PythonLaunchProfile {
                    schema_version: 1,
                    interpreter: interpreter.clone(),
                    entry: "server.py".into(),
                    args: vec![],
                    cwd_relative: ".".into(),
                    environment_keys: vec![],
                    port: LaunchPort {
                        mode: LaunchPortMode::Auto,
                        value: None,
                    },
                    open_path: "/".into(),
                    health_path: "/".into(),
                    startup_timeout_ms: 15_000,
                    is_venv: false,
                },
            )),
        };
        let t = now();
        let rec = LocalCreativeAppRecord {
            id: "loc-py".into(),
            title: "Python".into(),
            description: None,
            icon: None,
            canonical_project_root: cwd.to_string_lossy().to_string(),
            device_id: "d".into(),
            device_name: "n".into(),
            project_kind: LocalProjectKind::Unknown,
            launch_mode: LaunchMode::Smart,
            launch_plan_json: plan.to_json().unwrap(),
            plan_fingerprint: "fp-py".into(),
            state: CreativeAppState::InstalledStopped,
            status_detail_json: None,
            open_url: None,
            current_port: None,
            process_identity_json: None,
            volume_identity: String::new(),
            auto_open: false,
            startup_timeout_ms: 15_000,
            last_started_at: None,
            last_exit_reason: None,
            last_error: None,
            created_at: t.clone(),
            updated_at: t,
        };
        store::insert_app(&conn, &rec).unwrap();
        let app_id = crate::creative_app::runtime_store::find_or_create_application(
            &conn,
            CreativeAppSource::LocalProject,
            "loc-py",
        )
        .unwrap();
        let instance_id = crate::creative_app::runtime_store::create_instance(
            &conn,
            &app_id,
            None,
            "local_process",
        )
        .unwrap();
        crate::creative_app::service_store::upsert_main_service(&conn, &instance_id).unwrap();

        let mock = tauri::test::mock_app();
        let handle = mock.handle().clone();
        let _spawned = start_app(&conn, &handle, &rt, 18080, "loc-py", &instance_id)
            .await
            .expect("python spawn");
        let summary = await_start_ready(&conn, &handle, &rt, "loc-py", &instance_id)
            .await
            .expect("python healthy");
        assert_eq!(summary.state, CreativeAppState::Running);
        let port = store::get_app(&conn, "loc-py")
            .unwrap()
            .unwrap()
            .current_port
            .expect("python port");
        assert!(
            super::runtime::port_listening(port),
            "python server must listen"
        );

        let services =
            crate::creative_app::service_store::list_services(&conn, &instance_id).unwrap();
        assert_eq!(services[0].readiness, "ready");

        let stopped = stop_app(&conn, &handle, &rt, "loc-py", &instance_id)
            .await
            .expect("python stop");
        assert_eq!(stopped.state, CreativeAppState::InstalledStopped);
        assert!(
            !super::runtime::port_listening(port),
            "python port released"
        );
        let _ = std::fs::remove_dir_all(&cwd);
    }
