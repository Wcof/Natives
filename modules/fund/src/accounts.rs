//! 账户元数据域（R2 T03 / 方案 03「账户结构化查询」）。
//!
//! accounts 在 v19 起携带 version／sort_order／archived_at：
//! - 改名／排序／归档／恢复是显式命令，必须携带 expectedVersion（乐观锁，不一致 409）
//!   与 requestId（幂等：同 ID 同载荷返回原结果，同 ID 异载荷 409）；
//! - 归档只隐藏账户于交易选择与旧名称数组，历史流水／期初／资产按 ID 保留，不物理删；
//! - ensure_account 的自动建档（按名 upsert）不推进 version，也不命中归档账户
//!   （见 ledger::ensure_account），保证旧导入链行为兼容。

use crate::ledger::LedgerError;
use rusqlite::{params, Connection, OptionalExtension};

pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_ORDER: i64 = 999_999;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMeta {
    pub id: i64,
    pub name: String,
    pub version: i64,
    pub order: i64,
    pub archived: bool,
}

fn row_to_meta(r: &rusqlite::Row) -> rusqlite::Result<AccountMeta> {
    Ok(AccountMeta {
        id: r.get(0)?,
        name: r.get(1)?,
        version: r.get(2)?,
        order: r.get(3)?,
        archived: r.get::<_, Option<String>>(4)?.is_some(),
    })
}

const META_COLS: &str = "id, name, version, sort_order, archived_at";

pub fn get_account(conn: &Connection, id: i64) -> Result<Option<AccountMeta>, LedgerError> {
    let row = conn
        .query_row(
            &format!("SELECT {META_COLS} FROM accounts WHERE id = ?1"),
            [id],
            row_to_meta,
        )
        .optional()?;
    Ok(row)
}

/// 结构化列表：view=active|archived|all；q 为名称子串过滤；(sort_order,id) 稳定排序。
pub fn list_detail(
    conn: &Connection,
    view: &str,
    q: &str,
    limit: i64,
    offset: i64,
) -> Result<(Vec<AccountMeta>, i64), LedgerError> {
    let archived_cond = match view {
        "archived" => " AND archived_at IS NOT NULL",
        "all" => "",
        _ => " AND archived_at IS NULL",
    };
    let pattern = if q.is_empty() {
        String::new()
    } else {
        format!(
            "%{}%",
            q.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    };
    let has_q = !pattern.is_empty();
    let name_cond_count = if has_q {
        " AND name LIKE ?1 ESCAPE '\\'"
    } else {
        ""
    };
    let name_cond_items = if has_q {
        " AND name LIKE ?3 ESCAPE '\\'"
    } else {
        ""
    };
    let total: i64 = if has_q {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM accounts WHERE 1=1{archived_cond}{name_cond_count}"),
            params![pattern],
            |r| r.get(0),
        )?
    } else {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM accounts WHERE 1=1{archived_cond}"),
            [],
            |r| r.get(0),
        )?
    };
    let items_sql = format!(
        "SELECT {META_COLS} FROM accounts WHERE 1=1{archived_cond}{name_cond_items} ORDER BY sort_order, id LIMIT ?1 OFFSET ?2"
    );
    let mut stmt = conn.prepare(&items_sql)?;
    let rows = if has_q {
        stmt.query_map(params![limit, offset, pattern], row_to_meta)?
    } else {
        stmt.query_map(params![limit, offset], row_to_meta)?
    }
    .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
}

/// requestId 幂等表查询：命中同载荷返回缓存结果；命中异载荷返回冲突。
fn memo_lookup(
    conn: &Connection,
    request_id: &str,
    payload_json: &str,
) -> Result<Option<String>, LedgerError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_json, result_json FROM account_ops WHERE request_id = ?1",
            params![request_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some((payload, result)) if payload == payload_json => Ok(Some(result)),
        Some(_) => Err(LedgerError::Conflict(format!(
            "requestId {request_id} 已用于不同账户命令"
        ))),
    }
}

fn memo_store(
    conn: &Connection,
    request_id: &str,
    account_id: i64,
    op: &str,
    payload_json: &str,
    result_json: &str,
) -> Result<(), LedgerError> {
    let now = crate::finance::now_secs();
    conn.execute(
        "INSERT INTO account_ops(request_id, account_id, op, payload_json, result_json, created_at)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![request_id, account_id, op, payload_json, result_json, now],
    )?;
    Ok(())
}

fn check_expected_version(current: i64, expected: i64) -> Result<(), LedgerError> {
    if current != expected {
        return Err(LedgerError::Conflict(format!(
            "账户已被他处修改（当前版本 {current}，请求版本 {expected}）"
        )));
    }
    Ok(())
}

fn validate_account_name(name: &str) -> Result<String, LedgerError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(LedgerError::Invalid(format!(
            "账户名需 1~{MAX_NAME_CHARS} 字符"
        )));
    }
    Ok(trimmed.to_string())
}

