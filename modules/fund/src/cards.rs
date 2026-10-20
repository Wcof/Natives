//! cards 模块：`fund.cards.v1` 只读投影（ADR-0036 / 方案 02 §3、05 §4-§6）。
//!
//! 单一权威：投影与持仓/资产详情复用同一查询函数与定点计算；一次响应的
//! 全部字段来自同一数据库读快照（调用方在 `with_snapshot` 内持有事务），不得
//! 分别取到一次保存前后的数据。响应不含逐笔流水、账户标识、个人路径或
//! Secret；未实现的字段不返回，不伪造为零。

use crate::assets::{self, add_minor, AssetError};
use crate::fixed::Fixed;
use crate::planning;
use crate::portfolio;
use app_runtime_core::protocol::CardSnapshotResult;
use rusqlite::Connection;

pub const CARD_SCHEMA_VERSION: &str = "fund.cards.v1";

/// 投影生成时刻（RFC3339，UTC+8 北京口径；本地记账以日为粒度无外部依赖）。
fn now_rfc3339_cn() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let date = crate::assets::civil_from_days(days);
    format!(
        "{date}T{:02}:{:02}:{:02}+08:00",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// JSON 字符串字面量（复用 api 层转义）。
fn js(value: &str) -> String {
    crate::wire::json_str(value)
}

fn json(value: &str) -> String {
    js(value)
}

/// 金额对象：金额用十进制字符串（人民币分位），缺失为 null。
fn amount_obj(amount_raw: Option<i64>) -> String {
    match amount_raw {
        Some(raw) => {
            let text = Fixed::amount(raw).to_string_value();
            format!("{{\"amount\":{},\"currency\":\"CNY\"}}", js(&text))
        }
        None => "null".into(),
    }
}

/// P0：证券持仓摘要（方案 05 §4；覆盖基金、ETF 与股票）。
/// H = 当前在持（quantity_raw > 0）；空态不输出「¥0.00 总成本」；
/// 任一 cost_unknown=true 时 complete=false，只给已知部分与未知笔数。
fn holdings_summary(conn: &Connection) -> Result<String, AssetError> {
    let holdings = portfolio::holdings_with_cost_status(conn)?;
    if holdings.is_empty() {
        return Ok(format!(
            "{{\"positionCount\":0,\"accountCount\":0,\"cost\":null,\"holdingDates\":null,\"quality\":\"empty\"}}"
        ));
    }

    let mut known_cost_raw: i64 = 0;
    let mut unknown_count: usize = 0;
    let mut earliest = String::new();
    let mut latest = String::new();
    let mut account_ids: Vec<i64> = Vec::new();
    for position in &holdings {
        if position.cost_unknown {
            unknown_count += 1;
        } else {
            known_cost_raw = add_minor(known_cost_raw, position.cost_raw, "已知成本合计")?;
        }
        if !position.as_of.is_empty() {
            if earliest.is_empty() || position.as_of < earliest {
                earliest = position.as_of.clone();
            }
            if latest.is_empty() || position.as_of > latest {
                latest = position.as_of.clone();
            }
        }
        if !account_ids.contains(&position.account_id) {
            account_ids.push(position.account_id);
        }
    }

    let quality = if unknown_count > 0 {
        "incomplete"
    } else {
        "ready"
    };
    let cost = if unknown_count > 0 {
        // 成本不完整：只给已知部分，不给看似完整的总成本。
        format!(
            "{{\"complete\":false,\"known\":{},\"unknownCount\":{unknown_count}}}",
            amount_obj(Some(known_cost_raw))
        )
    } else {
        format!(
            "{{\"complete\":true,\"total\":{}}}",
            amount_obj(Some(known_cost_raw))
        )
    };
    let dates = if earliest.is_empty() {
        "null".to_string()
    } else {
        format!(
            "{{\"earliest\":{},\"latest\":{}}}",
            js(&earliest),
            js(&latest)
        )
    };
    Ok(format!(
        "{{\"positionCount\":{count},\"accountCount\":{accounts},\"cost\":{cost},\"holdingDates\":{dates},\"quality\":\"{quality}\"}}",
        count = holdings.len(),
        accounts = account_ids.len(),
    ))
}

/// P1：已记录投资资产聚合（方案 05 §4 P1）；复用与资产详情同一聚合函数。
fn assets_summary_json(summary: &assets::AssetsSummary) -> String {
    crate::assets::summary_json(summary)
}

/// P2：目标与进度（完整且新鲜才计算；空/缺/旧显式给原因）。
/// A52 单一权威：与新首页 /api/goals 读同一当前目标行
/// （archived=0，is_primary 优先，见 goals::current_goal），进度按目标范围
/// （A/B/专项）用 goals::goal_scope_value 计算；不再按旧 scope-A 过滤，
/// 也不把「未设置期限」（空 target_date）判为过期。
fn goal_json(
    conn: &Connection,
    today: &str,
    summary: &assets::AssetsSummary,
    cash_minor: Option<i64>,
) -> Result<String, AssetError> {
    let goal = crate::goals::current_goal(conn).map_err(|e| AssetError::Db(e.message()))?;
    let Some(goal) = goal else {
        return Ok("{\"configured\":false}".into());
    };
    let progress = crate::goals::goal_scope_value(conn, &goal, summary, cash_minor);
    let progress_json = if progress.computable {
        format!(
            "{{\"computable\":true,\"ratioPercent\":{ratio},\"currentValue\":{value},\"reasonCodes\":[]}}",
            ratio = progress
                .ratio_tenths
                .map(|t| json(&format!("{:.1}", t as f64 / 10.0)))
                .unwrap_or_else(|| "null".into()),
            value = progress
                .scope_value_minor
                .map(|v| amount_json(v))
                .unwrap_or_else(|| "null".into()),
        )
    } else {
        format!(
            "{{\"computable\":false,\"reasonCodes\":[{reasons}],\"ratioPercent\":null,\"currentValue\":null}}",
            reasons = progress
                .reason_codes
                .iter()
                .map(|c| json(c))
                .collect::<Vec<_>>()
                .join(","),
        )
    };
    Ok(format!(
        "{{\"configured\":true,\"targetAmount\":{amount},\"targetDate\":{date},\"expired\":{expired},\"progress\":{progress_json}}}",
        amount = amount_json(goal.target_minor),
        date = json(&goal.target_date),
        expired = planning::is_expired(&goal.target_date, today),
        progress_json = progress_json,
    ))
}

fn amount_json(raw: i64) -> String {
    format!(
        "{{\"amount\":{},\"currency\":\"CNY\"}}",
        json(&Fixed::amount(raw).to_string_value())
    )
}

/// P2：配置与检查（单一聚合同时驱动两张卡）。
fn allocation_json(
    conn: &Connection,
    summary: &assets::AssetsSummary,
) -> Result<String, AssetError> {
    let check = planning::check_allocation(conn, summary)?;
    let classes = check
        .classes
        .iter()
        .map(|c| {
            format!(
                "{{\"riskClass\":{rc},\"targetTenths\":{t},\"lowerTenths\":{lo},\"upperTenths\":{up},\"actualTenths\":{actual},\"amount\":{amount},\"status\":{status}}}",
                rc = json(&c.risk_class),
                t = c.target_tenths,
                lo = c.lower_tenths,
                up = c.upper_tenths,
                actual = c.actual_tenths.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
                amount = c.amount_minor.map(amount_json).unwrap_or_else(|| "null".into()),
                status = c.status.map(|s| json(s)).unwrap_or_else(|| "null".into()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let out_of_range = check
        .out_of_range
        .iter()
        .map(|o| {
            format!(
                "{{\"riskClass\":{rc},\"direction\":{dir},\"actualTenths\":{actual}}}",
                rc = json(&o.risk_class),
                dir = json(o.direction),
                actual = o.actual_tenths,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let reasons = check
        .reason_codes
        .iter()
        .map(|c| json(c))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"configured\":{configured},\"computable\":{computable},\"reasonCodes\":[{reasons}],\"unknownClassCount\":{unknown_cls},\"classes\":[{classes}],\"outOfRange\":[{out_of_range}]}}",
        configured = check.configured,
        computable = check.computable,
        reasons = reasons,
        unknown_cls = check.unknown_class_count,
        classes = classes,
        out_of_range = out_of_range,
    ))
}

/// 构建一次卡片投影；必须在同一读快照内完成全部查询。
/// 字段随功能阶段增量出现（方案 02 §3）；未实现字段不返回，不伪造为零。
pub fn build_snapshot(conn: &Connection) -> Result<CardSnapshotResult, AssetError> {
    if conn.is_autocommit() {
        return Err(AssetError::Invalid("卡片投影需要数据库读事务".into()));
    }
    let today = crate::assets::today_iso();
    let holdings = holdings_summary(conn)?;
    let (summary, _) = assets::summary_with_cached_quotes(conn, &today)?;
    let cash = crate::finance::cash_position(conn).map_err(|e| AssetError::Db(e.message()))?;
    let goal = goal_json(conn, &today, &summary, cash.cash_minor)?;
    let allocation = allocation_json(conn, &summary)?;
    let check = allocation.clone();
    let assets = assets_summary_json(&summary);
    let payload = format!(
        "{{\"registeredFundHoldings\":{holdings},\"recordedInvestmentAssets\":{assets},\"goal\":{goal},\"allocation\":{allocation},\"allocationCheck\":{check}}}"
    );
    let mut payload: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| AssetError::Db(e.to_string()))?;
    payload["dataRevision"] = crate::storage::Store::data_revision(conn)?.into();
    payload["cash"] = serde_json::json!({
        "cash": amount_value(cash.cash_minor),
        "reserved": amount_value(cash.reserved_minor),
        "available": amount_value(cash.available_minor),
        "shortfall": amount_value(cash.shortfall_minor),
        "reserveState": cash.reserve_state,
    });
    let groups = crate::group_plan::group_allocation(conn, &summary)
        .map_err(|e| AssetError::Db(e.message()))?;
    payload["groupAllocation"] = serde_json::json!({
        "configured": groups["configured"], "quality": groups["quality"],
        "invalidRefCount": groups["invalidRefs"].as_array().map_or(0, Vec::len),
        "unassigned": amount_value(groups["unassignedMinor"].as_i64()),
        "groups": groups["groups"].as_array().map(|rows| rows.iter().map(|row| serde_json::json!({
            "name": row["name"], "targetTenths": row["ratioTenths"],
            "actualTenths": row["currentTenths"],
            "amount": amount_value(row["currentMinor"].as_i64()),
        })).collect::<Vec<_>>()).unwrap_or_default(),
    });
    let overview = crate::overview::overview(conn, &summary, &today)
        .map_err(|e| AssetError::Db(e.message()))?;
    payload["portfolio"] = serde_json::json!({
        "basis": overview["basis"], "asOf": overview["asOf"],
        "securities": overview["securities"], "dailyReturn": overview["dailyReturn"],
        "unrealized": amount_value(overview["securities"]["unrealizedMinor"].as_i64()),
        "realized": amount_value(overview["securities"]["realizedMinor"].as_i64()),
    });
    crate::home::wire_numbers(&mut payload);
    Ok(CardSnapshotResult {
        schema: CARD_SCHEMA_VERSION.into(),
        generated_at: now_rfc3339_cn(),
        payload,
    })
}

fn amount_value(raw: Option<i64>) -> serde_json::Value {
    raw.map(
        |v| serde_json::json!({"amount": Fixed::amount(v).to_string_value(), "currency": "CNY"}),
    )
    .unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
