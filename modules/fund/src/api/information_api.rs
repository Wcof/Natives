//! 自动资讯 API（方案 03 §6 契约；M01～M05 生产入口）。
//!
//! - `GET /api/information/items`：scope=holdings（默认）／watchlist／favorites；
//!   返回条目、覆盖说明与来源状态。数据源失败不影响账本功能。
//! - `GET /api/information/item?id=`：详情（正文、完整度、关联、个人状态）。
//! - `POST /api/information/refresh`：用户显式刷新；进行中重复请求合并（409 提示）。
//! - `POST /api/information/content`：只接受已存在 itemId，正文按需获取。
//! - `GET/POST /api/information/sources`：固定来源白名单启停；不开放任意 URL。
//! - `POST /api/information/flags`、`/api/information/archive`：个人状态。
//!
//! 网络采集在 Store 锁外完成（adapters 自带超时），只短事务写结果。

use serde_json::{json, Value};

use crate::information::{self, store};
use crate::storage::Store;

/// 现在的 UTC ISO 时间（显示与存储统一 UTC；前端按 locale 渲染）。
fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix(secs as i64)
}

pub(crate) fn format_unix(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + i64::from(if m <= 2 { 1 } else { 0 }), m, d)
}

/// scope → 关联身份过滤列表。
fn scope_refs(store: &Store, scope: &str) -> Result<Vec<String>, (u16, String)> {
    match scope {
        // 持仓默认；基金用稳定 fund 身份。股票持仓（T04）落地前由公告 direct ref 兜底查询为空。
        "holdings" => {
            let (funds, stocks) = store
                .with_read(|conn| information::collect_instruments(conn))
                .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
            let names = store
                .with_read(information::fund_names)
                .map_err(|_| (500, "INFORMATION_DB".into()))?;
            Ok(funds
                .iter()
                .map(|c| information::instrument_ref_for_fund(c))
                .chain(
                    stocks
                        .iter()
                        .map(|c| information::instrument_ref_for_code(c)),
                )
                .chain(
                    names
                        .iter()
                        .map(|name| information::instrument_ref_for_keyword(name)),
                )
                .collect())
        }
        "watchlist" => {
            let symbols = store
                .with_read(|conn| {
                    let mut stmt = conn.prepare(
                        "SELECT DISTINCT symbol FROM watchlist_items WHERE asset_type IN ('stock','etf') ORDER BY symbol",
                    )?;
                    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                    rows.collect::<rusqlite::Result<Vec<_>>>()
                })
                .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
            Ok(symbols
                .iter()
                .filter_map(|c| information::public_stock_code(c))
                .map(|c| information::instrument_ref_for_code(&c))
                .collect())
        }
        "favorites" | "all" | "market" => Ok(vec![]),
        other => Err((400, format!("FIN_INVALID: 未知资讯范围 {other}"))),
    }
}

fn group_information_refs(store: &Store, group: &str) -> Result<Vec<String>, (u16, String)> {
    store.with_read(|c| -> rusqlite::Result<Vec<String>> {
        let mut refs=Vec::new();
        let mut stmt=c.prepare("SELECT DISTINCT f.instrument_kind,f.market,CASE WHEN f.display_code='' THEN f.code ELSE f.display_code END,f.name FROM group_plan_bindings b JOIN funds f ON b.fund_id=f.id JOIN positions p ON p.fund_id=f.id AND p.account_id=b.account_id WHERE b.group_id=?1 AND b.source_kind='fund' AND p.quantity_raw>0")?;
        for row in stmt.query_map([group],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))? {
            let (kind,market,code,name)=row?;
            if kind=="stock" {
                if let Some(code)=information::public_stock_code(&format!("security:{market}:{code}")){refs.push(information::instrument_ref_for_code(&code));}
            } else if market=="CN" || kind=="etf"&&matches!(market.as_str(),"SSE"|"SZSE"|"BSE") {
                refs.push(information::instrument_ref_for_fund(&code));
            }
            if !name.is_empty()&&matches!(market.as_str(),"CN"|"SSE"|"SZSE"|"BSE"){refs.push(information::instrument_ref_for_keyword(&name));}
        }
        let mut stmt=c.prepare("SELECT m.instrument_key FROM group_plan_bindings b JOIN manual_assets m ON b.asset_id=m.id WHERE b.group_id=?1 AND b.source_kind='manual' AND m.deleted_at IS NULL AND m.converted_ref IS NULL AND m.market_value_minor>0 AND m.product_type IN ('stock','etf')")?;
        for key in stmt.query_map([group],|r|r.get::<_,String>(0))?{if let Some(code)=information::public_stock_code(&key?){refs.push(information::instrument_ref_for_code(&code));}}
        refs.sort();refs.dedup();Ok(refs)
    }).map_err(|_|(500,"INFORMATION_DB".into()))
}

