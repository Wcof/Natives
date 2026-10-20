//! 自选领域操作回归（mode/T04 审计更正）：受控时间注入、行错误传播、
//! 领域操作与 HTTP 适配行为一致（A04 的受控创建时间证据）。

use super::*;
use crate::storage::Store;
use serde_json::json;

#[test]
fn bulk_moves_are_atomic_bounded_and_preserve_metadata() {
    let store = Store::open_in_memory().unwrap();
    let current = || serde_json::from_str::<Value>(&list(&store).unwrap()).unwrap();
    for id in ["a", "b", "c"] {
        create(
            &store,
            &WatchlistDraft {
                id: id.into(),
                name: id.into(),
                sort_order: 0,
            },
            "2026-10-10T00:00:00Z",
        )
        .unwrap();
    }
    for (id, symbol) in [("a", "sh600519"), ("b", "jj000001")] {
        let draft = WatchlistItemDraft::from_raw(&json!({"watchlistId":id,"symbol":symbol,"name":symbol,"assetType":if symbol.starts_with("jj") {"fund"} else {"stock"},"sortOrder":7})).unwrap();
        add_item(&store, &draft, "2026-10-10T00:00:00Z").unwrap();
    }
    let before = current();
    let items =
        json!([{"watchlistId":"a","symbol":"sh600519"},{"watchlistId":"b","symbol":"jj000001"}]);
    for bad in [
        json!([]),
        json!("invalid"),
        json!([{"watchlistId":"a","symbol":"sh600519"},{"watchlistId":"b","symbol":"sh600519"}]),
        json!([{"symbol":"sh600519"}]),
        json!([{"watchlistId":"a","symbol":"sh600519","extra":true}]),
        json!(vec![json!({"watchlistId":"a","symbol":"sh600519"}); 201]),
    ] {
        assert!(manage(
            &store,
            &json!({"op":"move","target":"c","items":bad,"expectedDigest":before["digest"]})
        )
        .is_err());
        assert_eq!(current(), before);
    }
    assert!(manage(&store,&json!({"op":"move","target":"c","items":items,"expectedDigest":before["digest"],"extra":true})).is_err());
    assert_eq!(current(), before);
    // The first update must roll back when the second source is missing or conflicts.
    for target in ["b", "missing"] {
        assert!(manage(
            &store,
            &json!({"op":"move","target":target,"items":items,"expectedDigest":before["digest"]})
        )
        .is_err());
        assert_eq!(current(), before);
    }
    let missing = json!([{"watchlistId":"a","symbol":"sh600519"},{"watchlistId":"missing","symbol":"jj000001"}]);
    assert!(manage(
        &store,
        &json!({"op":"move","target":"c","items":missing,"expectedDigest":before["digest"]})
    )
    .is_err());
    assert_eq!(current(), before);
    assert!(manage(
        &store,
        &json!({"op":"move","target":"c","items":items,"expectedDigest":"0".repeat(64)})
    )
    .is_err());
    assert_eq!(current(), before);
    manage(
        &store,
        &json!({"op":"move","target":"c","items":items,"expectedDigest":before["digest"]}),
    )
    .unwrap();
    let after = current();
    for row in after["items"].as_array().unwrap() {
        assert_eq!(row["watchlistId"], "c");
        let original = before["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|it| it["symbol"] == row["symbol"])
            .unwrap();
        for key in ["name", "assetType", "sortOrder", "createdAt"] {
            assert_eq!(row[key], original[key]);
        }
    }
}

#[test]
fn controlled_clock_creations_store_their_real_times() {
    // A04：两次不同受控时间创建 → 各自真实对应时间；upsert 不覆写 created_at。
    let store = Store::open_in_memory().unwrap();
    let draft = WatchlistDraft {
        id: "g1".into(),
        name: "科技".into(),
        sort_order: 1,
    };
    create(&store, &draft, "2026-09-17T00:00:00Z").unwrap();
    create(&store, &draft, "2027-01-02T00:00:00Z").unwrap(); // upsert 同 id
    let item = WatchlistItemDraft {
        watchlist_id: "g1".into(),
        symbol: "sh600519".into(),
        name: "贵州茅台".into(),
        asset_type: "stock".into(),
        sort_order: 0,
    };
    add_item(&store, &item, "2026-09-17T00:00:00Z").unwrap();
    let out = list(&store).unwrap();
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    let groups = value["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "upsert 不新建组");
    // upsert 更新 name/sort_order，但 created_at 保留首次写入的受控时间。
    assert_eq!(groups[0]["createdAt"], "2026-09-17T00:00:00Z");
    let items = value["items"].as_array().unwrap();
    assert_eq!(items[0]["createdAt"], "2026-09-17T00:00:00Z");
    // 第二个不同时间的新组保存自己的真实时间。
    let other = WatchlistDraft {
        id: "g2".into(),
        name: "消费".into(),
        sort_order: 2,
    };
    create(&store, &other, "2027-01-02T00:00:00Z").unwrap();
    let value: serde_json::Value = serde_json::from_str(&list(&store).unwrap()).unwrap();
    let by_id = |id: &str| {
        value["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(by_id("g1")["createdAt"], "2026-09-17T00:00:00Z");
    assert_eq!(by_id("g2")["createdAt"], "2027-01-02T00:00:00Z");
}

#[test]
fn production_clock_is_real_and_not_frozen() {
    // A04：生产默认时钟（now_rfc3339）产出真实当前时间（2026 年之后），
    // 不再是固定日期；与受控注入正交。
    let now = now_rfc3339();
    assert!(
        now.as_str() > "2026-09-17T00:00:00Z",
        "真实时钟应晚于旧固定日期: {now}"
    );
    assert_eq!(now.len(), 20);
    assert!(now.ends_with('Z'));
}

#[test]
fn formatter_maps_controlled_instants_exactly() {
    assert_eq!(rfc3339_utc_from_secs(0), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339_utc_from_secs(1_758_067_200), "2025-09-17T00:00:00Z");
    assert_eq!(rfc3339_utc_from_secs(1_798_848_000), "2027-01-02T00:00:00Z");
}

#[test]
fn domain_rejects_invalid_input_without_touching_store() {
    let store = Store::open_in_memory().unwrap();
    for raw in [
        json!({"id": "", "name": "n"}),
        json!({"id": "a", "name": 9}),
        json!({"id": "a", "sortOrder": "high"}),
    ] {
        assert!(matches!(
            WatchlistDraft::from_raw(&raw),
            Err(WatchlistError::Invalid(_))
        ));
    }
    for raw in [
        json!({"watchlistId": "a", "symbol": ""}),
        json!({"watchlistId": "a", "symbol": 42}),
        json!({"watchlistId": "a", "symbol": "sh600519", "assetType": 1}),
    ] {
        assert!(matches!(
            WatchlistItemDraft::from_raw(&raw),
            Err(WatchlistError::Invalid(_))
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&list(&store).unwrap()).unwrap();
    assert!(value["groups"].as_array().unwrap().is_empty());
    assert!(value["items"].as_array().unwrap().is_empty());
}

#[test]
fn corrupt_row_propagates_through_domain_list() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO watchlists(id, name, sort_order, created_at) VALUES('bad','坏行','abc','2026-01-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(list(&store), Err(WatchlistError::Db)));
}

#[test]
fn remove_item_is_idempotent_and_normalizes_symbol() {
    let store = Store::open_in_memory().unwrap();
    let item = WatchlistItemDraft {
        watchlist_id: "g1".into(),
        symbol: "sh600519".into(),
        name: "贵州茅台".into(),
        asset_type: "stock".into(),
        sort_order: 0,
    };
    add_item(&store, &item, "2026-01-01T00:00:00Z").unwrap();
    remove_item(&store, "g1", "sh600519").unwrap();
    remove_item(&store, "g1", "sh600519").unwrap();
    let value: serde_json::Value = serde_json::from_str(&list(&store).unwrap()).unwrap();
    assert!(value["items"].as_array().unwrap().is_empty());
}

#[test]
fn management_is_atomic_scoped_and_conflict_checked() {
    let store = Store::open_in_memory().unwrap();
    for id in ["default", "first", "second"] {
        create(
            &store,
            &WatchlistDraft {
                id: id.into(),
                name: id.into(),
                sort_order: 0,
            },
            "2026-10-05T00:00:00Z",
        )
        .unwrap();
    }
    add_item(
        &store,
        &WatchlistItemDraft {
            watchlist_id: "first".into(),
            symbol: "sz000001".into(),
            name: "Sample".into(),
            asset_type: "stock".into(),
            sort_order: 0,
        },
        "2026-10-05T00:00:00Z",
    )
    .unwrap();
    let current = || serde_json::from_str::<Value>(&list(&store).unwrap()).unwrap();
    let original = current();
    manage(
        &store,
        &json!({"op":"rename","id":"first","name":"Renamed","expectedDigest":original["digest"]}),
    )
    .unwrap();
    assert!(matches!(
        manage(
            &store,
            &json!({"op":"delete","id":"first","expectedDigest":original["digest"]})
        ),
        Err(WatchlistError::Conflict)
    ));
    manage(&store,&json!({"op":"move","id":"first","target":"second","symbol":"sz000001","expectedDigest":current()["digest"]})).unwrap();
    assert_eq!(current()["items"][0]["watchlistId"], "second");
    let before = current();
    assert!(manage(&store,&json!({"op":"move","id":"second","target":"missing","symbol":"sz000001","expectedDigest":before["digest"]})).is_err());
    assert_eq!(current(), before);
    manage(&store,&json!({"op":"orderItem","id":"second","symbol":"sz000001","sortOrder":7,"expectedDigest":current()["digest"]})).unwrap();
    assert_eq!(current()["items"][0]["sortOrder"], 7);
    manage(&store,&json!({"op":"orderGroup","id":"second","sortOrder":9,"expectedDigest":current()["digest"]})).unwrap();
    assert!(manage(
        &store,
        &json!({"op":"delete","id":"default","expectedDigest":current()["digest"]})
    )
    .is_err());
    manage(
        &store,
        &json!({"op":"delete","id":"second","expectedDigest":current()["digest"]}),
    )
    .unwrap();
    assert!(current()["items"].as_array().unwrap().is_empty());
    assert_eq!(
        store
            .with_read(
                |conn| conn.query_row("SELECT COUNT(*) FROM transactions", [], |row| row
                    .get::<_, i64>(0))
            )
            .unwrap(),
        0
    );
}

#[test]
fn selected_moves_use_one_target_merge_and_consolidate_all_atomically() {
    let store = Store::open_in_memory().unwrap();
    let current = || serde_json::from_str::<Value>(&list(&store).unwrap()).unwrap();
    for id in ["a", "b", "c"] {
        create(
            &store,
            &WatchlistDraft {
                id: id.into(),
                name: id.into(),
                sort_order: 0,
            },
            "2026-10-10T00:00:00Z",
        )
        .unwrap();
    }
    for (group, symbol, name) in [
        ("a", "sh600519", "stock a"),
        ("b", "sh600519", "stock b"),
        ("c", "sh600519", "target metadata"),
        ("a", "jj000004", "fund a"),
        ("b", "jj000004", "fund b"),
    ] {
        add_item(&store,&WatchlistItemDraft::from_raw(&json!({"watchlistId":group,"symbol":symbol,"name":name,"assetType":if symbol.starts_with("jj"){"fund"}else{"stock"},"sortOrder":7})).unwrap(),"2026-10-10T00:00:00Z").unwrap();
    }
    let before = current();
    for symbols in [
        json!([]),
        json!("bad"),
        json!([123]),
        json!(["600519", "sh600519"]),
        json!(vec!["sh600519"; 201]),
        json!(["sh600519", "jj999999"]),
    ] {
        assert!(manage(&store,&json!({"op":"moveSelected","source":"","target":"c","symbols":symbols,"expectedDigest":before["digest"]})).is_err());
        assert_eq!(current(), before);
    }
    for source in [json!(null), json!(5), json!("missing")] {
        assert!(manage(&store,&json!({"op":"moveSelected","source":source,"target":"c","symbols":["sh600519"],"expectedDigest":before["digest"]})).is_err());
        assert_eq!(current(), before);
    }
    assert!(manage(&store,&json!({"op":"moveSelected","source":"a","target":"a","symbols":["sh600519"],"expectedDigest":before["digest"]})).is_err());
    assert!(manage(&store,&json!({"op":"moveSelected","source":"","target":"missing","symbols":["sh600519"],"expectedDigest":before["digest"]})).is_err());
    assert!(manage(&store,&json!({"op":"moveSelected","source":"","target":"c","symbols":["sh600519"],"expectedDigest":"0".repeat(64)})).is_err());
    assert!(manage(&store,&json!({"op":"moveSelected","source":"","target":"c","symbols":["sh600519"],"expectedDigest":before["digest"],"extra":true})).is_err());
    assert_eq!(current(), before);
    manage(&store,&json!({"op":"moveSelected","source":"a","target":"c","symbols":["sh600519","jj000004"],"expectedDigest":before["digest"]})).unwrap();
    let grouped = current();
    assert_eq!(grouped["items"].as_array().unwrap().len(), 4);
    assert!(grouped["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["watchlistId"] != "a"));
    manage(&store,&json!({"op":"moveSelected","source":"","target":"c","symbols":["sh600519","jj000004"],"expectedDigest":grouped["digest"]})).unwrap();
    let after = current();
    assert_eq!(after["items"].as_array().unwrap().len(), 2);
    assert!(after["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["watchlistId"] == "c"));
    let stock = after["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["symbol"] == "sh600519")
        .unwrap();
    assert_eq!(stock["name"], "target metadata");
    assert_eq!(stock["sortOrder"], 7);
    assert_eq!(stock["assetType"], "stock");
    assert_eq!(stock["createdAt"], "2026-10-10T00:00:00Z");
    let unchanged = current();
    manage(&store,&json!({"op":"moveSelected","source":"","target":"c","symbols":["sh600519","jj000004"],"expectedDigest":unchanged["digest"]})).unwrap();
    assert_eq!(current(), unchanged);
    assert_eq!(
        store
            .with_read(
                |c| c.query_row("SELECT COUNT(*) FROM transactions", [], |r| r
                    .get::<_, i64>(0))
            )
            .unwrap(),
        0
    );
}
