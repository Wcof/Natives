//! 规则取数在锁外，判定基于单一版本，提交事件前检查版本，失败不解除风险。
use crate::assets::{add_minor, AssetsSummary};
use crate::finance::{self, CashPosition, FinanceError};
use crate::planning::AllocationCheck;
use crate::rules::{self, ActionResult, RuleError, RuleRow};
use crate::storage::Store;
use rusqlite::params;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;

struct Evaluation {
    key: String,
    name: String,
    kind: &'static str,
    reason: String,
    value: String,
    threshold: String,
    amount: Option<i64>,
}

fn observation(
    key: &str,
    name: &str,
    kind: &'static str,
    reason: String,
    value: String,
    threshold: String,
    amount: Option<i64>,
) -> Evaluation {
    Evaluation {
        key: key.into(),
        name: name.into(),
        kind,
        reason,
        value,
        threshold,
        amount,
    }
}

pub fn run_check(store: &Store) -> Result<(usize, Vec<ActionResult>), RuleError> {
    run_check_at(store, &finance::today_cn())
}

pub fn run_check_at(store: &Store, today: &str) -> Result<(usize, Vec<ActionResult>), RuleError> {
    let (summary,cash,allocation,items,revision,lag) = store.with_snapshot(|c| -> Result<_,RuleError> {
        let summary=crate::assets::assets_summary(c,today).map_err(FinanceError::from)?;
        let cash=finance::cash_position(c)?;
        let allocation=crate::planning::check_allocation(c,&summary).map_err(FinanceError::from)?;
        let items=rules::list_rules(c,false)?;
        let month=format!("{}-01", &today[..7]);
        let net=finance::net_external_flow(c,&month,today)?;
        let checked: bool=c.query_row("SELECT EXISTS(SELECT 1 FROM value_snapshots WHERE taken_at=?1 AND quality='ready' AND json_extract(breakdown_json,'$.flowsChecked')=1)", [today], |r| r.get(0))?;
        Ok((summary,cash,allocation,items,Store::data_revision(c)?,(net,checked)))
    })?;
    let mut evaluated = Vec::new();
    let mut remote_count = 0;
    // 方案 11 §4：新分组方案激活后旧 B 偏离规则暂停执行；规则与历史事件保留。
    let paused_legacy_rules: bool = store
        .with_read(|c| {
            Ok::<_, rusqlite::Error>(
                c.query_row(
                    "SELECT paused_legacy_rules FROM group_plan WHERE id=1",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
                    == 1,
            )
        })
        .unwrap_or(false);
    for rule in items.iter().filter(|r| r.enabled) {
        if paused_legacy_rules && rule.rule_type == "allocation_drift" {
            evaluated.push((
                rule,
                Err("新资金分组方案已激活：旧五类配置偏离规则已暂停（历史事件保留，可在分组方案中查看原因）".into()),
            ));
            continue;
        }
        let params: Value = serde_json::from_str(&rule.params_json)
            .map_err(|_| RuleError::Invalid("规则参数损坏".into()))?;
        let remote = matches!(
            rule.rule_type.as_str(),
            "price_threshold" | "daily_change" | "ma_cross" | "drawdown" | "strategy_exit"
        );
        if remote {
            remote_count += 1;
        }
        let result = if remote_count > 20 && remote {
            Err("本次行情规则超过 20 项，请分组检查".into())
        } else {
            evaluate(rule, &params, &summary, &cash, &allocation, today, lag)
        };
        evaluated.push((rule, result));
    }
    store.with_write(|c| -> Result<_,RuleError> {
        if Store::data_revision(c)?!=revision { return Err(RuleError::Conflict("检查期间数据已变更，请重新检查".into())); }
        let mut count=0;
        let mut actions=Vec::new();
        for (rule,result) in evaluated {
            match result {
                Err(reason) => {
                    c.execute("UPDATE watch_events SET ready_state='unknown', last_checked_at=?2,updated_at=?2 WHERE rule_id=?1 AND status!='resolved'",params![rule.id,finance::now_secs()])?;
                    actions.push(ActionResult { result_type:"补齐数据",object_key:rule.object_key.clone(),object_name:rule.name.clone(),reason:reason.clone(),rule_type:rule.rule_type.clone(),event_id:None,amount_cap_minor:None,missing:vec![reason] });
                }
                Ok(points) => {
                    let active: Vec<&str>=points.iter().map(|p| p.key.as_str()).collect();
                    for event in rules::list_events(c,None)?.into_iter().filter(|e| e.rule_id==rule.id) {
                        if !active.contains(&event.object_key.as_str()) { rules::resolve_event(c,event.id,"条件已解除")?; }
                    }
                    for point in points {
                        let state = match rule.rule_type.as_str() { "class_unknown" => "unknown", "stale_valuation" => "stale", _ => "ready" };
                        let (id,created)=rules::record_trigger(c,rule,&point.key,&point.value,&point.threshold,state)?;
                        c.execute("UPDATE watch_events SET snapshot_id=?2,notify_result=CASE WHEN ?3 THEN 'pending' ELSE notify_result END WHERE id=?1",params![id,format!("revision:{revision}"),created])?;
                        count+=1;
                        actions.push(ActionResult {result_type:point.kind,object_key:point.key,object_name:point.name,reason:point.reason,rule_type:rule.rule_type.clone(),event_id:Some(id),amount_cap_minor:point.amount,missing:Vec::new()});
                    }
                }
            }
        }
        // 没有配置投资阈值也展示数据问题，不自行增加投资决策规则。
        if summary.quality!="ready" || cash.available_minor.is_none() {
            actions.push(ActionResult {result_type:"补齐数据",object_key:"data".into(),object_name:"组合数据".into(),reason:"请核对资产估值、类别、币种与每个现金账户预留后再模拟金额".into(),rule_type:"data_quality".into(),event_id:None,amount_cap_minor:None,missing:vec![format!("估值状态 {}；预留状态 {}",summary.quality,cash.reserve_state)]});
        }
        c.execute("INSERT INTO app_meta(key,value) VALUES('last_rule_check',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[finance::now_secs()])?;
        Ok((count,actions))
    })
}

fn evaluate(
    rule: &RuleRow,
    p: &Value,
    s: &AssetsSummary,
    cash: &CashPosition,
    a: &AllocationCheck,
    today: &str,
    lag: (i64, bool),
) -> Result<Vec<Evaluation>, String> {
    let mut out = Vec::new();
    let mut push =
        |key: &str, name: &str, kind, reason: String, value: String, threshold: String, amount| {
            out.push(observation(
                key, name, kind, reason, value, threshold, amount,
            ))
        };
    match rule.rule_type.as_str() {
        "cash_short" => {
            if cash.available_minor.is_none() {
                return Err("现金或预留未确认".into());
            }
            if let Some(v) = cash.shortfall_minor.filter(|v| *v > 0) {
                push(
                    "cash",
                    "现金",
                    "补齐数据",
                    "现金不足以覆盖预留与占用".into(),
                    v.to_string(),
                    "0".into(),
                    None,
                );
            }
        }
        "class_unknown" => {
            if s.unknown_class_count > 0 {
                push(
                    "class",
                    "资产分类",
                    "补齐数据",
                    "存在未分类资产，配置判断暂停".into(),
                    s.unknown_class_count.to_string(),
                    "0".into(),
                    None,
                );
            }
        }
        "stale_valuation" => {
            if s.old_valuation_count > 0 {
                push(
                    "valuation",
                    "资产估值",
                    "补齐数据",
                    "估值超过 7 个自然日未核对".into(),
                    s.old_valuation_count.to_string(),
                    "0".into(),
                    None,
                );
            }
        }
        "allocation_drift" => {
            if !a.computable {
                return Err("配置基数或资产数据不完整".into());
            }
            for item in &a.out_of_range {
                let row = a
                    .classes
                    .iter()
                    .find(|c| c.risk_class == item.risk_class)
                    .unwrap();
                push(
                    &format!("class:{}", item.risk_class),
                    &item.risk_class,
                    if item.direction == "above" {
                        "减仓候选"
                    } else {
                        "等待"
                    },
                    format!(
                        "配置{}；先核对具体标的约束",
                        if item.direction == "above" {
                            "超出上界"
                        } else {
                            "低于下界"
                        }
                    ),
                    item.actual_tenths.to_string(),
                    format!("{}～{}", row.lower_tenths, row.upper_tenths),
                    if item.direction == "above" {
                        row.gap_minor.map(|v| (-v).max(0))
                    } else {
                        None
                    },
                );
            }
        }
        "concentration" => {
            let b = rules::computable_base(s, cash)
                .filter(|v| *v > 0)
                .ok_or("资产或预留不足，集中度暂无法判断")?;
            let limit = percent_tenths(p["threshold"].as_f64().ok_or("缺集中度上限")?)?;
            for (key, (name, value)) in holdings(s)? {
                if !rule.object_key.is_empty()
                    && rules::normalize_symbol_key(&rule.object_key) != key
                {
                    continue;
                }
                if value as i128 * 1000 > limit as i128 * b as i128 {
                    let cap = (limit as i128 * b as i128 / 1000) as i64;
                    push(
                        &key,
                        &name,
                        "减仓候选",
                        "标的集中度超过用户上限；金额为不计费复核参考，待到账卖出资金不可用于买入"
                            .into(),
                        value.to_string(),
                        limit.to_string(),
                        Some(value - cap),
                    );
                }
            }
        }
        "investment_lag" => {
            if !lag.1 {
                return Err("本期资金流水尚未核对".into());
            }
            let due = p["dueDate"].as_str().ok_or("缺投入核对日")?;
            if today >= due {
                let amount = p["monthlyPlan"].as_f64().ok_or("缺计划投入")?;
                let plan = crate::finance::parse_amount_minor(&format!("{amount:.2}"), "计划投入")
                    .map_err(|_| "计划金额无效")?;
                if lag.0 < plan {
                    push(
                        &rule.object_key,
                        "投入计划",
                        "等待",
                        "实际净外部投入低于本期计划；估值涨跌不计作投入".into(),
                        lag.0.to_string(),
                        plan.to_string(),
                        None,
                    );
                }
            }
        }
        "price_threshold" | "daily_change" | "strategy_exit" => {
            if rule.rule_type == "strategy_exit" && p["condition"].as_str() == Some("expiry") {
                if today >= p["expiryDate"].as_str().ok_or("缺退出复核日期")? {
                    push(
                        &rule.object_key,
                        &rule.object_key,
                        "减仓候选",
                        "用户配置策略期限已到，需复核持有理由与费用".into(),
                        today.into(),
                        p["expiryDate"].as_str().unwrap().into(),
                        None,
                    );
                }
                return Ok(out);
            }
            if rule.rule_type == "strategy_exit" && p["condition"].as_str() == Some("thesis") {
                let lost = p["invalidated"].as_bool().ok_or("持有理由尚未人工核验")?;
                if lost {
                    push(
                        &rule.object_key,
                        &rule.object_key,
                        "减仓候选",
                        "用户确认持有理由失效，复核退出条件".into(),
                        "失效".into(),
                        "用户核实".into(),
                        None,
                    );
                }
                return Ok(out);
            }
            let (price, prev) = fetch_quote(&rule.object_key)?;
            let threshold = p["threshold"].as_f64().ok_or("缺阈值")?;
            let value = if rule.rule_type == "daily_change" {
                (price / prev.filter(|v| *v > 0.0).ok_or("缺有效昨收")? - 1.0) * 100.0
            } else {
                price
            };
            let direction =
                p["direction"]
                    .as_str()
                    .unwrap_or(if rule.rule_type == "strategy_exit" {
                        "below"
                    } else {
                        "above"
                    });
            let hit = match direction {
                "below" => value <= threshold,
                "absolute" => value.abs() >= threshold.abs(),
                _ => value >= threshold,
            };
            if hit {
                push(
                    &rule.object_key,
                    &rule.object_key,
                    if rule.rule_type == "strategy_exit" {
                        "减仓候选"
                    } else {
                        "等待"
                    },
                    "用户配置的价格／异动条件达到；不保证成交，不自动下单".into(),
                    value.to_string(),
                    threshold.to_string(),
                    None,
                );
            }
        }
        "ma_cross" | "drawdown" => {
            let closes = fetch_closes(&rule.object_key, today)?;
            let need = if rule.rule_type == "ma_cross" {
                61
            } else {
                p["window"].as_u64().unwrap_or(60) as usize
            };
            if closes.len() < need {
                return Err(format!(
                    "仅有 {} 根已闭合 K 线，需要 {need} 根",
                    closes.len()
                ));
            }
            let value = if rule.rule_type == "ma_cross" {
                0.0
            } else {
                *closes.last().unwrap()
                    / closes[closes.len() - need..]
                        .iter()
                        .copied()
                        .fold(0.0, f64::max)
                    - 1.0
            };
            let hit = if rule.rule_type == "ma_cross" {
                cross(&closes, p["direction"].as_str().unwrap_or("down"))
            } else {
                value * 100.0 <= -p["threshold"].as_f64().ok_or("缺回撤阈值")?
            };
            if hit {
                push(
                    &rule.object_key,
                    &rule.object_key,
                    "等待",
                    if rule.rule_type == "ma_cross" {
                        "已闭合日线 MA20/MA60 交叉，作为趋势观察".into()
                    } else {
                        "滚动回撤达到用户阈值，作为风险复核；不构成补仓条件".into()
                    },
                    value.to_string(),
                    p.to_string(),
                    None,
                );
            }
        }
        _ => return Err("规则类型不支持".into()),
    }
    Ok(out)
}

pub(crate) fn holdings(
    s: &AssetsSummary,
) -> Result<std::collections::BTreeMap<String, (String, i64)>, String> {
    let mut map = std::collections::BTreeMap::new();
    let rows = s
        .funds
        .iter()
        .filter_map(|f| {
            f.value_minor
                .map(|v| (format!("fund:{}", f.fund_code), f.fund_name.clone(), v))
        })
        .chain(
            s.manual
                .iter()
                .filter(|m| !m.overlap_review && m.product_type != "cash")
                .map(|m| {
                    (
                        m.instrument_key.clone(),
                        m.name.clone(),
                        m.market_value_minor,
                    )
                }),
        );
    for (key, name, value) in rows {
        let e = map
            .entry(rules::normalize_symbol_key(&key))
            .or_insert((name, 0));
        e.1 = add_minor(e.1, value, "标的市值").map_err(|_| "标的金额溢出")?;
    }
    Ok(map)
}

pub(crate) fn percent_tenths(v: f64) -> Result<i64, String> {
    if !v.is_finite() || !(0.0..=100.0).contains(&v) || (v * 10.0 - (v * 10.0).round()).abs() > 1e-8
    {
        return Err("比例须为 0～100，最多一位小数".into());
    }
    Ok((v * 10.0).round() as i64)
}

fn quote_symbol(key: &str) -> Result<String, String> {
    let key = key.trim_start_matches("symbol:");
    if let Some(code) = key.strip_prefix("security:SSE:") {
        return Ok(format!("sh{code}"));
    }
    if let Some(code) = key.strip_prefix("security:SZSE:") {
        return Ok(format!("sz{code}"));
    }
    if let Some(code) = key.strip_prefix("security:BSE:") {
        return Ok(format!("bj{code}"));
    }
    if key.len() == 8
        && ["sh", "sz", "bj"].iter().any(|p| key.starts_with(p))
        && key[2..].bytes().all(|b| b.is_ascii_digit())
    {
        return Ok(key.into());
    }
    Err("行情规则需要带交易所的标的，基金净值不能当股票价格".into())
}

fn fetch_quote(key: &str) -> Result<(f64, Option<f64>), String> {
    let symbol = quote_symbol(key)?;
    let req = app_runtime_core::http::HttpRequest::for_internal(
        "GET",
        &format!("/api/market/detail?symbol={symbol}"),
        Vec::new(),
    );
    let text = crate::market::api_market_detail(&req).map_err(|_| "行情暂不可用")?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "行情格式无效")?;
    let price = v["price"]
        .as_f64()
        .filter(|p| p.is_finite() && *p > 0.0)
        .ok_or("缺有效现价")?;
    Ok((
        price,
        v["prevClose"]
            .as_f64()
            .filter(|p| p.is_finite() && *p > 0.0),
    ))
}

fn fetch_closes(key: &str, today: &str) -> Result<Vec<f64>, String> {
    let symbol = quote_symbol(key)?;
    let req = app_runtime_core::http::HttpRequest::for_internal(
        "GET",
        &format!("/api/market/kline?symbol={symbol}&period=day&fq=qfq"),
        Vec::new(),
    );
    let text = crate::market::api_market_kline(&req).map_err(|_| "K 线暂不可用")?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "K 线格式无效")?;
    parse_closes(&v, today)
}

