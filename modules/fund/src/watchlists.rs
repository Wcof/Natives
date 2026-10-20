//! 自选领域操作（mode/T04 审计更正，R-M3/R-M4）。
//!
//! 业务与数据操作从 api/watchlists.rs 收口到本领域模块：HTTP 适配只做
//! JSON 解析与错误 wire 映射，Store 仍是唯一事务 owner。创建时间使用
//! 真实当前时间（`now_rfc3339`）；受控时间仅供测试注入，生产路径固定
//! 使用系统时钟。行读取错误整体传播；DB 错误只暴露稳定 code。

use crate::storage::Store;
use serde_json::Value;

const ID_MAX: usize = 64;
const NAME_MAX: usize = 64;
const RAW_SYMBOL_MAX: usize = 48;
const SYMBOL_MAX: usize = 32;
const ASSET_TYPE_MAX: usize = 16;

/// 领域错误（稳定 code；HTTP 状态由适配层映射）。
#[derive(Debug)]
pub(crate) enum WatchlistError {
    Invalid(String),
    Db,
    Conflict,
}

impl From<rusqlite::Error> for WatchlistError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Db
    }
}

impl WatchlistError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "APP_PARAM_INVALID",
            Self::Db => "WATCHLIST_DB",
            Self::Conflict => "WATCHLIST_CONFLICT",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Invalid(message) => message.clone(),
            Self::Db => "watchlist storage read/write failed".into(),
            Self::Conflict => "watchlist changed; reload and review before saving".into(),
        }
    }
}

/// Unix 秒 → RFC3339 UTC（`Z` 口径，与既有 `createdAt` 字符串格式一致；
/// 沿用 `civil_from_days` 日历换算，无 chrono 依赖）。纯函数便于受控时钟测试。
pub(crate) fn rfc3339_utc_from_secs(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        crate::assets::civil_from_days(days),
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub(crate) fn now_rfc3339() -> String {
    rfc3339_utc_from_secs(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

fn invalid(field: &str) -> WatchlistError {
    WatchlistError::Invalid(format!("缺少 {field}"))
}

/// 字符串字段：缺省/null 用 default；存在但非法类型、空串或超长 → Invalid。
fn field_str(
    val: &Value,
    key: &str,
    default: Option<&str>,
    max: usize,
) -> Result<String, WatchlistError> {
    let value = match val.get(key) {
        None | Some(Value::Null) => {
            return default.map(str::to_owned).ok_or_else(|| invalid(key));
        }
        Some(value) => value,
    };
    let text = value
        .as_str()
        .ok_or_else(|| WatchlistError::Invalid(format!("{key} must be a string")))?;
    if text.is_empty() || text.chars().count() > max {
        return Err(WatchlistError::Invalid(format!(
            "{key} length must be 1..={max}"
        )));
    }
    Ok(text.to_owned())
}

fn field_i64(val: &Value, key: &str, default: i64) -> Result<i64, WatchlistError> {
    match val.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_i64()
            .ok_or_else(|| WatchlistError::Invalid(format!("{key} must be an integer"))),
    }
}

fn checked_symbol(raw: &str) -> Result<String, WatchlistError> {
    let symbol = crate::market::normalize_symbol(raw);
    if symbol.is_empty() || symbol.chars().count() > SYMBOL_MAX {
        return Err(WatchlistError::Invalid(format!(
            "symbol length must be 1..={SYMBOL_MAX}"
        )));
    }
    Ok(symbol)
}

/// 新建／更新自选组的已校验输入。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WatchlistDraft {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
}

impl WatchlistDraft {
    pub(crate) fn from_raw(val: &Value) -> Result<Self, WatchlistError> {
        Ok(Self {
            id: field_str(val, "id", Some("default"), ID_MAX)?,
            name: field_str(val, "name", Some("默认自选"), NAME_MAX)?,
            sort_order: field_i64(val, "sortOrder", 0)?,
        })
    }
}

/// 新增自选条目的已校验输入（symbol 归一化在领域内完成）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WatchlistItemDraft {
    pub watchlist_id: String,
    pub symbol: String,
    pub name: String,
    pub asset_type: String,
    pub sort_order: i64,
}