pub fn api_information_items(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let query = path.split('?').nth(1).unwrap_or("");
    let mut scope = "holdings".to_string();
    let mut page = 1usize;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("scope"), Some(v)) => scope = crate::market::url_decode(v),
            (Some("page"), Some(v)) => {
                page = v
                    .parse()
                    .map_err(|_| (400, "FIN_INVALID: page".to_string()))?;
            }
            _ => {}
        }
    }
    let field = |name| crate::market::query_param(path, name).unwrap_or_default();
    let group = field("group");
    let group_options = store
        .with_read(
            |c| -> Result<Vec<Value>, crate::group_plan::GroupPlanError> {
                Ok(crate::group_plan::get_plan(c)?
                    .map(|p| {
                        p.groups
                            .iter()
                            .map(|g| json!({"id":g.group_id,"name":g.name}))
                            .collect()
                    })
                    .unwrap_or_default())
            },
        )
        .map_err(|_| (500, "INFORMATION_DB".into()))?;
    if !group.is_empty() && !group_options.iter().any(|g| g["id"] == group) {
        return Err((400, "FIN_INVALID: group".into()));
    }
    let group_refs = if group.is_empty() {
        None
    } else {
        Some(group_information_refs(store, &group)?)
    };
    let filter = store::ListFilter {
        from: field("from"),
        to: field("to"),
        query: field("q"),
        asset: field("asset"),
        event_type: field("type"),
        sort: field("sort"),
        group_refs,
        market_only: scope == "market",
    };
    if [&filter.from, &filter.to]
        .iter()
        .any(|s| !s.is_empty() && !crate::assets::is_valid_calendar_date(s))
        || !filter.from.is_empty() && !filter.to.is_empty() && filter.from > filter.to
        || filter.query.chars().count() > 128
        || filter.asset.len() > 256
        || !matches!(filter.sort.as_str(), "" | "relevance" | "latest")
        || !matches!(
            filter.event_type.as_str(),
            "" | "notice"
                | "earnings"
                | "dividend"
                | "repurchase"
                | "regulatory"
                | "suspension"
                | "financing"
                | "insider"
                | "restructuring"
                | "major"
        )
    {
        return Err((400, "FIN_INVALID: information filters".into()));
    }
    page = page.clamp(1, 1_000_000);
    let refs = scope_refs(store, &scope)?;
    // 审计五-1：holdings/watchlist 无身份时返回空列表（成功无结果），
    // 不回退为全库查询——没有持仓就没有「与我有关」的资讯。
    if (scope == "holdings" || scope == "watchlist") && refs.is_empty() {
        let sources = source_states_json(store)?;
        return Ok(json!({
            "scope": scope,
            "items": [],
            "page": page,
            "pageSize": 50,
            "sources": sources,
            "coverage": information::COVERAGE_NOTE,
        })
        .to_string());
    }
    let limit = 50i64;
    let offset = (page.saturating_sub(1)) as i64 * limit;
    let cache_key = format!(
        "{}:{}",
        information::list_cache_key(&scope, &refs, page),
        serde_json::json!([
            filter.from,
            filter.to,
            filter.query,
            filter.asset,
            filter.event_type,
            filter.sort,
            group
        ])
    );
    let revision = store
        .with_read(Store::data_revision)
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    if let Some(cached) = store.information_cache.get(&cache_key, &revision) {
        return Ok(String::from_utf8_lossy(&cached).into_owned());
    }
    let mut asset_options = scope_refs(store, "holdings")?;
    asset_options.extend(scope_refs(store, "watchlist")?);
    asset_options.sort();
    asset_options.dedup();
    let favorites = scope == "favorites";
    let rows = store
        .with_read(|conn| {
            store::list_items_filtered(conn, &refs, favorites, limit, offset, &filter)
        })
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    let sources = source_states_json(store)?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "sourceId": r.source_id,
                "title": r.title,
                "url": r.url,
                "summary": r.summary,
                "hasContent": r.has_content,
                "publishedAt": r.published_at,
                "timePrecision": r.time_precision,
                "completeness": r.completeness,
                "eventType": r.event_type,
                "importance": r.importance,
                "read": r.read_status,
                "favorite": r.favorite,
                "archived": r.archived,
                "contentVersion": r.content_version,
            })
        })
        .collect();
    let body = json!({
        "scope": scope,
        "items": items,
        "page": page,
        "pageSize": limit,
        "assetOptions":asset_options,
        "groupOptions":group_options,
        "sources": sources,
        "coverage": information::COVERAGE_NOTE,
    })
    .to_string();
    store
        .information_cache
        .set(&cache_key, &revision, body.clone().into_bytes());
    Ok(body)
}

