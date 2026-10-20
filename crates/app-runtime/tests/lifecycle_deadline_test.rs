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
fn test_lifecycle_deadline_fast_eof() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");

    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .env("NATIVES_APP_RUNTIME_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn natives-app-runtime");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // 1. Handshake
    let hs_req = AppRequest {
        id: "hs-1".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "origin": TEST_ORIGIN,
            "appId": "fund"
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&hs_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().unwrap();
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok);

    // 2. Start
    let start_req = AppRequest {
        id: "start-1".into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&start_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().unwrap();
    let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok);
    let port = resp.result.unwrap().port;
    assert!(port > 0);

    // 3. Issue session
    let sess_req = AppRequest {
        id: "sess-1".into(),
        method: "app:session".into(),
        params: serde_json::json!({
            "op": "issue",
            "challenge": "challenge-1234567890abcdefghij"
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&sess_req).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().unwrap();
    let sess_val: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(sess_val["ok"], true);
    let token = sess_val["result"]["token"].as_str().unwrap().to_string();

    // 4. 发起一个正常请求
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(
            format!(
                "GET /api/positions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));

    // Hold the Fund writer lock and start a write request that reaches the
    // module's SQLite busy timeout path.
    let db_lock = rusqlite::Connection::open(apps_root.join("fund/data/fund.db")).unwrap();
    db_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let body = r#"{"account":"test","fundCode":"000001","fundName":"test","type":"BUY","quantity":"1","price":"1","fee":"0","tradeDate":"2026-09-28","requestId":"busy-write"}"#;
    let mut db_busy = TcpStream::connect(("127.0.0.1", port)).unwrap();
    db_busy.write_all(format!(
        "POST /api/transactions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ).as_bytes()).unwrap();

    // Keep one request worker blocked on an incomplete body. Shutdown must
    // remain bounded independently of request socket timeouts.
    let mut blocked = TcpStream::connect(("127.0.0.1", port)).unwrap();
    blocked
        .write_all(
            format!("POST /api/transactions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 1000000\r\n\r\n{{").as_bytes(),
        )
        .unwrap();

    // 5. 关闭 stdin（模拟页面关闭 / EOF）并计时，硬指标 ≤ 2.0s 退出（R-A7）
    let t0 = Instant::now();
    drop(stdin);
    let status = child.wait().expect("failed to wait on child");
    let elapsed = t0.elapsed();

    assert!(status.success());
    assert!(
        elapsed <= Duration::from_millis(2000),
        "EOF shutdown took {:?}, exceeding 2.0s SLA",
        elapsed
    );
    drop(blocked);
    drop(db_busy);
    drop(db_lock);
}

#[test]
fn test_lifecycle_multi_process_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), "fund");
    setup_test_apps_root(temp.path(), "tokenusage");

    // 进程 A: Fund
    let mut child_a = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .env("NATIVES_APP_RUNTIME_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn child A");

    // 进程 B: TokenUsage
    let mut child_b = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .env("NATIVES_APP_RUNTIME_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn child B");

    assert_ne!(
        child_a.id(),
        child_b.id(),
        "两个模块必须在不同独立进程中运行"
    );

    let mut stdin_a = child_a.stdin.take().unwrap();
    let mut stdout_a = child_a.stdout.take().unwrap();
    let mut stdin_b = child_b.stdin.take().unwrap();
    let mut stdout_b = child_b.stdout.take().unwrap();

    // Handshake & Start A
    let hs_a = AppRequest {
        id: "hs-a".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "origin": TEST_ORIGIN,
            "appId": "fund"
        }),
    };
    write_frame(&mut stdin_a, &serde_json::to_vec(&hs_a).unwrap()).unwrap();
    let frame_a = read_frame(&mut stdout_a).unwrap().unwrap();
    let resp_a: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame_a).unwrap();
    assert!(resp_a.ok);

    let start_a = AppRequest {
        id: "start-a".into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin_a, &serde_json::to_vec(&start_a).unwrap()).unwrap();
    let frame_start_a = read_frame(&mut stdout_a).unwrap().unwrap();
    let resp_start_a: AppResponse<StartResult> = serde_json::from_slice(&frame_start_a).unwrap();
    assert!(resp_start_a.ok);
    let port_a = resp_start_a.result.unwrap().port;

    // Handshake & Start B
    let hs_b = AppRequest {
        id: "hs-b".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "origin": TEST_ORIGIN,
            "appId": "tokenusage"
        }),
    };
    write_frame(&mut stdin_b, &serde_json::to_vec(&hs_b).unwrap()).unwrap();
    let frame_b = read_frame(&mut stdout_b).unwrap().unwrap();
    let resp_b: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame_b).unwrap();
    assert!(resp_b.ok);

    let start_b = AppRequest {
        id: "start-b".into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin_b, &serde_json::to_vec(&start_b).unwrap()).unwrap();
    let frame_start_b = read_frame(&mut stdout_b).unwrap().unwrap();
    let resp_start_b: AppResponse<StartResult> = serde_json::from_slice(&frame_start_b).unwrap();
    assert!(resp_start_b.ok);
    let port_b = resp_start_b.result.unwrap().port;

    // 两个进程端口必须独立分配，不存在端口冲突抢占
    assert_ne!(port_a, port_b);

    // 关闭进程 A（Fund），验证进程 B（TokenUsage）不受影响继续存活
    drop(stdin_a);
    let status_a = child_a.wait().unwrap();
    assert!(status_a.success());

    // 进程 B 依然健康响应
    let stat_b = AppRequest {
        id: "stat-b".into(),
        method: "app:status".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin_b, &serde_json::to_vec(&stat_b).unwrap()).unwrap();
    let frame_stat_b = read_frame(&mut stdout_b).unwrap().unwrap();
    let resp_stat_b: serde_json::Value = serde_json::from_slice(&frame_stat_b).unwrap();
    assert_eq!(resp_stat_b["ok"], true);

    // 关闭进程 B
    drop(stdin_b);
    let status_b = child_b.wait().unwrap();
    assert!(status_b.success());
}