pub(crate) fn parse_closes(v: &Value, today: &str) -> Result<Vec<f64>, String> {
    let rows = v["candles"].as_array().ok_or("缺 K 线序列")?;
    let mut previous = String::new();
    let mut closes = Vec::new();
    for row in rows {
        let date = row["time"].as_str().ok_or("K 线缺日期")?;
        if !crate::assets::is_valid_calendar_date(date) || date <= previous.as_str() {
            return Err("K 线日期无效、重复或乱序".into());
        }
        previous = date.into();
        let close = row["close"]
            .as_f64()
            .filter(|c| c.is_finite() && *c > 0.0)
            .ok_or("K 线缺有效收盘价")?;
        // 当日线保守按未闭合排除；不将盘中 qfq 绝对价用于原价阈值。
        if date < today {
            closes.push(close);
        }
    }
    Ok(closes)
}

fn cross(closes: &[f64], direction: &str) -> bool {
    if closes.len() < 61 {
        return false;
    }
    let ma = |n: usize, end: usize| closes[end - n..end].iter().sum::<f64>() / n as f64;
    let t = closes.len();
    if direction == "up" {
        ma(20, t - 1) <= ma(60, t - 1) && ma(20, t) > ma(60, t)
    } else {
        ma(20, t - 1) >= ma(60, t - 1) && ma(20, t) < ma(60, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_bars_and_cross_contract() {
        let v =
            json!({"candles":[{"time":"2026-09-29","close":2},{"time":"2026-09-30","close":3}]});
        assert_eq!(parse_closes(&v, "2026-09-30").unwrap(), vec![2.0]);
        assert!(parse_closes(
            &json!({"candles":[{"time":"2026-09-29","close":0}]}),
            "2026-09-30"
        )
        .is_err());
        let mut closes = vec![100.0; 60];
        closes.push(90.0);
        assert!(cross(&closes, "down"));
        assert!(!cross(&closes, "up"));
    }

    // 方案 11 §4 回归：激活前旧 allocation_drift 正常评估；激活后暂停且原因可见、历史事件保留。
    #[test]
    fn legacy_drift_rule_pauses_after_group_plan_activation() {
        let store = crate::storage::Store::open_in_memory().unwrap();
        store
            .with_write(|c| -> rusqlite::Result<()> {
                c.execute(
                    "INSERT INTO watch_rules(name,rule_type,object_key,params_json,period,enabled,version,archived,created_at,updated_at)
                     VALUES('旧配置偏离','allocation_drift','','{}','day',1,1,0,0,0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        // 激活前：规则被评估（可能因数据不足返回"补齐数据"，但不得出现暂停文案）。
        let (_, actions_before) = run_check(&store).unwrap();
        assert!(actions_before.iter().all(|a| !a.reason.contains("已暂停")));
        // 激活新分组方案。
        crate::group_plan::save_plan_tx(
            &store,
            &crate::group_plan::GroupPlanInput {
                groups: vec![
                    crate::group_plan::GroupInput {
                        group_id: "".into(),
                        name: "权益".into(),
                        description: "权益类资产配置说明".into(),
                        ratio_tenths: 600,
                        sort_order: 0,
                        fund_bindings: vec![],
                        manual_bindings: vec![],
                        offline_bindings: vec![],
                        cash_bindings: vec![],
                    },
                    crate::group_plan::GroupInput {
                        group_id: "".into(),
                        name: "现金".into(),
                        description: "现金类流动性说明".into(),
                        ratio_tenths: 400,
                        sort_order: 1,
                        fund_bindings: vec![],
                        manual_bindings: vec![],
                        offline_bindings: vec![],
                        cash_bindings: vec![],
                    },
                ],
                expected_revision: 0,
            },
        )
        .unwrap();
        // 激活后：allocation_drift 暂停且原因可见；规则本体保留（未删除/未归档）。
        let (_, actions_after) = run_check(&store).unwrap();
        let paused = actions_after
            .iter()
            .find(|a| a.rule_type == "allocation_drift")
            .expect("paused drift rule must surface an action");
        assert!(paused.reason.contains("已暂停"));
        let rules = store
            .with_read(|c| crate::rules::list_rules(c, true))
            .unwrap();
        assert!(rules
            .iter()
            .any(|r| r.rule_type == "allocation_drift" && r.enabled));
    }
}
