//! 自动资讯持久化（方案 03 §4/§5、04 §4）。
//!
//! - 同 sourceId＋externalId 幂等 upsert；内容 hash 不变只刷新 retrieved_at；
//!   hash 变化 content_version+1 并保留个人状态（read/favorite/archived）。
//! - 关联按稳定 instrument_ref 建立；candidate 不自动晋升（由采集层判定关系级别）。
//! - 正文最多一份 authority：content_text ≤64 KiB，超出截断并标 truncated。
//! - 所有写入短事务；不得持锁等待网络（采集在锁外完成，此处只接收结果）。

use rusqlite::Connection;

use super::adapters::RawItem;

/// 单篇正文存储上限（方案 03 §7/04 §7）。
pub const MAX_CONTENT_BYTES: usize = 64 * 1024;

pub fn content_hash(item: &RawItem) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(item.title.as_bytes());
    hasher.update([0]);
    hasher.update(item.summary.as_bytes());
    hasher.update([0]);
    hasher.update(item.url.as_bytes());
    hasher.update(
        serde_json::to_vec(&(
            item.published_at.as_deref(),
            item.precision.as_str(),
            item.importance,
            &item.event_types,
            &item.codes,
        ))
        .expect("serializable information metadata"),
    );
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// upsert 一批条目并重建关联；`now` 由调用方注入（测试时钟/动态今天）。
pub fn upsert_items(
    conn: &Connection,
    items: &[RawItem],
    relations_for: &dyn Fn(&RawItem) -> Vec<(String, &'static str, String)>,
    now: &str,
) -> rusqlite::Result<usize> {
    let mut written = 0usize;
    for item in items {
        let hash = content_hash(item);
        let existing: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, content_hash FROM information_items WHERE source_id=?1 AND external_id=?2",
                rusqlite::params![item.source_id, item.external_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
        let item_id = match existing {
            Some((id, old_hash)) => {
                if old_hash != hash {
                    conn.execute(
                        "UPDATE information_items SET title=?3, url=?4, summary_text=?5, \
                         published_at=?6, time_precision=?7, content_hash=?8, \
                         content_version=content_version+1, updated_at=?9, event_type=?10, importance=?11, \
                         content_text=NULL,content_fetched_at=NULL,completeness='link_only' \
                         WHERE id=?1 AND source_id=?2",
                        rusqlite::params![
                            id,
                            item.source_id,
                            item.title,
                            item.url,
                            item.summary,
                            item.published_at,
                            item.precision.as_str(),
                            hash,
                            now,
                            item.event_types.first().copied().unwrap_or("notice"),
                            item.importance
                        ],
                    )?;
                    written += 1;
                }
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO information_items(source_id, external_id, title, url, summary_text, \
                     published_at, time_precision, retrieved_at, content_hash, completeness, \
                     event_type, importance, created_at, updated_at) \
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'link_only',?10,?11,?12,?12)",
                    rusqlite::params![
                        item.source_id,
                        item.external_id,
                        item.title,
                        item.url,
                        item.summary,
                        item.published_at,
                        item.precision.as_str(),
                        now,
                        hash,
                        item.event_types.first().copied().unwrap_or("notice"),
                        item.importance,
                        now
                    ],
                )?;
                written += 1;
                conn.last_insert_rowid()
            }
        };
        conn.execute(
            "UPDATE information_items SET retrieved_at=?2 WHERE id=?1",
            rusqlite::params![item_id, now],
        )?;
        // 关联重建：同一条目的关联以本次采集结果为准；关系级别由采集层判定。
        conn.execute(
            "DELETE FROM information_relations WHERE item_id=?1",
            [item_id],
        )?;
        for (reference, relation, reason) in relations_for(item) {
            conn.execute(
                "INSERT OR IGNORE INTO information_relations(item_id, instrument_ref, relation, reason, created_at) \
                 VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![item_id, reference, relation, reason, now],
            )?;
        }
    }
    Ok(written)
}