impl WatchlistItemDraft {
    pub(crate) fn from_raw(val: &Value) -> Result<Self, WatchlistError> {
        let watchlist_id = field_str(val, "watchlistId", Some("default"), ID_MAX)?;
        let raw_symbol = field_str(val, "symbol", None, RAW_SYMBOL_MAX)?;
        let symbol = checked_symbol(&raw_symbol)?;
        let name = match val.get("name") {
            None | Some(Value::Null) => symbol.clone(),
            Some(_) => field_str(val, "name", None, NAME_MAX)?,
        };
        Ok(Self {
            watchlist_id,
            symbol,
            name,
            asset_type: field_str(val, "assetType", Some("stock"), ASSET_TYPE_MAX)?,
            sort_order: field_i64(val, "sortOrder", 0)?,
        })
    }
}

/// 全量读取（组＋条目投影）；行解码错误整体失败，空库返回空集合。
pub(crate) fn list(store: &Store) -> Result<String, WatchlistError> {
    store
        .with_read(|conn| {
            let raw = projection(conn)?;
            let mut value: Value =
                serde_json::from_str(&raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
            value["digest"] = digest(&raw).into();
            Ok(value.to_string())
        })
        .map_err(|_: rusqlite::Error| WatchlistError::Db)
}
fn projection(conn: &rusqlite::Connection) -> rusqlite::Result<String> {
    let mut g_stmt = conn.prepare(
                "SELECT id, name, sort_order, created_at FROM watchlists ORDER BY sort_order ASC, created_at ASC, id",
            )?;
    let groups: Vec<String> = g_stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let sort_order: i64 = row.get(2)?;
            let created_at: String = row.get(3)?;
            Ok(format!(
                "{{\"id\":{},\"name\":{},\"sortOrder\":{},\"createdAt\":{}}}",
                crate::wire::json_str(&id),
                crate::wire::json_str(&name),
                sort_order,
                crate::wire::json_str(&created_at)
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut i_stmt = conn.prepare(
                "SELECT watchlist_id, symbol, name, asset_type, sort_order, created_at FROM watchlist_items ORDER BY sort_order ASC, created_at ASC, watchlist_id, symbol",
            )?;
    let items: Vec<String> = i_stmt
                .query_map([], |row| {
                    let wid: String = row.get(0)?;
                    let symbol: String = row.get(1)?;
                    let name: String = row.get(2)?;
                    let asset_type: String = row.get(3)?;
                    let sort_order: i64 = row.get(4)?;
                    let created_at: String = row.get(5)?;
                    Ok(format!(
                        "{{\"watchlistId\":{},\"symbol\":{},\"name\":{},\"assetType\":{},\"sortOrder\":{},\"createdAt\":{}}}",
                        crate::wire::json_str(&wid),
                        crate::wire::json_str(&symbol),
                        crate::wire::json_str(&name),
                        crate::wire::json_str(&asset_type),
                        sort_order,
                        crate::wire::json_str(&created_at)
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

    Ok(format!(
        "{{\"groups\":[{}],\"items\":[{}]}}",
        groups.join(","),
        items.join(",")
    ))
}
fn digest(projection: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(projection.as_bytes()))
}

/// 新建／更新自选组（upsert 既有语义：created_at 不被覆写）。
/// `now` 由调用方传入（生产=now_rfc3339()，测试=受控时间）。
pub(crate) fn create(
    store: &Store,
    draft: &WatchlistDraft,
    now: &str,
) -> Result<(), WatchlistError> {
    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO watchlists(id, name, sort_order, created_at)
                 VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET name=?2, sort_order=?3",
                [
                    draft.id.as_str(),
                    draft.name.as_str(),
                    &draft.sort_order.to_string(),
                    now,
                ],
            )?;
            Ok(())
        })
        .map_err(|_| WatchlistError::Db)
}

/// 新增条目（确保组存在 → upsert 条目；created_at 不被覆写）。
pub(crate) fn add_item(
    store: &Store,
    draft: &WatchlistItemDraft,
    now: &str,
) -> Result<(), WatchlistError> {
    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT OR IGNORE INTO watchlists(id, name, sort_order, created_at) VALUES(?1, '默认自选', 0, ?2)",
                [&draft.watchlist_id, now],
            )?;
            conn.execute(
                "INSERT INTO watchlist_items(watchlist_id, symbol, name, asset_type, sort_order, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(watchlist_id, symbol) DO UPDATE SET name=?3, asset_type=?4, sort_order=?5",
                [
                    draft.watchlist_id.as_str(),
                    &draft.symbol,
                    &draft.name,
                    &draft.asset_type,
                    &draft.sort_order.to_string(),
                    now,
                ],
            )?;
            Ok(())
        })
        .map_err(|_| WatchlistError::Db)
}