/// 改名／排序（两者可选，至少一项）。返回结果 JSON（含新 version）。
pub fn update_account(
    conn: &Connection,
    id: i64,
    name: Option<&str>,
    order: Option<i64>,
    expected_version: i64,
    request_id: &str,
) -> Result<String, LedgerError> {
    if name.is_none() && order.is_none() {
        return Err(LedgerError::Invalid("至少提供 name 或 order".into()));
    }
    if request_id.is_empty() || request_id.chars().count() > 128 {
        return Err(LedgerError::Invalid("requestId 长度须为 1..=128".into()));
    }
    let normalized_name = name.map(validate_account_name).transpose()?;
    if let Some(order) = order {
        if order < 0 || order > MAX_ORDER {
            return Err(LedgerError::Invalid(format!("order 须在 0..={MAX_ORDER}")));
        }
    }
    let payload = serde_json::json!({
        "op": "update", "id": id, "name": normalized_name, "order": order,
    })
    .to_string();
    if let Some(cached) = memo_lookup(conn, request_id, &payload)? {
        return Ok(cached);
    }
    let account =
        get_account(conn, id)?.ok_or_else(|| LedgerError::NotFound(format!("账户 {id}")))?;
    check_expected_version(account.version, expected_version)?;
    let mut changed = false;
    if let Some(new_name) = &normalized_name {
        if new_name != &account.name {
            let clash: Option<i64> = conn
                .query_row(
                    "SELECT id FROM accounts WHERE name = ?1 AND id != ?2",
                    params![new_name, id],
                    |r| r.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(LedgerError::Invalid(format!("账户名「{new_name}」已存在")));
            }
            changed = true;
        }
    }
    if order.is_some_and(|o| o != account.order) {
        changed = true;
    }
    let now = crate::finance::now_secs().to_string();
    let version = if changed {
        conn.execute(
            "UPDATE accounts SET
               name = COALESCE(?2, name),
               sort_order = COALESCE(?3, sort_order),
               version = version + 1,
               updated_at = ?4
             WHERE id = ?1",
            params![id, normalized_name, order, now],
        )?;
        account.version + 1
    } else {
        // 无实际变化：不推进版本，仍记录幂等结果。
        account.version
    };
    let result = serde_json::json!({
        "ok": true, "id": id, "version": version, "changed": changed,
    })
    .to_string();
    memo_store(conn, request_id, id, "update", &payload, &result)?;
    Ok(result)
}

/// 归档：隐藏于旧名称数组／交易选择，历史关联按 ID 保留（A13）。
pub fn archive_account(
    conn: &Connection,
    id: i64,
    expected_version: i64,
    request_id: &str,
) -> Result<String, LedgerError> {
    transition(conn, id, expected_version, request_id, true)
}

/// 恢复归档账户（不重新分配 ID，历史继续可见于结构化查询）。
pub fn restore_account(
    conn: &Connection,
    id: i64,
    expected_version: i64,
    request_id: &str,
) -> Result<String, LedgerError> {
    transition(conn, id, expected_version, request_id, false)
}

fn transition(
    conn: &Connection,
    id: i64,
    expected_version: i64,
    request_id: &str,
    to_archived: bool,
) -> Result<String, LedgerError> {
    if request_id.is_empty() || request_id.chars().count() > 128 {
        return Err(LedgerError::Invalid("requestId 长度须为 1..=128".into()));
    }
    let op = if to_archived { "archive" } else { "restore" };
    let payload = serde_json::json!({ "op": op, "id": id }).to_string();
    if let Some(cached) = memo_lookup(conn, request_id, &payload)? {
        return Ok(cached);
    }
    let account =
        get_account(conn, id)?.ok_or_else(|| LedgerError::NotFound(format!("账户 {id}")))?;
    check_expected_version(account.version, expected_version)?;
    if account.archived == to_archived {
        return Err(LedgerError::Conflict(if to_archived {
            "账户已是归档状态".into()
        } else {
            "账户未归档，无需恢复".into()
        }));
    }
    let now = crate::finance::now_secs().to_string();
    if to_archived {
        conn.execute(
            "UPDATE accounts SET archived_at=?2, version=version+1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
    } else {
        conn.execute(
            "UPDATE accounts SET archived_at=NULL, version=version+1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
    }
    let result = serde_json::json!({
        "ok": true, "id": id, "version": account.version + 1, "archived": to_archived,
    })
    .to_string();
    memo_store(conn, request_id, id, op, &payload, &result)?;
    Ok(result)
}

/// 旧名称数组兼容投影（GET /api/accounts）：仅活跃账户，(sort_order,id) 排序，
/// 全 0 sort_order 时与旧 ORDER BY id 行为一致。
pub fn active_names(conn: &Connection) -> Result<Vec<String>, LedgerError> {
    let mut stmt = conn
        .prepare("SELECT name FROM accounts WHERE archived_at IS NULL ORDER BY sort_order, id")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
