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
    assert_eq!(desc.capability_version, 1);

    // 2. initialize
    assert!(module.initialize(&ctx).is_ok());

    // 3. health before start
    let health = module.health().unwrap();
    assert!(health.ok);

    // 4. start (creates DB & runs migrations)
    assert!(module.start().is_ok());

    // 5. handle_ui
    let ui_resp = module.handle_ui("/").unwrap();
    assert!(ui_resp.is_some());
    let html = String::from_utf8(ui_resp.unwrap().body).unwrap();
    assert!(html.contains("基金记账"));

    let ui_miss = module.handle_ui("/api/something").unwrap();
    assert!(ui_miss.is_none());

    // 6. handle_api (GET /api/positions)
    let get_req = HttpRequest::for_internal("GET", "/api/positions", Vec::new());
    let api_resp = module.handle_api(&get_req, "/api/positions").unwrap();
    assert_eq!(api_resp.status_code, 200);
    let body = String::from_utf8(api_resp.body).unwrap();
    assert!(body.contains("\"positions\":[]"));

    // 7. data_status
    let status = module.data_status().unwrap();
    assert_eq!(status.current_schema, 2);
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
    assert!(module.start().is_ok());

    // 1. 保存：前端 POST 口径（symbol 未带前缀，服务端归一化为 sh600900）
    let save_body = r##"{
        "id": "sd_test_1",
        "symbol": "600900",
        "period": "day",
        "toolType": "horizontal",
        "points": [{"date": "2026-08-12", "price": 27.96}],
        "options": {"color": "#ff2d2d", "width": 2, "dash": "solid", "name": "短期支撑"}
    }"##;
    let save_req =
        HttpRequest::for_internal("POST", "/api/drawings", save_body.as_bytes().to_vec());
    let save_resp = module.handle_api(&save_req, "/api/drawings").unwrap();
    assert_eq!(save_resp.status_code, 200);
    assert!(String::from_utf8(save_resp.body)
        .unwrap()
        .contains("\"success\":true"));

    // 2. 列表：真实 HTTP 口径——request.path 携带 query，route 已剥离 query
    let list_req =
        HttpRequest::for_internal("GET", "/api/drawings?symbol=600900&period=day", Vec::new());
    let list_resp = module.handle_api(&list_req, "/api/drawings").unwrap();
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
    let list_resp2 = module.handle_api(&list_req2, "/api/drawings").unwrap();
    assert_eq!(list_resp2.status_code, 200);
    assert!(String::from_utf8(list_resp2.body)
        .unwrap()
        .contains("sd_test_1"));

    // 4. 其他周期/标的不得串数据
    let list_req3 =
        HttpRequest::for_internal("GET", "/api/drawings?symbol=600900&period=week", Vec::new());
    let list_resp3 = module.handle_api(&list_req3, "/api/drawings").unwrap();
    assert!(!String::from_utf8(list_resp3.body)
        .unwrap()
        .contains("sd_test_1"));

    assert!(module.shutdown().is_ok());
}