/// 写入正文（只接受已存在条目；截断标记 truncated，空正文标 unavailable）。
pub fn save_content(
    conn: &Connection,
    item_id: i64,
    content: &str,
    completeness: &'static str,
    now: &str,
) -> rusqlite::Result<bool> {
    let truncated;
    let stored;
    if content.len() > MAX_CONTENT_BYTES {
        let mut end = MAX_CONTENT_BYTES;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        stored = &content[..end];
        truncated = "truncated";
    } else {
        stored = content;
        truncated = completeness;
    }
    let n = conn.execute(
        "UPDATE information_items SET content_text=?2, completeness=?3, content_version=content_version+1, \
         updated_at=?4 WHERE id=?1 AND (content_text IS NOT ?2 OR completeness != ?3)",
        rusqlite::params![item_id, stored, truncated, now],
    )?;
    conn.execute(
        "UPDATE information_items SET content_fetched_at=?2 WHERE id=?1",
        rusqlite::params![item_id, now],
    )?;
    Ok(n == 1
        || conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM information_items WHERE id=?1)",
            [item_id],
            |r| r.get::<_, bool>(0),
        )?)
}

/// Unknown, invalid, future, or 24-hour-old body timestamps require explicit refresh.
pub fn content_cache_fresh(conn: &Connection, id: i64, now: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT COALESCE(content_text IS NOT NULL AND julianday(?2)-julianday(content_fetched_at)>=0 AND julianday(?2)-julianday(content_fetched_at)<1,0) FROM information_items WHERE id=?1",rusqlite::params![id,now],|r|r.get(0))
}

/// 列表查询：`instrument_refs` 为空的 scope 不按关联过滤（调用方决定语义）。
pub struct ItemListRow {
    pub id: i64,
    pub source_id: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub has_content: bool,
    pub published_at: Option<String>,
    pub time_precision: String,
    pub completeness: String,
    pub event_type: String,
    pub importance: i32,
    pub read_status: bool,
    pub favorite: bool,
    pub archived: bool,
    pub content_version: i64,
}

#[derive(Default)]
pub struct ListFilter {
    pub from: String,
    pub to: String,
    pub query: String,
    pub asset: String,
    pub event_type: String,
    pub sort: String,
    pub group_refs: Option<Vec<String>>,
    pub market_only: bool,
}

/// Read-only candidates; never merge source identities, text, or personal state.
pub fn related_items(conn: &Connection, id: i64) -> rusqlite::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare("SELECT other.id,other.source_id,other.title,other.published_at \
        FROM information_items current JOIN information_items other \
        ON other.id!=current.id AND other.source_id!=current.source_id AND other.archived=0 \
        AND other.event_type=current.event_type \
        AND abs(julianday(other.published_at)-julianday(current.published_at))<=3 \
        WHERE current.id=?1 AND EXISTS(SELECT 1 FROM information_relations a \
        JOIN information_relations b ON a.instrument_ref=b.instrument_ref \
        WHERE a.item_id=current.id AND b.item_id=other.id AND a.relation='direct' AND b.relation='direct') \
        ORDER BY lower(trim(other.title))=lower(trim(current.title)) DESC,other.published_at DESC,other.id DESC LIMIT 10")?;
    let rows = stmt.query_map([id], |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"sourceId":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,"publishedAt":r.get::<_,Option<String>>(3)?,"relation":"possible","reason":"same_asset_event_within_3_days"})))?.collect();
    rows
}

