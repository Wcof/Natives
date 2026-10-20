//! evidence 域回归（伴生测试文件；规模门禁拆分自 evidence.rs）。
use super::*;

#[test]
fn default_evidence_omits_account_identity_and_amount() {
    let store = Store::open_in_memory().unwrap();
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('Private Account',0,0)", [])?;
            c.execute("INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,legacy_overlap_distinct,created_at,updated_at) VALUES(1,'cash','cash','cash','cash',12345678,?1,0,0,0)", [&today_cn()])?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let hidden = evidence(&store, &revision, &[], false).unwrap().to_string();
    assert!(!hidden.contains("Private Account") && !hidden.contains("12345678"));
    let disclosed = evidence(&store, &revision, &[], true).unwrap().to_string();
    assert!(disclosed.contains("Private Account") && disclosed.contains("12345678"));
}

// 方案 11 §7 回归：单项范围必须命中真实持仓并实际约束证据。
#[test]
fn single_asset_scope_is_validated_and_constrains_evidence() {
    let store = Store::open_in_memory().unwrap();
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('Acc',0,0)", [])?;
            c.execute("INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,legacy_overlap_distinct,created_at,updated_at) VALUES(1,'cash','cash','Gold Bar','gold',20000,?1,0,0,0)", [&today_cn()])?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    // 命中真实手工资产：scope 带真实名称与风险类。
    let hit = evidence_task(
        &store,
        &revision,
        &[],
        false,
        "diagnosis",
        "manual:1",
        &[],
        "",
        "",
    )
    .unwrap();
    let scope = &hit["scope"];
    assert_eq!(scope["kind"], "manual");
    assert_eq!(scope["name"], "Gold Bar");
    assert_eq!(scope["riskClass"], "gold");
    // 审计五-3：单项范围不得携带其他账户数据（无论是否披露金额）。
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute(
                "INSERT INTO accounts(name,created_at,updated_at) VALUES('OUT_OF_SCOPE_ACCOUNT',0,0)",
                [],
            )?;
            c.execute(
                "INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,legacy_overlap_distinct,created_at,updated_at) VALUES(2,'cash','cash','OTHER_ASSET','cash',90000,?1,0,0,0)",
                [&today_cn()],
            )?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let disclosed = evidence_task(
        &store,
        &revision,
        &[],
        true,
        "diagnosis",
        "manual:1",
        &[],
        "",
        "",
    )
    .unwrap()
    .to_string();
    assert!(
        !disclosed.contains("OUT_OF_SCOPE_ACCOUNT") && !disclosed.contains("90000"),
        "single scope + disclose must not leak other accounts: {disclosed}"
    );
    assert!(
        disclosed.contains("Gold Bar"),
        "single scope keeps the target asset"
    );
    let hidden = evidence_task(
        &store,
        &revision,
        &[],
        false,
        "diagnosis",
        "manual:1",
        &[],
        "",
        "",
    )
    .unwrap()
    .to_string();
    assert!(
        !hidden.contains("20000"),
        "single scope without disclose must not carry valueMinor: {hidden}"
    );

    // E3 范围约束（方案 05 §3/审查项 11）：单项范围只携带可关联到该资产的规则
    // 事件；组合级事件（class:/data/cash）不混入；组合范围仍完整保留。
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO watch_rules(name,rule_type,object_key,params_json,period,enabled,version,archived,created_at,updated_at) VALUES('r1','price_threshold','sh600519','{}','quote',1,1,0,'0','0')", [])?;
            c.execute("INSERT INTO watch_rules(name,rule_type,object_key,params_json,period,enabled,version,archived,created_at,updated_at) VALUES('r2','allocation_drift','class:gold','{}','daily',1,1,0,'0','0')", [])?;
            c.execute("INSERT INTO watch_events(rule_id,rule_version,rule_type,object_key,first_triggered_at,last_checked_at,status,ready_state,created_at,updated_at) VALUES(1,1,'price_threshold','sh600519','0','0','open','ready','0','0')", [])?;
            c.execute("INSERT INTO watch_events(rule_id,rule_version,rule_type,object_key,first_triggered_at,last_checked_at,status,ready_state,created_at,updated_at) VALUES(2,1,'allocation_drift','class:gold','0','0','open','ready','0','0')", [])?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    // manual:1 无证券代码：单项范围 E3 为空，且不出现组合级事件。
    let manual_single = evidence_task(
        &store,
        &revision,
        &[],
        true,
        "diagnosis",
        "manual:1",
        &[],
        "",
        "",
    )
    .unwrap();
    assert_eq!(
        manual_single["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "E3")
            .unwrap()["scope"],
        "single"
    );
    assert_eq!(
        manual_single["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "E3")
            .unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "manual asset without code must not inherit unrelated events"
    );
    // fund 单项：按证券代码匹配（sh600519 → security:SSE:600519 段匹配）。
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO funds(code,name,updated_at,instrument_kind,market,display_code) VALUES('stock:SSE:600519','贵州茅台','0','stock','SSE','600519')", [])?;
            c.execute("INSERT INTO transactions(account_id,fund_id,type,state,quantity_raw,price_raw,amount_raw,fee_raw,trade_date,source,created_at) VALUES(1,1,'BUY','confirmed',10000,500000,5000000000,0,'2026-10-01','manual','0')", [])?;
            crate::portfolio::recompute_position(c, 1, 1).map_err(|e| {
                rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR), Some(e.to_string()))
            })?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let fund_single = evidence_task(
        &store,
        &revision,
        &[],
        true,
        "diagnosis",
        "fund:1:1",
        &[],
        "",
        "",
    )
    .unwrap();
    let e3 = fund_single["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "E3")
        .unwrap()
        .clone();
    assert_eq!(e3["scope"], "single");
    let ids: Vec<&str> = e3["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"price_threshold"),
        "matched per-asset event must stay: {e3}"
    );
    assert!(
        !ids.contains(&"allocation_drift"),
        "portfolio-level event must not leak into single scope: {e3}"
    );
    // 组合范围：两类事件都在。
    let portfolio =
        evidence_task(&store, &revision, &[], true, "diagnosis", "", &[], "", "").unwrap();
    let p_e3 = portfolio["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "E3")
        .unwrap()
        .clone();
    assert_eq!(p_e3["scope"], "portfolio");
    assert_eq!(
        p_e3["events"].as_array().unwrap().len(),
        2,
        "portfolio scope keeps all events"
    );

    // 不存在的引用被拒绝，不贴标签放行。
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "diagnosis",
        "manual:99",
        &[],
        "",
        ""
    )
    .is_err());
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "diagnosis",
        "fund:1:2",
        &[],
        "",
        ""
    )
    .is_err());
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "diagnosis",
        "bogus",
        &[],
        "",
        ""
    )
    .is_err());
}

