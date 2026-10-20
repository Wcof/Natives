//! Cache eviction retains article identities, bookmarks, alerts and published content versions.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
pub(crate) fn revision(conn: &Connection) -> rusqlite::Result<String> {
    Ok(conn
        .query_row(
            "SELECT value FROM app_meta WHERE key='information_cache_revision'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "0".into()))
}
pub(crate) fn stats(store: &Store) -> Result<Value, FinanceError> {
    store.with_snapshot(|c|{
    let (items,bytes):(i64,i64)=c.query_row("SELECT COUNT(*),COALESCE(SUM(length(CAST(content_text AS BLOB))),0) FROM information_items WHERE content_text IS NOT NULL",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    Ok(json!({"items":items,"bytes":bytes,"dataRevision":Store::data_revision(c)?,"cacheRevision":revision(c)?}))
})
}
pub(crate) fn clear(store: &Store, input: &Value) -> Result<Value, FinanceError> {
    if !input.as_object().is_some_and(|o| {
        o.len() == 2 && o.contains_key("expectedRevision") && o.contains_key("requestId")
    }) {
        return Err(FinanceError::Invalid("缓存清理字段无效".into()));
    }
    crate::config_receipts::write(store, "information-cache", input, |c| {
        if input["expectedRevision"].as_str() != Some(Store::data_revision(c)?.as_str()) {
            return Err(FinanceError::Conflict("缓存状态已变化，请刷新".into()));
        }
        let next = revision(c)?
            .parse::<u64>()
            .ok()
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| FinanceError::Invalid("缓存版本无效".into()))?;
        let rows=c.execute("UPDATE information_items SET content_text=NULL,content_fetched_at=NULL,completeness='link_only' WHERE content_text IS NOT NULL",[])?;
        c.execute("INSERT INTO app_meta(key,value) VALUES('information_cache_revision',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![next.to_string()])?;
        Ok(json!({"ok":true,"cleared":rows}))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eviction_keeps_favorites_and_content_identity_and_rejects_stale() {
        let store = Store::open_in_memory().unwrap();
        store.with_write(|c|c.execute("INSERT INTO information_items(source_id,external_id,title,url,content_text,completeness,content_version,favorite,content_hash,retrieved_at,created_at,updated_at) VALUES('em_stock_ann','x','title','https://example.com','body','full',3,1,'hash','now','now','now')",[])).unwrap();
        let before = stats(&store).unwrap();
        let input = json!({"expectedRevision":before["dataRevision"],"requestId":"clear-cache"});
        assert_eq!(clear(&store, &input).unwrap()["cleared"], 1);
        assert_eq!(stats(&store).unwrap()["items"], 0);
        assert_eq!(
            store
                .with_read(|c| c.query_row(
                    "SELECT favorite,content_version FROM information_items",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
                ))
                .unwrap(),
            (1, 3)
        );
        let mut stale = input.clone();
        stale["requestId"] = json!("stale-cache");
        assert!(clear(&store, &stale).is_err());
    }
}
