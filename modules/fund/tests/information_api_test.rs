//! 资讯 API 集成测试（隔离内存库；网络路径由 information_upstream_probe 实网覆盖）。

use fund_module::api::information_api;
use fund_module::information::{self, store};
use fund_module::storage::Store;

fn seed_item(store: &Store) -> i64 {
    let item = information::adapters::RawItem {
        source_id: information::adapters::SRC_STOCK_ANN,
        external_id: "AN2026001".into(),
        title: "贵州茅台:关于回购股份的公告".into(),
        url: "https://data.eastmoney.com/notices/detail/600519/AN2026001.html".into(),
        summary: String::new(),
        codes: vec!["600519".into()],
        published_at: Some("2026-09-28T07:13:39Z".into()),
        precision: fund_module::information::normalize::TimePrecision::Second,
        importance: 2,
        event_types: vec!["repurchase"],
        event_date: None,
    };
    store
        .with_write(|conn| {
            store::upsert_items(
                conn,
                &[item],
                &|i| {
                    i.codes
                        .iter()
                        .map(|c| {
                            (
                                information::instrument_ref_for_code(c),
                                "direct",
                                "公告身份".to_string(),
                            )
                        })
                        .collect()
                },
                "2026-10-01T00:00:00Z",
            )
        })
        .unwrap();
    store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM information_items", [], |r| {
                r.get::<_, i64>(0)
            })
        })
        .unwrap()
}

#[test]
fn items_list_filters_by_scope() {
    let store = Store::open_in_memory().unwrap();
    seed_item(&store);
    // holdings scope：无持仓基金 → 空列表但来源状态与覆盖说明仍在（成功无结果语义）。
    let holdings =
        information_api::api_information_items(&store, "/api/information/items?scope=holdings")
            .unwrap();
    assert!(holdings.contains("\"scope\":\"holdings\""));
    assert!(holdings.contains("coverage"));
    // 审计五-1：无持仓不得返回全部资料（成功无结果语义）。
    assert!(
        holdings.contains("\"items\":[]"),
        "no holdings must yield empty items: {holdings}"
    );
    // all scope：能看到已采集条目。
    let all =
        information_api::api_information_items(&store, "/api/information/items?scope=all").unwrap();
    assert!(all.contains("回购股份的公告"));
    let other = Store::open_in_memory().unwrap();
    // Same revision/scope must never reuse another ledger's private list cache.
    other.with_write::<_, rusqlite::Error>(|_| Ok(())).unwrap();
    let empty =
        information_api::api_information_items(&other, "/api/information/items?scope=all").unwrap();
    assert!(
        empty.contains("\"items\":[]"),
        "cache belongs to the Store: {empty}"
    );
    // 非法 scope 400。
    assert_eq!(
        information_api::api_information_items(&store, "/api/information/items?scope=bogus")
            .unwrap_err()
            .0,
        400
    );
}