// M08（方案 05 §3/§4）：资讯影响任务的真实证据链。
#[test]
fn news_impact_requires_information_and_carries_evidence() {
    let store = Store::open_in_memory().unwrap();
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute(
                "INSERT INTO information_items(source_id, external_id, title, url, content_text, \
                 published_at, time_precision, retrieved_at, content_hash, completeness, content_version, created_at, updated_at) \
                 VALUES('em_stock_ann','AN1','回购公告','https://x','正文第一段','2026-09-28T07:13:39Z','second','2026-10-01T00:00:00Z','h','full',1,'2026-10-01T00:00:00Z','2026-10-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    // news_impact 无资讯 → 拒绝（不生成无对象的泛泛分析）。
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "news_impact",
        "",
        &[],
        "",
        ""
    )
    .is_err());
    // 不存在的资讯 → 拒绝。
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "news_impact",
        "",
        &[99],
        "",
        ""
    )
    .is_err());
    // 携带资讯 → I 证据带版本/精度/片段与 untrusted 标记。
    let pack = evidence_task(
        &store,
        &revision,
        &[],
        false,
        "news_impact",
        "",
        &[1],
        "",
        "",
    )
    .unwrap();
    let info = pack["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "I1")
        .expect("I1 evidence piece must exist");
    assert_eq!(info["completeness"], "full");
    assert_eq!(info["contentVersion"], 1);
    assert_eq!(info["untrusted"], json!(true));
    assert_eq!(info["excerpt"], "正文第一段");
    // 其他任务也可以带资讯（行动条件参考公告）。
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "conditions",
        "",
        &[1],
        "",
        ""
    )
    .is_ok());

    // M08（审查项 12）：单项范围校验资讯与范围的关系——无关资讯拒绝，
    // 有关联资讯通过并标注 scopeRelation=related。
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('Acc',0,0)", [])?;
            c.execute("INSERT INTO funds(code,name,updated_at,instrument_kind,market,display_code) VALUES('stock:SSE:600519','贵州茅台','0','stock','SSE','600519')", [])?;
            c.execute("INSERT INTO transactions(account_id,fund_id,type,state,quantity_raw,price_raw,amount_raw,fee_raw,trade_date,source,created_at) VALUES(1,1,'BUY','confirmed',10000,500000,5000000000,0,'2026-10-01','manual','0')", [])?;
            crate::portfolio::recompute_position(c, 1, 1).map_err(|e| {
                rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR), Some(e.to_string()))
            })?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    // 资讯 1 无 relations → 单项范围拒绝。
    assert!(
        evidence_task(
            &store,
            &revision,
            &[],
            false,
            "news_impact",
            "fund:1:1",
            &[1],
            "",
            ""
        )
        .is_err(),
        "news unrelated to the scoped asset must be rejected"
    );
    // 关联到该证券身份（公告 direct ref）→ 通过。
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO information_relations(item_id,instrument_ref,relation,reason,created_at) VALUES(1,'security:CN:stock:600519','direct','公告身份','2026-10-01T00:00:00Z')", [])?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let single_pack = evidence_task(
        &store,
        &revision,
        &[],
        false,
        "news_impact",
        "fund:1:1",
        &[1],
        "",
        "",
    )
    .unwrap();
    let info = single_pack["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "I1")
        .unwrap()
        .clone();
    assert_eq!(
        info["scopeRelation"], "related",
        "related news must be marked: {info}"
    );
}

