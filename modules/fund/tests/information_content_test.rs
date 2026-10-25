use app_runtime_core::{
    http::HttpRequest,
    module::{BuiltInAppModule, ModuleContext},
};
use fund_module::{
    information::{
        adapters::{self, RawItem},
        normalize::TimePrecision,
        store,
    },
    storage::Store,
};
use serde_json::{json, Value};

#[test]
fn all_information_sources_reuse_cached_body_without_network() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = ModuleContext::for_app(temp.path(), "fund", "0.1.0", 1);
    let mut module = fund_module::FundModule::new();
    module.initialize(&ctx).unwrap();
    let service = module.start().unwrap();
    let db = Store::open(&ctx.module_data_root).unwrap();
    for source in [
        adapters::SRC_FINANCE,
        adapters::SRC_STOCK_NEWS,
        adapters::SRC_FUND_ANN,
        adapters::SRC_STOCK_ANN,
    ] {
        let now: String = db
            .with_read(|c| {
                c.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        let external = if source.ends_with("ann") {
            "AN123456"
        } else {
            "202610201234567890"
        };
        let item = RawItem {
            source_id: source,
            external_id: external.into(),
            title: "Public fixture".into(),
            url: "https://example.test/unused".into(),
            summary: "Summary".into(),
            codes: vec![],
            published_at: None,
            precision: TimePrecision::Unknown,
            importance: 0,
            event_types: vec![],
            event_date: None,
        };
        let id = db
            .with_write(|c| {
                store::upsert_items(c, &[item], &|_| vec![], &now)?;
                let id = c.last_insert_rowid();
                store::save_content(c, id, "Cached full body", "full", &now)?;
                Ok::<_, rusqlite::Error>(id)
            })
            .unwrap();
        let input = json!({"id":id,"expectedVersion":2,"requestId":source}).to_string();
        let reply = service.handle_api(
            &HttpRequest::for_internal("POST", "/api/information/content", input.into_bytes()),
            "/api/information/content",
        );
        assert!(
            reply.is_ok(),
            "{source} must support cached text: {reply:?}"
        );
        let task: Value = serde_json::from_slice(&reply.unwrap().body).unwrap();
        let path = format!("/api/information/status?taskId={}", task["taskId"]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let response = service
                .handle_api(
                    &HttpRequest::for_internal("GET", &path, vec![]),
                    "/api/information/status",
                )
                .unwrap();
            let status: Value = serde_json::from_slice(&response.body).unwrap();
            if status["status"] == "complete" {
                assert!(status["result"]["error"].is_null(), "{source}: {status}");
                assert_eq!(status["result"]["content"]["cached"], true);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "content task did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[test]
#[ignore = "real public publisher through isolated Fund service; run explicitly"]
fn real_news_and_fund_body_task_persist_readable_text() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = ModuleContext::for_app(temp.path(), "fund", "0.1.0", 1);
    let mut module = fund_module::FundModule::new();
    module.initialize(&ctx).unwrap();
    let service = module.start().unwrap();
    let db = Store::open(&ctx.module_data_root).unwrap();
    let news = adapters::fetch_stock_news("贵州茅台").unwrap().remove(0);
    let fund = adapters::fetch_fund_announcements("510300")
        .unwrap()
        .remove(0);
    for item in [news, fund] {
        let source = item.source_id;
        let id = db
            .with_write(|c| {
                store::upsert_items(c, &[item], &|_| vec![], "public-probe")?;
                Ok::<_, rusqlite::Error>(c.last_insert_rowid())
            })
            .unwrap();
        let input = json!({"id":id,"expectedVersion":1,"requestId":source}).to_string();
        let reply = service
            .handle_api(
                &HttpRequest::for_internal("POST", "/api/information/content", input.into_bytes()),
                "/api/information/content",
            )
            .unwrap();
        let task: Value = serde_json::from_slice(&reply.body).unwrap();
        let path = format!("/api/information/status?taskId={}", task["taskId"]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
        loop {
            let response = service
                .handle_api(
                    &HttpRequest::for_internal("GET", &path, vec![]),
                    "/api/information/status",
                )
                .unwrap();
            let status: Value = serde_json::from_slice(&response.body).unwrap();
            if status["status"] == "complete" {
                assert!(status["result"]["error"].is_null(), "{status}");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "body task exceeded deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let path = format!("/api/information/item?id={id}");
        let response = service
            .handle_api(
                &HttpRequest::for_internal("GET", &path, vec![]),
                "/api/information/item",
            )
            .unwrap();
        let detail: Value = serde_json::from_slice(&response.body).unwrap();
        let text = detail["contentText"].as_str().unwrap();
        assert!(text.len() > 100 && text.len() <= store::MAX_CONTENT_BYTES);
        assert_eq!(detail["contentVersion"], 2);
        assert_eq!(detail["contentSupported"], true);
        assert_eq!(detail["contentCacheExpired"], false);
        assert!(!detail["url"].as_str().unwrap().contains("jjgg_"));
        println!(
            "{source}: stored {} bytes, {}, version {}",
            text.len(),
            detail["completeness"],
            detail["contentVersion"]
        );
    }
}
