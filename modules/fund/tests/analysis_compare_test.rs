//! 报告变化回顾（M07）API 集成测试：first 语义、同任务前次对比、资讯版本确定性比较。

use fund_module::api::reports_api;
use fund_module::storage::Store;

fn save_report(
    store: &Store,
    task: &str,
    scope: &str,
    data_revision: &str,
    evidence: &str,
    saved_at: &str,
) -> i64 {
    store
        .with_write(|c| -> rusqlite::Result<i64> {
            c.execute(
                "INSERT INTO analysis_reports(task, scope_json, evidence_json, snapshot_id, data_revision, provider_id, model_id, model_name, result_text, generated_at, saved_at) \
                 VALUES(?1,?2,?3,'snap',?4,'p/m','m','n','结果文本',?5,?5)",
                rusqlite::params![task, scope, evidence, data_revision, saved_at],
            )?;
            Ok(c.last_insert_rowid())
        })
        .unwrap()
}

#[test]
fn compare_first_and_information_version_changes() {
    let store = Store::open_in_memory().unwrap();
    // 首份报告（无更早报告）→ first:true。
    let r1 = save_report(
        &store,
        "news_impact",
        r#"{"task":"news_impact","range":""}"#,
        "rev1",
        r#"{"evidence":[{"id":"I1","kind":"information","contentVersion":1}]}"#,
        "2026-10-01T00:00:00Z",
    );
    let body = reports_api::api_analysis_report_compare(
        &store,
        &format!("/api/analysis/reports/compare?id={r1}"),
    )
    .unwrap();
    assert!(body.contains("\"first\":true"), "{body}");
    // 第二份同任务报告：I1 更新到 v2、新增 I2、去掉 I0。
    let r2 = save_report(
        &store,
        "news_impact",
        r#"{"task":"news_impact","range":""}"#,
        "rev2",
        r#"{"evidence":[{"id":"I1","kind":"information","contentVersion":2},{"id":"I2","kind":"information","contentVersion":1}]}"#,
        "2026-10-02T00:00:00Z",
    );
    let body = reports_api::api_analysis_report_compare(
        &store,
        &format!("/api/analysis/reports/compare?id={r2}"),
    )
    .unwrap();
    assert!(body.contains("\"first\":false"));
    assert!(body.contains("\"informationAdded\":[\"I2\"]"), "{body}");
    assert!(body.contains("\"informationUpdated\":[\"I1\"]"), "{body}");
    assert!(body.contains("\"dataRevisionChanged\":true"), "{body}");
    // 不同范围不强比（审计五-4）：manual:1 与 manual:2 两份同任务报告不比较。
    let r4 = save_report(
        &store,
        "news_impact",
        r#"{"task":"news_impact","asset":"manual:1"}"#,
        "rev1",
        r#"{"evidence":[{"id":"I1","kind":"information","contentVersion":1}]}"#,
        "2026-10-04T00:00:00Z",
    );
    let r5 = save_report(
        &store,
        "news_impact",
        r#"{"task":"news_impact","asset":"manual:2"}"#,
        "rev1",
        r#"{"evidence":[{"id":"I1","kind":"information","contentVersion":1}]}"#,
        "2026-10-05T00:00:00Z",
    );
    let body = reports_api::api_analysis_report_compare(
        &store,
        &format!("/api/analysis/reports/compare?id={r5}"),
    )
    .unwrap();
    assert!(
        body.contains("\"first\":true"),
        "different scope must not compare: {body}"
    );
    let _ = r4;
    // 不同任务不强比：conditions 报告没有同任务前次 → first:true。
    let r3 = save_report(
        &store,
        "conditions",
        r#"{"task":"conditions","range":""}"#,
        "rev2",
        r#"{"evidence":[]}"#,
        "2026-10-03T00:00:00Z",
    );
    let body = reports_api::api_analysis_report_compare(
        &store,
        &format!("/api/analysis/reports/compare?id={r3}"),
    )
    .unwrap();
    assert!(body.contains("\"first\":true"), "{body}");
    // 不存在 → 404。
    assert_eq!(
        reports_api::api_analysis_report_compare(&store, "/api/analysis/reports/compare?id=999")
            .unwrap_err()
            .0,
        404
    );
}

