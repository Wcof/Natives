//! T04 持仓完整快照接口 GET /api/positions/detail 的端到端断言
//! （A19~A22 API 级：身份/场馆、未知成本、待确认→确认回放、净值游标、归档过滤）。

use app_runtime_core::http::HttpRequest;
use app_runtime_core::module::{BuiltInAppModule, ModuleContext, ModuleService};
use fund_module::FundModule;
use serde_json::Value;
use std::sync::Arc;

struct Harness {
    // start() 返回 Arc<dyn ModuleService>；temp、module 与 service 在本测试内共同存活。
    _temp: tempfile::TempDir,
    #[allow(dead_code)]
    module: FundModule,
    service: Arc<dyn ModuleService>,
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let ctx = ModuleContext::for_app(temp.path(), "fund", "0.1.0", 1);
        let mut module = FundModule::new();
        assert!(module.initialize(&ctx).is_ok());
        let service = module.start().unwrap();
        Self {
            _temp: temp,
            module,
            service,
        }
    }

    fn call(&self, method: &str, path: &str, body: &str) -> (u16, Value) {
        let route = path.split('?').next().unwrap().to_string();
        let req = HttpRequest::for_internal(method, path, body.as_bytes().to_vec());
        let (status, text) = match self.service.handle_api(&req, &route) {
            Ok(resp) => (resp.status_code, String::from_utf8(resp.body).unwrap()),
            Err((status, text)) => (status, text),
        };
        let json: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{method} {path} -> 非 JSON: {text} ({e})"));
        (status, json)
    }

    fn ok(&self, method: &str, path: &str, body: &str) -> Value {
        let (status, json) = self.call(method, path, body);
        assert_eq!(status, 200, "{method} {path} 失败: {json}");
        json
    }

    fn detail(&self, query: &str) -> Value {
        self.ok("GET", &format!("/api/positions/detail{query}"), "")
    }

    fn holding<'a>(&self, snap: &'a Value, code: &str) -> &'a Value {
        snap["holdings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["fundCode"] == Value::from(code))
            .unwrap_or_else(|| panic!("快照缺少 {code}: {snap}"))
    }
}

