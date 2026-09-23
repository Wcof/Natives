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
