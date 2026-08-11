//! UDS integration harness: real socket + DaemonClient + Run lifecycle.
//!
//! Spins RpcServer in-process, connects via Unix domain socket, exercises:
//! create → start (detached) → getEvents/replay → cancel → terminal.
//! Also checks reconnect after drop (multi-session bootstrap).

use natives_agent_daemon::client::{client_protocol_version, DaemonClient};
use natives_agent_daemon::rpc::RpcServer;
use serde_json::json;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn temp_socket() -> PathBuf {
    // Keep path short — macOS unix socket path limit is ~104 bytes.
    PathBuf::from(format!(
        "/tmp/nuds-{}.sock",
        &uuid::Uuid::new_v4().to_string()[..8]
    ))
}

#[tokio::test]
async fn sidecar_binary_routes_engineering_project_identity() {
    let root = std::env::temp_dir().join(format!("natives-sidecar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create sidecar test directory");
    let socket = temp_socket();
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    let mut child = Command::new(env!("CARGO_BIN_EXE_natives-agent-daemon"))
        .env("NATIVES_DAEMON_SOCKET", &socket)
        .env("NATIVES_DAEMON_BOOTSTRAP", &bootstrap)
        .env("NATIVES_DB_PATH", root.join("natives.db"))
        .env("NATIVES_ASSISTANT_DB_PATH", root.join("assistant.db"))
        .env("NATIVES_RUNTIME_DIR", &root)
        .env("NATIVES_DAEMON_FIXTURE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn compiled daemon binary");

    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let result = async {
        let mut client =
            DaemonClient::connect(&socket, &bootstrap, client_protocol_version()).await?;
        let capabilities = client.call("daemon.getCapabilities", json!({})).await?;
        assert!(capabilities.to_string().contains("project.identity.list"));
        client.call("project.identity.list", json!({})).await
    }
    .await;

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_dir_all(&root);

    let listed = result.expect("compiled sidecar must route project.identity.list");
    assert!(listed["items"].is_array());
}

#[tokio::test]
async fn uds_create_start_replay_cancel_lifecycle() {
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    std::env::set_var("NATIVES_ALLOW_FIXTURE_FALLBACK", "1");
    // Multi-session bootstrap (production default) so reconnect works.
    std::env::remove_var("NATIVES_BOOTSTRAP_SINGLE_USE");
    // Isolate from any real ~/.natives DB: the in-process RpcServer opens the
    // daemon store through open_daemon_store, which honors NATIVES_DB_PATH /
    // NATIVES_ASSISTANT_DB_PATH. Without isolation a developer's real
    // assistant.db (with an older migration-11 checksum) would fail-closed on
    // startup and storage_ready would report Unavailable.
    let iso_root = std::env::temp_dir().join(format!("natives-uds-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&iso_root);
    std::env::set_var("NATIVES_DB_PATH", iso_root.join("natives.db"));
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", iso_root.join("assistant.db"));
    std::env::set_var("NATIVES_RUNTIME_DIR", &iso_root);

    let sock = temp_socket();
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    let sock_str = sock.to_string_lossy().to_string();

    let server = RpcServer::new(&sock_str, &bootstrap, "2.0.0", "0.1.0-test");
    let server_task = tokio::spawn(async move {
        if let Err(e) = server.run().await {
            eprintln!("RpcServer exited: {e}");
        }
    });

    // Wait for socket
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        if server_task.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        sock.exists(),
        "server socket should appear at {} (task finished={})",
        sock.display(),
        server_task.is_finished()
    );

    let mut client = DaemonClient::connect(&sock, &bootstrap, client_protocol_version())
        .await
        .expect("connect+handshake");

    // ping
    let pong = client.call("daemon.ping", json!({})).await.expect("ping");
    assert!(pong.get("pong").is_some() || pong.get("ok").is_some() || !pong.is_null());
    let daemon_status = client
        .call("daemon.getStatus", json!({}))
        .await
        .expect("daemon.getStatus");
    // W3 P0-03: DaemonStatusV2 deliberately carries NO db path. Readiness is
    // decomposed into protocol_version + storage/broker readiness + health;
    // a healthy sidecar must never be judged by a legacy db-path field.
    assert_eq!(
        daemon_status
            .get("protocol_version")
            .and_then(|v| v.as_str()),
        Some(natives_agent_daemon::client_protocol_version())
    );
    assert_eq!(
        daemon_status.get("storage_ready").and_then(|v| v.as_str()),
        Some("ready")
    );
    assert!(matches!(
        daemon_status.get("health").and_then(|v| v.as_str()),
        Some("ready") | Some("degraded")
    ));

    // create
    let created = client
        .call(
            "run.create",
            json!({
                "conversation_id": "uds-conv-1",
                "provider_id": "openai",
                "model_id": "gpt-4o",
                "key_id": "k-test",
                "permission_profile": "full_access",
                "content": "hello from uds harness",
                "max_steps": 5,
                "project_path": "/tmp/natives-uds-project",
                "idempotency_key": format!("uds-idem-{}", uuid::Uuid::new_v4()),
            }),
        )
        .await
        .expect("run.create");
    let run_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .expect("run id")
        .to_string();

    // start detached
    let started = client
        .call(
            "run.start",
            json!({
                "run_id": run_id,
                "provider_id": "openai",
                "model_id": "gpt-4o",
                "key_id": "k-test",
                "content": "hello from uds harness",
                "permission_profile": "full_access",
                "max_steps": 5,
                "project_path": "/tmp/natives-uds-project",
            }),
        )
        .await
        .expect("run.start");
    let status = started.get("status").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        status == "preparing" || status == "running" || status == "queued",
        "expected non-terminal immediate status, got {status}"
    );

    // poll events until terminal or timeout
    let mut last_seq = 0u64;
    let mut terminal = false;
    for _ in 0..80 {
        let events = client
            .call(
                "run.getEvents",
                json!({ "run_id": run_id, "after_sequence": last_seq }),
            )
            .await
            .unwrap_or(json!([]));
        let arr = if let Some(a) = events.as_array() {
            a.clone()
        } else if let Some(a) = events.get("events").and_then(|e| e.as_array()) {
            a.clone()
        } else {
            vec![]
        };
        for ev in &arr {
            if let Some(seq) = ev.get("sequence").and_then(|s| s.as_u64()) {
                last_seq = last_seq.max(seq);
            }
            let t = ev
                .get("type")
                .or_else(|| ev.get("event_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // payload may nest kind
            let kind = ev
                .get("payload")
                .and_then(|p| p.get("type").or_else(|| p.get("kind")))
                .and_then(|v| v.as_str())
                .unwrap_or(t);
            if kind.contains("completed")
                || kind.contains("failed")
                || kind.contains("interrupted")
                || t.contains("completed")
                || t.contains("failed")
                || t.contains("interrupted")
            {
                terminal = true;
            }
        }
        if terminal {
            break;
        }
        // cancel mid-flight if still running after a bit
        if last_seq > 0 {
            let _ = client.call("run.cancel", json!({ "run_id": run_id })).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // replay from 0 must be ordered
    let replay = client
        .call(
            "run.replay",
            json!({ "run_id": run_id, "after_sequence": 0 }),
        )
        .await
        .expect("replay");
    let replay_arr = if let Some(a) = replay.as_array() {
        a.clone()
    } else if let Some(a) = replay.get("events").and_then(|e| e.as_array()) {
        a.clone()
    } else {
        vec![]
    };
    let mut prev = 0u64;
    for ev in &replay_arr {
        if let Some(seq) = ev.get("sequence").and_then(|s| s.as_u64()) {
            assert!(seq > prev, "sequence must be monotonic: {prev} -> {seq}");
            prev = seq;
        }
    }
    assert!(
        !replay_arr.is_empty() || terminal || last_seq > 0,
        "expected some events from fixture engine or cancel"
    );

    // reconnect path: drop client, reconnect, list runs
    drop(client);
    let mut client2 = DaemonClient::connect(&sock, &bootstrap, client_protocol_version())
        .await
        .expect("reconnect handshake");
    let listed = client2
        .call("run.list", json!({}))
        .await
        .expect("run.list after reconnect");
    let runs = listed
        .get("runs")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        runs.iter()
            .any(|r| r.get("id").and_then(|i| i.as_str()) == Some(run_id.as_str())),
        "run should still be listed after reconnect"
    );

    // capabilities honesty
    let caps = client2
        .call("daemon.getCapabilities", json!({}))
        .await
        .expect("capabilities");
    assert_eq!(
        caps.get("protocol_version").and_then(|v| v.as_str()),
        Some("2.0.0")
    );
    let methods = caps
        .get("methods")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(methods.iter().any(|m| m.as_str() == Some("run.start")));
    assert!(
        !methods
            .iter()
            .any(|m| m.as_str() == Some("not.a.real.method")),
        "must not invent methods"
    );

    server_task.abort();
    let _ = std::fs::remove_file(&sock);
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");
    std::env::remove_var("NATIVES_ALLOW_FIXTURE_FALLBACK");
}
