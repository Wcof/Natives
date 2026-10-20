mod support;
use app_runtime_core::framing::{read_frame, write_frame};
use app_runtime_core::protocol::{AppRequest, AppResponse, HandshakeResultV2, StartResult};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TEST_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn setup_test_apps_root(root: &std::path::Path, app_id: &str) -> std::path::PathBuf {
    let apps_root = root.join("apps");
    std::fs::create_dir_all(apps_root.join(app_id)).unwrap();
    let runtime_sha = app_runtime_core::sha256_hex(&std::fs::read(bin_path()).unwrap());
    let act = serde_json::json!({
        "receiptVersion": 3,
        "productVersion": "0.1.0",
        "productGeneration": 1,
        "productManifestSha256": "0".repeat(64),
        "runtime": { "host": "com.natives.local.app_runtime", "protocolVersion": 2, "sha256": runtime_sha },
        "modules": {
            "fund": { "moduleApiVersion": 1, "dataSchemaVersion": 4, "capabilityVersion": 2, "uiTreeSha256": "0".repeat(64) },
            "tokenusage": { "moduleApiVersion": 1, "dataSchemaVersion": 2, "capabilityVersion": 2, "uiTreeSha256": "0".repeat(64) }
        },
        "allowedOrigins": [TEST_ORIGIN]
    });
    std::fs::create_dir_all(root.join("product")).unwrap();
    std::fs::write(
        root.join("product/activation.json"),
        serde_json::to_vec(&act).unwrap(),
    )
    .unwrap();
    apps_root
}

fn bin_path() -> std::path::PathBuf {
    support::runtime_binary()
}

#[test]
fn test_app_runtime_health() {
    let output = Command::new(bin_path())
        .arg("--health")
        .output()
        .expect("failed to execute natives-app-runtime --health");

    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\"ok\":true"));
    assert!(text.contains("\"protocolVersion\":2"));
    assert!(text.contains("fund"));
    // Release Module Gate（计划 §45.2/§13）：正式 Runtime 不得出现 sample/fixture。
    // 仅对未启用 test-fixture 的构建生效；fixture 构建由 multi_module 测试单独验证。
    if !cfg!(feature = "test-fixture") {
        assert!(
            !text.contains("sample"),
            "release runtime must not contain sample module"
        );
        assert!(
            !text.contains("fixture"),
            "release runtime must not contain fixture module"
        );
    }
}

/// 计划 §44：test-only 第二模块验证——test-fixture feature 下 Registry
/// 包含两个模块，证明架构不是"只对 Fund 有效"。CI 以
/// `cargo test -p app-runtime --features test-fixture` 单独运行。
#[cfg(feature = "test-fixture")]
#[test]
fn test_app_runtime_multi_module_registry() {
    let output = Command::new(bin_path())
        .arg("--health")
        .output()
        .expect("failed to execute natives-app-runtime --health");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("fund"));
    assert!(text.contains("fixture-second"));
}

#[test]
fn test_app_runtime_lifecycle_roundtrip_and_fast_eof() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn app-runtime");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // 1. Send app:handshake
    let handshake_req = AppRequest {
        id: "req-1".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "appId": "fund",
            "productVersion": "0.1.0",
            "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&handshake_req).unwrap()).unwrap();

    let frame = read_frame(&mut stdout).unwrap().expect("handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok);
    let hs = resp.result.unwrap();
    assert_eq!(hs.app_id, "fund");
    assert_eq!(hs.state, "initialized");

    // 2. Send app:start
    let start_req = AppRequest {
        id: "req-2".into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&start_req).unwrap()).unwrap();

    let frame = read_frame(&mut stdout).unwrap().expect("start frame");
    let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok);
    let start = resp.result.unwrap();
    assert_eq!(start.state, "ready");
    assert!(start.port > 0);

    // 3. Connect to loopback HTTP
    let mut http = TcpStream::connect(("127.0.0.1", start.port)).unwrap();
    http.write_all(
        format!(
            "GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            start.port
        )
        .as_bytes(),
    )
    .unwrap();
    let mut http_resp = String::new();
    http.read_to_string(&mut http_resp).unwrap();
    assert!(http_resp.contains("200 OK"));

    // 4. EOF shutdown timing test: close stdin and verify exit <= 2 seconds
    let t0 = Instant::now();
    drop(stdin);
    let status = child.wait().unwrap();
    let elapsed = t0.elapsed();
    assert!(status.success());
    assert!(
        elapsed <= Duration::from_secs(2),
        "Process exit on EOF took {:?}, expected <= 2s",
        elapsed
    );
}

