//! 账户结构化接口（方案 03；R2 T03）。
//! GET /api/accounts/detail；POST /api/accounts/update|archive|restore。
//! 旧 GET/POST /api/accounts（名称数组）保持在 positions.rs 兼容路径。

use crate::accounts;
use crate::storage::Store;

fn ledger_err(e: crate::ledger::LedgerError) -> (u16, String) {
    let code = match e.code() {
        "LEDGER_CONFLICT" => 409,
        "LEDGER_NOT_FOUND" => 404,
        "LEDGER_OVERSELL" => 400,
        "LEDGER_DB" => 500,
        _ => 400,
    };
    super::api_error(code, e.code(), &e.message())
}

pub(crate) fn api_accounts_detail(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let view = crate::market::query_param(path, "view").unwrap_or_else(|| "active".into());
    if !matches!(view.as_str(), "active" | "archived" | "all") {
        return Err(super::api_error(
            400,
            "ACCOUNT_INVALID",
            "view 须为 active|archived|all",
        ));
    }
    let q = crate::market::query_param(path, "q").unwrap_or_default();
    if q.chars().count() > 64 {
        return Err(super::api_error(400, "ACCOUNT_INVALID", "q 过长"));
    }
    let parse_i64 = |key: &str, default: i64, max: i64| -> Result<i64, (u16, String)> {
        crate::market::query_param(path, key)
            .map(|v| {
                v.parse::<i64>()
                    .ok()
                    .filter(|n| *n >= 0 && *n <= max)
                    .ok_or_else(|| super::api_error(400, "ACCOUNT_INVALID", &format!("{key} 无效")))
            })
            .unwrap_or(Ok(default))
    };
    let limit = parse_i64("limit", 50, 200)?;
    let offset = parse_i64("offset", 0, i64::MAX)?;
    let (rows, total, cash) = store.with_read(
        |conn| -> Result<
            (
                Vec<accounts::AccountMeta>,
                i64,
                std::collections::HashMap<i64, i64>,
            ),
            (u16, String),
        > {
            let (rows, total) =
                accounts::list_detail(conn, &view, &q, limit, offset).map_err(ledger_err)?;
            // 现金域质量：manual_assets cash 合计（未建账与真实 0 区分）。
            let mut stmt = conn
                .prepare(
                    "SELECT account_id, SUM(market_value_minor)
                 FROM manual_assets
                 WHERE product_type = 'cash' AND deleted_at IS NULL
                 GROUP BY account_id",
                )
                .map_err(|e| ledger_err(e.into()))?;
            let cash = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| ledger_err(e.into()))?
                .collect::<Result<std::collections::HashMap<_, _>, _>>()
                .map_err(|e| ledger_err(e.into()))?;
            Ok((rows, total, cash))
        },
    )?;
    let items: Vec<String> = rows
        .iter()
        .map(|a| {
            let cash_obj = match cash.get(&a.id) {
                Some(sum) => format!(
                    "{{\"amount\":{},\"quality\":\"recorded\"}}",
                    super::json_str(&crate::fixed::Fixed::amount(*sum).to_string_value())
                ),
                None => "{\"amount\":null,\"quality\":\"unbooked\"}".into(),
            };
            format!(
                "{{\"id\":{},\"name\":{},\"version\":{},\"order\":{},\"archived\":{},\
                 \"currency\":\"CNY\",\"cash\":{}}}",
                a.id,
                super::json_str(&a.name),
                a.version,
                a.order,
                a.archived,
                cash_obj,
            )
        })
        .collect();
    Ok(format!(
        "{{\"items\":[{}],\"total\":{total},\"limit\":{limit},\"offset\":{offset}}}",
        items.join(","),
    ))
}

fn require_common(v: &serde_json::Value) -> Result<(i64, i64, String), (u16, String)> {
    let id = super::home_api::get_i64(v, "id")?;
    let expected_version = super::home_api::get_i64(v, "expectedVersion")?;
    let request_id = super::home_api::get_str(v, "requestId")?.to_string();
    Ok((id, expected_version, request_id))
}

pub(crate) fn api_account_update(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = super::home_api::body_json(body)?;
    let (id, expected_version, request_id) = require_common(&value)?;
    let name = super::home_api::get_opt_str(&value, "name");
    let order = match value.get("order") {
        None => None,
        Some(v) => Some(
            v.as_i64()
                .ok_or_else(|| super::api_error(400, "ACCOUNT_INVALID", "order 须为整数"))?,
        ),
    };
    if name.is_none() && order.is_none() {
        return Err(super::api_error(
            400,
            "ACCOUNT_INVALID",
            "至少提供 name 或 order",
        ));
    }
    store
        .with_write(|conn| {
            accounts::update_account(conn, id, name, order, expected_version, &request_id)
        })
        .map_err(ledger_err)
}

pub(crate) fn api_account_archive(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = super::home_api::body_json(body)?;
    let (id, expected_version, request_id) = require_common(&value)?;
    store
        .with_write(|conn| accounts::archive_account(conn, id, expected_version, &request_id))
        .map_err(ledger_err)
}

pub(crate) fn api_account_restore(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = super::home_api::body_json(body)?;
    let (id, expected_version, request_id) = require_common(&value)?;
    store
        .with_write(|conn| accounts::restore_account(conn, id, expected_version, &request_id))
        .map_err(ledger_err)
}

/// 拖动完成后只提交一次；任一版本冲突都会回滚整份顺序和幂等记录。
pub(crate) fn api_account_order(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = super::home_api::body_json(body)?;
    let request = super::home_api::get_str(&value, "requestId")?;
    if request.is_empty() || request.len() > 96 {
        return Err(super::api_error(
            400,
            "ACCOUNT_INVALID",
            "requestId 长度无效",
        ));
    }
    let items = value["items"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 200)
        .ok_or_else(|| super::api_error(400, "ACCOUNT_INVALID", "账户排序须为 1～200 项"))?;
    let rows: Vec<(i64, i64)> = items
        .iter()
        .map(|item| {
            let id = super::home_api::get_i64(item, "id")?;
            let version = super::home_api::get_i64(item, "expectedVersion")?;
            if id <= 0 || version <= 0 {
                return Err(super::api_error(400, "ACCOUNT_INVALID", "账户版本无效"));
            }
            Ok((id, version))
        })
        .collect::<Result<_, _>>()?;
    let ids: std::collections::BTreeSet<_> = rows.iter().map(|r| r.0).collect();
    if ids.len() != rows.len() {
        return Err(super::api_error(400, "ACCOUNT_INVALID", "账户重复"));
    }
    store
        .with_write(|conn| {
            let (active, total) = accounts::list_detail(conn, "active", "", 200, 0)?;
            if total as usize != rows.len() || active.iter().any(|a| !ids.contains(&a.id)) {
                return Err(crate::ledger::LedgerError::Conflict(
                    "账户列表已变化，请刷新".into(),
                ));
            }
            for (index, (id, version)) in rows.iter().enumerate() {
                accounts::update_account(
                    conn,
                    *id,
                    None,
                    Some(index as i64),
                    *version,
                    &format!("{request}:{id}"),
                )?;
            }
            Ok("{\"ok\":true}".into())
        })
        .map_err(ledger_err)
}
