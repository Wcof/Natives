use app_runtime_core::http::HttpRequest;
use app_runtime_core::module::{BuiltInAppModule, ModuleContext};
use fund_module::FundModule;
use std::path::Path;

fn test_context(root: &Path) -> ModuleContext {
    ModuleContext::for_app(root, "fund", "0.1.0", 1)
}

#[test]
fn test_fund_module_contract_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = test_context(temp.path());

    let mut module = FundModule::new();

    // 1. descriptor
    let desc = module.descriptor();
    assert_eq!(desc.app_id, "fund");
    assert_eq!(desc.module_api_version, 1);
    assert_eq!(desc.capability_version, 4);
    assert_eq!(
        desc.data_schema_version,
        fund_module::storage::SCHEMA_VERSION as u32,
        "signed module schema declaration must match the actual migration target"
    );

    // 2. initialize
    assert!(module.initialize(&ctx).is_ok());

    // 3. health before start
    let health = module.health().unwrap();
    assert!(health.ok);

    // 4. start (creates DB & runs migrations)
    let service = module.start().unwrap();

    // 5. handle_ui
    let ui_resp = service.handle_ui("/").unwrap();
    assert!(ui_resp.is_some());
    let html = String::from_utf8(ui_resp.unwrap().body).unwrap();
    assert!(html.contains("<title>投资终端</title>"));
    assert!(html.contains("id=\"pos-tab-stock\""));
    assert!(html.contains("id=\"pos-tab-otc\""));

    let ui_miss = service.handle_ui("/api/something").unwrap();
    assert!(ui_miss.is_none());

    // 6. handle_api (GET /api/positions)
    let get_req = HttpRequest::for_internal("GET", "/api/positions", Vec::new());
    let api_resp = service.handle_api(&get_req, "/api/positions").unwrap();
    assert_eq!(api_resp.status_code, 200);
    let body = String::from_utf8(api_resp.body).unwrap();
    assert!(body.contains("\"positions\":[]"));

    // 7. data_status
    let status = module.data_status().unwrap();
    assert_eq!(
        u32::from(status.current_schema),
        fund_module::storage::SCHEMA_VERSION as u32
    );
    assert_eq!(status.readiness.as_deref(), Some("ready"));
    assert_eq!(status.migration_state, "committed");

    // 8. shutdown
    assert!(module.shutdown().is_ok());
}

