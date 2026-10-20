//! 同一快照的财务分配金额与比例投影。
use crate::group_plan::{get_plan, GroupPlanError, TENTHS_SUM};
use rusqlite::Connection;
use serde_json::{json, Value};

/// 首页分组投影：与 assets_summary 同一快照计算，不二次触库。
/// 分母 A = summary.complete_value_minor（含投资账户现金）。
/// 零 A 时占比不可计算（ratio 可计算性区分于真实零值）。
pub fn group_allocation(
    conn: &Connection,
    summary: &crate::assets::AssetsSummary,
) -> Result<Value, GroupPlanError> {
    let plan = get_plan(conn)?;
    let Some(plan) = plan else {
        return Ok(json!({
            "configured": false,
            "reason": "no_group_plan",
            "denominatorMinor": summary.complete_value_minor,
            "quality": summary.quality,
        }));
    };
    // 索引当前持有资产：fund => (account,fund)；manual => asset_id。
    // 价值 None（缺价/币种）与 0 分开；争议（overlap_review）不计入。
    let mut fund_map: std::collections::HashMap<(i64, i64), (String, String, Option<i64>)> =
        std::collections::HashMap::new();
    for f in &summary.funds {
        fund_map.insert(
            (f.account_id, f.fund_id),
            (f.fund_name.clone(), f.fund_code.clone(), f.value_minor),
        );
    }
    let mut manual_map: std::collections::HashMap<i64, (String, String, Option<i64>)> =
        std::collections::HashMap::new();
    for m in &summary.manual {
        let v = if m.overlap_review {
            None
        } else {
            Some(m.market_value_minor)
        };
        manual_map.insert(m.id, (m.name.clone(), m.product_type.clone(), v));
    }
    let mut invalid_refs: Vec<Value> = Vec::new();
    let mut extra_values = std::collections::HashMap::new();
    let mut offline_names = std::collections::HashMap::new();
    let mut stmt =
        conn.prepare("SELECT id, name, value_minor FROM offline_assets WHERE deleted_at IS NULL")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (id, name, value) = row?;
        offline_names.insert(id, name);
        extra_values.insert(("offline", id), value);
    }
    let mut cash_names = std::collections::HashMap::new();
    for m in &summary.manual {
        if m.product_type == "cash" && !m.overlap_review {
            cash_names.insert(m.account_id, m.name.clone());
            extra_values.insert(("cash", m.account_id), m.market_value_minor);
        }
    }
    let ratios: Vec<i64> = plan
        .groups
        .iter()
        .map(|g| g.ratio_tenths)
        .chain(std::iter::once(
            TENTHS_SUM - plan.groups.iter().map(|g| g.ratio_tenths).sum::<i64>(),
        ))
        .collect();
    let references = summary
        .complete_value_minor
        .map(|a| reference_amounts(a, &ratios))
        .transpose()?;
    let mut bound_total: Option<i64> = Some(0);
    let mut groups_json = Vec::with_capacity(plan.groups.len());
    for g in &plan.groups {
        let mut current: Option<i64> = Some(0);
        let mut group_assets: Vec<Value> = Vec::new();
        for (kind, ids) in [("offline", &g.offline_bindings), ("cash", &g.cash_bindings)] {
            for id in ids {
                match extra_values.get(&(kind, *id)) {
                    Some(value) => {
                        current = current
                            .map(|n| {
                                n.checked_add(*value)
                                    .ok_or_else(|| GroupPlanError::Invalid("分组金额溢出".into()))
                            })
                            .transpose()?;
                        let asset_name = if kind == "offline" {
                            offline_names
                                .get(id)
                                .cloned()
                                .unwrap_or_else(|| "线下资产".into())
                        } else {
                            cash_names
                                .get(id)
                                .cloned()
                                .unwrap_or_else(|| "现金账户".into())
                        };
                        group_assets.push(json!({
                            "name": asset_name,
                            "kind": kind,
                            "valueMinor": value,
                        }));
                    }
                    None => {
                        invalid_refs.push(json!({"groupId":g.group_id,"kind":kind,"assetId":id,"reason":"source_gone"}));
                    }
                }
            }
        }
        for (a, f) in &g.fund_bindings {
            match fund_map.get(&(*a, *f)) {
                Some((name, code, v)) => {
                    current = match (current, v) {
                        (Some(acc), Some(val)) => Some(
                            acc.checked_add(*val)
                                .ok_or_else(|| GroupPlanError::Invalid("分组金额溢出".into()))?,
                        ),
                        _ => None, // 任一引用缺价 → 该组金额未知
                    };
                    group_assets.push(json!({
                        "name": name,
                        "code": code,
                        "kind": "fund",
                        "accountId":a,"fundId":f,
                        "instrumentKind":crate::ledger::instrument_parts(code).ok().map(|p|p.0),
                        "venue":crate::ledger::instrument_parts(code).ok().map(|p|if p.0=="fund"{"otc"}else{"exchange"}),
                        "valueMinor": v,
                    }));
                }
                None => invalid_refs.push(json!({
                    "groupId": g.group_id,
                    "kind": "fund",
                    "accountId": a,
                    "fundId": f,
                    "reason": "source_gone",
                })),
            }
        }
        for id in &g.manual_bindings {
            match manual_map.get(id) {
                Some((name, ptype, v)) => {
                    current = match (current, v) {
                        (Some(acc), Some(val)) => Some(
                            acc.checked_add(*val)
                                .ok_or_else(|| GroupPlanError::Invalid("分组金额溢出".into()))?,
                        ),
                        _ => None,
                    };
                    group_assets.push(json!({
                        "name": name,
                        "kind": ptype,
                        "valueMinor": v,
                    }));
                }
                None => invalid_refs.push(json!({
                    "groupId": g.group_id,
                    "kind": "manual",
                    "assetId": id,
                    "reason": "source_gone",
                })),
            }
        }
        if let Some(cur) = current {
            bound_total = match bound_total {
                Some(acc) => Some(
                    acc.checked_add(cur)
                        .ok_or_else(|| GroupPlanError::Invalid("分组合计溢出".into()))?,
                ),
                None => None,
            };
        }
        let plan_minor = references.as_ref().map(|v| v[groups_json.len()]);
        groups_json.push(json!({
            "groupId": g.group_id,
            "name": g.name,
            "description": g.description,
            "ratioTenths": g.ratio_tenths,
            "sortOrder": g.sort_order,
            "currentMinor": current,
            "planMinor": plan_minor,
            "bindingCount": g.fund_bindings.len() + g.manual_bindings.len() + g.offline_bindings.len() + g.cash_bindings.len(),
            "assets": group_assets,
        }));
    }
    // 未归类 = A − 已归类合计（任一侧未知则未知）。
    let unassigned_minor = match (summary.complete_value_minor, bound_total) {
        (Some(a), Some(b)) => Some(a - b),
        _ => None,
    };
    // 占比 tenths：基于已确认金额；金额未知的组不输出比例。
    for (i, g) in plan.groups.iter().enumerate() {
        let current_minor = groups_json[i]["currentMinor"].as_i64();
        let ratio = match (current_minor, summary.complete_value_minor) {
            (Some(cur), Some(a)) if a > 0 => {
                Some((i128::from(cur) * i128::from(TENTHS_SUM) / i128::from(a)) as i64)
            }
            _ => None,
        };
        groups_json[i]["currentTenths"] = match ratio {
            Some(r) => json!(r),
            None => json!(null),
        };
        groups_json[i]["gapTenths"] = match (ratio, Some(g.ratio_tenths)) {
            (Some(r), _) => json!(g.ratio_tenths - r),
            _ => json!(null),
        };
    }
    Ok(json!({
        "configured": true,
        "revision": plan.revision,
        "pausedLegacyRules": plan.paused_legacy_rules,
        "denominatorMinor": summary.complete_value_minor,
        "quality": summary.quality,
        "groups": groups_json,
        "unassignedMinor": unassigned_minor,
        "unallocatedTenths": TENTHS_SUM - plan.groups.iter().map(|g| g.ratio_tenths).sum::<i64>(),
        "unallocatedMinor":references.as_ref().and_then(|v|v.last()),
        "invalidRefs": invalid_refs,
    }))
}