/// 删除条目（重复删除仍成功，与既有语义一致）。
pub(crate) fn remove_item(
    store: &Store,
    watchlist_id: &str,
    symbol: &str,
) -> Result<(), WatchlistError> {
    store
        .with_write(|conn: &rusqlite::Connection| -> rusqlite::Result<()> {
            conn.execute(
                "DELETE FROM watchlist_items WHERE watchlist_id=?1 AND symbol=?2",
                [watchlist_id, symbol],
            )?;
            Ok(())
        })
        .map_err(|_| WatchlistError::Db)
}

/// Single atomic owner for explicit group management; no financial data is touched.
pub(crate) fn manage(store: &Store, value: &Value) -> Result<(), WatchlistError> {
    let expected = field_str(value, "expectedDigest", None, 64)?;
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(WatchlistError::Invalid("expectedDigest invalid".into()));
    }
    let op = field_str(value, "op", None, 16)?;
    if op == "moveSelected" {
        return move_selected(store, value, &expected);
    }
    if op == "move" {
        let single;
        let items = if let Some(items) = value.get("items") {
            if value.as_object().is_some_and(|fields| {
                fields
                    .keys()
                    .any(|key| !["op", "target", "items", "expectedDigest"].contains(&key.as_str()))
            }) {
                return Err(invalid("move fields"));
            }
            items.as_array().ok_or_else(|| invalid("items"))?
        } else {
            single =
                vec![serde_json::json!({"watchlistId": value["id"], "symbol": value["symbol"]})];
            &single
        };
        return move_items(
            store,
            items,
            &field_str(value, "target", None, ID_MAX)?,
            &expected,
        );
    }
    let id = field_str(value, "id", None, ID_MAX)?;
    store.with_write(|conn| {
        let tx = conn;
        if digest(&projection(&tx).map_err(|_| WatchlistError::Db)?) != expected {
            return Err(WatchlistError::Conflict);
        }
        let exists = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM watchlists WHERE id=?1)",
                [&id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|_| WatchlistError::Db)?;
        if !exists {
            return Err(WatchlistError::Conflict);
        }
        let changed = match op.as_str() {
            "rename" => {
                let name = field_str(value, "name", None, NAME_MAX)?;
                tx.execute("UPDATE watchlists SET name=?2 WHERE id=?1", [&id, &name])
            }
            "delete" => {
                if id == "default" {
                    return Err(WatchlistError::Invalid(
                        "default group cannot be removed".into(),
                    ));
                }
                tx.execute("DELETE FROM watchlist_items WHERE watchlist_id=?1", [&id])
                    .map_err(|_| WatchlistError::Db)?;
                tx.execute("DELETE FROM watchlists WHERE id=?1", [&id])
            }
            "orderGroup" => {
                let order = field_i64(value, "sortOrder", 0)?;
                tx.execute(
                    "UPDATE watchlists SET sort_order=?2 WHERE id=?1",
                    rusqlite::params![id, order],
                )
            }
            "orderItem" => {
                let draft = WatchlistItemDraft::from_raw(
                    &serde_json::json!({"watchlistId":id,"symbol":value["symbol"]}),
                )?;
                let order = field_i64(value, "sortOrder", 0)?;
                tx.execute(
                    "UPDATE watchlist_items SET sort_order=?3 WHERE watchlist_id=?1 AND symbol=?2",
                    rusqlite::params![id, draft.symbol, order],
                )
            }
            _ => {
                return Err(WatchlistError::Invalid(
                    "unsupported watchlist operation".into(),
                ))
            }
        }
        .map_err(|_| WatchlistError::Db)?;
        if changed != 1 {
            return Err(WatchlistError::Conflict);
        }
        Ok(())
    })
}

