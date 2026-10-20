//! 行动模拟预算（03 §4）：rules 拆分（架构规模门禁）。
//! 买入本金上界 = min(K, H, U, 单笔上限, 可用现金−费用)；集中度/费用约束缺失时不可计算。

use crate::finance;
use crate::rules::{computable_base, list_rules, normalize_symbol_key, RuleError};
use crate::storage::Store;
use serde::Serialize;
use serde_json::Value;

pub(crate) fn today_cn() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    crate::assets::civil_from_days((secs / 86400) as i64)
}

/// 只测算本金上界，不生成份额，也不执行交易。
pub struct SimulateBuyInput {
    pub risk_class: String,
    pub symbol: String,
    pub single_limit_text: Option<String>,
    pub fee_text: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateResult {
    pub computable: bool,
    pub reason: Option<String>,
    pub cap_minor: Option<i64>,
    pub breakdown: Vec<(&'static str, i64)>,
    pub data_revision: String,
    pub account_id: i64,
}

fn money(text: &Option<String>, label: &str, positive: bool) -> Result<Option<i64>, RuleError> {
    let Some(text) = text.as_ref().filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let value = finance::parse_amount_minor(text, label)?;
    if value < 0 || (positive && value == 0) {
        return Err(RuleError::Invalid(format!("{label}金额无效")));
    }
    Ok(Some(value))
}

pub fn simulate_buy(store: &Store, input: &SimulateBuyInput) -> Result<SimulateResult, RuleError> {
    let account = store.with_read(|c| -> Result<i64, RuleError> {
        let cash = finance::cash_position(c)?;
        let accounts: Vec<_> = cash
            .accounts
            .iter()
            .filter(|a| a.cash_minor.is_some())
            .collect();
        if accounts.len() != 1 {
            return Err(RuleError::Invalid("请指定付款账户".into()));
        }
        Ok(accounts[0].account_id)
    })?;
    Ok(simulate_basket(store, account, None, std::slice::from_ref(input))?.remove(0))
}

/// 按列表顺序共享现金、投入、类别和标的额度；整批基于同一业务版本。
pub fn simulate_basket(
    store: &Store,
    account_id: i64,
    expected: Option<&str>,
    inputs: &[SimulateBuyInput],
) -> Result<Vec<SimulateResult>, RuleError> {
    if inputs.is_empty() || inputs.len() > 20 {
        return Err(RuleError::Invalid("一次模拟 1～20 个候选".into()));
    }
    store.with_snapshot(|c| ->Result<_,RuleError> {
        let revision=Store::data_revision(c)?;
        if expected.is_some_and(|v| v!=revision) {return Err(RuleError::Conflict("数据已变化，请刷新后重新模拟".into()))}
        let s=crate::assets::assets_summary(c,&today_cn()).map_err(finance::FinanceError::from)?;
        let cash=finance::cash_position(c)?;
        let account=cash.accounts.iter().find(|a| a.account_id==account_id).ok_or_else(||RuleError::Invalid("付款账户不存在".into()))?;
        let targets=crate::planning::get_allocation(c)?;
        let rules=list_rules(c,false)?;
        let held=crate::rules_eval::holdings(&s).map_err(RuleError::Invalid)?;
        let mut classes=std::collections::BTreeMap::<String,i64>::new();
        for (class,value) in s.funds.iter().filter_map(|f|f.value_minor.map(|v|(f.risk_class.clone(),v)))
            .chain(s.manual.iter().filter(|m| !m.overlap_review).map(|m|(m.risk_class.clone(),m.market_value_minor))) {
            let entry=classes.entry(class).or_default(); *entry=crate::assets::add_minor(*entry,value,"类别市值").map_err(finance::FinanceError::from)?;
        }
        let unknown:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM cash_reservations WHERE status='held' AND risk_class='unknown')",[],|r|r.get(0))?;
        let mut available=account.available_minor;
        let mut fees=0i64; let mut spent=0i64;
        let mut queued_class=std::collections::BTreeMap::<String,i64>::new();
        let mut queued_symbol=std::collections::BTreeMap::<String,i64>::new();
        let mut out=Vec::new();
        for input in inputs {
            if !["equity","gold","fixed_income","other"].contains(&input.risk_class.as_str()) || input.symbol.trim().is_empty() || input.symbol.len()>128 {
                return Err(RuleError::Invalid("请确认非现金类别与标的身份".into()));
            }
            let key=normalize_symbol_key(&input.symbol);
            let mut result=SimulateResult {computable:false,reason:None,cap_minor:None,breakdown:Vec::new(),data_revision:revision.clone(),account_id};
            let single=money(&input.single_limit_text,"单笔上限",true)?;
            let fee=money(&input.fee_text,"费用",false)?;
            let reason=if unknown {Some("已有占用的类别待核实")} else if s.quality!="ready" || s.unknown_class_count>0 {Some("估值、币种或分类待核实")}
                else if computable_base(&s,&cash).is_none() || available.is_none() {Some("付款账户现金或预留待核实")}
                else if single.is_none() {Some("单笔上限未确认")}
                else if fee.is_none() {Some("费用未确认，未知不等于零")}
                else if targets.len()!=5 {Some("请完整配置五类目标")} else {None};
            if let Some(reason)=reason {result.reason=Some(reason.into());out.push(result);continue}
            let single=single.unwrap(); let fee=fee.unwrap();
            let next_fees=crate::assets::add_minor(fees,fee,"模拟费用").map_err(finance::FinanceError::from)?;
            let b=computable_base(&s,&cash).unwrap().checked_sub(next_fees).filter(|v|*v>0);
            let Some(b)=b else {result.reason=Some("扣费后无可配置资产".into());out.push(result);continue};
            let amounts=crate::planning::target_amounts(b,&targets);
            let target:std::collections::BTreeMap<_,_>=targets.iter().zip(amounts).map(|(t,a)|(t.risk_class.clone(),a)).collect();
            let mut h=None;
            for rule in rules.iter().filter(|r|r.enabled && r.rule_type=="concentration" && (r.object_key.is_empty() || normalize_symbol_key(&r.object_key)==key)) {
                let params:Value=serde_json::from_str(&rule.params_json).map_err(|_|RuleError::Invalid("集中度参数损坏".into()))?;
                let ratio=crate::rules_eval::percent_tenths(params["threshold"].as_f64().ok_or_else(||RuleError::Invalid("集中度缺上限".into()))?).map_err(RuleError::Invalid)?;
                let current=held.get(&key).map(|(_,v)|*v).unwrap_or(0);
                let pending:i64=c.query_row("SELECT COALESCE(SUM(amount_minor),0) FROM cash_reservations WHERE status='held' AND symbol=?1",[&input.symbol],|r|r.get(0))?;
                let room=(b as i128*ratio as i128/1000-current as i128-pending as i128-*queued_symbol.get(&key).unwrap_or(&0) as i128).max(0) as i64;
                h=Some(h.map_or(room,|old:i64|old.min(room)));
            }
            let Some(h)=h else {result.reason=Some("未确认适用的集中度上限".into());out.push(result);continue};
            let pending_class:i64=c.query_row("SELECT COALESCE(SUM(amount_minor),0) FROM cash_reservations WHERE status='held' AND risk_class=?1",[&input.risk_class],|r|r.get(0))?;
            let k=(target.get(&input.risk_class).copied().unwrap_or(0) as i128-*classes.get(&input.risk_class).unwrap_or(&0) as i128-pending_class as i128-*queued_class.get(&input.risk_class).unwrap_or(&0) as i128).max(0) as i64;
            let invested=s.complete_value_minor.unwrap()-cash.cash_minor.unwrap();
            let u=(b as i128-target.get("cash").copied().unwrap_or(0) as i128-invested as i128-cash.reserved_by_orders_minor as i128-spent as i128).max(0) as i64;
            let f=available.unwrap().checked_sub(fee).unwrap_or(i64::MIN).max(0);
            let cap=k.min(h).min(u).min(single).min(f);
            result.breakdown=vec![("类别余量K",k),("集中度余量H",h),("计划余量U",u),("单笔上限",single),("付款账户现金−费用",f)];
            result.computable=true;result.cap_minor=Some(cap);
            if cap>0 {
                available=Some(f-cap);fees=next_fees;spent=crate::assets::add_minor(spent,cap,"模拟本金").map_err(finance::FinanceError::from)?;
                *queued_class.entry(input.risk_class.clone()).or_default()+=cap;
                *queued_symbol.entry(key).or_default()+=cap;
            }
            out.push(result);
        }
        Ok(out)
    })
}