fn setup_test_v3_product_receipt(root: &std::path::Path, app_id: &str, generation: u64) {
    let product_dir = root.parent().unwrap_or(root).join("product");
    std::fs::create_dir_all(&product_dir).unwrap();
    let exe = bin_path();
    let bytes = std::fs::read(&exe).unwrap();
    let actual_sha = app_runtime_core::sha256_hex(&bytes);

    let receipt = serde_json::json!({
        "receiptVersion": 3,
        "productVersion": "0.1.0",
        "productGeneration": generation,
        "productManifestSha256": "0".repeat(64),
        "runtime": {
            "host": "com.natives.local.app_runtime",
            "protocolVersion": 2,
            "sha256": actual_sha
        },
        "modules": {
            app_id: {
                "moduleApiVersion": 1,
                "dataSchemaVersion": 2,
                "capabilityVersion": 2,
                "uiTreeSha256": "0".repeat(64)
            }
        },
        "allowedOrigins": [TEST_ORIGIN]
    });
    std::fs::write(
        product_dir.join("activation.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn test_product_generation_change_invalidates_stale_handshake() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = temp.path().join("apps");
    std::fs::create_dir_all(&apps_root).unwrap();

    // 1. 配置产品：generation 1, fund 启用
    setup_test_v3_product_receipt(&apps_root, "fund", 1);

    // 握手 1：应当成功
    let mut child1 = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .env("NATIVES_APP_RUNTIME_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin1 = child1.stdin.take().unwrap();
    let mut stdout1 = child1.stdout.take().unwrap();

    let hs_req1 = AppRequest {
        id: "hs-1".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "origin": TEST_ORIGIN,
            "appId": "fund",
            "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin1, &serde_json::to_vec(&hs_req1).unwrap()).unwrap();
    let frame1 = read_frame(&mut stdout1).unwrap().unwrap();
    let resp1: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame1).unwrap();
    assert!(resp1.ok);
    drop(stdin1);
    let _ = child1.wait();

    // 2. 产品配置升级后，旧 generation 必须失败。
    setup_test_v3_product_receipt(&apps_root, "fund", 2);

    let mut child2 = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .env("NATIVES_APP_RUNTIME_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin2 = child2.stdin.take().unwrap();
    let mut stdout2 = child2.stdout.take().unwrap();

    let hs_req2 = AppRequest {
        id: "hs-2".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "origin": TEST_ORIGIN,
            "appId": "fund",
            "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin2, &serde_json::to_vec(&hs_req2).unwrap()).unwrap();
    let frame2 = read_frame(&mut stdout2).unwrap().unwrap();
    let resp2: serde_json::Value = serde_json::from_slice(&frame2).unwrap();
    // 旧 generation 握手必须失败。
    assert_eq!(resp2["ok"], false);
    drop(stdin2);
    let _ = child2.wait();

    // 3. 使用当前 generation 后恢复成功。

    let mut child3 = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .env("NATIVES_APP_RUNTIME_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin3 = child3.stdin.take().unwrap();
    let mut stdout3 = child3.stdout.take().unwrap();

    let hs_req3 = AppRequest {
        id: "hs-3".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2,
            "origin": TEST_ORIGIN,
            "appId": "fund",
            "activationGeneration": 2
        }),
    };
    write_frame(&mut stdin3, &serde_json::to_vec(&hs_req3).unwrap()).unwrap();
    let frame3 = read_frame(&mut stdout3).unwrap().unwrap();
    let resp3: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame3).unwrap();
    // 当前 generation 握手成功。
    assert!(resp3.ok);
    drop(stdin3);
    let _ = child3.wait();
}
