use crate::{assets::AssetsSummary, storage::Store};
use rusqlite::params;
use serde_json::json;

#[test]
fn whole_asset_ranges_use_real_endpoints_flows_and_explicit_live_reference_without_writes() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let date = |n| crate::assets::minus_days(&today, n);
    store.with_write(|c|{
        c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('range','0','0')",[])?;
        c.execute("INSERT INTO cash_events(account_id,event_type,amount_minor,occurred_at,created_at) VALUES(1,'external_in',3000,?1,'0')",[date(1)])?;
        for (n,value,through) in [(10,10000,0),(4,12000,0),(2,14000,0),(1,15000,1)]{
            let meta=json!({"flowsChecked":true,"cashEventThrough":through,"transactionThrough":0,"revision":"0"});
            c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?1,?2,?2,0,'ready','','0',?3)",params![date(n),value,meta.to_string()])?;
        }Ok::<_,rusqlite::Error>(())
    }).unwrap();
    let summary = AssetsSummary {
        complete_value_minor: Some(18000),
        quality: "ready".into(),
        ..Default::default()
    };
    let run = |from: &str, to: &str| {
        store
            .with_read(|c| crate::home_period::project(c, &summary, from, to, &today))
            .unwrap()
    };
    let all = run("", &today);
    assert_eq!(all["netReturnMinor"], "5000");
    assert_eq!(all["status"], "reference");
    let short = run(&date(3), &today);
    assert_eq!(short["netReturnMinor"], "3000");
    assert_eq!(short["baselineDate"], date(4));
    let historical = run(&date(3), &date(2));
    assert_eq!(historical["netReturnMinor"], "2000");
    assert_eq!(historical["status"], "ready");
    let daily = run(&today, &today);
    assert_eq!(daily["netReturnMinor"], "3000");
    assert!(run(&date(20), &date(15))["netReturnMinor"].is_null());
    assert_eq!(
        store
            .with_read(
                |c| c.query_row("SELECT COUNT(*) FROM value_snapshots", [], |r| r
                    .get::<_, i64>(0))
            )
            .unwrap(),
        4
    );
    store
        .with_write(|c| {
            c.execute(
                "UPDATE cash_events SET event_type='reconciliation' WHERE id=1",
                [],
            )
        })
        .unwrap();
    assert_eq!(run("", &today)["reason"], "unexplained_adjustment");
}

#[test]
fn selected_history_range_and_today_are_not_a_fixed_sixty_day_window() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let first = crate::assets::minus_days(&today, 4);
    store.with_write(|c|{
        for n in 0..5 {
            let date=crate::assets::minus_days(&today,4-n);
            let meta=json!({"flowsChecked":true,"cashEventThrough":0,"transactionThrough":0,"revision":"0"});
            c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?1,?2,0,?2,'ready','','0',?3)",params![date,10000+n*1000,meta.to_string()])?;
        }
        Ok::<_,rusqlite::Error>(())
    }).unwrap();
    let from = crate::assets::minus_days(&today, 1);
    let summary = AssetsSummary {
        complete_value_minor: Some(14000),
        quality: "ready".into(),
        ..Default::default()
    };
    let selected = store
        .with_read(|c| {
            crate::home::snapshot_inner(c, &today, &from, &today, "custom".into(), &summary, false)
        })
        .unwrap();
    assert_eq!(selected["portfolio"]["history"]["from"], from);
    assert_eq!(selected["portfolio"]["history"]["to"], today);
    assert!(!selected["portfolio"]["history"]["points"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["date"] == first));
    assert_eq!(
        crate::home::resolve_range("today", None, None, &today).0,
        today
    );
}
