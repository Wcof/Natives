//! mode/T02 回归：自选／图形创建时间真实化、行读取错误传播、输入校验。
//! A04（真实时间）、A05（损坏行明确失败、空库成功）、A06/A07（稳定脱敏
//! code、非法输入无副作用、正常 CRUD 保持）。

use super::*;
use crate::api::drawings::{api_drawings_list, api_save_drawing};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;
use serde_json::{json, Value};

fn list_json(store: &Store) -> Value {
    serde_json::from_str(&api_watchlists(store).unwrap()).unwrap()
}

#[test]
fn rfc3339_utc_formatter_maps_controlled_times() {
    // A04：受控时间 → 真实对应时间；不同时刻结果不同（纯函数部分）。
    assert_eq!(rfc3339_utc_from_secs(0), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339_utc_from_secs(1_758_067_200), "2025-09-17T00:00:00Z");
    assert_eq!(rfc3339_utc_from_secs(1_758_153_600), "2025-09-18T00:00:00Z");
    // 不再出现旧实现固定写入的 2026-09-17：任何真实时钟都晚于它
    let store = Store::open_in_memory().unwrap();
    api_create_watchlist(&store, r#"{"id":"a","name":"甲"}"#).unwrap();
    let groups = list_json(&store)["groups"].as_array().unwrap().clone();
    let created = groups[0]["createdAt"].as_str().unwrap();
    assert_eq!(created.len(), 20, "RFC3339 Z 口径格式保持: {created}");
    assert!(created.ends_with('Z'));
    assert!(
        created > "2026-09-17T00:00:00Z",
        "创建时间必须真实: {created}"
    );
}

#[test]
fn empty_store_returns_success_with_empty_collections() {
    // A05：合法空库成功返回空集合，不报错也不伪造数据。
    let store = Store::open_in_memory().unwrap();
    let out = list_json(&store);
    assert!(out["groups"].as_array().unwrap().is_empty());
    assert!(out["items"].as_array().unwrap().is_empty());
    let request = HttpRequest::for_internal(
        "GET",
        "/api/drawings?symbol=sh600519&period=day",
        Vec::new(),
    );
    let drawings = api_drawings_list(&store, &request).unwrap();
    assert_eq!(drawings, "{\"drawings\":[]}");
}

fn insert_corrupt_watchlist_row(store: &Store, sql: &str) {
    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(sql, [])?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn corrupt_watchlist_row_fails_instead_of_partial_success() {
    // A05：行解码错误（sort_order 混入 TEXT）必须整体失败，不能静默丢行。
    let store = Store::open_in_memory().unwrap();
    api_create_watchlist(&store, r#"{"id":"ok","name":"正常"}"#).unwrap();
    insert_corrupt_watchlist_row(
        &store,
        "INSERT INTO watchlists(id, name, sort_order, created_at) VALUES('bad','坏行','abc','2026-01-01T00:00:00Z')",
    );
    let err = api_watchlists(&store).unwrap_err();
    assert_eq!(err.0, 500);
    assert_eq!(
        err.1,
        "{\"error\":\"WATCHLIST_DB\",\"message\":\"watchlist storage read failed\"}"
    );
    // items 行损坏同样失败
    let store2 = Store::open_in_memory().unwrap();
    insert_corrupt_watchlist_row(
        &store2,
        "INSERT INTO watchlist_items(watchlist_id, symbol, name, asset_type, sort_order, created_at) VALUES('d','sh600519','n','stock','xyz','2026-01-01T00:00:00Z')",
    );
    let err = api_watchlists(&store2).unwrap_err();
    assert_eq!(err.0, 500);
    assert!(!err.1.contains("SELECT"), "错误文案不得包含 SQL: {}", err.1);
}

#[test]
fn corrupt_drawing_row_fails_instead_of_partial_success() {
    // A05：图形行 points_json 为非法 UTF-8 BLOB → 读失败而不是丢行成功。
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO chart_drawings(id, symbol, period, tool_type, points_json, options_json, created_at, updated_at)
                 VALUES('d1','sh600519','day','trendline',?1,'{}','1','1')",
                rusqlite::params![vec![0xFFu8, 0xFEu8]],
            )?;
            Ok(())
        })
        .unwrap();
    let request = HttpRequest::for_internal(
        "GET",
        "/api/drawings?symbol=sh600519&period=day",
        Vec::new(),
    );
    let err = api_drawings_list(&store, &request).unwrap_err();
    assert_eq!(err.0, 500);
    assert!(err.1.starts_with("{\"error\":\"DRAWINGS_DB\""));
    assert!(!err.1.to_lowercase().contains("select") && !err.1.contains("rusqlite"));
}

