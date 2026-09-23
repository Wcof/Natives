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
fn test_tokenusage_module_e2e_lifecycle_and_fast_eof() {
    let temp = tempfile::tempdir().unwrap();
    setup_test_apps_root(temp.path(), "tokenusage");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn app-runtime");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // 1. app:handshake
    let handshake_req = AppRequest {
        id: "hs-tokenusage-1".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "appId": "tokenusage",
            "productVersion": "0.1.0",
            "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&handshake_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "handshake failed: {:?}", resp.error);
    let hs = resp.result.unwrap();
    assert_eq!(hs.app_id, "tokenusage");
    assert_eq!(hs.state, "initialized");

    // 2. app:start
    let start_req = AppRequest {
        id: "st-tokenusage-1".into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&start_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("start frame");
    let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "start failed: {:?}", resp.error);
    let start = resp.result.unwrap();
    assert_eq!(start.state, "ready");
    let port = start.port;
    let _instance_id = start.instance_id;

    // 3. UI static resource GET /
    let mut http = TcpStream::connect(("127.0.0.1", port)).unwrap();
    http.write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes())
        .unwrap();
    let mut http_resp = String::new();
    http.read_to_string(&mut http_resp).unwrap();
    assert!(http_resp.starts_with("HTTP/1.1 200 OK"));
    assert!(http_resp.contains("Token Monitor"));

    // 4. Issue Bearer token
    let session_req = AppRequest {
        id: "sess-issue-1".into(),
        method: "app:session".into(),
        params: serde_json::json!({
            "op": "issue",
            "challenge": "challenge-1234567890abcdefghij"
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&session_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout)
        .unwrap()
        .expect("session issue frame");
    let resp: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(resp["ok"], true);
    let token = resp["result"]["token"].as_str().unwrap().to_string();

    // 5. API call: GET /api/overview
    let mut http_api = TcpStream::connect(("127.0.0.1", port)).unwrap();
    http_api.write_all(
        format!(
            "GET /api/overview HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\n\r\n"
        ).as_bytes()
    ).unwrap();
    let mut api_resp = String::new();
    http_api.read_to_string(&mut api_resp).unwrap();
    assert!(api_resp.starts_with("HTTP/1.1 200 OK"));
    assert!(api_resp.contains("\"currency\":\"USD\""));

    // 6. API call: GET /api/tray/state
    let mut http_tray = TcpStream::connect(("127.0.0.1", port)).unwrap();
    http_tray.write_all(
        format!(
            "GET /api/tray/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\n\r\n"
        ).as_bytes()
    ).unwrap();
    let mut tray_resp = String::new();
    http_tray.read_to_string(&mut tray_resp).unwrap();
    assert!(tray_resp.starts_with("HTTP/1.1 200 OK"));
    assert!(tray_resp.contains("\"displayText\":"));

    // 7. Lifecycle & Fast EOF: Close stdin -> process MUST exit in <= 2 seconds
    let t0 = Instant::now();
    drop(stdin);
    let status = child.wait().expect("failed to wait on child");
    let elapsed = t0.elapsed();

    assert!(status.success(), "process must exit with status 0");
    assert!(
        elapsed <= Duration::from_secs(2),
        "process exit took {:?} which exceeds 2s threshold (R-A7)",
        elapsed
    );
}
