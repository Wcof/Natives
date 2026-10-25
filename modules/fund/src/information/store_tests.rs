use super::*;
use crate::information::normalize::TimePrecision;

fn raw(external_id: &str, title: &str) -> RawItem {
    RawItem {
        source_id: super::super::adapters::SRC_STOCK_ANN,
        external_id: external_id.into(),
        title: title.into(),
        url: "https://example.com/a".into(),
        summary: String::new(),
        codes: vec!["600519".into()],
        published_at: Some("2026-09-28T07:13:39Z".into()),
        precision: TimePrecision::Second,
        importance: 2,
        event_types: vec!["earnings"],
        event_date: None,
    }
}

fn rel(item: &RawItem) -> Vec<(String, &'static str, String)> {
    item.codes
        .iter()
        .map(|c| {
            (
                format!("security:CN:stock:{c}"),
                "direct",
                "announcement codes".to_string(),
            )
        })
        .collect()
}

#[test]
fn public_finance_scope_does_not_impersonate_holdings() {
    let store = crate::storage::Store::open_in_memory().unwrap();
    store
        .with_write::<_, rusqlite::Error>(|c| {
            let mut public = raw("public", "Public policy news");
            public.source_id = super::super::adapters::SRC_FINANCE;
            public.codes.clear();
            let mut routine = raw("holding", "Routine announcement");
            routine.importance = 0;
            routine.published_at = Some("2026-09-28T08:00:00Z".into());
            upsert_items(c, &[public, routine], &rel, "2026-10-14T00:00:00Z")?;
            Ok(())
        })
        .unwrap();
    let read = |scope| {
        let response = crate::api::information_api::api_information_items(
            &store,
            &format!("/api/information/items?scope={scope}"),
        )
        .unwrap();
        serde_json::from_str::<serde_json::Value>(&response).unwrap()
    };
    assert_eq!(read("all")["items"].as_array().unwrap().len(), 2);
    assert_eq!(read("all")["items"][0]["sourceId"], "em_finance");
    assert_eq!(
        read("all&sort=latest")["items"][0]["title"],
        "Routine announcement"
    );
    assert_eq!(read("market")["items"].as_array().unwrap().len(), 1);
    assert_eq!(read("market")["items"][0]["sourceId"], "em_finance");
    assert!(read("holdings")["items"].as_array().unwrap().is_empty());
}

#[test]
fn upsert_is_idempotent_and_preserves_flags() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::migration::SCHEMA_V8_SQL).unwrap();
    conn.execute_batch(crate::migration_late::SCHEMA_V18_SQL)
        .unwrap();
    let now = "2026-10-01T00:00:00Z";
    let items = vec![raw("A1", "公告一")];
    assert_eq!(upsert_items(&conn, &items, &rel, now).unwrap(), 1);
    // 重采同内容：幂等不新增。
    assert_eq!(upsert_items(&conn, &items, &rel, now).unwrap(), 0);
    // 个人状态保留：收藏后重采仍在。
    assert!(set_flags(&conn, 1, Some(true), Some(true), now).unwrap());
    assert_eq!(upsert_items(&conn, &items, &rel, now).unwrap(), 0);
    let rows = list_items(&conn, &[], false, 10, 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].favorite && rows[0].read_status);
    // 内容变化：version 升级。
    let changed = vec![raw("A1", "公告一（更正）")];
    assert_eq!(upsert_items(&conn, &changed, &rel, now).unwrap(), 1);
    let rows = list_items(&conn, &[], false, 10, 0).unwrap();
    assert_eq!(rows[0].content_version, 2);
    assert!(
        rows[0].favorite,
        "content update must not reset personal state"
    );
}