#[test]
fn positions_detail_snapshot_end_to_end() {
    let h = Harness::new();

    // 建档 + 三类标的：场外基金（未知成本）/ 场内 ETF（已知成本）/ 股票在第二账户。
    h.ok("POST", "/api/accounts", r#"{"name":"测试甲"}"#);
    h.ok("POST", "/api/accounts", r#"{"name":"测试乙"}"#);
    h.ok(
        "POST",
        "/api/openings",
        r#"{"account":"测试甲","fundCode":"012414","fundName":"场外基金A","quantity":"1000.0000","asOf":"2026-09-30"}"#,
    );
    h.ok(
        "POST",
        "/api/openings",
        r#"{"account":"测试甲","fundCode":"510300","instrumentKind":"etf","market":"SSE","fundName":"沪深300ETF","quantity":"100.0000","totalCost":"300.00","asOf":"2026-09-30"}"#,
    );
    h.ok(
        "POST",
        "/api/openings",
        r#"{"account":"测试乙","fundCode":"600519","instrumentKind":"stock","market":"SSE","fundName":"贵州茅台","quantity":"100.0000","totalCost":"158000.00","asOf":"2026-09-30"}"#,
    );

    let snap = h.detail("");
    assert!(snap["dataRevision"].is_string());
    let accounts = snap["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0]["version"], 1);
    assert_eq!(accounts[0]["archived"], false);

    let otc = h.holding(&snap, "012414");
    assert_eq!(otc["venue"], "otc");
    assert!(otc["quoteSymbol"].is_null());
    assert_eq!(otc["costUnknown"], true);
    assert_eq!(otc["quantity"], "1000");
    assert_eq!(otc["pendingCount"], 0);
    assert!(otc["nav"].is_null());
    assert!(otc["marketValue"].is_null());
    assert_eq!(otc["valuationQuality"], "missing_nav");

    let etf = h.holding(&snap, "510300");
    assert_eq!(etf["venue"], "exchange");
    assert_eq!(etf["quoteSymbol"], "sh510300");
    assert_eq!(etf["instrumentKind"], "etf");
    assert_eq!(etf["costUnknown"], false);
    assert_eq!(etf["cost"], "300");

    // 场外待确认：pending BUY 不进在持数量，只以 pendingCount 暴露。
    h.ok(
        "POST",
        "/api/transactions",
        r#"{"account":"测试甲","fundCode":"012414","fundName":"场外基金A","type":"BUY","state":"pending","quantity":"500.0000","price":"1.2345","fee":"0.00","tradeDate":"2026-10-01","requestId":"pd-buy-1"}"#,
    );
    let snap = h.detail("");
    let otc = h.holding(&snap, "012414");
    assert_eq!(otc["pendingCount"], 1);
    assert_eq!(otc["quantity"], "1000");

    // 确认回放：份额入账、待确认清零、未知成本标记不被后续买入覆盖。
    let pending = h.ok("GET", "/api/transactions?state=pending", "");
    let tx = pending["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["source"] == Value::from("manual"))
        .expect("pending 流水");
    let (id, version) = (tx["id"].as_i64().unwrap(), tx["version"].as_i64().unwrap());
    h.ok(
        "POST",
        "/api/transactions/confirm",
        &format!(r#"{{"id":{id},"expectedVersion":{version}}}"#),
    );
    let snap = h.detail("");
    let otc = h.holding(&snap, "012414");
    assert_eq!(otc["pendingCount"], 0);
    assert_eq!(otc["quantity"], "1500");
    assert_eq!(otc["costUnknown"], true);

    // 净值游标：用户确认估值后 nav 携带日期。
    let rev = snap["dataRevision"].as_str().unwrap().to_string();
    let fund_id = otc["fundId"].as_i64().unwrap();
    h.ok(
        "POST",
        "/api/holding/valuation",
        &format!(
            r#"{{"fundId":{fund_id},"price":"1.3600","asOf":"2026-09-30","navCurrency":"CNY","dataRevision":"{rev}"}}"#
        ),
    );
    let snap = h.detail("");
    let otc = h.holding(&snap, "012414");
    assert_eq!(otc["nav"]["nav"], "1.36");
    assert_eq!(otc["nav"]["date"], "2026-09-30");
    assert_eq!(otc["marketValue"], "2040");

    // 归档过滤：测试乙默认隐藏（含其持仓），includeArchived=1 可见且带标记。
    let accounts_all = h.ok("GET", "/api/accounts/detail?view=all", "");
    let yi = accounts_all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == Value::from("测试乙"))
        .unwrap();
    let yi_id = yi["id"].as_i64().unwrap();
    h.ok(
        "POST",
        "/api/accounts/archive",
        &format!(r#"{{"id":{yi_id},"expectedVersion":1,"requestId":"ar-1"}}"#),
    );
    let snap = h.detail("");
    assert_eq!(snap["accounts"].as_array().unwrap().len(), 1);
    assert!(h.detail("")["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .all(|x| x["fundCode"] != "600519"));
    let snap = h.detail("?includeArchived=1");
    assert_eq!(snap["accounts"].as_array().unwrap().len(), 2);
    assert!(snap["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["id"] == Value::from(yi_id) && a["archived"] == Value::from(true)));
    assert!(snap["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["fundCode"] == "600519"));

    // 乐观锁：重复归档 expectedVersion 过期 → 409。
    let (status, body) = h.call(
        "POST",
        "/api/accounts/archive",
        &format!(r#"{{"id":{yi_id},"expectedVersion":1,"requestId":"ar-2"}}"#),
    );
    assert_eq!(status, 409, "过期版本应冲突: {body}");
}

#[test]
fn positions_value_uses_exact_round_down_and_currency_authority() {
    let h = Harness::new();
    let date = fund_module::home::resolve_today(&rusqlite::Connection::open_in_memory().unwrap());
    h.ok("POST", "/api/openings", &format!(r#"{{"account":"小额","fundCode":"012414","quantity":"0.0123","totalCost":"0","asOf":"{date}"}}"#));
    let snap = h.detail("");
    let fund = h.holding(&snap, "012414")["fundId"].as_i64().unwrap();
    let rev = snap["dataRevision"].as_str().unwrap();
    h.ok("POST", "/api/holding/valuation", &format!(r#"{{"fundId":{fund},"price":"1.2345","asOf":"{date}","navCurrency":"CNY","dataRevision":"{rev}"}}"#));
    let snap = h.detail("");
    let row = h.holding(&snap, "012414");
    assert_eq!(row["marketValue"], "0.01"); // 0.01518435 向零截断，与账本一致。
    assert_eq!(row["valuationQuality"], "complete");
    assert_eq!(row["costUnknown"], false);
    let rev = snap["dataRevision"].as_str().unwrap();
    h.ok("POST", "/api/holding/valuation", &format!(r#"{{"fundId":{fund},"price":"1.2345","asOf":"{date}","navCurrency":"unsupported","dataRevision":"{rev}"}}"#));
    let snap = h.detail("");
    let row = h.holding(&snap, "012414");
    assert!(row["marketValue"].is_null());
    assert_eq!(row["valuationQuality"], "currency_unsupported");
    let before = snap["dataRevision"].clone();
    let (status, _) = h.call("GET", "/api/positions/detail?refresh=1&kind=invalid", "");
    assert_eq!(status, 400);
    assert_eq!(h.detail("")["dataRevision"], before);
}

#[test]
fn account_order_conflict_rolls_back_all_accounts_and_retry_is_idempotent() {
    let h = Harness::new();
    h.ok("POST", "/api/accounts", r#"{"name":"甲"}"#);
    h.ok("POST", "/api/accounts", r#"{"name":"乙"}"#);
    let a = h.detail("")["accounts"].as_array().unwrap().clone();
    let (first, second) = (a[0]["id"].as_i64().unwrap(), a[1]["id"].as_i64().unwrap());
    let payload = |version| {
        format!(
            r#"{{"items":[{{"id":{second},"expectedVersion":1}},{{"id":{first},"expectedVersion":{version}}}],"requestId":"sort-check"}}"#
        )
    };
    let (status, _) = h.call("POST", "/api/accounts/order", &payload(99));
    assert_eq!(status, 409);
    assert_eq!(h.detail("")["accounts"], serde_json::json!(a));
    h.ok("POST", "/api/accounts/order", &payload(1));
    let sorted = h.detail("")["accounts"].clone();
    assert_eq!(sorted[0]["id"], second);
    assert_eq!(sorted[1]["id"], first);
    h.ok("POST", "/api/accounts/order", &payload(1));
    assert_eq!(h.detail("")["accounts"], sorted);
    let (status, _) = h.call("POST", "/api/accounts/order", &format!(r#"{{"items":[{{"id":{first},"expectedVersion":1}},{{"id":{first},"expectedVersion":1}}],"requestId":"duplicate"}}"#));
    assert_eq!(status, 400);
    assert_eq!(h.detail("")["accounts"], sorted);
}

#[test]
fn four_decimal_quantity_and_price_preserve_remaining_cost_and_realized_profit() {
    let h = Harness::new();
    let today = fund_module::home::resolve_today(&rusqlite::Connection::open_in_memory().unwrap());
    h.ok("POST", "/api/openings", &format!(r#"{{"account":"精度","fundCode":"012414","quantity":"100.1234","totalCost":"123.45","asOf":"{today}"}}"#));
    h.ok("POST", "/api/transactions", &format!(r#"{{"account":"精度","fundCode":"012414","type":"BUY","state":"confirmed","quantity":"1.2345","price":"2.3456","fee":"0.01","tradeDate":"{today}","requestId":"four-buy"}}"#));
    let snap = h.detail("");
    let row = h.holding(&snap, "012414");
    assert_eq!(row["quantity"], "101.3579");
    assert_eq!(row["cost"], "126.35"); // 123.45 + trunc(1.2345 × 2.3456) + 0.01.
    h.ok("POST", "/api/transactions", &format!(r#"{{"account":"精度","fundCode":"012414","type":"SELL","state":"confirmed","quantity":"1.2345","price":"3","fee":"0.02","tradeDate":"{today}","requestId":"four-sell"}}"#));
    let snap = h.detail("");
    let row = h.holding(&snap, "012414");
    assert_eq!(row["quantity"], "100.1234");
    assert_eq!(row["cost"], "124.82");
    assert_eq!(row["averageCost"], "1.2466");
    assert_eq!(row["realized"], "2.15"); // 3.70 − 0.02 − 1.53; remaining cost + sold cost = 126.35.
    let before = snap["dataRevision"].clone();
    for (quantity, price) in [
        ("1.23451", "1"),
        ("1", "1.23451"),
        ("1e2", "1"),
        ("-1", "1"),
    ] {
        let (status, _) = h.call("POST", "/api/transactions", &format!(r#"{{"account":"精度","fundCode":"012414","type":"BUY","state":"confirmed","quantity":"{quantity}","price":"{price}","fee":"0","tradeDate":"{today}","requestId":"invalid-four"}}"#));
        assert!(status >= 400);
        assert_eq!(h.detail("")["dataRevision"], before);
    }
}

#[test]
fn closed_positions_keep_realized_profit_and_unknown_cost_remains_a_gap() {
    for cost in [Some("100"), None] {
        let h = Harness::new();
        let today =
            fund_module::home::resolve_today(&rusqlite::Connection::open_in_memory().unwrap());
        let mut opening =
            serde_json::json!({"account":"清仓","fundCode":"012414","quantity":"100","asOf":today});
        if let Some(cost) = cost {
            opening["totalCost"] = serde_json::json!(cost);
        }
        h.ok("POST", "/api/openings", &opening.to_string());
        h.ok("POST","/api/transactions",&format!(r#"{{"account":"清仓","fundCode":"012414","type":"SELL","quantity":"100","price":"2","fee":"0","tradeDate":"{today}","requestId":"close-profit"}}"#));
        let snap = h.detail("");
        assert!(snap["holdings"].as_array().unwrap().is_empty());
        assert_eq!(snap["realized"][0]["kind"], "fund");
        assert_eq!(
            snap["realized"][0]["value"],
            if cost.is_some() {
                serde_json::json!("100")
            } else {
                Value::Null
            }
        );
    }
}