/// 最大余数法；未分配计划也是份额，相同余数按稳定输入顺序分配。
fn reference_amounts(total: i64, ratios: &[i64]) -> Result<Vec<i64>, GroupPlanError> {
    if total < 0 || ratios.iter().any(|r| *r < 0) || ratios.iter().sum::<i64>() != TENTHS_SUM {
        return Err(GroupPlanError::Invalid("参考金额分母或比例无效".into()));
    }
    let products: Vec<i128> = ratios
        .iter()
        .map(|r| i128::from(total) * i128::from(*r))
        .collect();
    let mut values: Vec<i64> = products
        .iter()
        .map(|v| (v / i128::from(TENTHS_SUM)) as i64)
        .collect();
    let mut order: Vec<usize> = (0..ratios.len()).collect();
    order.sort_by_key(|i| (std::cmp::Reverse(products[*i] % i128::from(TENTHS_SUM)), *i));
    for i in order
        .into_iter()
        .take((total - values.iter().sum::<i64>()) as usize)
    {
        values[i] += 1;
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_conserve_cents_and_large_amounts() {
        assert_eq!(reference_amounts(1, &[500, 500, 0]).unwrap(), vec![1, 0, 0]);
        assert_eq!(
            reference_amounts(3, &[400, 300, 300]).unwrap(),
            vec![1, 1, 1]
        );
        let values = reference_amounts(i64::MAX, &[333, 333, 334]).unwrap();
        assert_eq!(values.iter().sum::<i64>(), i64::MAX);
    }
}