pub fn list_items(
    conn: &Connection,
    refs: &[String],
    favorites_only: bool,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<ItemListRow>> {
    list_items_filtered(
        conn,
        refs,
        favorites_only,
        limit,
        offset,
        &ListFilter::default(),
    )
}

pub fn list_items_filtered(
    conn: &Connection,
    refs: &[String],
    favorites_only: bool,
    limit: i64,
    offset: i64,
    filter: &ListFilter,
) -> rusqlite::Result<Vec<ItemListRow>> {
    let mut sql = String::from(
        "SELECT i.id, i.source_id, i.title, i.url, i.summary_text, i.content_text IS NOT NULL, \
         i.published_at, i.time_precision, i.completeness, i.event_type, i.importance, \
         i.read_status, i.favorite, i.archived, i.content_version \
         FROM information_items i",
    );
    // 归档条目不出现在默认列表（个人归档语义；归档视图后续按需扩展）。
    let mut where_started = false;
    if !refs.is_empty() {
        // M04（方案 04 §4）：关联分层排序——直接关联重要公告 → 其他直接 →
        // 间接 → 候选；同层按发布时间降序与稳定 id。层取该条目在匹配身份中
        // 的最强关联（MIN(rel_rank)）。
        sql.push_str(
            " LEFT JOIN (SELECT item_id, MIN(CASE relation WHEN 'direct' THEN 0 \
             WHEN 'indirect' THEN 1 ELSE 2 END) AS rel_rank FROM information_relations \
             WHERE instrument_ref IN (",
        );
        for (idx, _) in refs.iter().enumerate() {
            if idx > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("?{}", idx + 1));
        }
        sql.push_str(") GROUP BY item_id) r ON r.item_id = i.id");
        sql.push_str(
            " WHERE i.archived=0 AND i.id IN (SELECT item_id FROM information_relations \
             WHERE instrument_ref IN (",
        );
        for (idx, _) in refs.iter().enumerate() {
            if idx > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("?{}", idx + 1));
        }
        sql.push_str("))");
        where_started = true;
    }
    if favorites_only {
        sql.push_str(if where_started {
            " AND i.favorite=1"
        } else {
            " WHERE i.archived=0 AND i.favorite=1"
        });
        where_started = true;
    }
    if !where_started {
        sql.push_str(" WHERE i.archived=0");
    }
    if filter.market_only {
        sql.push_str(" AND i.source_id='em_finance'");
    }
    // Filters run before LIMIT/OFFSET, so paging cannot hide matching later records.
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = refs
        .iter()
        .map(|r| Box::new(r.clone()) as Box<dyn rusqlite::ToSql>)
        .collect();
    for (condition,value) in [
        ("substr(i.published_at,1,10)>=?",&filter.from),
        ("substr(i.published_at,1,10)<=?",&filter.to),
        ("instr(lower(i.title || ' ' || i.summary_text),lower(?))>0",&filter.query),
        ("EXISTS(SELECT 1 FROM information_relations x WHERE x.item_id=i.id AND x.instrument_ref=?)",&filter.asset),
        ("i.event_type=?",&filter.event_type),
    ] {
        if !value.is_empty(){sql.push_str(" AND ");sql.push_str(condition);params.push(Box::new(value.clone()));}
    }
    if let Some(group_refs) = &filter.group_refs {
        if group_refs.is_empty() {
            sql.push_str(" AND 0");
        } else {
            sql.push_str(" AND EXISTS(SELECT 1 FROM information_relations g WHERE g.item_id=i.id AND g.instrument_ref IN (");
            for (idx, reference) in group_refs.iter().enumerate() {
                if idx > 0 {
                    sql.push(',');
                }
                sql.push('?');
                params.push(Box::new(reference.clone()));
            }
            sql.push_str("))");
        }
    }
    if refs.is_empty() && filter.sort != "latest" {
        sql.push_str(" ORDER BY i.published_at IS NULL, date(i.published_at,'+8 hours') DESC, CASE WHEN i.importance>=2 THEN 0 WHEN i.source_id='em_finance' AND i.importance=1 THEN 1 WHEN i.source_id='em_finance' THEN 2 ELSE 3 END, i.published_at DESC, i.id DESC LIMIT ? OFFSET ?");
    } else if filter.sort == "latest" {
        sql.push_str(
            " ORDER BY i.published_at IS NULL, i.published_at DESC, i.id DESC LIMIT ? OFFSET ?",
        );
    } else {
        sql.push_str(
            " ORDER BY CASE WHEN r.rel_rank = 0 AND i.importance >= 2 THEN 0 \
             WHEN r.rel_rank = 0 THEN 1 WHEN r.rel_rank = 1 THEN 2 ELSE 3 END, \
             i.published_at IS NULL, i.published_at DESC, i.id DESC LIMIT ? OFFSET ?",
        );
    }
    params.push(Box::new(limit));
    params.push(Box::new(offset));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter().map(|b| b.as_ref())),
        |r| {
            Ok(ItemListRow {
                id: r.get(0)?,
                source_id: r.get(1)?,
                title: r.get(2)?,
                url: r.get(3)?,
                summary: r.get(4)?,
                has_content: r.get(5)?,
                published_at: r.get(6)?,
                time_precision: r.get(7)?,
                completeness: r.get(8)?,
                event_type: r.get(9)?,
                importance: r.get(10)?,
                read_status: r.get::<_, i64>(11)? == 1,
                favorite: r.get::<_, i64>(12)? == 1,
                archived: r.get::<_, i64>(13)? == 1,
                content_version: r.get(14)?,
            })
        },
    )?;
    rows.collect()
}

