//! model_host_query_test: 验证 TokenUsage 模块基于 Model Host 8766 窄接口的查询契约。

use app_runtime_core::http::HttpRequest;
use app_runtime_core::module::BuiltInAppModule;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tokenusage_module::model_host::ModelUsageQueryClient;
use tokenusage_module::TokenUsageModule;

fn load_contract_fixture(name: &str) -> String {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // to modules/
    path.pop(); // to repo root
    path.push("contracts");
    path.push("ai-usage-query");
    path.push("v1");
    path.push(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read contract fixture {}: {e}", path.display()))
}

/// 启动一个模拟的 Model Host 8766 HTTP 服务（真实读取 contracts/ai-usage-query/v1 契约文件）
fn start_mock_model_host(called: Arc<AtomicBool>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");

    let handle = thread::spawn(move || {
        for _ in 0..15 {
            if let Ok((mut stream, _)) = listener.accept() {
                called.store(true, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req_text = String::from_utf8_lossy(&buf[..n]);

                let (body, status) = if req_text.contains("/ai/usage/overview") {
                    (load_contract_fixture("overview.json"), 200)
                } else if req_text.contains("/ai/usage/sources") {
                    (load_contract_fixture("sources.json"), 200)
                } else if req_text.contains("/ai/usage/analysis") {
                    (load_contract_fixture("analysis.json"), 200)
                } else if req_text.contains("/ai/quota/refresh") {
                    (r#"{"status":"ok","refreshed":true}"#.to_string(), 200)
                } else if req_text.contains("/ai/quota") {
                    (load_contract_fixture("quota.json"), 200)
                } else if req_text.contains("/ai/usage/sessions") {
                    (load_contract_fixture("sessions.json"), 200)
                } else if req_text.contains("/ai/usage/events") {
                    (load_contract_fixture("events.json"), 200)
                } else {
                    (r#"{"status":"ok"}"#.to_string(), 200)
                };

                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        }
    });

    (base_url, handle)
}

#[test]
fn test_tokenusage_queries_via_model_host_narrow_api() {
    let called = Arc::new(AtomicBool::new(false));
    let (base_url, _handle) = start_mock_model_host(called.clone());

    let client = ModelUsageQueryClient::with_base_url(&base_url);
    let mut module = TokenUsageModule::with_client(client);

    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let cancel = Arc::new(app_runtime_core::CancellationToken::new());

    let ctx = app_runtime_core::module::ModuleContext {
        app_id: "tokenusage".into(),
        product_version: "0.1.0".into(),
        module_data_root: data_dir.clone(),
        imports_root: temp.path().join("imports"),
        cache_root: temp.path().join("cache"),
        logs_root: temp.path().join("logs"),
        keychain_namespace: "com.natives.app.tokenusage".into(),
        secrets: std::sync::Arc::new(app_runtime_core::secrets::SecretStore::for_app(
            "tokenusage",
        )),
        activation_generation: 1,
        cancellation: cancel,
        limits: app_runtime_core::limits::RuntimeLimits::default(),
    };

    module.initialize(&ctx).unwrap();
    let service = module.start().unwrap();

    let mut cursor = std::io::Cursor::new(b"GET /api/overview HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    let dummy_req = HttpRequest::read(&mut cursor).unwrap();

    let overview_resp = service.handle_api(&dummy_req, "/api/overview").unwrap();
    assert_eq!(overview_resp.status_code, 200);
    let body_str = String::from_utf8_lossy(&overview_resp.body);
    // 从真实的 contracts/ai-usage-query/v1/overview.json 读取验证
    assert!(body_str.contains("\"today\":{\"totalTokens\":2450000"));
    assert!(
        called.load(Ordering::SeqCst),
        "必须通过 8766 窄接口请求数据"
    );

    // 验证 /api/limits
    let limits_resp = service.handle_api(&dummy_req, "/api/limits").unwrap();
    assert_eq!(limits_resp.status_code, 200);
    let limits_str = String::from_utf8_lossy(&limits_resp.body);
    // 从真实的 contracts/ai-usage-query/v1/quota.json 读取验证
    assert!(limits_str.contains("\"providerId\":\"google\""));
    assert!(limits_str.contains("\"remainingPercent\":67.5"));

    // 验证 /api/tray/state
    let tray_resp = service.handle_api(&dummy_req, "/api/tray/state").unwrap();
    assert_eq!(tray_resp.status_code, 200);
    let tray_str = String::from_utf8_lossy(&tray_resp.body);
    assert!(tray_str.contains("2.5M"));

    // 验证本地数据库不包含旧的 usage_records / sessions 权威表
    let db_path = data_dir.join("tokenusage.db");
    assert!(db_path.exists());
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert!(!tables.contains(&"usage_records".to_string()));
    assert!(!tables.contains(&"sessions".to_string()));
    assert!(tables.contains(&"settings".to_string()));
}

#[test]
fn test_tokenusage_fails_closed_when_model_host_offline() {
    // 指向一个未监听端口，模拟 Model Host 未启动/崩溃场景
    let client = ModelUsageQueryClient::with_base_url("http://127.0.0.1:59999");
    let mut module = TokenUsageModule::with_client(client);

    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let cancel = Arc::new(app_runtime_core::CancellationToken::new());

    let ctx = app_runtime_core::module::ModuleContext {
        app_id: "tokenusage".into(),
        product_version: "0.1.0".into(),
        module_data_root: data_dir.clone(),
        imports_root: temp.path().join("imports"),
        cache_root: temp.path().join("cache"),
        logs_root: temp.path().join("logs"),
        keychain_namespace: "com.natives.app.tokenusage".into(),
        secrets: std::sync::Arc::new(app_runtime_core::secrets::SecretStore::for_app(
            "tokenusage",
        )),
        activation_generation: 1,
        cancellation: cancel,
        limits: app_runtime_core::limits::RuntimeLimits::default(),
    };

    module.initialize(&ctx).unwrap();
    let service = module.start().unwrap();

    let mut cursor = std::io::Cursor::new(b"GET /api/overview HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    let dummy_req = HttpRequest::read(&mut cursor).unwrap();

    // 严禁 unwrap_or_default() 伪装成 0 用量与 200 成功，必须如实返回 503 MODEL_HOST_UNAVAILABLE
    let err = service.handle_api(&dummy_req, "/api/overview").unwrap_err();
    assert_eq!(err.0, 503);
    assert!(err.1.contains("MODEL_HOST_UNAVAILABLE"));
}