#[test]
fn invalid_inputs_are_rejected_without_side_effects() {
    // A07：无效身份／长度／类型拒绝且无副作用（不写入 default 组或脏行）。
    let store = Store::open_in_memory().unwrap();
    let long_id = "x".repeat(65);
    let long_symbol = "s".repeat(49);
    let watchlist_bodies = vec![
        json!({"id": 123, "name": "n"}),         // id 类型错误
        json!({"id": "", "name": "n"}),          // 空 id
        json!({"id": long_id, "name": "n"}),     // 超长 id
        json!({"id": "a", "name": ""}),          // 空 name
        json!({"id": "a", "name": 9}),           // name 类型错误
        json!({"id": "a", "sortOrder": "high"}), // sort_order 类型
    ];
    for body in &watchlist_bodies {
        let err = api_create_watchlist(&store, &body.to_string()).unwrap_err();
        assert_eq!(err.0, 400, "应 400: {body}");
    }
    let item_bodies = vec![
        json!({"watchlistId": "a", "symbol": ""}), // 空 symbol
        json!({"watchlistId": "a", "symbol": long_symbol}), // 超长 symbol
        json!({"watchlistId": "a", "symbol": "sh600519", "assetType": 1}), // assetType 类型
        json!({"watchlistId": "a", "symbol": "sh600519", "name": {}}), // name 类型
        json!({"watchlistId": "a", "symbol": 42}), // symbol 类型
    ];
    for body in &item_bodies {
        let err = api_add_watchlist_item(&store, &body.to_string()).unwrap_err();
        assert_eq!(err.0, 400, "应 400: {body}");
    }
    let out = list_json(&store);
    assert!(
        out["groups"].as_array().unwrap().is_empty(),
        "拒绝后不得创建组"
    );
    assert!(
        out["items"].as_array().unwrap().is_empty(),
        "拒绝后不得写入条目"
    );
}

#[test]
fn drawing_save_rejects_unknown_period_tool_and_shape() {
    // A07：period／toolType 白名单与 points 形状校验，非法 400 无副作用。
    let store = Store::open_in_memory().unwrap();
    let base = |extra: Value| -> String {
        let mut obj = extra;
        let map = obj.as_object_mut().unwrap();
        map.insert("id".into(), json!("d1"));
        map.insert("symbol".into(), json!("sh600519"));
        obj.to_string()
    };
    let long_id = "x".repeat(65);
    let cases = vec![
        base(json!({"period": "hour"})),     // 不支持的周期
        base(json!({"toolType": "magnet"})), // 不支持的工具
        base(json!({"tool": "cursor"})),     // 选择模式不是图形
        base(json!({"points": {"x": 1}})),   // points 必须是数组
        base(json!({"points": "[]"})),       // 字符串不是数组
        base(json!({"options": [1]})),       // options 必须是对象
    ];
    for body in &cases {
        let err = api_save_drawing(&store, body).unwrap_err();
        assert_eq!(err.0, 400, "应 400: {body}");
    }
    // 超长 id（不经 base，避免默认 id 覆盖）
    let overlong = json!({"id": long_id, "symbol": "sh600519"}).to_string();
    let err = api_save_drawing(&store, &overlong).unwrap_err();
    assert_eq!(err.0, 400);
    let request = HttpRequest::for_internal(
        "GET",
        "/api/drawings?symbol=sh600519&period=day",
        Vec::new(),
    );
    assert_eq!(
        api_drawings_list(&store, &request).unwrap(),
        "{\"drawings\":[]}",
        "拒绝后无残留"
    );
}

