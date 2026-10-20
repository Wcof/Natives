use super::*;
fn template() -> Value {
    json!({"formatVersion":1,"settings":{"colorMode":"cn","linkPlatform":"xueqiu"},"accounts":[{"name":"Template account","sortOrder":0}],"strategies":[{"code":"trend_follow","enabled":true,"weightPpm":1200000}],"rules":[{"name":"Review","ruleType":"daily_change","objectKey":"sh600519","params":{"threshold":5,"direction":"absolute"},"period":"day"}],"sources":[{"sourceId":"em_stock_news","enabled":false}],"channels":[{"name":"Template channel","kind":"wecom","config":{}}],"watchlists":[{"name":"Template group","items":[{"symbol":"sh600519","name":"Maotai","assetType":"stock"}]}]})
}
#[test]
fn import_is_preview_bound_atomic_idempotent_and_never_activates_external_work() {
    let store = Store::open_in_memory().unwrap();
    let input = template();
    let before = store.with_read(Store::data_revision).unwrap();
    let p = preview(&store, &input).unwrap();
    assert_eq!(store.with_read(Store::data_revision).unwrap(), before);
    assert_eq!(
        store
            .with_read(|c| c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0)))
            .unwrap(),
        0
    );
    let command = json!({"template":input,"expectedRevision":p["expectedRevision"],"previewToken":p["previewToken"],"requestId":"template-one"});
    assert_eq!(commit(&store, &command).unwrap()["ok"], true);
    assert_eq!(commit(&store, &command).unwrap()["ok"], true);
    assert_eq!(store.with_read(|c|c.query_row("SELECT COUNT(*) FROM notify_channels WHERE enabled=0 AND is_default=0 AND secret_refs='{}'",[],|r|r.get::<_,i64>(0))).unwrap(),1);
    assert_eq!(
        store
            .with_read(|c| c.query_row(
                "SELECT COUNT(*) FROM watch_rules WHERE enabled=0",
                [],
                |r| r.get::<_, i64>(0)
            ))
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .with_read(
                |c| c.query_row("SELECT COUNT(*) FROM transactions", [], |r| r
                    .get::<_, i64>(0))
            )
            .unwrap(),
        0
    );
    let exported = export(&store).unwrap();
    assert!(!exported.to_string().contains("secret_refs"));
    assert_eq!(exported["watchlists"][0]["items"][0]["symbol"], "sh600519");
    let mut changed = command.clone();
    changed["requestId"] = json!("template-stale");
    assert!(matches!(
        commit(&store, &changed),
        Err(FinanceError::Conflict(_))
    ));
}
#[test]
fn rejects_secrets_invalid_protocols_unknown_fields_and_large_inputs() {
    let store = Store::open_in_memory().unwrap();
    for change in [
        json!({"settings":{"colorMode":"cn","linkPlatform":"xueqiu","token":"SECRET"}}),
        json!({"channels":[{"name":"evil","kind":"bark","config":{"server_url":"https://127.0.0.1/"}}]}),
        json!({"strategies":[{"code":"unknown","enabled":true,"weightPpm":1200000}]}),
        json!({"formatVersion":2}),
        json!({"privateKey":"SECRET"}),
    ] {
        let mut input = template();
        input
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(preview(&store, &input).is_err());
    }
    let mut input = template();
    input["accounts"] = json!([{"name":"x".repeat(MAX_BYTES),"sortOrder":0}]);
    assert!(preview(&store, &input).is_err());
}

#[test]
fn selected_modules_do_not_overwrite_display_or_duplicate_accounts() {
    let store = Store::open_in_memory().unwrap();
    let mut input = template();
    input["settings"] = Value::Null;
    input["strategies"] = json!([]);
    input["rules"] = json!([]);
    input["sources"] = json!([]);
    input["channels"] = json!([]);
    input["watchlists"] = json!([]);
    input["modules"] = json!(["accounts"]);
    let before = crate::settings::get(&store).unwrap();
    let p = preview(&store, &input).unwrap();
    commit(&store,&json!({"template":p["template"],"expectedRevision":p["expectedRevision"],"previewToken":p["previewToken"],"requestId":"accounts-only"})).unwrap();
    assert_eq!(before, crate::settings::get(&store).unwrap());
    assert_eq!(
        export(&store).unwrap()["accounts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    input["modules"] = json!(["channels"]);
    assert!(preview(&store, &input).is_err());
}
