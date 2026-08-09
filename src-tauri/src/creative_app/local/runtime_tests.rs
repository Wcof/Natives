use super::*;

    #[test]
    fn free_port_is_nonzero() {
        let p = pick_free_port();
        assert!(p > 0);
    }

    #[test]
    fn static_url_shape() {
        let u = static_open_url(1234, "abc", "run-1", "/");
        assert_eq!(u, "http://127.0.0.1:1234/local-projects/run-1/abc/");
    }

    /// Batch 5: the Compose project name is stable per app and unique across apps.
    #[test]
    fn compose_project_name_is_stable_and_unique() {
        let a = compose_project_name("app-1", "freq");
        let a2 = compose_project_name("app-1", "freq");
        let b = compose_project_name("app-2", "freq");
        assert_eq!(a, a2, "same app + seed must derive the same project");
        assert_ne!(a, b, "different apps must never share a compose project");
        assert!(a.starts_with("natives-freq-"), "unexpected prefix: {a}");
        assert_eq!(
            compose_project_name("x", ""),
            compose_project_name("x", "compose")
        );
    }

    /// T06/T09: the Host-trusted binary identity is recomputed at spawn —
    /// `build_command` returns the canonical path only when the current content
    /// hash still matches the recorded approval; a swap invalidates it.
    #[test]
    fn build_command_verifies_binary_identity_at_spawn() {
        use crate::creative_app::model::{BinaryLaunchProfile, ProcessProfile};
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("myapp");
        std::fs::write(&bin, b"#!/bin/sh\necho v1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let (canonical, hash) =
            crate::creative_app::process_driver::resolve_binary_identity(bin.to_str().unwrap())
                .unwrap();

        let base_plan = LaunchPlan {
            schema_version: 1,
            source: crate::creative_app::model::LaunchPlanSource::Ai,
            project_kind: crate::creative_app::model::LocalProjectKind::Unknown,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: crate::creative_app::model::LaunchProgram::Node,
            cwd_relative: ".".into(),
            script: None,
            entry_file: None,
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            auto_open: false,
            confidence: None,
            reason: "test".into(),
            compose: None,
            trade_approval: None,
            process_profile: Some(ProcessProfile::Binary(BinaryLaunchProfile {
                schema_version: 1,
                executable_path: canonical.clone(),
                executable_hash: hash.clone(),
                approved: true,
                args: vec![],
                cwd_relative: ".".into(),
                environment_keys: vec![],
                port: crate::creative_app::model::LaunchPort {
                    mode: crate::creative_app::model::LaunchPortMode::Auto,
                    value: None,
                },
                open_path: "/".into(),
                health_path: "/".into(),
                startup_timeout_ms: 60_000,
            })),
        };

        // Same content → the canonical path is returned (spawnable).
        let (program, _) = build_command(&base_plan, 0).unwrap();
        assert_eq!(program, canonical);

        // Content swap → the identity no longer matches → refused at spawn.
        std::fs::write(&bin, b"#!/bin/sh\necho v2-swapped\n").unwrap();
        let err = build_command(&base_plan, 0).unwrap_err();
        assert!(
            err.to_string().contains("changed since approval"),
            "a swapped binary must fail identity verification: {err}"
        );

        // Restore content → passes again (hash is content-based).
        std::fs::write(&bin, b"#!/bin/sh\necho v1\n").unwrap();
        let (program2, _) = build_command(&base_plan, 0).unwrap();
        assert_eq!(program2, canonical);
    }

    /// T06/T09: the Python interpreter is re-resolved to its Host-trusted
    /// canonical path at spawn; a shell interpreter is never executed.
    #[test]
    fn build_command_requires_python_interpreter_identity() {
        use crate::creative_app::model::{ProcessProfile, PythonLaunchProfile};
        let tmp = tempfile::tempdir().unwrap();
        let py = tmp.path().join("python3");
        std::fs::write(&py, b"#!/usr/bin/env python3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let canonical =
            crate::creative_app::process_driver::resolve_python_interpreter(py.to_str().unwrap())
                .unwrap();

        let plan = LaunchPlan {
            schema_version: 1,
            source: crate::creative_app::model::LaunchPlanSource::Ai,
            project_kind: crate::creative_app::model::LocalProjectKind::Unknown,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: crate::creative_app::model::LaunchProgram::Node,
            cwd_relative: ".".into(),
            script: Some("app.py".into()),
            entry_file: Some("app.py".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            auto_open: false,
            confidence: None,
            reason: "test".into(),
            compose: None,
            trade_approval: None,
            process_profile: Some(ProcessProfile::Python(PythonLaunchProfile {
                schema_version: 1,
                interpreter: canonical.clone(),
                entry: "app.py".into(),
                args: vec![],
                cwd_relative: ".".into(),
                environment_keys: vec![],
                port: crate::creative_app::model::LaunchPort {
                    mode: crate::creative_app::model::LaunchPortMode::Auto,
                    value: None,
                },
                open_path: "/".into(),
                health_path: "/".into(),
                startup_timeout_ms: 60_000,
                is_venv: true,
            })),
        };

        // The trusted interpreter resolves; argv starts with the entry module.
        let (program, args) = build_command(&plan, 0).unwrap();
        assert_eq!(program, canonical);
        assert_eq!(args[0], "app.py");

        // A shell interpreter is refused at the spawn point.
        let mut shell = plan.clone();
        shell.process_profile = Some(ProcessProfile::Python(PythonLaunchProfile {
            schema_version: 1,
            interpreter: "/bin/sh".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            is_venv: false,
        }));
        assert!(
            build_command(&shell, 0).is_err(),
            "a shell pseudo-python must never reach the spawn point"
        );
    }

    /// Batch 6: URL candidates follow the documented priority (explicit plan
    /// port first, framework default last) and always target loopback.
    #[test]
    fn preview_url_priority_and_loopback() {
        let mut node = LaunchPlan {
            schema_version: 1,
            source: crate::creative_app::model::LaunchPlanSource::Rule,
            project_kind: crate::creative_app::model::LocalProjectKind::Vite,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: crate::creative_app::model::LaunchProgram::Npm,
            cwd_relative: ".".into(),
            script: Some("dev".into()),
            entry_file: None,
            script_runner: Some(crate::creative_app::model::ScriptRunner::Vite),
            args: vec![],
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Fixed,
                value: Some(5173),
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            auto_open: true,
            confidence: None,
            reason: "t".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        };
        let cands = resolve_preview_urls(&node, 1234, "app", "run-1");
        assert_eq!(cands[0].source, "explicit_plan");
        assert_eq!(cands[0].url, "http://127.0.0.1:5173/");

        node.port.value = None;
        let cands = resolve_preview_urls(&node, 1234, "app", "run-1");
        assert!(
            cands.is_empty(),
            "no fixed port → no deterministic candidate"
        );

        // 0.0.0.0 host must normalize to loopback for preview.
        assert_eq!(
            normalize_loopback("http://0.0.0.0:8080/api"),
            "http://127.0.0.1:8080/api"
        );
        assert_eq!(
            normalize_loopback("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/"
        );
    }

    /// P0: a process that ignores SIGTERM must still be killed (KILL follows the
    /// grace window), its process group verified gone, its port verified released,
    /// and the direct child reaped — never left as a zombie.
    #[cfg(unix)]
    #[tokio::test]
    async fn term_timeout_kills_group_and_releases_port() {
        use std::process::Stdio;
        use tokio::process::Command;

        let Ok(_v) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available; cannot verify group kill");
            return;
        };

        let port = pick_free_port();
        let js = format!(
            "process.on('SIGTERM', () => {{}}); \
             require('http').createServer((_q,s)=>s.end('ok')).listen({port}, '127.0.0.1');"
        );

        let mut cmd = Command::new("node");
        cmd.arg("-e")
            .arg(&js)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: pre_exec runs in the forked child before exec; setpgid is the
        // only libc call and its use here is the standard new-process-group pattern.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn node");

        let pid = child.id().expect("node pid") as i32;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !port_listening(port) {
            assert!(
                std::time::Instant::now() < deadline,
                "node never bound port {port}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // node ignores SIGTERM, so the grace window must end in SIGKILL.
        terminate_tree_with_grace(&mut child, Some(pid), Duration::from_millis(300)).await;

        // Direct child must be reaped.
        let _status = child.wait().await.expect("reap node child");

        assert!(
            !process_group_exists(pid),
            "process group {pid} still has members after kill"
        );
        assert!(
            wait_port_released(port, Duration::from_secs(2)),
            "port {port} still accepting connections after kill"
        );
    }

    /// Batch 2: stop must set the cancel flag (preempting health/readers) and
    /// drain the instance's tracked tasks instead of leaving them running.
    #[tokio::test]
    async fn stop_sets_cancel_flag_and_drains_tasks() {
        use std::future::pending;

        let mgr = LocalRuntimeManager::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_handle = tokio::spawn(async move {
            let _ = pending::<()>().await;
        });
        {
            let mut map = mgr.procs.lock().await;
            map.insert(
                "a-run".into(),
                LiveLocalProcess {
                    runtime_id: "a-run".into(),
                    app_id: "a".into(),
                    child: None,
                    identity: ProcessIdentity::default(),
                    plan_fingerprint: String::new(),
                    started_at: Instant::now(),
                    port: None,
                    open_url: None,
                    program: "x".into(),
                    cwd: PathBuf::from("/"),
                    log: mgr.logs.get_or_open("a", "a-run"),
                    lease: None,
                    cancelled: cancelled.clone(),
                },
            );
            mgr.task_handles
                .lock()
                .await
                .insert("a-run".into(), vec![task_handle]);
        }

        mgr.stop::<tauri::Wry>("a-run", None)
            .await
            .expect("stop succeeds");
        assert!(
            cancelled.load(Ordering::SeqCst),
            "stop must set the cancel flag before reaping"
        );
        let handles = mgr.task_handles.lock().await;
        assert!(
            !handles.contains_key("a-run"),
            "stop must drain tracked reader/health tasks"
        );
    }

    /// CR-301 (#05/#22): two runs of the same app live in separate runtime slots.
    /// live_runtime_ids / stop are keyed by runtime id, so a late event from run 1
    /// can never touch run 2's resources.
    #[tokio::test]
    async fn two_runs_of_same_app_are_isolated() {
        let mgr = LocalRuntimeManager::new();
        let mk = |rt: &str, pid: u32| LiveLocalProcess {
            runtime_id: rt.into(),
            app_id: "app-iso".into(),
            child: None,
            identity: ProcessIdentity {
                pid: Some(pid),
                ..ProcessIdentity::default()
            },
            plan_fingerprint: String::new(),
            started_at: Instant::now(),
            port: None,
            open_url: None,
            program: "x".into(),
            cwd: PathBuf::from("/"),
            log: mgr.logs.get_or_open("app-iso", rt),
            lease: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        {
            let mut map = mgr.procs.lock().await;
            map.insert("run-1".into(), mk("run-1", 101));
            map.insert("run-2".into(), mk("run-2", 202));
        }
        let mut ids = mgr.live_runtime_ids().await;
        ids.sort();
        assert_eq!(ids, vec!["run-1".to_string(), "run-2".to_string()]);
        // Stopping run 1 must not touch run 2.
        mgr.stop::<tauri::Wry>("run-1", None)
            .await
            .expect("stop run-1");
        assert_eq!(mgr.live_runtime_ids().await, vec!["run-2".to_string()]);
        mgr.purge_app_logs("app-iso");
    }

    /// CR-302: stop must return Err (never claim released) when the port stays
    /// bound after the tree is gone. The caller preserves the identity so a retry
    /// stop stays possible and never writes stopped.
    #[tokio::test]
    async fn stop_fails_when_port_not_released() {
        let mgr = LocalRuntimeManager::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        {
            let mut map = mgr.procs.lock().await;
            map.insert(
                "run-1".into(),
                LiveLocalProcess {
                    runtime_id: "run-1".into(),
                    app_id: "app-p".into(),
                    child: None,
                    identity: ProcessIdentity::default(),
                    plan_fingerprint: String::new(),
                    started_at: Instant::now(),
                    port: Some(port),
                    open_url: None,
                    program: "x".into(),
                    cwd: PathBuf::from("/"),
                    log: mgr.logs.get_or_open("app-p", "run-1"),
                    lease: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        let err = mgr.stop::<tauri::Wry>("run-1", None).await.unwrap_err();
        assert!(err.to_string().contains("stop incomplete"), "{err}");
        assert!(err.to_string().contains("port"), "{err}");
        drop(listener);
        mgr.purge_app_logs("app-p");
    }

    /// CR-302: repeated stop is idempotent — a second stop on an already-stopped
    /// runtime is Ok and drains nothing.
    #[tokio::test]
    async fn repeated_stop_is_idempotent() {
        let mgr = LocalRuntimeManager::new();
        {
            let mut map = mgr.procs.lock().await;
            map.insert(
                "run-1".into(),
                LiveLocalProcess {
                    runtime_id: "run-1".into(),
                    app_id: "app-r".into(),
                    child: None,
                    identity: ProcessIdentity::default(),
                    plan_fingerprint: String::new(),
                    started_at: Instant::now(),
                    port: None,
                    open_url: None,
                    program: "x".into(),
                    cwd: PathBuf::from("/"),
                    log: mgr.logs.get_or_open("app-r", "run-1"),
                    lease: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        mgr.stop::<tauri::Wry>("run-1", None)
            .await
            .expect("first stop");
        assert!(
            mgr.stop::<tauri::Wry>("run-1", None).await.is_ok(),
            "second stop is idempotent"
        );
        mgr.purge_app_logs("app-r");
    }

    /// CR-302: a reused PID (different start time) must NOT match the persisted
    /// identity — an unknown process is never killed by stop/reconcile (#10).
    #[cfg(unix)]
    #[tokio::test]
    async fn pid_reuse_is_rejected_by_strict_identity() {
        let Ok(_) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available");
            return;
        };
        let mut cmd = Command::new("node");
        cmd.arg("-e")
            .arg("setInterval(()=>{},1000)")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // SAFETY: pre_exec runs in the forked child before exec; setpgid is the
        // standard new-process-group pattern used elsewhere in this module.
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
        let started = process_start_time_unix(Some(pid)).expect("start time");

        let good = ProcessIdentity {
            pid: Some(pid),
            started_at_unix: Some(started),
            executable: Some("node".into()),
            cwd: Some(
                std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            ),
            plan_fingerprint: Some("fp".into()),
            process_group_id: Some(pid as i32),
        };
        assert!(
            identity_matches_live_strict(&good),
            "the correct identity must match the live process"
        );
        // A different process now owns the same PID (reuse): start time differs.
        let reused = ProcessIdentity {
            started_at_unix: Some(started + 10_000),
            ..good.clone()
        };
        assert!(
            !identity_matches_live_strict(&reused),
            "a reused PID with a different start time must be rejected"
        );
        // Unknown PID / missing fingerprint are also rejected (fail closed).
        assert!(!identity_matches_live_strict(&ProcessIdentity {
            pid: Some(pid),
            started_at_unix: Some(started),
            executable: Some("node".into()),
            cwd: Some(
                std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            ),
            plan_fingerprint: None,
            process_group_id: None,
        }));

        unsafe {
            let _ = libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let _ = child.wait().await;
    }

    /// T09 acceptance: two projects run in parallel on distinct ports and never
    /// share resources — stopping one leaves the other untouched.
    #[tokio::test]
    async fn two_projects_do_not_share_resources() {
        let Ok(_) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available");
            return;
        };
        let mgr = LocalRuntimeManager::new();
        let mock = tauri::test::mock_app();
        let handle = mock.handle().clone();

        let mk_plan = || LaunchPlan {
            schema_version: 1,
            source: crate::creative_app::model::LaunchPlanSource::Rule,
            project_kind: crate::creative_app::model::LocalProjectKind::Vite,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: crate::creative_app::model::LaunchProgram::Node,
            cwd_relative: ".".into(),
            script: Some("server.js".into()),
            entry_file: Some("server.js".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 15_000,
            auto_open: false,
            confidence: None,
            reason: "two-projects".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        };
        let mk_dir = |tag: &str| {
            let d = std::env::temp_dir()
                .join(format!("natives-t09-two-{tag}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("server.js"),
                "require('http').createServer((q,s)=>s.end('ok')).listen(process.env.PORT,'127.0.0.1');",
            )
            .unwrap();
            d
        };

        let dir_a = mk_dir("a");
        let dir_b = mk_dir("b");
        let (port_a, url_a, _id_a) = mgr
            .start_node_dev(
                &handle,
                "run-a",
                "app-a",
                &dir_a,
                &mk_plan(),
                "fp-a",
                &[],
                None,
                None,
            )
            .await
            .expect("start a");
        let (port_b, url_b, _id_b) = mgr
            .start_node_dev(
                &handle,
                "run-b",
                "app-b",
                &dir_b,
                &mk_plan(),
                "fp-b",
                &[],
                None,
                None,
            )
            .await
            .expect("start b");

        assert_ne!(port_a, port_b, "two projects must bind distinct ports");
        assert!(url_a.contains(&port_a.to_string()));
        assert!(url_b.contains(&port_b.to_string()));
        assert!(mgr.is_running("run-a").await && mgr.is_running("run-b").await);

        // Stop A — B stays live on its own port.
        mgr.stop::<tauri::test::MockRuntime>("run-a", Some(&handle))
            .await
            .expect("stop a");
        assert!(!mgr.is_running("run-a").await);
        assert!(mgr.is_running("run-b").await);
        assert!(!port_listening(port_a), "project A port must be released");
        assert!(port_listening(port_b), "project B port must stay live");

        mgr.stop::<tauri::test::MockRuntime>("run-b", Some(&handle))
            .await
            .expect("stop b");
        assert!(
            mgr.live_runtime_ids().await.is_empty(),
            "no live resources after stopping both projects"
        );
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }

    /// T09 acceptance: a process that ignores SIGTERM is still reaped by the
    /// TERM → grace → KILL chain; stop verifies the port is released.
    #[tokio::test]
    async fn stop_reaps_term_ignoring_child() {
        let Ok(_) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available");
            return;
        };
        let mgr = LocalRuntimeManager::new();
        let mock = tauri::test::mock_app();
        let handle = mock.handle().clone();

        let dir = std::env::temp_dir().join(format!("natives-t09-term-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("server.js"),
            "process.on('SIGTERM',()=>{});process.on('SIGINT',()=>{});\
             require('http').createServer((q,s)=>s.end('ok')).listen(process.env.PORT,'127.0.0.1');\
             setInterval(()=>{},1000);",
        )
        .unwrap();
        let plan = LaunchPlan {
            schema_version: 1,
            source: crate::creative_app::model::LaunchPlanSource::Rule,
            project_kind: crate::creative_app::model::LocalProjectKind::Vite,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: crate::creative_app::model::LaunchProgram::Node,
            cwd_relative: ".".into(),
            script: Some("server.js".into()),
            entry_file: Some("server.js".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 15_000,
            auto_open: false,
            confidence: None,
            reason: "term-ignore".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        };
        let (port, _url, _identity) = mgr
            .start_node_dev(
                &handle,
                "run-term",
                "app-term",
                &dir,
                &plan,
                "fp-term",
                &[],
                None,
                None,
            )
            .await
            .expect("spawn term-ignoring server");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !port_listening(port) {
            assert!(
                std::time::Instant::now() < deadline,
                "server never bound port"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        // The child ignores TERM; stop must still reap it (grace → KILL) and
        // verify the port is released.
        mgr.stop::<tauri::test::MockRuntime>("run-term", Some(&handle))
            .await
            .expect("stop reaps TERM-ignoring child");
        assert!(!mgr.is_running("run-term").await);
        assert!(!port_listening(port), "port must be released after stop");
        let _ = std::fs::remove_dir_all(&dir);
    }
