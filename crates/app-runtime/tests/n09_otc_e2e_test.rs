//! N09 场外基金操作：真实 natives-app-runtime 二进制 + 协议握手 + HTTP 全链。
//! 覆盖：登记持仓（净值推导）、编辑校准、加仓申请→确认（现金核销）、
//! 减仓申请→确认→到账（一次）、转换两腿、定投计划与期次登记。
//! 隔离 NATIVES_APPS_ROOT；迁移在测试库执行，不触真实用户数据。

mod support;
use app_runtime_core::framing::{read_frame, write_frame};
use app_runtime_core::protocol::{AppRequest, AppResponse, HandshakeResultV2, StartResult};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

const TEST_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn bin_path() -> std::path::PathBuf {
    support::runtime_binary()
}

fn setup(root: &std::path::Path) -> std::path::PathBuf {
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
            "fund": { "moduleApiVersion": 1, "dataSchemaVersion": 26, "capabilityVersion": 4, "uiTreeSha256": "0".repeat(64) },
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

fn api(port: u16, token: &str, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let body = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: null\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    let status: u16 = resp
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let payload = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, payload)
}

#[test]
fn test_n09_otc_operations_end_to_end_through_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let apps_root = setup(temp.path());
    let mut child = Command::new(bin_path())
        .arg(TEST_ORIGIN)
        .env("NATIVES_APPS_ROOT", &apps_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn app-runtime");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    let handshake = AppRequest {
        id: "hs".into(),
        method: "app:handshake".into(),
        params: serde_json::json!({
            "protocolVersion": 2, "appId": "fund", "productVersion": "0.1.0", "activationGeneration": 1
        }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&handshake).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().expect("handshake frame");
    let resp: AppResponse<HandshakeResultV2> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "handshake failed: {:?}", resp.error);

    let start = AppRequest {
        id: "st".into(),
        method: "app:start".into(),
        params: serde_json::json!({}),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&start).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().unwrap();
    let resp: AppResponse<StartResult> = serde_json::from_slice(&frame).unwrap();
    assert!(resp.ok, "start failed: {:?}", resp.error);
    let port = resp.result.unwrap().port;

    let session = AppRequest {
        id: "se".into(),
        method: "app:session".into(),
        params: serde_json::json!({ "op": "issue", "challenge": "challenge-n09-otc-0123456789ab" }),
    };
    write_frame(&mut stdin, &serde_json::to_vec(&session).unwrap()).unwrap();
    let frame = read_frame(&mut stdout).unwrap().unwrap();
    let resp: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    let token = resp["result"]["token"].as_str().unwrap().to_string();

    // UI 内嵌资源：新菜单与场外 bundle 必须在编译期产物内。
    let get = |path: &str| -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes())
            .unwrap();
        let mut r = String::new();
        s.read_to_string(&mut r).unwrap();
        r
    };
    let ui_resp = get("/");
    assert!(ui_resp.contains("200 OK"));
    assert!(
        ui_resp.contains("id=\"pos-tab-otc\""),
        "UI must embed the OTC tab"
    );
    let bundle = get("/app.js");
    assert!(
        bundle.contains("otcBuy"),
        "bundle must embed the N09 OTC menu actions"
    );
    assert!(
        bundle.contains("fundLedgerOtcPlansTitle"),
        "bundle must embed OTC plan wiring"
    );
    assert!(
        bundle.contains("applyNonTrading") || bundle.contains("otcPlansDialog"),
        "bundle must embed the OTC plan dialog"
    );

    let today = "2026-10-06";
    // 1) 播种隔离测试库基金身份与 NAV（schema 已由 runtime 建立；不触真实
    //    上游与用户库）。runtime 持有连接，WAL 允许并发写者排队。
    let db_path = apps_root.join("fund/data/fund.db");
    assert!(db_path.exists(), "fund.db must exist after start");
    let seed_sql = format!(
        "INSERT OR IGNORE INTO funds(code, name, fund_type, updated_at) VALUES
             ('110022','消费行业','fund','{today}'),
             ('005827','蓝筹混合','fund','{today}');
         INSERT OR IGNORE INTO fund_nav(fund_id, nav_date, unit_nav_raw, source, fetched_at)
             SELECT id, '{today}', 12345, 'test-fixture', '1' FROM funds WHERE code='110022';
         INSERT OR IGNORE INTO fund_nav(fund_id, nav_date, unit_nav_raw, source, fetched_at)
             SELECT id, '{today}', 10175, 'test-fixture', '1' FROM funds WHERE code='005827';"
    );
    let out = Command::new("/usr/bin/sqlite3")
        .arg(&db_path)
        .arg(&seed_sql)
        .output()
        .expect("sqlite3 seed");
    assert!(out.status.success(), "nav seed failed: {:?}", out.stderr);

    // 2) 登记场外持仓：市值 1234.56、收益 0 → 份额 1000.0486。
    let holding = serde_json::json!({
        "account": "测试账户", "fundCode": "110022", "fundName": "消费行业",
        "value": "1234.56", "gain": "0.00", "asOf": today, "requestId": "n09-hold-1"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/holding",
        Some(&holding.to_string()),
    );
    assert_eq!(status, 200, "holding create: {payload}");
    let (status, payload) = api(port, &token, "GET", "/api/positions/detail", None);
    assert_eq!(status, 200);
    assert_eq!(status, 200);
    assert!(payload.contains("110022"), "holding visible: {payload}");
    assert!(payload.contains("1000.0486"), "derived shares: {payload}");

    // 3) 编辑校准：有后续交易前是 opening；此处先建一笔买入再校准。
    let buy = serde_json::json!({
        "account": "测试账户", "fundCode": "110022", "fundName": "消费行业",
        "type": "BUY", "state": "confirmed", "quantity": "100.0000",
        "price": "1.2000", "fee": "0.00", "tradeDate": today,
        "source": "manual", "requestId": "n09-tx-1"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/transactions",
        Some(&buy.to_string()),
    );
    assert_eq!(status, 200, "tx: {payload}");
    let preview = serde_json::json!({
        "account": "测试账户", "fundCode": "110022",
        "value": "2000.00", "gain": "100.00", "asOf": today
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/holding/edit/preview",
        Some(&preview.to_string()),
    );
    assert_eq!(status, 200, "edit preview: {payload}");
    assert!(
        payload.contains("\"mode\":\"calibration\""),
        "must route to calibration: {payload}"
    );
    let edit = serde_json::json!({
        "mode": "calibration", "account": "测试账户", "fundCode": "110022",
        "value": "2000.00", "gain": "100.00", "asOf": today,
        "shares": "1100.0000", "reason": "渠道对账", "requestId": "n09-cal-1"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/holding/edit",
        Some(&edit.to_string()),
    );
    assert_eq!(status, 200, "calibration: {payload}");
    let (status, payload) = api(port, &token, "GET", "/api/positions/detail", None);
    assert_eq!(status, 200);
    assert!(
        payload.contains("\"quantity\":\"1100\""),
        "calibrated quantity: {payload}"
    );
    // 相同输入重放原结果；变更输入仍冲突，账本不能重复校准。
    let before: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let (status, _) = api(
        port,
        &token,
        "POST",
        "/api/otc/holding/edit",
        Some(&edit.to_string()),
    );
    assert_eq!(
        status, 200,
        "identical calibration retries replay the receipt"
    );
    let (status, payload) = api(port, &token, "GET", "/api/positions/detail", None);
    assert_eq!(status, 200);
    let after: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(after["dataRevision"], before["dataRevision"]);
    assert_eq!(after["holdings"], before["holdings"]);
    let mut conflicting = edit.clone();
    conflicting["reason"] = serde_json::json!("另一项更正");
    let (status, _) = api(
        port,
        &token,
        "POST",
        "/api/otc/holding/edit",
        Some(&conflicting.to_string()),
    );
    assert_eq!(
        status, 409,
        "request identity cannot accept changed content"
    );

    // 3.5) 播种现金资产（走既有 /api/assets）：使加仓/到账走完整资金链。
    let cash = serde_json::json!({
        "account": "测试账户", "productType": "cash", "name": "现金",
        "riskClass": "cash", "marketValue": "10000.00", "valuedAt": today
    });
    let (status, payload) = api(port, &token, "POST", "/api/assets", Some(&cash.to_string()));
    assert_eq!(status, 200, "cash asset: {payload}");
    // 配置账户预留（available 口径要求 configured；R01 语义）。
    // account_id=1：NATIVES_APPS_ROOT 隔离库首建账户（本测试仅此一户）。
    let reserve = serde_json::json!({"accountId": 1, "amount": "0.00"});
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/finance/reserve",
        Some(&reserve.to_string()),
    );
    assert_eq!(status, 200, "reserve config: {payload}");

    // 4) 加仓：申请（资金占用）→ 确认（现金恰扣一次）。
    let order = serde_json::json!({
        "op": "buy", "account": "测试账户", "fundCode": "110022", "fundName": "消费行业",
        "appliedDate": today, "cutoff": "before_1500", "amount": "1000.00", "requestId": "n09-buy-1"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders",
        Some(&order.to_string()),
    );
    assert_eq!(status, 200, "buy order: {payload}");
    let order_id = serde_json::from_str::<serde_json::Value>(&payload).unwrap()["id"]
        .as_i64()
        .unwrap();
    let confirm = serde_json::json!({
        "id": order_id, "expectedVersion": 1, "confirmedShares": "800.0000",
        "confirmedNavDate": today, "confirmedNav": "1.2300"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/confirm",
        Some(&confirm.to_string()),
    );
    assert_eq!(status, 200, "buy confirm: {payload}");
    // 重复确认 409。
    let (status, _) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/confirm",
        Some(&confirm.to_string()),
    );
    assert_eq!(status, 409, "duplicate confirm must 409");

    // 5) 减仓：申请 400 份 → 确认 → 到账一次 → 重复到账 409。
    let sell = serde_json::json!({
        "op": "sell", "account": "测试账户", "fundCode": "110022", "fundName": "消费行业",
        "appliedDate": today, "cutoff": "after_1500", "shares": "400.0000", "requestId": "n09-sell-1"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders",
        Some(&sell.to_string()),
    );
    assert_eq!(status, 200, "sell order: {payload}");
    let sell_id = serde_json::from_str::<serde_json::Value>(&payload).unwrap()["id"]
        .as_i64()
        .unwrap();
    let sell_confirm = serde_json::json!({
        "id": sell_id, "expectedVersion": 1, "confirmedShares": "300.0000",
        "confirmedAmount": "330.00", "fee": "3.30", "feeKnown": true,
        "confirmedNavDate": today, "confirmedNav": "1.1000"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/confirm",
        Some(&sell_confirm.to_string()),
    );
    assert_eq!(status, 200, "sell confirm: {payload}");
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/arrival",
        Some(&serde_json::json!({"id": sell_id, "date": today, "mode": "post"}).to_string()),
    );
    assert_eq!(status, 200, "arrival: {payload}");
    let (status, _) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/arrival",
        Some(&serde_json::json!({"id": sell_id, "date": today, "mode": "post"}).to_string()),
    );
    assert_eq!(status, 409, "double arrival must 409");

    // 6) 转换：两腿一次创建，out 幂等重试，out 确认后 in 才可确认。
    let conv = serde_json::json!({
        "op": "convert", "account": "测试账户", "fundCode": "110022", "fundName": "消费行业",
        "appliedDate": today, "cutoff": "before_1500", "shares": "500.0000",
        "targetCode": "005827", "targetName": "蓝筹混合", "requestId": "n09-conv-1"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders",
        Some(&conv.to_string()),
    );
    assert_eq!(status, 200, "convert: {payload}");
    let conv1 = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
    let retry = serde_json::from_str::<serde_json::Value>(
        &api(
            port,
            &token,
            "POST",
            "/api/otc/orders",
            Some(&conv.to_string()),
        )
        .1,
    )
    .unwrap();
    assert_eq!(conv1["id"], retry["id"], "convert retry must be idempotent");
    let out_id = conv1["id"].as_i64().unwrap();
    let in_id = conv1["inId"].as_i64().unwrap();
    let out_confirm = serde_json::json!({
        "id": out_id, "expectedVersion": 1, "confirmedShares": "500.0000",
        "confirmedAmount": "545.00", "fee": "5.45", "feeKnown": true,
        "confirmedNavDate": today, "confirmedNav": "1.0900"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/confirm",
        Some(&out_confirm.to_string()),
    );
    assert_eq!(status, 200, "convert out confirm: {payload}");
    let in_confirm = serde_json::json!({
        "id": in_id, "expectedVersion": 1, "confirmedShares": "530.0000",
        "confirmedAmount": "539.28", "fee": "0.00", "feeKnown": true,
        "confirmedNavDate": today, "confirmedNav": "1.0175"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/confirm",
        Some(&in_confirm.to_string()),
    );
    assert_eq!(status, 200, "convert in confirm: {payload}");
    // 转换转出腿不得登记到账（资金随转入过渡）。
    let (status, _) = api(
        port,
        &token,
        "POST",
        "/api/otc/orders/arrival",
        Some(&serde_json::json!({"id": out_id, "date": today, "mode": "post"}).to_string()),
    );
    assert_eq!(status, 400, "convert out must not have cash arrival");

    // 7) 定投：建计划 → 期次登记 → 重复期次 409。
    let plan = serde_json::json!({
        "account": "测试账户", "fundCode": "110022", "fundName": "消费行业",
        "amount": "500.00", "freq": "weekly", "weekday": 3,
        "startDate": "2026-09-09", "nonTrading": "next"
    });
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/plans",
        Some(&plan.to_string()),
    );
    assert_eq!(status, 200, "plan create: {payload}");
    let plan_id = serde_json::from_str::<serde_json::Value>(&payload).unwrap()["id"]
        .as_i64()
        .unwrap();
    let period = "2026-09-30"; // 2026-09-30 是周三。
    let run = serde_json::json!({"planId": plan_id, "periodKey": period, "mode": "buy", "requestId": "n09-run-1"});
    let (status, payload) = api(
        port,
        &token,
        "POST",
        "/api/otc/plans/runs/register",
        Some(&run.to_string()),
    );
    assert_eq!(status, 200, "run register: {payload}");
    let (dup_status, dup_body) = api(
        port,
        &token,
        "POST",
        "/api/otc/plans/runs/register",
        Some(&run.to_string()),
    );
    assert_eq!(dup_status, 409, "duplicate period must 409: {dup_body}");

    // 8) 最终对账：持仓与订单在途字段一致。
    let (status, payload) = api(port, &token, "GET", "/api/positions/detail", None);
    assert_eq!(status, 200);
    assert!(
        payload.contains("otcPendingBuyMinor"),
        "in-flight fields present: {payload}"
    );

    // 收尾：EOF 让 runtime 退出。
    drop(stdin);
    let _ = child.wait();
}
