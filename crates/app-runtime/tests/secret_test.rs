//! app:secret（ADR-0038）集成测试：Ready 门控、instance/generation 校验、
//! 能力门（platform.secret）、参数上限与 set→status→delete 往返。
//! 真实 macOS Keychain 往返使用隔离账户（ns=com.natives.app.fund,
//! acct=r2test:<随机后缀>），用例结束幂等删除，不触碰生产账户。

mod support;

use app_runtime_core::framing::{read_frame, write_frame};
use app_runtime_core::protocol::{AppRequest, HandshakeResultV2, StartResult};
use std::process::{Command, Stdio};

const TEST_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn setup_test_apps_root(root: &std::path::Path, fund_capabilities: &[&str]) -> std::path::PathBuf {
    let apps_root = root.join("apps");
    std::fs::create_dir_all(apps_root.join("fund")).unwrap();
    let runtime_sha = app_runtime_core::sha256_hex(&std::fs::read(bin_path()).unwrap());
    let capabilities: Vec<serde_json::Value> = fund_capabilities
        .iter()
        .map(|c| serde_json::Value::String((*c).to_string()))
        .collect();
    let act = serde_json::json!({
        "receiptVersion": 3,
        "productVersion": "0.1.0",
        "productGeneration": 1,
        "productManifestSha256": "0".repeat(64),
        "runtime": { "host": "com.natives.local.app_runtime", "protocolVersion": 2, "sha256": runtime_sha },
        "modules": {
            "fund": {
                "moduleApiVersion": 1,
                "dataSchemaVersion": 5,
                "capabilityVersion": 4,
                "uiTreeSha256": "0".repeat(64),
                "capabilities": capabilities,
                "views": ["home", "positions"]
            },
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
    stdout: Option<std::process::ChildStdout>,
    instance_id: String,
    generation: String,
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
        let stdout = child.stdout.take();
        Self {
            child,
            stdin,
            stdout,
            instance_id: String::new(),
            generation: String::new(),
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
        let frame = read_frame(self.stdout.as_mut().unwrap())
            .unwrap()
            .unwrap_or_else(|| panic!("no frame for {method}"));
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
        self.instance_id = start.instance_id.clone();
        self.generation = start.generation.clone();
        (start.instance_id, start.generation)
    }

    fn secret(&mut self, params: serde_json::Value) -> serde_json::Value {
        self.call("app:secret", params)
    }

    fn shutdown(mut self) {
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

/// 隔离账户：只用 r2test: 前缀 + 进程唯一后缀，绝不触碰真实业务账户。
fn test_account(tag: &str) -> String {
    format!("r2test:{}", tag)
}

/// 直接经 /usr/bin/security 幂等清理测试账户（兜底，即使 runtime 侧断言失败）。
fn force_cleanup(account: &str) {
    let _ = Command::new("/usr/bin/security")
        .args([
            "delete-generic-password",
            "-s",
            "com.natives.app.fund",
            "-a",
            account,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[test]
fn secret_rejected_without_capability() {
    // A05：激活收据模块条目未声明 platform.secret → 任何 Keychain 副作用前拒绝。
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), &["platform.network"]);
    let mut rt = Runtime::spawn(&apps_root);
    rt.handshake();
    let (instance_id, generation) = rt.start();

    let resp = rt.secret(serde_json::json!({
        "instanceId": instance_id,
        "generation": generation,
        "op": "status",
        "accountId": test_account("nocap"),
    }));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_CAPABILITY_DENIED", "{resp}");
    rt.shutdown();
}

#[test]
fn secret_session_and_state_gates() {
    // A04：Bound（未 start）拒绝；instance/generation 不匹配拒绝；未知字段拒绝。
    let temp = tempfile::tempdir().unwrap();
    let account = test_account(&format!("gate-{}", std::process::id()));
    let apps_root = setup_test_apps_root(temp.path(), &["platform.secret"]);

    // Bound 状态：handshake 后未 start。
    let mut rt = Runtime::spawn(&apps_root);
    rt.handshake();
    let resp = rt.secret(serde_json::json!({
        "instanceId": "unused",
        "generation": "unused",
        "op": "status",
        "accountId": account,
    }));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_RUNTIME_INVALID_STATE", "{resp}");

    rt.start();
    // 正确会话基线可用（同时验证 status 在未设置时 exists=false）。
    let resp = rt.secret(serde_json::json!({
        "instanceId": rt.instance_id,
        "generation": rt.generation,
        "op": "status",
        "accountId": account,
    }));
    assert_eq!(resp["ok"], true, "baseline status failed: {resp}");
    assert_eq!(resp["result"]["exists"], false);

    let resp = rt.secret(serde_json::json!({
        "instanceId": "bogus-instance",
        "generation": "whatever",
        "op": "status",
        "accountId": account,
    }));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_SESSION_INVALID", "{resp}");

    let resp = rt.secret(serde_json::json!({
        "instanceId": "bogus-instance",
        "generation": "bogus-generation",
        "op": "status",
        "accountId": account,
    }));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_SESSION_INVALID", "{resp}");

    // 未知字段（R-S2 参数白名单）。
    let resp = rt.secret(serde_json::json!({
        "instanceId": "bogus-instance",
        "generation": "bogus-generation",
        "op": "status",
        "accountId": account,
        "extra": true,
    }));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_PROTOCOL_MISMATCH", "{resp}");
    rt.shutdown();
}

#[test]
fn secret_set_status_delete_roundtrip_never_echoes_value() {
    // A06：capability 声明后 set→status→delete 真实往返；协议永不回显明文；
    // delete 必须 confirm:true；重复 delete 幂等。
    if !cfg!(target_os = "macos") {
        eprintln!("non-macos: keychain backend unsupported path covered by unit tests");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let account = test_account(&format!("rt-{}", std::process::id()));
    force_cleanup(&account);
    let apps_root = setup_test_apps_root(temp.path(), &["platform.secret"]);
    let mut rt = Runtime::spawn(&apps_root);
    rt.handshake();
    let (instance_id, generation) = rt.start();

    let base = serde_json::json!({
        "instanceId": instance_id,
        "generation": generation,
        "accountId": account,
    });
    let mut params = base.as_object().unwrap().clone();

    // status：初始不存在。
    params.insert("op".into(), serde_json::json!("status"));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], true, "status failed: {resp}");
    assert_eq!(resp["result"]["exists"], false);

    // set 缺 value → PROTOCOL_MISMATCH。
    params.insert("op".into(), serde_json::json!("set"));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_PROTOCOL_MISMATCH", "{resp}");

    // set 超 8192 字节 → 拒绝且不落 Keychain。
    params.insert("value".into(), serde_json::json!("x".repeat(8193)));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_PROTOCOL_MISMATCH", "{resp}");

    // set 合法值。
    let secret_value = "r2-test-secret-value";
    params.insert("value".into(), serde_json::json!(secret_value));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], true, "set failed: {resp}");
    assert!(!resp.to_string().contains(secret_value));

    // status 存在且有更新时间。
    params.insert("op".into(), serde_json::json!("status"));
    params.remove("value");
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], true, "status failed: {resp}");
    assert_eq!(resp["result"]["exists"], true);
    assert!(
        resp["result"]["updatedAt"].is_string(),
        "updatedAt missing: {resp}"
    );

    // delete 无 confirm → 拒绝；Secret 仍在。
    params.insert("op".into(), serde_json::json!("delete"));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_PROTOCOL_MISMATCH", "{resp}");

    // 未知 op → 拒绝。
    params.insert("op".into(), serde_json::json!("reveal"));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"]["code"], "APP_PROTOCOL_MISMATCH", "{resp}");

    // delete confirm:true → 成功；再 status exists=false；重复 delete 幂等。
    params.insert("op".into(), serde_json::json!("delete"));
    params.insert("confirm".into(), serde_json::json!(true));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], true, "delete failed: {resp}");

    params.insert("op".into(), serde_json::json!("status"));
    params.remove("confirm");
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["result"]["exists"], false, "{resp}");

    params.insert("op".into(), serde_json::json!("delete"));
    params.insert("confirm".into(), serde_json::json!(true));
    let resp = rt.secret(serde_json::Value::Object(params.clone()));
    assert_eq!(resp["ok"], true, "idempotent delete failed: {resp}");

    rt.shutdown();
    force_cleanup(&account);
}

#[test]
fn secret_rejects_invalid_account_id_before_backend() {
    // accountId 形状非法（大写/超长/非法首字符）→ PROTOCOL_MISMATCH，
    // 且必须在任何 /usr/bin/security 调用前拒绝（validate 先行）。
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup_test_apps_root(temp.path(), &["platform.secret"]);
    let mut rt = Runtime::spawn(&apps_root);
    rt.handshake();
    rt.start();

    for account in [
        "Bad:Upper",
        ":leading-colon",
        "",
        "x".repeat(65).as_str(),
        "space account",
    ] {
        let resp = rt.secret(serde_json::json!({
            "instanceId": rt.instance_id,
            "generation": rt.generation,
            "op": "status",
            "accountId": account,
        }));
        assert_eq!(resp["ok"], false, "account {account:?} accepted");
        assert_eq!(resp["error"]["code"], "APP_PROTOCOL_MISMATCH", "{resp}");
    }
    rt.shutdown();
}