/// 详情（含正文与关联）。
pub struct ItemDetail {
    pub row: ItemListRow,
    pub content_text: Option<String>,
    pub retrieved_at: String,
    pub content_fetched_at: Option<String>,
    pub relations: Vec<RelationRow>,
}

pub struct RelationRow {
    pub instrument_ref: String,
    pub relation: String,
    pub reason: String,
}

pub fn item_detail(conn: &Connection, id: i64) -> rusqlite::Result<Option<ItemDetail>> {
    let list = list_page_one(conn, id)?;
    let Some(row) = list else { return Ok(None) };
    let (content_text, retrieved_at, content_fetched_at): (Option<String>, String, Option<String>) = conn.query_row(
        "SELECT content_text, retrieved_at, content_fetched_at FROM information_items WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut stmt = conn.prepare(
        "SELECT instrument_ref, relation, reason FROM information_relations WHERE item_id=?1 ORDER BY relation, instrument_ref",
    )?;
    let relations = stmt
        .query_map([id], |r| {
            Ok(RelationRow {
                instrument_ref: r.get(0)?,
                relation: r.get(1)?,
                reason: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(ItemDetail {
        row,
        content_text,
        retrieved_at,
        content_fetched_at,
        relations,
    }))
}

fn list_page_one(conn: &Connection, id: i64) -> rusqlite::Result<Option<ItemListRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_id, title, url, summary_text, content_text IS NOT NULL, \
         published_at, time_precision, completeness, event_type, importance, \
         read_status, favorite, archived, content_version \
         FROM information_items WHERE id=?1",
    )?;
    let mut rows = stmt.query_map([id], |r| {
        Ok(ItemListRow {
            id: r.get(0)?,
            source_id: r.get(1)?,
            title: r.get(2)?,
            url: r.get(3)?,
            summary: r.get(4)?,
            has_content: r.get(5)?,
            published_at: r.get(6)?,
            time_precision: r.get(7)?,
            completeness: r.get(8)?,
            event_type: r.get(9)?,
            importance: r.get(10)?,
            read_status: r.get::<_, i64>(11)? == 1,
            favorite: r.get::<_, i64>(12)? == 1,
            archived: r.get::<_, i64>(13)? == 1,
            content_version: r.get(14)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 个人状态：read/favorite（API 与手工资料共用语义，权威在本表）。
pub fn set_flags(
    conn: &Connection,
    id: i64,
    read: Option<bool>,
    favorite: Option<bool>,
    now: &str,
) -> rusqlite::Result<bool> {
    let n_read = match read {
        Some(v) => conn.execute(
            "UPDATE information_items SET read_status=?2, updated_at=?3 WHERE id=?1",
            rusqlite::params![id, i64::from(v), now],
        )?,
        None => 1,
    };
    let n_fav = match favorite {
        Some(v) => conn.execute(
            "UPDATE information_items SET favorite=?2, updated_at=?3 WHERE id=?1",
            rusqlite::params![id, i64::from(v), now],
        )?,
        None => 1,
    };
    Ok(n_read == 1 && n_fav == 1)
}

/// 归档／恢复（false=恢复）。
pub fn set_archived(
    conn: &Connection,
    id: i64,
    archived: bool,
    now: &str,
) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE information_items SET archived=?2, updated_at=?3 WHERE id=?1",
        rusqlite::params![id, i64::from(archived), now],
    )? == 1)
}

/// 来源状态读取（固定白名单；未知来源视为禁用）。
pub struct SourceState {
    pub source_id: String,
    pub enabled: bool,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_at: Option<String>,
    pub last_count: i64,
    pub last_latency_ms: i64,
}

pub const SOURCE_IDS: &[&str] = &[
    super::adapters::SRC_STOCK_NEWS,
    super::adapters::SRC_STOCK_ANN,
    super::adapters::SRC_FUND_ANN,
    super::adapters::SRC_FINANCE,
];

pub fn source_states(conn: &Connection) -> rusqlite::Result<Vec<SourceState>> {
    let mut out = Vec::new();
    for id in SOURCE_IDS {
        let row = conn
            .query_row(
                "SELECT enabled, last_attempt_at, last_success_at, last_error_code, last_error_at, \
                 last_count, last_latency_ms FROM information_source_state WHERE source_id=?1",
                [id],
                |r| {
                    Ok(SourceState {
                        source_id: (*id).to_string(),
                        enabled: r.get::<_, i64>(0)? == 1,
                        last_attempt_at: r.get(1)?,
                        last_success_at: r.get(2)?,
                        last_error_code: r.get(3)?,
                        last_error_at: r.get(4)?,
                        last_count: r.get(5)?,
                        last_latency_ms: r.get(6)?,
                    })
                },
            )
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
        out.push(row.unwrap_or_else(|| SourceState {
            source_id: (*id).to_string(),
            enabled: true,
            last_attempt_at: None,
            last_success_at: None,
            last_error_code: None,
            last_error_at: None,
            last_count: 0,
            last_latency_ms: 0,
        }));
    }
    Ok(out)
}

pub fn set_source_enabled(
    conn: &Connection,
    source_id: &str,
    enabled: bool,
) -> rusqlite::Result<bool> {
    if !SOURCE_IDS.contains(&source_id) {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO information_source_state(source_id, enabled) VALUES(?1,?2) \
         ON CONFLICT(source_id) DO UPDATE SET enabled=?2",
        rusqlite::params![source_id, i64::from(enabled)],
    )?;
    Ok(true)
}

pub fn record_source_result(
    conn: &Connection,
    source_id: &str,
    ok: bool,
    count: i64,
    latency_ms: i64,
    error_code: Option<&str>,
    now: &str,
) -> rusqlite::Result<()> {
    let success_at = if ok { Some(now) } else { None };
    let err_code = error_code;
    let err_at = if error_code.is_some() {
        Some(now)
    } else {
        None
    };
    conn.execute(
        "INSERT INTO information_source_state(source_id, enabled, last_attempt_at, last_success_at, \
         last_error_code, last_error_at, last_count, last_latency_ms) \
         VALUES(?1,1,?2,?3,?4,?5,?6,?7) \
         ON CONFLICT(source_id) DO UPDATE SET last_attempt_at=?2, last_success_at=COALESCE(?3,information_source_state.last_success_at), \
         last_error_code=?4, last_error_at=?5, last_count=?6, last_latency_ms=?7",
        rusqlite::params![source_id, now, success_at, err_code, err_at, count, latency_ms],
    )?;
    Ok(())
}

/// 自动未收藏资料软上限（方案 03 §7：90 天／10,000 条；有界批次清理，
/// 只清既未收藏也未归档的自动条目，用户手工内容不在此表）。
pub fn prune(conn: &Connection, keep: i64) -> rusqlite::Result<usize> {
    let mut stmt=conn.prepare("SELECT id FROM information_items WHERE favorite=0 AND archived=0 AND (retrieved_at < strftime('%Y-%m-%dT%H:%M:%SZ','now','-90 days') OR id NOT IN (SELECT id FROM information_items ORDER BY published_at DESC,id DESC LIMIT ?1)) ORDER BY id LIMIT 200")?;
    let ids = stmt
        .query_map([keep.max(0)], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for id in &ids {
        conn.execute("DELETE FROM information_relations WHERE item_id=?1", [id])?;
        conn.execute("DELETE FROM information_items WHERE id=?1", [id])?;
    }
    // ponytail: evict at most 200 unfavorited bodies per refresh; retain metadata and report snapshots.
    let bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(length(CAST(content_text AS BLOB))),0) FROM information_items",
        [],
        |r| r.get(0),
    )?;
    if bytes > 50 * 1024 * 1024 {
        conn.execute("UPDATE information_items SET content_text=NULL,content_fetched_at=NULL,completeness='link_only',content_version=content_version+1 WHERE id IN (SELECT id FROM information_items WHERE content_text IS NOT NULL AND favorite=0 AND archived=0 ORDER BY retrieved_at,id LIMIT 200)",[])?;
    }
    Ok(ids.len())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
