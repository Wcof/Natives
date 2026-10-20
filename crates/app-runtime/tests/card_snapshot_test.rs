//! app:card_snapshot（ADR-0036）集成测试：Ready 门控、instance/generation 校验、
//! 参数白名单与响应形状。使用 test-fixture 无业务数据的 fund 模块。

mod support;

use app_runtime_core::framing::{read_frame, write_frame};
use app_runtime_core::protocol::{AppRequest, HandshakeResultV2, StartResult};
use std::io::{BufReader, Write};
use std::process::{Command, Stdio};

const TEST_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn setup_test_apps_root(root: &std::path::Path) -> std::path::PathBuf {
    let apps_root = root.join("apps");
    std::fs::create_dir_all(apps_root.join("fund")).unwrap();
    let runtime_sha = app_runtime_core::sha256_hex(&std::fs::read(bin_path()).unwrap());
    let act = serde_json::json!({
        "receiptVersion": 3,
        "productVersion": "0.1.0",
        "productGeneration": 1,
        "productManifestSha256": "0".repeat(64),
        "runtime": { "host": "com.natives.local.app_runtime", "protocolVersion": 2, "sha256": runtime_sha },
        "modules": {
            "fund": { "moduleApiVersion": 1, "dataSchemaVersion": 5, "capabilityVersion": 2, "uiTreeSha256": "0".repeat(64) },
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

struct Runtime {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    reader: BufReader<std::process::ChildStdout>,
}

impl Runtime {
    fn spawn(apps_root: &std::path::Path) -> Self {
        let mut child = Command::new(bin_path())
            .arg(TEST_ORIGIN)
            .env("NATIVES_APPS_ROOT", apps_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn app-runtime");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
        }
    }

    fn call(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let req = AppRequest {
            id: format!("req-{method}"),
            method: method.into(),
            params,
        };
        write_frame(
            self.stdin.as_mut().unwrap(),
            &serde_json::to_vec(&req).unwrap(),
        )
        .unwrap();
        let frame = read_frame(&mut self.reader).unwrap().expect("frame");
        serde_json::from_slice(&frame).unwrap()
    }

    fn handshake(&mut self) {
        let resp = self.call(
            "app:handshake",
            serde_json::json!({
                "protocolVersion": 2,
                "appId": "fund",
                "productVersion": "0.1.0",
                "activationGeneration": 1
            }),
        );
        assert!(resp["ok"].as_bool().unwrap(), "handshake failed: {resp}");
        let hs: HandshakeResultV2 = serde_json::from_value(resp["result"].clone()).unwrap();
        assert_eq!(hs.app_id, "fund");
    }

    fn start(&mut self) -> (String, String) {
        let resp = self.call("app:start", serde_json::json!({}));
        assert!(resp["ok"].as_bool().unwrap(), "start failed: {resp}");
        let start: StartResult = serde_json::from_value(resp["result"].clone()).unwrap();
        (start.instance_id, start.generation)
    }

    fn assert_data_ready(&mut self) {
        let status = self.call("app:data_status", serde_json::json!({}));
        assert_eq!(status["ok"].as_bool().unwrap(), true);
        assert_eq!(
            status["result"]["readiness"], "ready",
            "data_status: {status}"
        );
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // 关 stdin 触发 EOF 退出；进程必须 ≤2 秒退出（生命周期硬指标复检）。
        let t0 = std::time::Instant::now();
        drop(self.stdin.take());
        let _ = self.child.wait();
        assert!(
            t0.elapsed() <= std::time::Duration::from_secs(2),
            "EOF shutdown took {:?}",
            t0.elapsed()
        );
    }
}

#[allow(dead_code)]
impl Runtime {
    fn shutdown(mut self) {
        // 显式关闭（Drop 逻辑相同），用于需要断言退出耗时的场景。
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

/// 首次 start 初始化 fund.db 并 EOF 退出；返回后 readiness=ready（真实首用流程）。
fn init_database(apps_root: &std::path::Path) {
    let mut rt = Runtime::spawn(apps_root);
    rt.handshake();
    let _ = rt.start();
    // Drop 触发 EOF 退出。
}

#[test]
fn card_snapshot_happy_path_and_validation() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path());
    init_database(&apps_root);
    let mut rt = Runtime::spawn(&apps_root);
    rt.handshake();
    rt.assert_data_ready();
    let (instance_id, generation) = rt.start();

    // 正确 instanceId/generation：返回 fund.cards.v1 投影。
    let resp = rt.call(
        "app:card_snapshot",
        serde_json::json!({ "instanceId": instance_id, "generation": generation }),
    );
    assert!(resp["ok"].as_bool().unwrap(), "snapshot failed: {resp}");
    assert_eq!(resp["result"]["schema"], "fund.cards.v1");
    assert!(resp["result"]["generatedAt"].is_string());
    let payload = resp["result"]["payload"].clone();
    assert!(payload["registeredFundHoldings"].is_object());
    assert_eq!(payload["registeredFundHoldings"]["quality"], "empty");
}

#[test]
fn card_snapshot_rejects_wrong_instance_or_generation() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path());
    init_database(&apps_root);
    let mut rt = Runtime::spawn(&apps_root);
    rt.handshake();
    rt.assert_data_ready();
    let (instance_id, generation) = rt.start();

    let resp = rt.call(
        "app:card_snapshot",
        serde_json::json!({ "instanceId": "bogus-instance", "generation": generation }),
    );
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_SESSION_INVALID");

    let resp = rt.call(
        "app:card_snapshot",
        serde_json::json!({ "instanceId": instance_id, "generation": "bogus-gen" }),
    );
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_SESSION_INVALID");
}

#[test]
fn card_snapshot_rejects_unknown_fields_and_pre_start_state() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path());
    init_database(&apps_root);
    let mut rt = Runtime::spawn(&apps_root);
    rt.handshake();
    rt.assert_data_ready();
    let (instance_id, generation) = rt.start();

    // 参数白名单：未知字段必须拒绝（R-S2）。
    let resp = rt.call(
        "app:card_snapshot",
        serde_json::json!({
            "instanceId": instance_id,
            "generation": generation,
            "route": "/api/positions"
        }),
    );
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_PROTOCOL_MISMATCH");

    // 第一个实例 EOF 退出释放锁，再起新进程验证 Bound 状态拒绝。
    rt.shutdown();
    let mut rt2 = Runtime::spawn(&apps_root);
    let resp = rt2.call(
        "app:handshake",
        serde_json::json!({
            "protocolVersion": 2,
            "appId": "fund",
            "productVersion": "0.1.0",
            "activationGeneration": 1
        }),
    );
    assert!(resp["ok"].as_bool().unwrap(), "handshake failed: {resp}");
    let resp = rt2.call(
        "app:card_snapshot",
        serde_json::json!({ "instanceId": instance_id, "generation": generation }),
    );
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_RUNTIME_INVALID_STATE");
}