// 方案 05「AI 四类任务」：盘前／盘后简报按市场＋时段＋日期圈定范围；
// 无记录市场如实 coverage=none，不虚构行情概览。
#[test]
fn briefing_scope_validated_and_coverage_is_honest() {
    let store = Store::open_in_memory().unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    // 无效市场／时段／单项范围拒绝。
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "briefing",
        "",
        &[],
        "GLOBAL",
        "pre"
    )
    .is_err());
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "briefing",
        "",
        &[],
        "CN",
        "noon"
    )
    .is_err());
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        false,
        "briefing",
        "fund:1:1",
        &[],
        "CN",
        "pre"
    )
    .is_err());
    // 有持仓与两期净值：coverage 记账本事实，recentMoves 带相邻两期变化。
    store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('Acc',0,0)", [])?;
            c.execute("INSERT INTO funds(code,name,updated_at,instrument_kind,market,display_code) VALUES('stock:SSE:600519','贵州茅台','0','stock','SSE','600519')", [])?;
            for (date, raw) in [("2026-09-29", 1_500_000i64), ("2026-09-30", 1_515_000i64)] {
                c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,?2,'manual','0')", rusqlite::params![date, raw])?;
            }
            c.execute("INSERT INTO transactions(account_id,fund_id,type,state,quantity_raw,price_raw,amount_raw,fee_raw,trade_date,source,created_at) VALUES(1,1,'BUY','confirmed',10000,500000,5000000000,0,'2026-09-30','manual','0')", [])?;
            crate::portfolio::recompute_position(c, 1, 1).map_err(|e| {
                rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR), Some(e.to_string()))
            })?;
            Ok(())
        }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let pack = evidence_task(
        &store,
        &revision,
        &[],
        false,
        "briefing",
        "",
        &[],
        "CN",
        "post",
    )
    .unwrap();
    assert_eq!(pack["task"], "briefing");
    assert_eq!(pack["scope"]["market"], "CN");
    assert_eq!(pack["scope"]["session"], "post");
    assert!(pack["instructions"]
        .as_str()
        .unwrap()
        .contains("盘前／盘后简报"));
    let e6 = pack["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "E6")
        .unwrap();
    assert_eq!(e6["quoteCoverage"]["coveredFunds"], json!(1));
    assert_eq!(e6["quoteCoverage"]["latestAsOf"], json!("2026-09-30"));
    let mv = &e6["recentMoves"][0];
    assert_eq!(mv["asOf"], json!("2026-09-30"));
    // (1515000-1500000)*1e6/1500000 = 10000 ppm = 1%。
    assert_eq!(mv["change"]["changePpm"], json!(10_000));
    assert_eq!(mv["change"]["previousAsOf"], json!("2026-09-29"));
    // 账本只有境内标的：US 简报覆盖如实为 0（能力矩阵口径），不虚构。
    let us = evidence_task(
        &store,
        &revision,
        &[],
        false,
        "briefing",
        "",
        &[],
        "US",
        "pre",
    )
    .unwrap();
    let e6 = us["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "E6")
        .unwrap();
    assert_eq!(e6["quoteCoverage"]["coveredFunds"], json!(0));
    assert!(e6["quoteCoverage"]["statement"]
        .as_str()
        .unwrap()
        .contains("No recorded quotes"));
}

#[test]
fn public_research_is_scoped_and_rejects_private_fields() {
    let store = Store::open_in_memory().unwrap();
    store.with_write(|c| -> rusqlite::Result<()> {
        c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('Private Research Account',0,0)",[])?;
        c.execute("INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,legacy_overlap_distinct,created_at,updated_at) VALUES(1,'cash','cash','Private Cash','cash',12345678,?1,0,0,0)",[&today_cn()])?;
        Ok(())
    }).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    let packet = evidence_task(
        &store,
        &revision,
        &[],
        false,
        "diagnosis",
        "research:sh600519",
        &[],
        "",
        "",
    )
    .unwrap();
    assert_eq!(packet["scope"]["research"], "sh600519");
    assert_eq!(packet["evidence"].as_array().unwrap().len(), 1);
    for text in [
        "Private Research Account",
        "Private Cash",
        "12345678",
        "financialGroups",
        "user-strategies",
        "recorded-quotes",
    ] {
        assert!(!packet.to_string().contains(text));
    }
    assert!(evidence_task(
        &store,
        &revision,
        &[],
        true,
        "diagnosis",
        "research:sh600519",
        &[],
        "",
        ""
    )
    .is_err());
    let sample = json!({"identity":{"symbol":"sh600519","period":"dayk","fq":"qfq","seriesKind":"OHLC","range":{"start":0,"end":100}},"rows":[{"date":"2026-09-01","open":10.2,"close":10.4}],"source":"public-provider","capturedAt":"2026-09-01T07:00:00Z"});
    let parsed = research_context(&sample).unwrap();
    assert_eq!(parsed["untrusted"], true);
    let mut invalid = sample.clone();
    invalid["accountName"] = "Private Research Account".into();
    assert!(research_context(&invalid).is_err());
    invalid = sample.clone();
    invalid["rows"][0]["quantityMinor"] = json!(12345678);
    assert!(research_context(&invalid).is_err());
    invalid = sample.clone();
    invalid["rows"][0]["date"] = "2026-02-30".into();
    assert!(research_context(&invalid).is_err());
    invalid = sample.clone();
    invalid["rows"][0]["close"] = "NaN".into();
    assert!(research_context(&invalid).is_err());
    invalid = sample.clone();
    invalid["identity"]["fq"] = "made-up".into();
    assert!(research_context(&invalid).is_err());
}