#[test]
fn content_truncation_marks_truncated() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::migration::SCHEMA_V8_SQL).unwrap();
    conn.execute_batch(crate::migration_late::SCHEMA_V18_SQL)
        .unwrap();
    upsert_items(&conn, &[raw("A1", "标题")], &rel, "now").unwrap();
    let big = "中".repeat(MAX_CONTENT_BYTES);
    assert!(save_content(&conn, 1, &big, "full", "now").unwrap());
    let detail = item_detail(&conn, 1).unwrap().unwrap();
    assert_eq!(detail.row.completeness, "truncated");
    let stored = detail.content_text.as_ref().unwrap();
    // 截断落在字符边界（「中」3 字节，边界回退可能少 1～2 字节）。
    assert!(stored.len() <= MAX_CONTENT_BYTES);
    assert!(stored.len() > MAX_CONTENT_BYTES - 3);
    // 空正文 → unavailable（上游确认无正文，不是错误）。
    assert!(save_content(&conn, 1, "", "unavailable", "now").unwrap());
    let detail = item_detail(&conn, 1).unwrap().unwrap();
    assert_eq!(detail.row.completeness, "unavailable");
}

#[test]
fn relation_filtering_by_instrument_ref() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::migration::SCHEMA_V8_SQL).unwrap();
    conn.execute_batch(crate::migration_late::SCHEMA_V18_SQL)
        .unwrap();
    let mut other = raw("A2", "其他公告");
    other.codes = vec!["000001".into()];
    let items = vec![raw("A1", "公告一"), other];
    upsert_items(&conn, &items, &rel, "now").unwrap();
    let rows = list_items(
        &conn,
        &["security:CN:stock:600519".to_string()],
        false,
        10,
        0,
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "公告一");
}

#[test]
fn health_failure_keeps_success_and_prune_preserves_saved_material() {
    let store = crate::storage::Store::open_in_memory().unwrap();
    store
        .with_write::<_, rusqlite::Error>(|c| {
            record_source_result(c, SOURCE_IDS[0], true, 1, 1, None, "2026-01-01T00:00:00Z")?;
            record_source_result(
                c,
                SOURCE_IDS[0],
                false,
                0,
                1,
                Some("blocked"),
                "2026-01-02T00:00:00Z",
            )?;
            let state = source_states(c)?.remove(0);
            assert_eq!(
                state.last_success_at.as_deref(),
                Some("2026-01-01T00:00:00Z")
            );
            assert_eq!(state.last_error_code.as_deref(), Some("blocked"));
            record_source_result(
                c,
                SOURCE_IDS[0],
                true,
                1,
                1,
                Some("network_error"),
                "2026-01-03T00:00:00Z",
            )?;
            assert_eq!(
                source_states(c)?.remove(0).last_error_code.as_deref(),
                Some("network_error")
            );
            upsert_items(
                c,
                &[raw("a", "A"), raw("b", "B")],
                &rel,
                "2020-01-01T00:00:00Z",
            )?;
            c.execute(
                "UPDATE information_items SET favorite=1 WHERE external_id='b'",
                [],
            )?;
            assert!(save_content(c, 1, "body", "full", "now")?);
            let before: i64 = c.query_row(
                "SELECT content_version FROM information_items WHERE id=1",
                [],
                |r| r.get(0),
            )?;
            assert!(save_content(c, 1, "body", "full", "now")?);
            assert_eq!(
                before,
                c.query_row(
                    "SELECT content_version FROM information_items WHERE id=1",
                    [],
                    |r| r.get::<_, i64>(0)
                )?
            );
            assert_eq!(prune(c, 1)?, 1);
            assert!(item_detail(c, 2)?.is_some());
            Ok(())
        })
        .unwrap();
}

#[test]
fn source_state_roundtrip() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::migration::SCHEMA_V8_SQL).unwrap();
    conn.execute_batch(crate::migration_late::SCHEMA_V18_SQL)
        .unwrap();
    assert!(source_states(&conn).unwrap().iter().all(|s| s.enabled));
    assert!(set_source_enabled(&conn, "em_stock_news", false).unwrap());
    assert!(
        !source_states(&conn)
            .unwrap()
            .iter()
            .find(|s| s.source_id == "em_stock_news")
            .unwrap()
            .enabled
    );
    assert!(
        !set_source_enabled(&conn, "unknown_vendor", true).unwrap(),
        "白名单外拒绝"
    );
    record_source_result(
        &conn,
        "em_stock_ann",
        false,
        0,
        120,
        Some("network_error"),
        "now",
    )
    .unwrap();
    let st = source_states(&conn)
        .unwrap()
        .into_iter()
        .find(|s| s.source_id == "em_stock_ann")
        .unwrap();
    assert_eq!(st.last_error_code.as_deref(), Some("network_error"));
}

