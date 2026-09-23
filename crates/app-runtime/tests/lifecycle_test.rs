use app_runtime_core::framing::{read_frame, write_frame};
use app_runtime_core::protocol::{AppRequest, AppResponse, HandshakeResultV2, StartResult};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TEST_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn setup_test_apps_root(root: &std::path::Path, app_id: &str) {
    let app_dir = root.join(app_id);
    std::fs::create_dir_all(&app_dir).unwrap();
    let act = serde_json::json!({
        "receiptVersion": 2,
        "appId": app_id,
        "productVersion": "0.1.0",
        "runtimeHost": "com.natives.app_runtime",
        "appProtocolVersion": 2,
        "activeVersion": "0.1.0",
        "generation": 1,
        "activationState": "ready",
        "enabled": true,
        "allowedOrigins": [TEST_ORIGIN]
    });
    std::fs::write(
        app_dir.join("activation.json"),
        serde_json::to_vec(&act).unwrap(),
    )
    .unwrap();
}

fn bin_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // drop test binary name
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("natives-app-runtime");
    path
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
    setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", temp.path())
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
fn test_app_runtime_handshake_twice_rejected() {
    // 计划 §41：Handshake twice → APP_RUNTIME_ALREADY_BOUND；
    // 一个 Runtime Process 只能绑定一次 appId（计划 §11）。
    let temp = tempfile::tempdir().unwrap();
    setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", temp.path())
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

    // 第二次握手（即使同 appId）必须被拒绝
    write_frame(&mut stdin, &serde_json::to_vec(&handshake("hs-2")).unwrap()).unwrap();
    let frame = read_frame(&mut stdout)
        .unwrap()
        .expect("second handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(!resp.ok, "second handshake must be rejected");
    let error = resp.error.expect("second handshake error body");
    assert_eq!(error.code, "APP_RUNTIME_ALREADY_BOUND", "{error:?}");

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_app_runtime_start_before_handshake_rejected() {
    // 计划 §41：Start before handshake → FAIL（APP_RUNTIME_NOT_INITIALIZED）。
    let temp = tempfile::tempdir().unwrap();
    setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", temp.path())
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
fn test_app_runtime_start_twice_rejected() {
    // 计划 §41：Start twice → APP_RUNTIME_ALREADY_STARTED，无重复 server。
    let temp = tempfile::tempdir().unwrap();
    setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", temp.path())
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
    assert!(!resp.ok, "second start must be rejected");
    let error = resp.error.expect("second start error body");
    assert_eq!(error.code, "APP_RUNTIME_ALREADY_STARTED", "{error:?}");

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
    let _ = port1;
}

#[test]
fn test_app_runtime_app_stop_exits_promptly() {
    // 计划 §41：Stop 请求 → 确定性退出（不再等待 EOF）。
    let temp = tempfile::tempdir().unwrap();
    setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", temp.path())
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
fn test_app_runtime_20_cycle_leak_test() {
    let temp = tempfile::tempdir().unwrap();
    setup_test_apps_root(temp.path(), "fund");

    for cycle in 0..20 {
        let mut child = Command::new(bin_path())
            .arg(TEST_ORIGIN)
            .env("NATIVES_APPS_ROOT", temp.path())
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
        let frame = read_frame(&mut stdout).unwrap().expect("handshake frame");
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
