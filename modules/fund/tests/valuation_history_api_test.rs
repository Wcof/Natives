use app_runtime_core::http::HttpRequest;
use fund_module::{api::handle_api_request, storage::Store};
use serde_json::{json, Value};

fn request(store: &Store, method: &str, path: &str, body: Value) -> Result<Value, (u16, String)> {
    let request = HttpRequest::for_internal(
        method,
        path,
        if method == "GET" {
            Vec::new()
        } else {
            body.to_string().into_bytes()
        },
    );
    handle_api_request(store, None, &request, path.split('?').next().unwrap())
        .map(|s| serde_json::from_str(&s).unwrap())
}
fn count(store: &Store, table: &str) -> i64 {
    store
        .with_read(|c| c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)))
        .unwrap()
}
#[test]
fn registered_fund_with_nav_history_has_trend_without_saved_valuations() {
    use fund_module::{fixed::Fixed, ledger};
    let store = Store::open_in_memory().unwrap();
    let today = store
        .with_read(|c| Ok::<_, rusqlite::Error>(fund_module::home::resolve_today(c)))
        .unwrap();
    let dates: Vec<String> = store
        .with_read(|c| {
            (0..4)
                .map(|n| {
                    c.query_row(
                        "SELECT date(?1,?2)",
                        rusqlite::params![today, format!("-{} days", 3 - n)],
                        |r| r.get(0),
                    )
                })
                .collect::<rusqlite::Result<_>>()
        })
        .unwrap();
    let fund = ledger::record_opening(
        &store,
        &ledger::OpeningInput {
            account_name: "history".into(),
            fund_code: "110022".into(),
            fund_name: "Fixture".into(),
            quantity: Fixed::share(100000),
            total_cost: None,
            as_of: dates[0].clone(),
        },
        &today,
    )
    .unwrap();
    store.with_write(|c| -> rusqlite::Result<()> {
        c.execute("INSERT INTO fund_asset_metadata(fund_id,nav_currency,risk_class,updated_at) VALUES(?1,'CNY','equity','0') ON CONFLICT(fund_id) DO UPDATE SET nav_currency='CNY'",[fund])?;
        for (n,date) in dates.iter().enumerate() {
            c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(?1,?2,?3,'user_confirmed','0')",rusqlite::params![fund,date,10000+n as i64*1000])?;
        }
        Ok(())
    }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let result = request(&store, "GET", "/api/home/snapshot?range=all", Value::Null).unwrap();
    let points = result["portfolio"]["history"]["points"].as_array().unwrap();
    assert_eq!(
        points.len(),
        4,
        "registered holdings and NAV must produce a multi-day chart without manual snapshots"
    );
    assert_eq!(points[0]["date"], dates[0]);
    assert_eq!(points[0]["valueMinor"], "1000");
    assert_eq!(points[2]["valueMinor"], "1200");
    assert!(result["portfolio"]["history"]["twrPpm"].is_null());
    assert_eq!(count(&store, "value_snapshots"), 0);
    assert_eq!(count(&store, "valuation_observations"), 0);
    assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
}
#[test]
fn observation_api_uses_server_valuation_rejects_inputs_and_preserves_read_only_get() {
    let store = Store::open_in_memory().unwrap();
    let today = store
        .with_read(|c| Ok::<_, rusqlite::Error>(fund_module::home::resolve_today(c)))
        .unwrap();
    request(&store,"POST","/api/offline/assets",json!({"name":"isolated","productType":"other","location":"test","valueMinor":"800000","costMinor":"800000","valuedAt":today,"riskClass":"other","attrs":{},"note":""})).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let path = "/api/home/valuation/observe?range=today";
    assert_eq!(
        request(&store, "POST", path, json!({"valueMinor":"1"}))
            .unwrap_err()
            .0,
        400
    );
    assert_eq!(request(&store, "POST", path, json!([])).unwrap_err().0, 400);
    assert_eq!(
        request(
            &store,
            "POST",
            "/api/home/valuation/observe?range=invalid",
            json!({})
        )
        .unwrap_err()
        .0,
        400
    );
    assert_eq!(
        request(&store, "GET", path, Value::Null).unwrap_err().0,
        404
    );
    assert_eq!(count(&store, "valuation_observations"), 0);
    let result = request(&store, "POST", path, json!({})).unwrap();
    assert_eq!(result["wealth"]["totalA"], 800000);
    assert_eq!(result["portfolio"]["history"]["timeResolution"], "minute");
    assert_eq!(count(&store, "valuation_observations"), 1);
    assert_eq!(count(&store, "value_snapshots"), 0);
    assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
    request(&store, "GET", "/api/home/snapshot?range=today", Value::Null).unwrap();
    assert_eq!(count(&store, "valuation_observations"), 1);
    request(&store, "POST", path, json!({})).unwrap();
    assert_eq!(count(&store, "valuation_observations"), 1);
    let past = "2000-01-01";
    request(
        &store,
        "POST",
        &format!("/api/home/valuation/observe?range=custom&from={past}&to={past}"),
        json!({}),
    )
    .unwrap();
    assert_eq!(count(&store, "valuation_observations"), 1);
}