#[test]
fn test_chart_drawings_roundtrip_and_list_query() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = test_context(temp.path());
    let mut module = FundModule::new();
    assert!(module.initialize(&ctx).is_ok());
    let service = module.start().unwrap();

    // 1. 保存：前端 POST 口径（symbol 未带前缀，服务端归一化为 sh600900）
    let save_body = r##"{
        "id": "sd_test_1",
        "symbol": "600900",
        "period": "day",
        "toolType": "horizontal",
        "points": [{"date": "2026-08-12", "price": 27.96}],
        "options": {"coordinateVersion": 2, "viewContext": {"fq": "qfq", "seriesKind": "OHLC"}, "color": "#ff2d2d", "width": 2, "dash": "solid", "name": "短期支撑"}
    }"##;
    let save_req =
        HttpRequest::for_internal("POST", "/api/drawings", save_body.as_bytes().to_vec());
    let save_resp = service.handle_api(&save_req, "/api/drawings").unwrap();
    assert_eq!(save_resp.status_code, 200);
    assert!(String::from_utf8(save_resp.body)
        .unwrap()
        .contains("\"success\":true"));

    // 2. 列表：真实 HTTP 口径——request.path 携带 query，route 已剥离 query
    let list_req =
        HttpRequest::for_internal("GET", "/api/drawings?symbol=600900&period=day", Vec::new());
    let list_resp = service.handle_api(&list_req, "/api/drawings").unwrap();
    assert_eq!(list_resp.status_code, 200);
    let body = String::from_utf8(list_resp.body).unwrap();
    assert!(
        body.contains("sd_test_1"),
        "list should contain saved drawing: {body}"
    );
    assert!(
        body.contains("sh600900"),
        "symbol should be normalized: {body}"
    );
    assert!(
        body.contains("短期支撑"),
        "options_json should round-trip: {body}"
    );

    // 3. 前端二次口径：GET 带 sh 前缀 + 不同大小写也能命中同一行
    let list_req2 = HttpRequest::for_internal(
        "GET",
        "/api/drawings?symbol=sh600900&period=day",
        Vec::new(),
    );
    let list_resp2 = service.handle_api(&list_req2, "/api/drawings").unwrap();
    assert_eq!(list_resp2.status_code, 200);
    assert!(String::from_utf8(list_resp2.body)
        .unwrap()
        .contains("sd_test_1"));

    // 4. 其他周期/标的不得串数据
    let list_req3 =
        HttpRequest::for_internal("GET", "/api/drawings?symbol=600900&period=week", Vec::new());
    let list_resp3 = service.handle_api(&list_req3, "/api/drawings").unwrap();
    assert!(!String::from_utf8(list_resp3.body)
        .unwrap()
        .contains("sd_test_1"));

    // DES-D02: stale writes/deletes never overwrite a newer confirmed object.
    let first: serde_json::Value = serde_json::from_str(&body).unwrap();
    let digest = first["drawings"][0]["digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    let mut edit: serde_json::Value = serde_json::from_str(save_body).unwrap();
    edit["expectedDigest"] = serde_json::json!(digest);
    edit["points"][0]["price"] = serde_json::json!(28.5);
    let edit_req =
        HttpRequest::for_internal("POST", "/api/drawings", edit.to_string().into_bytes());
    let edited = service.handle_api(&edit_req, "/api/drawings").unwrap();
    assert_eq!(edited.status_code, 200);
    let confirmed: serde_json::Value = serde_json::from_slice(&edited.body).unwrap();
    let next_digest = confirmed["drawing"]["digest"].as_str().unwrap();
    assert_ne!(digest, next_digest);
    assert_eq!(
        service
            .handle_api(&edit_req, "/api/drawings")
            .unwrap_err()
            .0,
        409
    );
    let stale_delete = HttpRequest::for_internal(
        "DELETE",
        &format!("/api/drawings?id=sd_test_1&expectedDigest={digest}"),
        Vec::new(),
    );
    assert_eq!(
        service
            .handle_api(&stale_delete, "/api/drawings")
            .unwrap_err()
            .0,
        409
    );
    let intact = service.handle_api(&list_req, "/api/drawings").unwrap();
    let intact: serde_json::Value = serde_json::from_slice(&intact.body).unwrap();
    assert_eq!(intact["drawings"][0]["points"][0]["price"], 28.5);
    edit["expectedDigest"] = serde_json::json!(next_digest);
    for bad in [
        serde_json::json!({"date": "2026-02-30", "price": 1}),
        serde_json::json!({"timeIndex": 1, "price": 1}),
        serde_json::json!({"date": "2026-09-01", "price": "NaN"}),
    ] {
        edit["points"] = serde_json::json!([bad]);
        let req = HttpRequest::for_internal("POST", "/api/drawings", edit.to_string().into_bytes());
        assert_eq!(
            service.handle_api(&req, "/api/drawings").unwrap_err().0,
            400
        );
    }
    let delete = HttpRequest::for_internal(
        "DELETE",
        &format!("/api/drawings?id=sd_test_1&expectedDigest={next_digest}"),
        Vec::new(),
    );
    assert_eq!(
        service
            .handle_api(&delete, "/api/drawings")
            .unwrap()
            .status_code,
        200
    );
    let remaining = service.handle_api(&list_req, "/api/drawings").unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&remaining.body).unwrap()["drawings"],
        serde_json::json!([])
    );

    assert!(module.shutdown().is_ok());
}