#[test]
fn normal_crud_fields_and_upsert_semantics_are_preserved() {
    // A07：正常 CRUD 字段、排序、删除／重复行为保持；created_at 不被覆写。
    let store = Store::open_in_memory().unwrap();
    api_create_watchlist(&store, r#"{"id":"g1","name":"科技","sortOrder":2}"#).unwrap();
    api_add_watchlist_item(
        &store,
        r#"{"watchlistId":"g1","symbol":"SH600519","name":"贵州茅台"}"#,
    )
    .unwrap();
    let out = list_json(&store);
    assert_eq!(out["groups"][0]["name"], "科技");
    assert_eq!(out["groups"][0]["sortOrder"], 2);
    let item = &out["items"][0];
    assert_eq!(item["watchlistId"], "g1");
    assert_eq!(item["symbol"], "sh600519", "symbol 归一化保持");
    assert_eq!(item["assetType"], "stock", "缺省 assetType 保持");
    let created_first = item["createdAt"].as_str().unwrap().to_owned();

    // 重复添加同 symbol：created_at 不被重写（历史不改写），sort_order 更新生效
    api_add_watchlist_item(
        &store,
        r#"{"watchlistId":"g1","symbol":"sh600519","name":"茅台","sortOrder":9}"#,
    )
    .unwrap();
    let item = &list_json(&store)["items"][0];
    assert_eq!(item["name"], "茅台");
    assert_eq!(item["sortOrder"], 9);
    assert_eq!(item["createdAt"].as_str().unwrap(), created_first);

    // 删除后为空集合；重复删除仍成功
    let request = HttpRequest::for_internal(
        "DELETE",
        "/api/watchlists/items?watchlistId=g1&symbol=sh600519",
        Vec::new(),
    );
    api_remove_watchlist_item(&store, &request).unwrap();
    api_remove_watchlist_item(&store, &request).unwrap();
    assert!(list_json(&store)["items"].as_array().unwrap().is_empty());
}

#[test]
fn drawing_illegal_field_types_are_rejected_without_side_effects() {
    // mode/T02 更正（A07）：显式非法类型（数字/对象/数组）必须 400 且无副作用；
    // 缺省与显式 null 沿既有契约走默认值并成功保存。
    let store = Store::open_in_memory().unwrap();
    let cases = vec![
        json!({"id": "d1", "symbol": "sh600519", "period": 5}), // period 数字
        json!({"id": "d2", "symbol": "sh600519", "period": {"k": "day"}}), // period 对象
        json!({"id": "d3", "symbol": "sh600519", "toolType": {"tool": "trendline"}}), // toolType 对象
        json!({"id": "d4", "symbol": "sh600519", "tool": ["trendline"]}), // tool 别名数组
    ];
    for body in &cases {
        let err = api_save_drawing(&store, &body.to_string()).unwrap_err();
        assert_eq!(err.0, 400, "非法类型应 400: {body}");
        assert!(
            err.1.contains("must be a string"),
            "应说明类型错误: {}",
            err.1
        );
    }
    let request = HttpRequest::for_internal(
        "GET",
        "/api/drawings?symbol=sh600519&period=day",
        Vec::new(),
    );
    assert_eq!(
        api_drawings_list(&store, &request).unwrap(),
        "{\"drawings\":[]}",
        "非法类型不得写入任何行"
    );
    // 缺省与显式 null 仍走默认（现行契约保持）。
    for id in ["ok1", "ok2"] {
        let body = json!({"id": id, "symbol": "sh600519", "period": null, "toolType": null,
            "points": [{"date": "2026-09-01", "price": 10}, {"date": "2026-09-02", "price": 11}],
            "options": {"coordinateVersion": 2, "viewContext": {"fq": "qfq", "seriesKind": "OHLC"}}});
        api_save_drawing(&store, &body.to_string()).unwrap();
    }
    let list = api_drawings_list(&store, &request).unwrap();
    assert!(
        list.contains("\"toolType\":\"trendline\""),
        "默认 toolType 保持"
    );
    assert_eq!(
        list.matches("\"period\":\"day\"").count(),
        2,
        "默认 period 保持"
    );
}

#[test]
fn busy_database_fails_with_stable_sanitized_code_and_no_half_state() {
    // mode/T02 更正（A06）：真实 busy 竞争——第二个连接持写锁时，本路径在
    // busy_timeout 耗尽后整体失败，返回稳定脱敏 code，不留下半状态。
    let dir = std::env::temp_dir().join(format!("fund-t02-busy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    // 第二连接持 RESERVED 锁（未提交），busy_timeout(50ms) 让本用例快速返回。
    let blocker = rusqlite::Connection::open(dir.join("fund.db")).unwrap();
    blocker
        .busy_timeout(std::time::Duration::from_millis(50))
        .unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE; INSERT INTO watchlists(id,name,sort_order,created_at) VALUES('lock','锁',0,'x');").unwrap();
    let result = api_create_watchlist(&store, r#"{"id":"g1","name":"科技"}"#);
    assert!(result.is_err(), "持锁期间写入应失败");
    let (code, message) = result.unwrap_err();
    assert_eq!(code, 500);
    assert_eq!(
        message, "{\"error\":\"WATCHLIST_DB\",\"message\":\"watchlist storage write failed\"}",
        "busy 必须映射为稳定脱敏 code"
    );
    blocker.execute_batch("ROLLBACK;").unwrap();
    drop(blocker);
    // 锁释放后同一操作成功：失败路径未留下半状态，也未污染后续写入。
    api_create_watchlist(&store, r#"{"id":"g1","name":"科技"}"#).unwrap();
    let groups = list_json(&store)["groups"].as_array().unwrap().clone();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["id"], "g1");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mid_transaction_failure_rolls_back_whole_write_and_revision() {
    // mode/T02 更正（A06）：同一 with_write 事务内第二步失败 → 全部回滚，
    // data_revision 不推进（审计要求本路径证据，不以 Store 通用测试代替）。
    let store = Store::open_in_memory().unwrap();
    api_create_watchlist(&store, r#"{"id":"g0","name":"已有"}"#).unwrap();
    let revision_before: String = store.with_read(|conn| Store::data_revision(conn)).unwrap();
    let result: Result<(), rusqlite::Error> = store.with_write(|conn| {
        conn.execute(
            "INSERT INTO watchlists(id, name, sort_order, created_at) VALUES('g1','第一步',0,'x')",
            [],
        )?;
        conn.execute(
            "INSERT INTO watchlist_items(watchlist_id, symbol, name, asset_type, sort_order, created_at) VALUES('g1',NULL,'第二步','stock',0,'x')",
            [],
        )?;
        Ok(())
    });
    assert!(result.is_err(), "第二步 NOT NULL 约束失败应传播");
    let groups = list_json(&store)["groups"].as_array().unwrap().clone();
    assert_eq!(groups.len(), 1, "第一步写入必须回滚");
    assert_eq!(groups[0]["id"], "g0");
    let revision_after: String = store.with_read(|conn| Store::data_revision(conn)).unwrap();
    assert_eq!(revision_before, revision_after, "失败事务不得推进 revision");
}