#[test]
fn report_save_real_api_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let evidence = fund_module::home::evidence_task(
        &store,
        &revision,
        &[],
        false,
        "diagnosis",
        "",
        &[],
        "",
        "",
    )
    .unwrap();
    let body = serde_json::json!({"task":"diagnosis","resultText":"未验证原文","snapshotId":evidence["snapshotId"],"dataRevision":evidence["dataRevision"],"scope":evidence["scope"],"evidence":evidence});
    let result = reports_api::api_analysis_report_save(&store, &body.to_string()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap()["validation"]["status"],
        "unverified"
    );
    let mut invalid = body.clone();
    invalid["snapshotId"] = serde_json::json!("wrong");
    assert_eq!(
        reports_api::api_analysis_report_save(&store, &invalid.to_string())
            .unwrap_err()
            .0,
        400
    );
    // 立即删除走真实 API（审计 REPORT_DELETE 曾 404）。
    let del = reports_api::api_analysis_report_delete(&store, r#"{"id":1,"expectedVersion":1}"#);
    println!("DELETE_RESULT: {del:?}");
    assert!(del.is_ok(), "delete must succeed: {del:?}");
    assert_eq!(
        reports_api::api_analysis_report_restore(&store, r#"{"id":1,"expectedVersion":1}"#)
            .unwrap_err()
            .0,
        409
    );
    reports_api::api_analysis_report_restore(&store, r#"{"id":1,"expectedVersion":2}"#).unwrap();
}

#[test]
fn research_preview_and_saved_report_keep_public_scope() {
    let store = Store::open_in_memory().unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let body = serde_json::json!({"dataRevision":revision,"sourceIds":[],"discloseFinance":false,"task":"diagnosis","scope":{"asset":"research:sh600519"},"research":{"identity":{"symbol":"sh600519","period":"dayk","fq":"qfq","seriesKind":"OHLC","range":{"start":0,"end":100}},"rows":[{"date":"2026-09-01","open":10,"close":11}],"source":"isolated-public-series","capturedAt":"2026-09-01T07:00:00Z"}});
    let packet: serde_json::Value = serde_json::from_str(
        &fund_module::api::home_api::api_home_evidence(&store, &body.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(packet["scope"]["research"], "sh600519");
    assert_eq!(packet["evidence"].as_array().unwrap().len(), 2);
    assert_eq!(packet["evidence"][1]["id"], "R1");
    assert_eq!(packet["evidence"][1]["untrusted"], true);
    assert!(packet["snapshotId"]
        .as_str()
        .unwrap()
        .contains(":research:"));
    let mut changed = body.clone();
    changed["research"]["identity"]["symbol"] = "sz000001".into();
    assert_eq!(
        fund_module::api::home_api::api_home_evidence(&store, &changed.to_string())
            .unwrap_err()
            .0,
        400
    );
    changed = body.clone();
    changed["research"]["rows"][0]["close"] = serde_json::json!(12);
    let newer: serde_json::Value = serde_json::from_str(
        &fund_module::api::home_api::api_home_evidence(&store, &changed.to_string()).unwrap(),
    )
    .unwrap();
    assert_ne!(newer["snapshotId"], packet["snapshotId"]);
    let save = serde_json::json!({"task":"diagnosis","scope":packet["scope"],"snapshotId":packet["snapshotId"],"dataRevision":packet["dataRevision"],"providerId":"isolated","modelId":"public-test","modelName":"isolated/public-test","resultText":"Unverified test analysis [R1]","evidence":packet});
    let saved = reports_api::api_analysis_report_save(&store, &save.to_string()).unwrap();
    assert!(saved.contains("id"));
    let list: serde_json::Value = serde_json::from_str(
        &reports_api::api_analysis_reports(&store, "/api/analysis/reports?page=1").unwrap(),
    )
    .unwrap();
    assert_eq!(list["reports"][0]["scope"]["research"], "sh600519");
}