#[test]
fn test_app_runtime_same_module_handshake_reuses_binding() {
    // 一个 Runtime Process 只绑定一个 appId；同 appId 的后续握手复用绑定。
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn app-runtime");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    let handshake = |id: &str| AppRequest {
        id: id.into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "appId": "fund",
            "productVersion": "0.1.0",
            "activationGeneration": 1
        }),
    };

    // 第一次握手成功
    write_frame(&mut stdin, &serde_json::to_vec(&handshake("hs-1")).unwrap()).unwrap();
    let frame = read_frame(&mut stdout)
        .unwrap()
        .expect("first handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "first handshake failed: {:?}", resp.error);

    // 同模块第二个 Surface 复用现有绑定，不重新初始化模块。
    write_frame(&mut stdin, &serde_json::to_vec(&handshake("hs-2")).unwrap()).unwrap();
    let frame = read_frame(&mut stdout)
        .unwrap()
        .expect("second handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(
        resp.ok,
        "same-module handshake must reuse binding: {:?}",
        resp.error
    );
    assert_eq!(resp.result.unwrap().app_id, "fund");

    let other_module = AppRequest {
        id: "hs-other".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "appId": "tokenusage",
            "productVersion": "0.1.0",
            "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&other_module).unwrap()).unwrap();
    let frame = read_frame(&mut stdout)
        .unwrap()
        .expect("other-module handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(!resp.ok, "another module must not reuse this runtime");
    assert_eq!(resp.error.unwrap().code, "APP_RUNTIME_ALREADY_BOUND");

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_app_runtime_start_before_handshake_rejected() {
    // 计划 §41：Start before handshake → FAIL（APP_RUNTIME_NOT_INITIALIZED）。
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn app-runtime");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    let start_req = AppRequest {
        id: "st-early".into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&start_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("early start frame");
    let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
    assert!(!resp.ok, "start before handshake must fail");
    let error = resp.error.expect("early start error body");
    assert_eq!(error.code, "APP_RUNTIME_NOT_INITIALIZED", "{error:?}");

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_app_runtime_start_twice_reuses_listener() {
    // 同模块第二个 Surface 复用现有 listener，不再绑定第二个端口。
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn app-runtime");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    let handshake_req = AppRequest {
        id: "hs".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "appId": "fund",
            "productVersion": "0.1.0",
            "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&handshake_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "handshake failed: {:?}", resp.error);

    let start = |id: &str| AppRequest {
        id: id.into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };

    write_frame(&mut stdin, &serde_json::to_vec(&start("st-1")).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("first start frame");
    let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "first start failed: {:?}", resp.error);
    let port1 = resp.result.unwrap().port;

    write_frame(&mut stdin, &serde_json::to_vec(&start("st-2")).unwrap()).unwrap();
    let frame = read_frame(&mut stdout)
        .unwrap()
        .expect("second start frame");
    let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
    assert!(
        resp.ok,
        "second start must reuse listener: {:?}",
        resp.error
    );
    assert_eq!(resp.result.unwrap().port, port1);

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_app_runtime_app_stop_exits_promptly() {
    // 计划 §41：Stop 请求 → 确定性退出（不再等待 EOF）。
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn app-runtime");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    let handshake_req = AppRequest {
        id: "hs".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "appId": "fund",
            "productVersion": "0.1.0",
            "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&handshake_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "handshake failed: {:?}", resp.error);

    let stop_req = AppRequest {
        id: "stop-1".into(),
        method: "app:stop".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&stop_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("stop frame");
    let resp: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(resp["ok"].as_bool(), Some(true));

    // stdin 保持打开：进程仍须自行退出（stop 语义）。
    let t0 = Instant::now();
    let status = child.wait().unwrap();
    let elapsed = t0.elapsed();
    assert!(
        status.success(),
        "process must exit successfully on app:stop"
    );
    assert!(
        elapsed <= Duration::from_secs(2),
        "app:stop exit took {:?}, expected <= 2s",
        elapsed
    );
}

#[test]
fn test_app_runtime_50_cycle_leak_test() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");

    for cycle in 0..50 {
        let mut child = Command::new(bin_path())
            .arg(TEST_ORIGIN)
            .env("NATIVES_APPS_ROOT", &apps_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("cycle {cycle}: failed to spawn: {e}"));

        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();

        // app:handshake
        let handshake_req = AppRequest {
            id: format!("hs-{cycle}"),
            method: "app:handshake".into(),
            params: serde_json::json!({
                "protocolVersion": 2,
                "appId": "fund",
                "productVersion": "0.1.0",
                "activationGeneration": 1
            }),
        };
        write_frame(&mut stdin, &serde_json::to_vec(&handshake_req).unwrap()).unwrap();
        let frame = read_frame(&mut stdout).unwrap().unwrap_or_else(|| {
            panic!(
                "cycle {cycle}: handshake EOF; child status {:?}",
                child.wait()
            );
        });
        let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
        assert!(resp.ok, "cycle {cycle} handshake failed: {:?}", resp.error);

        // app:start
        let start_req = AppRequest {
            id: format!("st-{cycle}"),
            method: "app:start".into(),
            params: serde_json::json!({}),
        };
        write_frame(&mut stdin, &serde_json::to_vec(&start_req).unwrap()).unwrap();
        let frame = read_frame(&mut stdout).unwrap().expect("start frame");
        let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
        assert!(resp.ok, "cycle {cycle} start failed: {:?}", resp.error);

        // Drop stdin (EOF) and ensure exit <= 2s
        let t0 = Instant::now();
        drop(stdin);
        let status = child.wait().unwrap();
        let elapsed = t0.elapsed();
        assert!(status.success(), "cycle {cycle} process exit failed");
        assert!(
            elapsed <= Duration::from_secs(2),
            "cycle {cycle} took {:?}",
            elapsed
        );
    }
}