/// 资讯复核提醒（R02；方案 05 §8）：状态机与规则事件同语义；
/// 经稳定引用关联，不冒充 /api/events。
pub fn api_information_alerts(store: &Store) -> Result<String, (u16, String)> {
    let now = now_iso();
    let expired=store.with_read(|c|c.query_row("SELECT EXISTS(SELECT 1 FROM information_alerts WHERE status='snoozed' AND snoozed_until<=?1)",[&now],|r|r.get::<_,bool>(0))).map_err(|_|(500,"INFORMATION_DB".into()))?;
    if expired {
        store.with_write(|c|c.execute("UPDATE information_alerts SET status='open',snoozed_until=NULL,updated_at=?1 WHERE status='snoozed' AND snoozed_until<=?1",[&now])).map_err(|_|(500,"INFORMATION_DB".into()))?;
    }
    let rows = store
        .with_read(|conn| -> rusqlite::Result<Vec<Value>> {
            let mut stmt = conn.prepare(
                "SELECT a.id, a.asset_ref, a.source_ref, a.content_version, a.title, a.event_type, \
                 a.status, a.snoozed_until, a.first_triggered_at, a.updated_at, \
                 (SELECT i.id FROM information_items i \
                  WHERE i.source_id = substr(a.source_ref, 1, instr(a.source_ref, ':') - 1) \
                    AND i.external_id = substr(a.source_ref, instr(a.source_ref, ':') + 1)) \
                 FROM information_alerts a \
                 WHERE a.status != 'resolved' ORDER BY a.first_triggered_at DESC LIMIT 50",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "assetRef": r.get::<_, String>(1)?,
                        "sourceRef": r.get::<_, String>(2)?,
                        "contentVersion": r.get::<_, i64>(3)?,
                        "title": r.get::<_, String>(4)?,
                        "eventType": r.get::<_, String>(5)?,
                        "status": r.get::<_, String>(6)?,
                        "snoozedUntil": r.get::<_, Option<String>>(7)?,
                        "firstTriggeredAt": r.get::<_, String>(8)?,
                        "updatedAt": r.get::<_, String>(9)?,
                        "itemId": r.get::<_, Option<i64>>(10)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    Ok(json!({"alerts": rows}).to_string())
}

/// 提醒操作：acknowledge/snooze/resolve（方案 05 §8 状态复用语义）。
pub fn api_information_alert_action(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| (400, format!("FIN_INVALID: {e}")))?;
    let id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let action = value["action"].as_str().unwrap_or("").to_string();
    if !matches!(action.as_str(), "acknowledge" | "snooze" | "resolve") {
        return Err((400, "FIN_INVALID: 未知操作".into()));
    }
    let mut snoozed_until = value["snoozedUntil"].as_str().unwrap_or("").to_string();
    let now = now_iso();
    if action == "snooze" {
        if crate::assets::is_valid_calendar_date(&snoozed_until) {
            snoozed_until.push_str("T00:00:00Z");
        }
        let bytes = snoozed_until.as_bytes();
        let valid = bytes.is_ascii()
            && bytes.len() == 20
            && bytes[10] == b'T'
            && bytes[13] == b':'
            && bytes[16] == b':'
            && bytes[19] == b'Z'
            && crate::assets::is_valid_calendar_date(&snoozed_until[..10])
            && [11..13, 14..16, 17..19]
                .iter()
                .all(|r| bytes[r.clone()].iter().all(u8::is_ascii_digit))
            && &bytes[11..13] < b"24".as_slice()
            && &bytes[14..16] < b"60".as_slice()
            && &bytes[17..19] < b"60".as_slice();
        if !valid || snoozed_until <= now {
            return Err((400, "FIN_INVALID: 请选择未来提醒日期".into()));
        }
    }
    let n = store
        .with_write(|conn| -> rusqlite::Result<usize> {
            match action.as_str() {
                "acknowledge" => conn.execute(
                    "UPDATE information_alerts SET status='acknowledged', updated_at=?2 WHERE id=?1 AND status='open'",
                    rusqlite::params![id, now],
                ),
                "snooze" => conn.execute(
                    "UPDATE information_alerts SET status='snoozed', snoozed_until=?2, updated_at=?3 WHERE id=?1 AND status IN ('open','acknowledged','snoozed')",
                    rusqlite::params![id, snoozed_until, now],
                ),
                _ => conn.execute(
                    "UPDATE information_alerts SET status='resolved', resolved_at=?2, updated_at=?2 WHERE id=?1 AND status!='resolved'",
                    rusqlite::params![id, now],
                ),
            }
        })
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    if n == 0 {
        return Err((404, "INFORMATION_NOT_FOUND: 提醒不存在或状态不允许".into()));
    }
    Ok(json!({"id": id, "action": action}).to_string())
}

fn source_states_json(store: &Store) -> Result<Value, (u16, String)> {
    let states = store
        .with_read(store::source_states)
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    Ok(json!(states
        .iter()
        .map(|s| {
            json!({
                "sourceId": s.source_id,
                "enabled": s.enabled,
                "lastAttemptAt": s.last_attempt_at,
                "lastSuccessAt": s.last_success_at,
                "lastErrorCode": s.last_error_code,
                "lastErrorAt": s.last_error_at,
                "lastCount": s.last_count,
                "lastLatencyMs": s.last_latency_ms,
            })
        })
        .collect::<Vec<_>>()))
}

pub fn api_information_item(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let query = path.split('?').nth(1).unwrap_or("");
    let mut id: Option<i64> = None;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if let (Some("id"), Some(v)) = (kv.next(), kv.next()) {
            id = v.parse().ok();
        }
    }
    let id = id.ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let detail = store
        .with_read(|conn| store::item_detail(conn, id))
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    let Some(d) = detail else {
        return Err((404, "INFORMATION_NOT_FOUND: 条目不存在".into()));
    };
    let related = store
        .with_read(|conn| store::related_items(conn, id))
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    let fresh = store
        .with_read(|c| store::content_cache_fresh(c, id, &now_iso()))
        .map_err(|_| (500, "INFORMATION_DB".into()))?;
    Ok(json!({
        "contentFetchedAt":d.content_fetched_at,
        "contentCacheExpired":d.row.has_content && !fresh,
        "relatedItems": related,
        "id": d.row.id,
        "sourceId": d.row.source_id,
        "title": d.row.title,
        "url": d.row.url,
        "summary": d.row.summary,
        "contentText": d.content_text,
        "hasContent": d.row.has_content,
        "publishedAt": d.row.published_at,
        "timePrecision": d.row.time_precision,
        "retrievedAt": d.retrieved_at,
        "completeness": d.row.completeness,
        "eventType": d.row.event_type,
        "importance": d.row.importance,
        "read": d.row.read_status,
        "favorite": d.row.favorite,
        "archived": d.row.archived,
        "contentVersion": d.row.content_version,
        "relations": d.relations.iter().map(|r| json!({
            "instrumentRef": r.instrument_ref,
            "relation": r.relation,
            "reason": r.reason,
        })).collect::<Vec<_>>(),
    })
    .to_string())
}

pub fn api_information_sources(store: &Store) -> Result<String, (u16, String)> {
    let sources = source_states_json(store)?;
    Ok(json!({
        "sources": sources,
        "coverage": information::COVERAGE_NOTE,
    })
    .to_string())
}

pub fn api_information_source_enabled(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| (400, format!("FIN_INVALID: {e}")))?;
    let source_id = value["sourceId"].as_str().unwrap_or("");
    let enabled = value["enabled"]
        .as_bool()
        .ok_or((400, "FIN_INVALID: enabled 必须是布尔值".into()))?;
    let ok = store
        .with_write(|conn| store::set_source_enabled(conn, source_id, enabled))
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    if !ok {
        return Err((400, "FIN_INVALID: 未知来源（固定白名单外不可启用）".into()));
    }
    Ok(json!({"sourceId": source_id, "enabled": enabled}).to_string())
}

pub fn api_information_flags(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| (400, format!("FIN_INVALID: {e}")))?;
    let id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let read = value["read"].as_bool();
    let favorite = value["favorite"].as_bool();
    if read.is_none() && favorite.is_none() {
        return Err((400, "FIN_INVALID: 缺少 read/favorite".to_string()));
    }
    let ok = store
        .with_write(|conn| store::set_flags(conn, id, read, favorite, &now_iso()))
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    if !ok {
        return Err((404, "INFORMATION_NOT_FOUND: 条目不存在".into()));
    }
    Ok(json!({"id": id}).to_string())
}

pub fn api_information_archive(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| (400, format!("FIN_INVALID: {e}")))?;
    let id = value["id"]
        .as_i64()
        .ok_or((400, "FIN_INVALID: 缺少 id".to_string()))?;
    let ok = store
        .with_write(|conn| store::set_archived(conn, id, true, &now_iso()))
        .map_err(|e| (500, format!("INFORMATION_DB: {e}")))?;
    if !ok {
        return Err((404, "INFORMATION_NOT_FOUND: 条目不存在".into()));
    }
    Ok(json!({"id": id}).to_string())
}