#[test]
fn filters_apply_before_paging_and_do_not_treat_sql_as_code() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(crate::migration::SCHEMA_V8_SQL).unwrap();
    c.execute_batch(crate::migration_late::SCHEMA_V18_SQL)
        .unwrap();
    let mut old = raw("older", "Needle");
    old.published_at = Some("2026-09-01T00:00:00Z".into());
    let mut recent = raw("recent", "Other");
    recent.published_at = Some("2026-10-01T00:00:00Z".into());
    let mut unknown = raw("unknown", "Needle");
    unknown.published_at = None;
    upsert_items(&c, &[old, recent, unknown], &rel, "now").unwrap();
    let f = ListFilter {
        from: "2026-09-01".into(),
        to: "2026-09-30".into(),
        query: "Needle".into(),
        asset: "security:CN:stock:600519".into(),
        ..Default::default()
    };
    let rows = list_items_filtered(&c, &[], false, 1, 0, &f).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Needle");
    assert!(list_items_filtered(&c, &[], false, 1, 1, &f)
        .unwrap()
        .is_empty());
    let hostile = ListFilter {
        query: "' OR 1=1 --".into(),
        ..Default::default()
    };
    assert!(list_items_filtered(&c, &[], false, 50, 0, &hostile)
        .unwrap()
        .is_empty());
}

#[test]
fn body_cache_age_does_not_follow_flags_or_list_refresh() {
    let store = crate::storage::Store::open_in_memory().unwrap();
    store
        .with_write::<_, rusqlite::Error>(|c| {
            upsert_items(c, &[raw("A1", "title")], &rel, "2026-10-01T00:00:00Z")?;
            save_content(c, 1, "body", "full", "2026-10-01T00:00:00Z")?;
            set_flags(c, 1, Some(true), Some(true), "2026-10-02T00:00:00Z")?;
            upsert_items(c, &[raw("A1", "title")], &rel, "2026-10-02T00:00:00Z")?;
            assert!(content_cache_fresh(c, 1, "2026-10-01T23:59:59Z")?);
            assert!(!content_cache_fresh(c, 1, "2026-10-02T00:00:00Z")?);
            assert!(!content_cache_fresh(c, 1, "2026-09-30T00:00:00Z")?);
            let version = item_detail(c, 1)?.unwrap().row.content_version;
            save_content(c, 1, "body", "full", "2026-10-02T00:00:01Z")?;
            assert!(content_cache_fresh(c, 1, "2026-10-02T00:00:02Z")?);
            assert_eq!(item_detail(c, 1)?.unwrap().row.content_version, version);
            upsert_items(c, &[raw("A1", "corrected")], &rel, "2026-10-02T00:00:03Z")?;
            assert!(!content_cache_fresh(c, 1, "2026-10-02T00:00:04Z")?);
            Ok(())
        })
        .unwrap();
}