// Single and bulk moves share one transaction; a conflict never leaves a partial batch.
fn move_items(
    store: &Store,
    items: &[Value],
    target: &str,
    expected: &str,
) -> Result<(), WatchlistError> {
    if items.is_empty() || items.len() > 200 {
        return Err(invalid("items"));
    }
    let mut symbols = std::collections::HashSet::new();
    let drafts = items
        .iter()
        .map(|item| {
            if !item.is_object()
                || item
                    .as_object()
                    .is_some_and(|v| v.keys().any(|k| k != "watchlistId" && k != "symbol"))
            {
                return Err(invalid("items"));
            }
            // Unlike legacy add defaults, a move must explicitly identify its source.
            let source = field_str(item, "watchlistId", None, ID_MAX)?;
            let draft = WatchlistItemDraft::from_raw(
                &serde_json::json!({"watchlistId":source,"symbol":item["symbol"]}),
            )?;
            if !symbols.insert(draft.symbol.clone()) {
                return Err(invalid("duplicate symbol"));
            }
            Ok(draft)
        })
        .collect::<Result<Vec<_>, WatchlistError>>()?;
    store.with_write(|tx| {
        if digest(&projection(tx).map_err(|_| WatchlistError::Db)?) != expected {
            return Err(WatchlistError::Conflict);
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM watchlists WHERE id=?1)",
            [target],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(WatchlistError::Conflict);
        }
        for draft in &drafts {
            let duplicate: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM watchlist_items WHERE watchlist_id=?1 AND symbol=?2)",
                rusqlite::params![target, draft.symbol],
                |row| row.get(0),
            )?;
            if duplicate {
                return Err(WatchlistError::Conflict);
            }
            let changed = tx.execute(
                "UPDATE watchlist_items SET watchlist_id=?3 WHERE watchlist_id=?1 AND symbol=?2",
                rusqlite::params![draft.watchlist_id, draft.symbol, target],
            )?;
            if changed != 1 {
                return Err(WatchlistError::Conflict);
            }
        }
        Ok(())
    })
}

// One target for the selection; All consolidates memberships, a group moves only its own.
fn move_selected(store: &Store, value: &Value, expected: &str) -> Result<(), WatchlistError> {
    if value.as_object().is_some_and(|fields| {
        fields.keys().any(|key| {
            !["op", "source", "target", "symbols", "expectedDigest"].contains(&key.as_str())
        })
    }) {
        return Err(invalid("move fields"));
    }
    let source = value["source"].as_str().ok_or_else(|| invalid("source"))?;
    if source.chars().count() > ID_MAX {
        return Err(invalid("source"));
    }
    let target = field_str(value, "target", None, ID_MAX)?;
    if source == target {
        return Err(invalid("target"));
    }
    let raw = value["symbols"]
        .as_array()
        .ok_or_else(|| invalid("symbols"))?;
    if raw.is_empty() || raw.len() > 200 {
        return Err(invalid("symbols"));
    }
    let mut seen = std::collections::HashSet::new();
    let symbols = raw
        .iter()
        .map(|symbol| {
            let text = symbol.as_str().ok_or_else(|| invalid("symbol"))?;
            if text.chars().count() > RAW_SYMBOL_MAX {
                return Err(invalid("symbol"));
            }
            let symbol = checked_symbol(text)?;
            if !seen.insert(symbol.clone()) {
                return Err(invalid("duplicate symbol"));
            }
            Ok(symbol)
        })
        .collect::<Result<Vec<_>, WatchlistError>>()?;
    store.with_write(|tx| {
        if digest(&projection(tx).map_err(|_| WatchlistError::Db)?) != expected { return Err(WatchlistError::Conflict); }
        for id in [target.as_str(),source].into_iter().filter(|id| !id.is_empty()) {
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM watchlists WHERE id=?1)",[id],|r|r.get(0))?;
            if !exists { return Err(WatchlistError::Conflict); }
        }
        for symbol in &symbols {
            use rusqlite::OptionalExtension;
            let from:Option<String>=tx.query_row("SELECT watchlist_id FROM watchlist_items WHERE symbol=?1 AND (?2='' OR watchlist_id=?2) ORDER BY sort_order,created_at,watchlist_id LIMIT 1",rusqlite::params![symbol,source],|r|r.get(0)).optional()?;
            let from=from.ok_or(WatchlistError::Conflict)?;
            let included:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM watchlist_items WHERE watchlist_id=?1 AND symbol=?2)",rusqlite::params![target,symbol],|r|r.get(0))?;
            if !included {
                let changed=tx.execute("UPDATE watchlist_items SET watchlist_id=?3 WHERE watchlist_id=?1 AND symbol=?2",rusqlite::params![from,symbol,target])?;
                if changed!=1 { return Err(WatchlistError::Conflict); }
            }
            tx.execute("DELETE FROM watchlist_items WHERE symbol=?1 AND watchlist_id!=?2 AND (?3='' OR watchlist_id=?3)",rusqlite::params![symbol,target,source])?;
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "watchlists_domain_tests.rs"]
mod tests;
