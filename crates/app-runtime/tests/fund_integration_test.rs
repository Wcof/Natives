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
fn test_fund_module_e2e_lifecycle_and_data_persistence() {
    let temp = tempfile::tempdir().unwrap();
    setup_test_apps_root(temp.path(), "fund");

    let port: u16;
    let token: String;

    // --- Session 1: Open Fund, Issue Token, Add Transaction, Verify Persistence ---
    {
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
            id: "hs-1".into(),
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
        let hs = resp.result.unwrap();
        assert_eq!(hs.app_id, "fund");
        assert_eq!(hs.state, "initialized");

        // 2. app:start
        let start_req = AppRequest {
            id: "st-1".into(),
            method: "app:start".into(),
            params: serde_json::json!({}),
        };
        write_frame(&mut stdin, &serde_json::to_vec(&start_req).unwrap()).unwrap();
        let frame = read_frame(&mut stdout).unwrap().expect("start frame");
        let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
        assert!(resp.ok, "start failed: {:?}", resp.error);
        let start = resp.result.unwrap();
        assert_eq!(start.state, "ready");
        port = start.port;

        // 3. UI static resource GET /
        let mut http = TcpStream::connect(("127.0.0.1", port)).unwrap();
        http.write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes())
            .unwrap();
        let mut http_resp = String::new();
        http.read_to_string(&mut http_resp).unwrap();
        assert!(http_resp.contains("200 OK"));
        assert!(http_resp.contains("基金记账"));

        // 4. Issue bearer session
        let session_req = AppRequest {
            id: "sess-1".into(),
            method: "app:session".into(),
            params: serde_json::json!({
                "op": "issue",
                "challenge": "challenge-1234567890abcdefghij"
            }),
        };
        write_frame(&mut stdin, &serde_json::to_vec(&session_req).unwrap()).unwrap();
        let frame = read_frame(&mut stdout).unwrap().expect("session frame");
        let resp: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        assert!(resp["ok"].as_bool().unwrap());
        token = resp["result"]["token"].as_str().unwrap().to_string();

        // 5. Query positions (empty initially)
        let mut http = TcpStream::connect(("127.0.0.1", port)).unwrap();
        http.write_all(
            format!(
                "GET /api/positions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
        let mut http_resp = String::new();
        http.read_to_string(&mut http_resp).unwrap();
        assert!(http_resp.contains("200 OK"));
        assert!(http_resp.contains("\"positions\":[]"));

        // 6. Create a transaction (buy fund)
        let tx_payload = serde_json::json!({
            "account": "招商证券",
            "fundCode": "000001",
            "fundName": "华夏成长混合",
            "type": "BUY",
            "quantity": "5000.0000",
            "price": "2.0000",
            "fee": "10.00",
            "tradeDate": "2026-09-01",
            "requestId": "req-tx-001"
        });
        let tx_body = serde_json::to_string(&tx_payload).unwrap();
        let mut http = TcpStream::connect(("127.0.0.1", port)).unwrap();
        http.write_all(
            format!(
                "POST /api/transactions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                tx_body.len(),
                tx_body
            )
            .as_bytes(),
        )
        .unwrap();
        let mut http_resp = String::new();
        http.read_to_string(&mut http_resp).unwrap();
        assert!(
            http_resp.contains("200 OK"),
            "create tx response: {http_resp}"
        );

        // 7. Re-query positions -> should show 5000 shares of 000001
        let mut http = TcpStream::connect(("127.0.0.1", port)).unwrap();
        http.write_all(
            format!(
                "GET /api/positions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
        let mut http_resp = String::new();
        http.read_to_string(&mut http_resp).unwrap();
        assert!(http_resp.contains("200 OK"));
        assert!(http_resp.contains("000001"));
        assert!(http_resp.contains("华夏成长混合"));

        // 8. Close stdin (EOF) and verify exit <= 2 seconds
        let t0 = Instant::now();
        drop(stdin);
        let status = child.wait().unwrap();
        let elapsed = t0.elapsed();
        assert!(status.success());
        assert!(elapsed <= Duration::from_secs(2), "exit took {:?}", elapsed);
    }

    // --- Session 2: Relaunch Fund and Verify Data was saved to fund.db ---
    {
        let mut child = Command::new(bin_path())
            .arg(TEST_ORIGIN)
            .env("NATIVES_APPS_ROOT", temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn app-runtime 2nd time");

        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();

        // 1. app:handshake
        let handshake_req = AppRequest {
            id: "hs-2".into(),
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

        // 2. app:start
        let start_req = AppRequest {
            id: "st-2".into(),
            method: "app:start".into(),
            params: serde_json::json!({}),
        };
        write_frame(&mut stdin, &serde_json::to_vec(&start_req).unwrap()).unwrap();
        let frame = read_frame(&mut stdout).unwrap().expect("start frame");
        let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
        assert!(resp.ok);
        let port2 = resp.result.unwrap().port;

        // 3. Issue token
        let session_req = AppRequest {
            id: "sess-2".into(),
            method: "app:session".into(),
            params: serde_json::json!({
                "op": "issue",
                "challenge": "challenge-second-session"
            }),
        };
        write_frame(&mut stdin, &serde_json::to_vec(&session_req).unwrap()).unwrap();
        let frame = read_frame(&mut stdout).unwrap().expect("session frame");
        let resp: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        let token2 = resp["result"]["token"].as_str().unwrap();

        // 4. Positions must still be present from Session 1!
        let mut http = TcpStream::connect(("127.0.0.1", port2)).unwrap();
        http.write_all(
            format!(
                "GET /api/positions HTTP/1.1\r\nHost: 127.0.0.1:{port2}\r\nOrigin: null\r\nAuthorization: Bearer {token2}\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
        let mut http_resp = String::new();
        http.read_to_string(&mut http_resp).unwrap();
        assert!(http_resp.contains("200 OK"));
        assert!(http_resp.contains("000001"));
        assert!(http_resp.contains("华夏成长混合"));

        drop(stdin);
        let status = child.wait().unwrap();
        assert!(status.success());
    }
}