#[test]
fn exact_news_duplicates_are_collapsed_after_filters_before_pagination() {
    let store = crate::storage::Store::open_in_memory().unwrap();
    store
        .with_write::<_, rusqlite::Error>(|c| {
            let mut first = raw("first", "Same story");
            first.source_id = super::super::adapters::SRC_STOCK_NEWS;
            first.summary = "Same full summary".into();
            let mut second = first.clone();
            second.external_id = "second".into();
            second.published_at = Some("2026-09-28T08:13:39Z".into());
            let mut third = first.clone();
            third.external_id = "third".into();
            third.title = "Different story".into();
            upsert_items(c, &[first, second, third], &rel, "2026-09-28T09:00:00Z")?;
            let rows = list_items(c, &[], false, 2, 0)?;
            assert_eq!(rows.len(), 2);
            assert_eq!(rows.iter().filter(|r| r.title == "Same story").count(), 1);
            assert!(list_items(c, &[], false, 2, 2)?.is_empty());
            assert_eq!(
                c.query_row("SELECT COUNT(*) FROM information_items", [], |r| r
                    .get::<_, i64>(0))?,
                3
            );
            c.execute(
                "UPDATE information_items SET favorite=1 WHERE external_id='first'",
                [],
            )?;
            assert_eq!(list_items(c, &[], true, 2, 0)?.len(), 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn same_headline_across_news_sources_collapses_but_announcements_stay_separate() {
    let store = crate::storage::Store::open_in_memory().unwrap();
    store
        .with_write::<_, rusqlite::Error>(|c| {
            let mut news = raw("news", "Shared headline");
            news.source_id = super::super::adapters::SRC_STOCK_NEWS;
            news.summary = "Short public summary".into();
            let mut finance = raw("finance", "Shared headline");
            finance.source_id = super::super::adapters::SRC_FINANCE;
            finance.summary = "A longer public story summary with the same headline".into();
            let mut announcement = raw("announcement", "Shared headline");
            announcement.source_id = super::super::adapters::SRC_STOCK_ANN;
            announcement.summary = "Official announcement text".into();
            upsert_items(
                c,
                &[news, finance, announcement],
                &rel,
                "2026-09-28T10:00:00Z",
            )?;
            let rows = list_items(c, &[], false, 10, 0)?;
            assert_eq!(
                rows.len(),
                2,
                "news merge; announcement remains its own story"
            );
            assert_eq!(
                rows.iter()
                    .filter(|r| r.source_id == super::super::adapters::SRC_FINANCE)
                    .count(),
                1,
                "the richer public-news copy is canonical"
            );
            assert_eq!(
                rows.iter()
                    .filter(|r| r.source_id == super::super::adapters::SRC_STOCK_ANN)
                    .count(),
                1
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn duplicate_stories_stay_collapsed_after_read_or_favorite_changes() {
    let store = crate::storage::Store::open_in_memory().unwrap();
    store
        .with_write::<_, rusqlite::Error>(|c| {
            let mut first = raw("first", "Same syndicated story");
            first.source_id = super::super::adapters::SRC_STOCK_NEWS;
            first.summary = "Same full summary".into();
            let mut second = first.clone();
            second.external_id = "second".into();
            second.published_at = Some("2026-09-28T08:30:00Z".into());
            let mut cross_source = first.clone();
            cross_source.source_id = super::super::adapters::SRC_FINANCE;
            cross_source.external_id = "cross".into();
            cross_source.summary = "A richer syndicated summary".into();
            cross_source.published_at = Some("2026-09-28T09:00:00Z".into());
            upsert_items(
                c,
                &[first, second, cross_source],
                &rel,
                "2026-09-28T10:00:00Z",
            )?;
            // Reading one copy must not reveal the hidden duplicates.
            set_flags(c, 1, Some(true), None, "2026-09-28T11:00:00Z")?;
            let rows = list_items(c, &[], false, 10, 0)?;
            assert_eq!(rows.len(), 1);
            assert!(rows[0].read_status);
            // Favoriting another copy keeps the story at one visible row.
            set_flags(c, 2, None, Some(true), "2026-09-28T12:00:00Z")?;
            let rows = list_items(c, &[], false, 10, 0)?;
            assert_eq!(rows.len(), 1);
            assert!(rows[0].favorite);
            // Relation-ranked scope applies the same dedup and keeps rel_rank sortable.
            let refs = vec!["security:CN:stock:600519".to_string()];
            let scope = list_items_filtered(c, &refs, false, 10, 0, &ListFilter::default())?;
            assert_eq!(scope.len(), 1);
            assert_eq!(
                c.query_row("SELECT COUNT(*) FROM information_items", [], |r| r
                    .get::<_, i64>(0))?,
                3
            );
            Ok(())
        })
        .unwrap();
}
