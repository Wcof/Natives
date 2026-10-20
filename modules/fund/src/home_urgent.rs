//! 首页「今日要紧事」合并（R2 · A26/A32 部分）。
//!
//! 供体对照（MIT，commit 00b345b，`Dashboard.tsx` urgent useMemo）：候选 =
//! 持仓异动 ∪ 触发信号，权重 = (触发告警 ? 1000 : 0) + (持仓 ? 500 : 0) +
//! |涨跌幅|，去重后按权重取前 8。本实现把合并放在首页同一读事务内（03 契约：
//! 单一快照权威、无二次请求），信号源为 watch_events（未解除，≤50 条，最近
//! 优先），异动源为同事务已生成的机会投影（持仓复核 + 有向候选 ≤3）。
//! identity 与机会投影同构（security:{market}:{code} / fund:{code}）；无法映射
//! 证券身份的事件（如组合数据质量）保留为独立条目、不提供候选定位。

use crate::finance::FinanceError;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 输出上限与供体一致。
pub(crate) const LIMIT: usize = 8;
/// 事件读取上限（有界）。
const EVENT_LIMIT: i64 = 50;
/// 非事件候选的上限（供体 action_center 机会取前 3）。
const OPPORTUNITY_LIMIT: usize = 3;

struct Urgent {
    kind: &'static str,
    security_id: String,
    name: String,
    market: String,
    weight: i64,
    change_pct: Option<f64>,
    detail: String,
    event_id: Option<i64>,
    event_status: Option<String>,
    candidate_ref: Option<String>,
}

fn change_pct_of(item: &Value) -> Option<f64> {
    item["evidence"]["lastChangePct"]
        .as_f64()
        .filter(|p| p.is_finite())
}

fn weight_of(has_event: bool, holding: bool, pct: Option<f64>) -> i64 {
    // 供体：should_alert 1000 + has_position 500 + |change_pct|。
    (if has_event { 1000 } else { 0 })
        + (if holding { 500 } else { 0 })
        + pct.map(|p| (p.abs().round() as i64).min(999)).unwrap_or(0)
}

fn row_of(u: &Urgent) -> Value {
    json!({
        "kind": u.kind,
        "securityId": u.security_id,
        "name": u.name,
        "market": u.market,
        "weight": u.weight,
        "changePct": u.change_pct,
        "detail": u.detail,
        "eventId": u.event_id,
        "eventStatus": u.event_status,
        "candidateRef": u.candidate_ref,
    })
}

/// 同一读事务内合并要紧事；只读，不写库、不联网。
pub(crate) fn merge(conn: &Connection, opportunities: &Value) -> Result<Value, FinanceError> {
    let mut by_id: BTreeMap<String, Urgent> = BTreeMap::new();

    // 事件源：未解除事件，最近优先；规则名 join 供展示。
    let mut stmt = conn.prepare(
        "SELECT e.id,e.object_key,e.rule_type,e.trigger_value,e.status,COALESCE(r.name,'') FROM watch_events e LEFT JOIN watch_rules r ON r.id=e.rule_id WHERE e.status != 'resolved' ORDER BY e.last_checked_at DESC, e.id DESC LIMIT ?1",
    )?;
    let events = stmt.query_map([EVENT_LIMIT], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    for row in events {
        let (id, object_key, rule_type, trigger_value, status, rule_name) = row?;
        let identity = crate::rules::normalize_symbol_key(&object_key);
        let is_security = identity.starts_with("security:") || identity.starts_with("fund:");
        let candidate_ref = is_security.then(|| identity.clone());
        let security_id = if is_security {
            identity
        } else {
            format!("event:{id}")
        };
        let detail = if rule_name.is_empty() {
            format!("{rule_type} {trigger_value}")
        } else {
            format!("{rule_name} · {trigger_value}")
        };
        by_id.insert(
            security_id.clone(),
            Urgent {
                kind: "event",
                security_id,
                name: if rule_name.is_empty() {
                    rule_type
                } else {
                    rule_name
                },
                market: String::new(),
                weight: 1000,
                change_pct: None,
                detail,
                event_id: Some(id),
                event_status: Some(status),
                candidate_ref,
            },
        );
    }

    // 机会投影源：持仓复核不受限；有向候选只取 rank 前 3（供体 action_center）。
    let items = opportunities["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut ranked: Vec<&Value> = items
        .iter()
        .filter(|i| {
            matches!(
                i["action"].as_str().unwrap_or(""),
                "build_candidate" | "avoid" | "review_add" | "review_reduce"
            )
        })
        .collect();
    ranked.sort_by(|a, b| {
        b["rankScore"]
            .as_i64()
            .unwrap_or(-1)
            .cmp(&a["rankScore"].as_i64().unwrap_or(-1))
            .then_with(|| a["securityId"].as_str().cmp(&b["securityId"].as_str()))
    });
    let directional: Vec<&Value> = ranked.into_iter().take(OPPORTUNITY_LIMIT).collect();
    for item in items.iter() {
        let holding = item["isHolding"] == true;
        if !holding
            && !directional
                .iter()
                .any(|d| d["securityId"] == item["securityId"])
        {
            continue;
        }
        let Some(security_id) = item["securityId"].as_str().map(str::to_string) else {
            continue;
        };
        let pct = change_pct_of(item);
        let action = item["action"].as_str().unwrap_or("observe");
        let constraints = item["constraints"].as_array().map(|a| a.len()).unwrap_or(0);
        match by_id.get_mut(&security_id) {
            // 同身份已有事件：只补持仓权重与行情细节，事件字段保留。
            Some(existing) => {
                let holding_bonus: i64 = if holding { 500 } else { 0 };
                let pct_bonus: i64 = pct.map(|p| (p.abs().round() as i64).min(999)).unwrap_or(0);
                existing.weight += holding_bonus + pct_bonus;
                existing.kind = "event";
                if existing.name.is_empty() {
                    existing.name = item["name"].as_str().unwrap_or_default().to_string();
                }
                if holding {
                    existing.market = item["market"].as_str().unwrap_or_default().to_string();
                    existing.change_pct = pct.or(existing.change_pct);
                }
            }
            None => {
                let detail = if holding && action == "observe" && constraints > 0 {
                    format!("constraints:{constraints}")
                } else {
                    action.to_string()
                };
                let kind = if holding { "holding" } else { "opportunity" };
                let weight = weight_of(false, holding, pct);
                by_id.insert(
                    security_id.clone(),
                    Urgent {
                        kind,
                        security_id: security_id.clone(),
                        name: item["name"].as_str().unwrap_or_default().to_string(),
                        market: item["market"].as_str().unwrap_or_default().to_string(),
                        weight,
                        change_pct: pct,
                        detail,
                        event_id: None,
                        event_status: None,
                        candidate_ref: Some(security_id),
                    },
                );
            }
        }
    }

    let mut rows: Vec<Urgent> = by_id.into_values().collect();
    rows.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| a.security_id.cmp(&b.security_id))
    });
    let truncated = rows.len() > LIMIT;
    let items: Vec<Value> = rows.iter().take(LIMIT).map(row_of).collect();
    Ok(json!({
        "items": items,
        "truncated": truncated,
        "coverage": "same-snapshot-merge-of-events-and-opportunities",
        "priorityModel": "event-1000-holding-500-absolute-change",
    }))
}

#[cfg(test)]
#[path = "home_urgent_tests.rs"]
mod tests;