#[test]
fn item_detail_and_flags_and_archive() {
    let store = Store::open_in_memory().unwrap();
    let id = seed_item(&store);
    let detail =
        information_api::api_information_item(&store, &format!("/api/information/item?id={id}"))
            .unwrap();
    assert!(
        detail.contains("\"relation\":\"direct\""),
        "公告 codes → direct 关联"
    );
    assert!(detail.contains("security:CN:stock:600519"));
    // 个人状态。
    information_api::api_information_flags(
        &store,
        &format!(r#"{{"id":{id},"read":true,"favorite":true}}"#),
    )
    .unwrap();
    let detail =
        information_api::api_information_item(&store, &format!("/api/information/item?id={id}"))
            .unwrap();
    assert!(detail.contains("\"read\":true") && detail.contains("\"favorite\":true"));
    // 归档。
    information_api::api_information_archive(&store, &format!(r#"{{"id":{id}}}"#)).unwrap();
    let detail =
        information_api::api_information_item(&store, &format!("/api/information/item?id={id}"))
            .unwrap();
    assert!(detail.contains("\"archived\":true"));
    // 缺 id 400；不存在 404。
    assert_eq!(
        information_api::api_information_item(&store, "/api/information/item")
            .unwrap_err()
            .0,
        400
    );
    assert_eq!(
        information_api::api_information_item(&store, "/api/information/item?id=999")
            .unwrap_err()
            .0,
        404
    );
}

/// R02（方案 05 §8）：复核提醒经路由可查、可处理（acknowledge→resolve），
/// 输出 itemId 供前端打开依据详情。
#[test]
fn alert_lifecycle_through_routes() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO information_alerts(asset_ref, source_ref, content_version, title, \
                 event_type, status, first_triggered_at, updated_at) \
                 VALUES('security:CN:stock:600519','em_stock_ann:AN2026001',1,'回购公告','repurchase', \
                 'open','2026-10-01T00:00:00Z','2026-10-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO information_items(source_id, external_id, title, url, \
                 published_at, time_precision, retrieved_at, content_hash, completeness, \
                 content_version, created_at, updated_at) \
                 VALUES('em_stock_ann','AN2026001','回购公告','https://x', \
                 '2026-09-28T07:13:39Z','second','2026-10-01T00:00:00Z','h','full',1, \
                 '2026-10-01T00:00:00Z','2026-10-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    // 列表带 itemId（供前端查看依据）。
    let list = information_api::api_information_alerts(&store).unwrap();
    assert!(
        list.contains("\"itemId\":"),
        "alerts must expose itemId: {list}"
    );
    assert!(!list.contains("\"status\":\"resolved\""));
    // 未知操作 400。
    assert_eq!(
        information_api::api_information_alert_action(&store, r#"{"id":1,"action":"bogus"}"#)
            .unwrap_err()
            .0,
        400
    );
    // acknowledge → resolve → 列表为空。
    information_api::api_information_alert_action(&store, r#"{"id":1,"action":"acknowledge"}"#)
        .unwrap();
    let list = information_api::api_information_alerts(&store).unwrap();
    assert!(list.contains("\"status\":\"acknowledged\""));
    information_api::api_information_alert_action(&store, r#"{"id":1,"action":"resolve"}"#)
        .unwrap();
    let list = information_api::api_information_alerts(&store).unwrap();
    assert!(
        list.contains("\"alerts\":[]"),
        "resolved alerts leave the list: {list}"
    );
}

#[test]
fn sources_whitelist_rejects_unknown() {
    let store = Store::open_in_memory().unwrap();
    let body = information_api::api_information_sources(&store).unwrap();
    assert!(
        body.contains("em_stock_news")
            && body.contains("em_stock_ann")
            && body.contains("em_fund_ann")
    );
    assert!(body.contains("coverage"), "必须返回实际覆盖说明");
    // 白名单内启停。
    information_api::api_information_source_enabled(
        &store,
        r#"{"sourceId":"em_fund_ann","enabled":false}"#,
    )
    .unwrap();
    let body = information_api::api_information_sources(&store).unwrap();
    let fund_entry = body.split('{').find(|s| s.contains("em_fund_ann")).unwrap();
    assert!(fund_entry.contains("\"enabled\":false"));
    // 白名单外拒绝。
    assert_eq!(
        information_api::api_information_source_enabled(
            &store,
            r#"{"sourceId":"xueqiu","enabled":true}"#
        )
        .unwrap_err()
        .0,
        400
    );
}

/// M04（方案 04 §4）：生产列表按关联分层排序，而非纯时间；
/// 归档条目不进入默认列表；缓存随写事务（revision）失效。
#[test]
fn list_ranks_by_relation_layer_and_excludes_archived() {
    let store = Store::open_in_memory().unwrap();
    let mk = |external_id: &str, title: &str, published: &str, importance: i32| {
        information::adapters::RawItem {
            source_id: information::adapters::SRC_STOCK_ANN,
            external_id: external_id.into(),
            title: title.into(),
            url: format!("https://data.eastmoney.com/notices/detail/600519/{external_id}.html"),
            summary: String::new(),
            codes: vec!["600519".into()],
            published_at: Some(published.into()),
            precision: fund_module::information::normalize::TimePrecision::Second,
            importance,
            event_types: vec!["notice"],
            event_date: None,
        }
    };
    // 三条均为 direct 关联、时间倒序：老的重要公告（importance 2）必须排在
    // 新的一般公告（importance 0）之前；候选关联的条目最后。
    let items = vec![
        mk("AN1", "一般公告新", "2026-09-28T09:00:00Z", 0),
        mk("AN2", "重要公告旧", "2026-09-27T09:00:00Z", 2),
        mk("AN3", "一般公告更旧", "2026-09-26T09:00:00Z", 1),
    ];
    store
        .with_write(|conn| {
            store::upsert_items(
                conn,
                &items,
                &|i| {
                    i.codes
                        .iter()
                        .map(|c| {
                            (
                                information::instrument_ref_for_code(c),
                                "direct",
                                "公告身份".to_string(),
                            )
                        })
                        .collect()
                },
                "2026-10-01T00:00:00Z",
            )
        })
        .unwrap();
    let refs = vec![information::instrument_ref_for_code("600519")];
    let ranked = store
        .with_read(|conn| store::list_items(conn, &refs, false, 50, 0))
        .unwrap();
    let titles: Vec<&str> = ranked.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(
        titles,
        vec!["重要公告旧", "一般公告新", "一般公告更旧"],
        "直接关联重要公告 → 其他直接（时间倒序）：{titles:?}"
    );
    // 归档后不进入默认列表。
    let archived_id: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id FROM information_items WHERE external_id='AN2'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    store
        .with_write(|conn| store::set_archived(conn, archived_id, true, "2026-10-01T00:00:00Z"))
        .unwrap();
    let after = store
        .with_read(|conn| store::list_items(conn, &refs, false, 50, 0))
        .unwrap();
    assert!(
        after.iter().all(|r| r.id != archived_id),
        "archived items must not appear in the default list"
    );
}

#[test]
fn content_requires_existing_item_and_supported_source() {
    use app_runtime_core::{
        http::HttpRequest,
        module::{BuiltInAppModule, ModuleContext},
    };
    let temp = tempfile::tempdir().unwrap();
    let ctx = ModuleContext::for_app(temp.path(), "fund", "0.1.0", 1);
    let mut module = fund_module::FundModule::new();
    module.initialize(&ctx).unwrap();
    let service = module.start().unwrap();
    let store = Store::open(&ctx.module_data_root).unwrap();
    let content = |body: &str| match service.handle_api(
        &HttpRequest::for_internal("POST", "/api/information/content", body.as_bytes().to_vec()),
        "/api/information/content",
    ) {
        Ok(response) => (
            response.status_code,
            String::from_utf8(response.body).unwrap(),
        ),
        Err(error) => error,
    };
    // 不存在 → 404。
    assert_eq!(
        content(r#"{"id":42,"expectedVersion":1,"requestId":"missing"}"#).0,
        404
    );
    // 基金公告来源无正文端点 → 明确 unsupported（不伪造正文）。
    let fund_item = information::adapters::RawItem {
        source_id: information::adapters::SRC_FUND_ANN,
        external_id: "111".into(),
        title: "华泰柏瑞沪深300ETF 2026年中期报告".into(),
        url: "https://fundf10.eastmoney.com/jjgg_510300.html".into(),
        summary: String::new(),
        codes: vec!["510300".into()],
        published_at: Some("2026-08-28T16:00:00Z".into()),
        precision: fund_module::information::normalize::TimePrecision::Day,
        importance: 3,
        event_types: vec!["earnings"],
        event_date: None,
    };
    store
        .with_write(|conn| {
            store::upsert_items(
                conn,
                &[fund_item],
                &|i| {
                    i.codes
                        .iter()
                        .map(|c| {
                            (
                                information::instrument_ref_for_fund(c),
                                "direct",
                                "基金身份".to_string(),
                            )
                        })
                        .collect()
                },
                "2026-10-01T00:00:00Z",
            )
        })
        .unwrap();
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id FROM information_items WHERE source_id='em_fund_ann'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    let err = content(&format!(
        r#"{{"id":{fund_id},"expectedVersion":1,"requestId":"unsupported"}}"#
    ));
    assert_eq!(err.0, 400);
    assert!(err.1.contains("UNSUPPORTED"));
    let stock = seed_item(&store);
    assert_eq!(
        content(&format!(
            r#"{{"id":{stock},"expectedVersion":999,"requestId":"stale"}}"#
        ))
        .0,
        409
    );
    assert_eq!(
        content(&format!(
            r#"{{"id":{stock},"expectedVersion":1,"requestId":"url","url":"http://127.0.0.1"}}"#
        ))
        .0,
        400
    );
    information_api::api_information_source_enabled(
        &store,
        r#"{"sourceId":"em_stock_ann","enabled":false}"#,
    )
    .unwrap();
    assert_eq!(
        content(&format!(
            r#"{{"id":{stock},"expectedVersion":1,"requestId":"disabled"}}"#
        ))
        .0,
        400
    );
    store
        .with_write(|c| store::set_archived(c, stock, true, "now"))
        .unwrap();
    assert_eq!(
        content(&format!(
            r#"{{"id":{stock},"expectedVersion":1,"requestId":"archived"}}"#
        ))
        .0,
        404
    );
    module.shutdown().unwrap();
}

#[test]
fn filter_validation_and_cache_keys_are_distinct() {
    let s = Store::open_in_memory().unwrap();
    seed_item(&s);
    let list = |p| information_api::api_information_items(&s, p);
    assert!(list("/api/information/items?scope=all&from=2026-02-30").is_err());
    assert!(list("/api/information/items?scope=all&from=2026-10-01&to=2026-09-01").is_err());
    assert!(list("/api/information/items?scope=all&q=NoMatch")
        .unwrap()
        .contains("\"items\":[]"));
    assert!(!list("/api/information/items?scope=all")
        .unwrap()
        .contains("\"items\":[]"));
    assert!(
        list("/api/information/items?scope=all&type=earnings&sort=latest")
            .unwrap()
            .contains("\"items\":[]")
    );
    assert!(
        !list("/api/information/items?scope=all&type=repurchase&sort=latest")
            .unwrap()
            .contains("\"items\":[]")
    );
    assert!(list("/api/information/items?scope=all&type=unknown").is_err());
    assert!(list("/api/information/items?scope=all&sort=arbitrary").is_err());
    assert_eq!(
        information::normalize::parse_publish_time("2026-02-30").0,
        None
    );
}

#[test]
fn alert_deferral_validates_time_expires_and_cannot_reopen_resolved() {
    let s = Store::open_in_memory().unwrap();
    seed_item(&s);
    s.with_write(|c| {
        information::upsert_review_alert(
            c,
            "security:SSE:600519",
            "em_stock_ann:AN2026001",
            1,
            "公告",
            "repurchase",
            "2026-10-01T00:00:00Z",
        )
    })
    .unwrap();
    let alert: i64 = s
        .with_read(|c| {
            c.query_row("SELECT id FROM information_alerts LIMIT 1", [], |r| {
                r.get(0)
            })
        })
        .unwrap();
    let act = |action: &str, until: &str| {
        information_api::api_information_alert_action(
            &s,
            &serde_json::json!({"id":alert,"action":action,"snoozedUntil":until}).to_string(),
        )
    };
    assert_eq!(act("snooze", "2026-02-30").unwrap_err().0, 400);
    assert_eq!(act("snooze", "2000-01-01").unwrap_err().0, 400);
    assert_eq!(act("snooze", "中文中文中文abcd").unwrap_err().0, 400);
    act("snooze", "2999-01-01").unwrap();
    assert!(information_api::api_information_alerts(&s)
        .unwrap()
        .contains("snoozed"));
    s.with_write(|c| {
        c.execute(
            "UPDATE information_alerts SET snoozed_until='2000-01-01T00:00:00Z' WHERE id=?1",
            [alert],
        )
    })
    .unwrap();
    let expired = information_api::api_information_alerts(&s).unwrap();
    assert!(expired.contains("\"status\":\"open\""));
    act("acknowledge", "").unwrap();
    act("resolve", "").unwrap();
    assert_eq!(act("snooze", "2999-01-01").unwrap_err().0, 404);
    assert!(information_api::api_information_alerts(&s)
        .unwrap()
        .contains("\"alerts\":[]"));
}

#[test]
fn information_notifications_share_existing_claim_owner_and_are_once_per_revision() {
    let s = Store::open_in_memory().unwrap();
    let upsert = |version| {
        s.with_write(|c| {
            information::upsert_review_alert(
                c,
                "security:SSE:600519",
                "em_stock_ann:notice",
                version,
                "重要公告",
                "notice",
                "2026-10-02T00:00:00Z",
            )
        })
        .unwrap()
    };
    upsert(1);
    let claim = || {
        serde_json::from_str::<serde_json::Value>(
            &fund_module::api::home_api::api_notification_claim(&s).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(claim()["informationIds"], serde_json::json!([]));
    s.with_write(|c|c.execute("INSERT INTO app_meta(key,value) VALUES('home_notifications_enabled','true') ON CONFLICT(key) DO UPDATE SET value='true'",[])).unwrap();
    let ids = claim();
    let id = ids["informationIds"][0].as_i64().unwrap();
    assert_eq!(claim()["informationIds"], serde_json::json!([]));
    let result = serde_json::json!({"kind":"information","id":id,"result":"failed"}).to_string();
    fund_module::api::home_api::api_notification_result(&s, &result).unwrap();
    assert_eq!(
        fund_module::api::home_api::api_notification_result(&s, &result)
            .unwrap_err()
            .0,
        409
    );
    information_api::api_information_alert_action(
        &s,
        &serde_json::json!({"id":id,"action":"resolve"}).to_string(),
    )
    .unwrap();
    upsert(1);
    assert_eq!(claim()["informationIds"], serde_json::json!([]));
    upsert(2);
    assert_eq!(claim()["informationIds"], serde_json::json!([id]));
}

#[test]
fn changed_information_metadata_updates_rank_and_invalidates_cached_text_preserving_flags() {
    let s = Store::open_in_memory().unwrap();
    let id = seed_item(&s);
    s.with_write(|c| {
        store::save_content(c, id, "old announcement text", "full", "now")?;
        store::set_flags(c, id, Some(true), Some(true), "now")
    })
    .unwrap();
    let item = information::adapters::RawItem {
        source_id: information::adapters::SRC_STOCK_ANN,
        external_id: "AN2026001".into(),
        title: "贵州茅台:关于回购股份的公告".into(),
        url: "https://data.eastmoney.com/notices/detail/600519/AN2026001.html".into(),
        summary: String::new(),
        codes: vec!["600519".into()],
        published_at: Some("2026-09-29T07:13:39Z".into()),
        precision: information::normalize::TimePrecision::Second,
        importance: 3,
        event_types: vec!["regulatory"],
        event_date: None,
    };
    s.with_write(|c| store::upsert_items(c, &[item], &|_| vec![], "now"))
        .unwrap();
    let detail = s.with_read(|c| store::item_detail(c, id)).unwrap().unwrap();
    assert!(detail.row.read_status && detail.row.favorite);
    assert_eq!(detail.row.importance, 3);
    assert_eq!(detail.row.event_type, "regulatory");
    assert!(detail.content_text.is_none());
    assert_eq!(detail.row.completeness, "link_only");
    assert!(detail.row.content_version > 1);
}

#[test]
fn related_reports_keep_separate_sources_and_exclude_wrong_asset_or_time() {
    let s = Store::open_in_memory().unwrap();
    let id = seed_item(&s);
    s.with_write(|c| -> rusqlite::Result<()> {
        for (external, code, date) in [
            ("match", "600519", "2026-09-29T00:00:00Z"),
            ("wrong-asset", "000001", "2026-09-29T00:00:00Z"),
            ("old", "600519", "2026-08-01T00:00:00Z"),
        ] {
            let item = information::adapters::RawItem {
                source_id: information::adapters::SRC_STOCK_NEWS,
                external_id: external.into(),
                title: "Opposing report".into(),
                url: format!("https://finance.eastmoney.com/a/{external}.html"),
                summary: "Independent statement".into(),
                codes: vec![code.into()],
                published_at: Some(date.into()),
                precision: information::normalize::TimePrecision::Second,
                importance: 1,
                event_types: vec!["repurchase"],
                event_date: None,
            };
            store::upsert_items(
                c,
                &[item],
                &|i| {
                    i.codes
                        .iter()
                        .map(|code| {
                            (
                                information::instrument_ref_for_code(code),
                                "direct",
                                "identity".into(),
                            )
                        })
                        .collect()
                },
                "now",
            )?;
        }
        Ok(())
    })
    .unwrap();
    let data: serde_json::Value = serde_json::from_str(
        &information_api::api_information_item(&s, &format!("/api/information/item?id={id}"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(data["relatedItems"].as_array().unwrap().len(), 1);
    assert_eq!(data["relatedItems"][0]["relation"], "possible");
    assert_eq!(data["title"], "贵州茅台:关于回购股份的公告");
    assert_eq!(
        s.with_read(
            |c| c.query_row("SELECT count(*) FROM information_items", [], |r| r
                .get::<_, i64>(0))
        )
        .unwrap(),
        4
    );
}

#[test]
fn information_group_filter_uses_real_bindings_and_empty_group_is_empty() {
    let s = Store::open_in_memory().unwrap();
    seed_item(&s);
    let today: String = s
        .with_read(|c| c.query_row("SELECT date('now','+8 hours')", [], |r| r.get(0)))
        .unwrap();
    let fund = fund_module::ledger::record_opening(
        &s,
        &fund_module::ledger::OpeningInput {
            account_name: "Group account".into(),
            fund_code: "stock:SSE:600519".into(),
            fund_name: "Stock".into(),
            quantity: fund_module::fixed::Fixed::parse("100", 4).unwrap(),
            as_of: today.clone(),
            total_cost: None,
        },
        &today,
    )
    .unwrap();
    s.with_write(|c|->rusqlite::Result<()> {
        c.execute("INSERT INTO group_plan(id,created_at,updated_at) VALUES(1,'now','now')",[])?;
        for name in ["bound","empty"]{c.execute("INSERT INTO group_plan_groups(group_id,name,name_norm,ratio_tenths,created_at,updated_at) VALUES(?1,?1,?1,100,'now','now')",[name])?;}
        c.execute("INSERT INTO group_plan_bindings(group_id,source_kind,account_id,fund_id,created_at) SELECT 'bound','fund',account_id,fund_id,'now' FROM positions WHERE fund_id=?1",[fund])?;Ok(())
    }).unwrap();
    let list = |group| {
        serde_json::from_str::<serde_json::Value>(
            &information_api::api_information_items(
                &s,
                &format!("/api/information/items?scope=all&group={group}"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    assert_eq!(list("bound")["items"].as_array().unwrap().len(), 1);
    assert_eq!(list("empty")["items"].as_array().unwrap().len(), 0);
    assert_eq!(
        information_api::api_information_items(&s, "/api/information/items?scope=all&group=gone")
            .unwrap_err()
            .0,
        400
    );
}
