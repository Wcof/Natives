//! 要紧事合并测试（R2 · A26/A32 部分）：供体优先级权重、去重、事件字段保留、
//! 有界输出与只读语义。

use super::*;
use crate::storage::Store;

fn opp(
    security_id: &str,
    name: &str,
    holding: bool,
    rank: i64,
    action: &str,
    pct: Option<f64>,
    constraints: usize,
) -> Value {
    let mut item = json!({
        "securityId": security_id,
        "name": name,
        "market": "SSE",
        "isHolding": holding,
        "rankScore": rank,
        "action": action,
        "constraints": vec!["x"; constraints],
        "evidence": {"lastChangePct": pct},
    });
    item["market"] = if security_id.starts_with("fund:") {
        "CN".into()
    } else {
        "SSE".into()
    };
    item
}

fn seed_event(store: &Store, object_key: &str, trigger: &str, rule_name: &str) -> i64 {
    store
        .with_write(|c| -> Result<i64, crate::finance::FinanceError> {
            c.execute(
                "INSERT INTO watch_rules(name,rule_type,object_key,params_json,enabled,created_at,updated_at) VALUES(?1,'price_threshold',?2,'{}',1,'0','0')",
                rusqlite::params![rule_name, object_key],
            )?;
            let rule_id = c.last_insert_rowid();
            c.execute(
                "INSERT INTO watch_events(rule_id,rule_version,rule_type,object_key,first_triggered_at,last_checked_at,trigger_value,threshold_value,snapshot_id,status,ready_state,created_at,updated_at) VALUES(?1,1,'price_threshold',?2,'0','0',?3,'','','open','ready','0','0')",
                rusqlite::params![rule_id, object_key, trigger],
            )?;
            Ok(c.last_insert_rowid())
        })
        .unwrap()
}

#[test]
fn donor_priority_weights_dedup_and_fields_preserved() {
    let store = Store::open_in_memory().unwrap();
    // 事件命中持仓证券：weight = 1000(事件) + 500(持仓) + |pct|。
    let event_id = seed_event(&store, "sh600519", "12.34", "价格突破");
    let items = json!({"items": [
        opp("security:SSE:600519", "贵州茅台", true, 80, "observe", Some(2.3), 1),
        // 持仓无约束无事件：weight = 500 + pct。
        opp("security:SSE:600036", "招商银行", true, 60, "observe", Some(1.0), 0),
        // 有向自选候选：weight = |pct|；涨幅更高者排前。
        opp("security:SSE:601318", "中国平安", false, 77, "build_candidate", Some(5.0), 0),
        opp("security:SSE:601398", "工商银行", false, 75, "avoid", Some(3.0), 0),
        // 有向候选第 3 名：入场。
        opp("security:SSE:601988", "中国银行", false, 74, "build_candidate", Some(9.0), 0),
        // 第 4 个有向候选：超过供体前 3 上限，不入列。
        opp("security:SSE:601288", "农业银行", false, 73, "build_candidate", Some(8.0), 0),
        // observe 自选无约束：不成为候选。
        opp("security:SSE:601939", "建设银行", false, 70, "observe", Some(8.0), 0),
    ]});
    let merged = store.with_snapshot(|c| merge(c, &items)).unwrap();
    assert_eq!(
        merged["priorityModel"],
        "event-1000-holding-500-absolute-change"
    );
    let rows = merged["items"].as_array().unwrap();
    // 前排：茅台（1000+500+2=1502）、平安（5→5）、工行（3）、招行（501）、中行不入。
    let moutai = rows
        .iter()
        .find(|r| r["securityId"] == "security:SSE:600519")
        .unwrap();
    assert_eq!(
        moutai["kind"], "event",
        "event identity preserved when merged with holding"
    );
    assert_eq!(moutai["weight"], 1000 + 500 + 2);
    assert_eq!(moutai["eventId"], event_id);
    assert_eq!(moutai["eventStatus"], "open");
    assert_eq!(
        moutai["changePct"], 2.3,
        "holding merge enriches market data"
    );
    assert_eq!(moutai["candidateRef"], "security:SSE:600519");
    let pingan = rows
        .iter()
        .find(|r| r["securityId"] == "security:SSE:601318")
        .unwrap();
    assert_eq!(pingan["kind"], "opportunity");
    assert_eq!(pingan["weight"], 5);
    let zhaohang = rows
        .iter()
        .find(|r| r["securityId"] == "security:SSE:600036")
        .unwrap();
    assert_eq!(zhaohang["kind"], "holding");
    assert_eq!(zhaohang["weight"], 501);
    assert!(
        !rows
            .iter()
            .any(|r| r["securityId"] == "security:SSE:601288"),
        "4th directional candidate beyond donor top-3 must not enter"
    );
    assert!(
        !rows
            .iter()
            .any(|r| r["securityId"] == "security:SSE:601939"),
        "plain observe watch item is not an urgent candidate"
    );
    // 权重排序：茅台(1502) > 招行(501) > 中行(9) > 平安(5) > 工行(3)。
    let order: Vec<&str> = rows
        .iter()
        .map(|r| r["securityId"].as_str().unwrap())
        .collect();
    assert_eq!(
        order,
        vec![
            "security:SSE:600519",
            "security:SSE:600036",
            "security:SSE:601988",
            "security:SSE:601318",
            "security:SSE:601398"
        ]
    );
}

#[test]
fn non_security_events_data_quality_and_limits_and_readonly() {
    let store = Store::open_in_memory().unwrap();
    // 数据质量事件（object_key=data）：无候选定位但保留条目。
    seed_event(&store, "data", "估值过期", "组合数据");
    let merged = store
        .with_snapshot(|c| merge(c, &json!({"items": []})))
        .unwrap();
    let rows = merged["items"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "event");
    assert_eq!(
        rows[0]["candidateRef"],
        Value::Null,
        "data-quality events cannot locate a candidate"
    );
    assert!(rows[0]["securityId"]
        .as_str()
        .unwrap()
        .starts_with("event:"));
    // 上限：9 条事件只保留权重排序后的前 8 条，truncated=true。
    for i in 0..9 {
        seed_event(
            &store,
            &format!("sh60051{i}"),
            &format!("{i}"),
            &format!("规则{i}"),
        );
    }
    let merged = store
        .with_snapshot(|c| merge(c, &json!({"items": []})))
        .unwrap();
    assert_eq!(merged["items"].as_array().unwrap().len(), LIMIT);
    assert_eq!(merged["truncated"], true);
    // 同 object_key 多行事件在合并层去重为一个身份（取排序稳定后的首位）。
    seed_event(&store, "sh600510", "13.00", "另一规则");
    let merged = store
        .with_snapshot(|c| merge(c, &json!({"items": []})))
        .unwrap();
    let rows = merged["items"].as_array().unwrap();
    let dupe: Vec<_> = rows
        .iter()
        .filter(|r| r["candidateRef"] == "security:SSE:600510")
        .collect();
    assert_eq!(
        dupe.len(),
        1,
        "multiple events on one security merge to one row"
    );
    // 只读：merge 前后 data_revision 不变。
    let before = store
        .with_read(crate::storage::Store::data_revision)
        .unwrap();
    let _ = store
        .with_snapshot(|c| merge(c, &json!({"items": []})))
        .unwrap();
    assert_eq!(
        store
            .with_read(crate::storage::Store::data_revision)
            .unwrap(),
        before
    );
}
