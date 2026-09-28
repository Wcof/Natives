//! 信号引擎（1:1 移植自 `engine/analysis/signal_engine.py`）。
//!
//! 五模块聚合与决策：
//! 综合分 = int(趋势×25% + CANSLIM×20% + 突破×20% + 量价×20% + 形态×15%)
//! 形态分 = 50 + Σ(方向 × confidence × 0.2)
//! 风险等级、信号强度、交易计划与大白话总结生成。

use crate::trends::modules::breakout::{analyze_breakout, BreakoutResult};
use crate::trends::modules::canslim::{analyze_canslim, CanslimResult};
use crate::trends::modules::pattern::{analyze_patterns, PatternResult};
use crate::trends::modules::trend::{analyze_trend, TrendResult};
use crate::trends::modules::volume_price::{analyze_volume_price, VolumePriceResult};
use crate::trends::{py_round, FundFlowInput, KlineBar, QuoteInput};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const RISK_HEAVY: i64 = 5;
pub const RISK_MEDIUM: i64 = 3;
pub const STRONG_SCORE: i64 = 75;
pub const MEDIUM_SCORE: i64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalEngineResult {
    pub action: String,
    pub score: i64,
    pub confidence: i64,
    pub risk_level: String,
    pub signal_strength: String,
    pub trend: Option<TrendResult>,
    pub patterns: Vec<PatternResult>,
    pub volume_price: Option<VolumePriceResult>,
    pub breakouts: Vec<BreakoutResult>,
    pub canslim: Option<CanslimResult>,
    pub module_scores: BTreeMap<String, i64>,
    pub buy_signals: Vec<String>,
    pub sell_signals: Vec<String>,
    pub risk_warnings: Vec<String>,
    pub key_levels: BTreeMap<String, f64>,
    pub description: String,
    pub plain_summary: String,
    pub trade_plan: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub original_action: Option<String>,
    #[serde(default)]
    pub optimized_action: Option<String>,
    #[serde(default)]
    pub veto_reason: Option<String>,
    #[serde(default)]
    pub position_advice: Option<String>,
    #[serde(default)]
    pub risk_notes: Vec<String>,
    #[serde(default)]
    pub risk_reward: f64,
}

fn trend_to_score(trend: &TrendResult) -> i64 {
    trend.strength
}

fn pattern_to_score(patterns: &[PatternResult]) -> i64 {
    let mut total = 50.0;
    for p in patterns {
        let sign = if p.direction == "看涨" {
            1.0
        } else if p.direction == "看跌" {
            -1.0
        } else {
            0.0
        };
        total += sign * p.confidence as f64 * 0.2;
    }
    (total.trunc() as i64).clamp(20, 100)
}

fn volume_price_to_score(vp: &VolumePriceResult) -> i64 {
    if vp.direction == "看涨" {
        vp.confidence
    } else if vp.direction == "看跌" {
        (100 - vp.confidence).max(20)
    } else {
        50
    }
}

fn breakout_to_score(breakouts: &[BreakoutResult]) -> i64 {
    let mut score = 50i64;
    let mut has_signal = false;
    let mut has_short_cover = false;
    for b in breakouts {
        if b.signal == "持仓" || b.signal == "持仓空头" || b.signal == "多头止损" {
            has_signal = true;
        }
        if b.signal == "空头平仓" {
            has_short_cover = true;
        }
    }
    if has_signal {
        score = 60;
    } else if has_short_cover {
        score = 60;
    }
    if has_short_cover {
        score += 3;
    }
    score.min(100)
}

fn calc_risk_level(
    score: i64,
    trend: &TrendResult,
    vp: &VolumePriceResult,
    canslim: &CanslimResult,
    breakouts: &[BreakoutResult],
) -> (String, String) {
    let mut risk_points = 0;
    if trend.direction == "下降" {
        risk_points += 2;
    }
    if vp.direction == "看跌" {
        risk_points += 2;
    }
    if canslim.m_score < 30 {
        risk_points += 1;
    }
    if breakouts.iter().any(|b| b.signals.iter().any(|s| s.contains("止损"))) {
        risk_points += 1;
    }

    let risk_level = if risk_points >= RISK_HEAVY {
        "高".to_string()
    } else if risk_points >= RISK_MEDIUM {
        "中".to_string()
    } else {
        "低".to_string()
    };

    let strength = if score >= STRONG_SCORE {
        "强".to_string()
    } else if score >= MEDIUM_SCORE {
        "中".to_string()
    } else {
        "弱".to_string()
    };

    (risk_level, strength)
}

fn build_trade_plan(
    action: &str,
    score: i64,
    patterns: &[PatternResult],
    breakouts: &[BreakoutResult],
    klines: &[KlineBar],
) -> BTreeMap<String, serde_json::Value> {
    let entry = klines.last().map(|k| k.close).unwrap_or(0.0);
    let stop = py_round(entry * 0.95, 2);

    let priority_map = |name: &str| -> i32 {
        match name {
            "头肩底" => 0,
            "双底" => 1,
            "箱体" => 2,
            _ => 9,
        }
    };
    let mut ordered: Vec<&PatternResult> = patterns
        .iter()
        .filter(|p| p.direction == "看涨" && p.target_price.is_some())
        .collect();
    ordered.sort_by_key(|p| priority_map(&p.name));

    let mut target = ordered
        .iter()
        .find_map(|p| p.target_price.filter(|&t| t > entry));
    if target.is_none() {
        for p in patterns {
            if let Some(&box_upper) = p.key_levels.get("箱体上沿") {
                target = Some(box_upper);
                break;
            }
        }
    }
    let target = target.unwrap_or(entry * 1.10);

    let risk_amt = entry - stop;
    let reward_amt = target - entry;
    let risk_reward = if risk_amt > 0.0 {
        py_round(reward_amt / risk_amt, 1)
    } else {
        0.0
    };
    let max_loss_pct = 5.0;

    let position_size = if action == "观望" {
        "空仓等待"
    } else if score >= STRONG_SCORE {
        "正常仓位"
    } else {
        "半仓(1/2)"
    };

    let holding_period = "中线(1-3月)";

    let mut notes = Vec::new();
    for b in breakouts {
        if b.signal == "持仓" && b.entry_price.is_some() {
            let ep = b.entry_price.unwrap();
            let ep_str = if ep.fract() == 0.0 {
                format!("{:.1}", ep)
            } else {
                format!("{}", ep)
            };
            let mut note = format!("{}：持仓中(突破价{})，止损{:.2}", b.system, ep_str, b.stop_loss);
            if let Some(nap) = b.next_add_price {
                note.push_str(&format!("；加仓价{:.2}", nap));
            }
            notes.push(note);
            break;
        }
    }

    let mut plan = BTreeMap::new();
    plan.insert("action".into(), serde_json::json!(action));
    plan.insert("entry_price".into(), serde_json::json!(py_round(entry, 2)));
    plan.insert("stop_loss".into(), serde_json::json!(py_round(stop, 2)));
    plan.insert("target_price".into(), serde_json::json!(py_round(target, 2)));
    plan.insert("position_size".into(), serde_json::json!(position_size));
    plan.insert("holding_period".into(), serde_json::json!(holding_period));
    plan.insert("risk_reward_ratio".into(), serde_json::json!(risk_reward));
    plan.insert("max_loss_pct".into(), serde_json::json!(max_loss_pct));
    plan.insert("notes".into(), serde_json::json!(notes.join("；")));
    plan
}

fn build_plain_summary(
    action: &str,
    trend: &TrendResult,
    patterns: &[PatternResult],
    vp: &VolumePriceResult,
    canslim: &CanslimResult,
    plan: &BTreeMap<String, serde_json::Value>,
) -> String {
    let has_head_shoulder = patterns.iter().any(|p| p.name == "头肩底");
    let has_flow_out = vp.signals.iter().any(|s| s.contains("流出"));
    let volume_price_ok = vp.direction == "看涨" && vp.confidence >= 70;

    if action == "观望" {
        let mut desc_parts = vec![format!("处于{}趋势", trend.direction)];
        if has_flow_out {
            desc_parts.push("主力资金流出".into());
        }
        if canslim.m_score < 30 {
            desc_parts.push("大盘环境偏空".into());
        }
        return format!("建议观望，{}。建议耐心等待信号明确后再操作。", desc_parts.join("，"));
    }

    let trend_desc = if trend.strength >= 70 {
        "强势上升趋势"
    } else {
        "上升趋势"
    };
    let mut desc_parts = vec![format!("处于{}", trend_desc)];
    if canslim.m_score < 30 {
        desc_parts.push("⚠️大盘偏空".into());
    }
    if has_head_shoulder {
        desc_parts.push("头肩底形态确认".into());
    }
    if volume_price_ok {
        desc_parts.push("量价配合良好".into());
    }

    let entry = plan.get("entry_price").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let stop = plan.get("stop_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let target = plan.get("target_price").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let rr = plan.get("risk_reward_ratio").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let pos = plan.get("position_size").and_then(|v| v.as_str()).unwrap_or("");

    format!(
        "出现买入信号，{}。建议{}入场，买入价{:.2}，止损{:.2}，目标{:.2}（盈亏比{}）。",
        desc_parts.join("，"),
        pos,
        entry,
        stop,
        target,
        rr
    )
}

/// 信号引擎主分析入口。
pub fn run_analysis(
    klines: &[KlineBar],
    quote: Option<&QuoteInput>,
    flows: Option<&[FundFlowInput]>,
    index_klines: Option<&[KlineBar]>,
) -> SignalEngineResult {
    let trend = analyze_trend(klines);
    let patterns = analyze_patterns(klines);
    let vp = analyze_volume_price(klines, quote, flows);
    let breakouts = analyze_breakout(klines);
    let canslim = analyze_canslim(klines, quote, flows, index_klines);

    let trend_score = trend_to_score(&trend);
    let pattern_score = pattern_to_score(&patterns);
    let vp_score = volume_price_to_score(&vp);
    let breakout_score = breakout_to_score(&breakouts);
    let canslim_score = canslim.total;

    let mut module_scores = BTreeMap::new();
    module_scores.insert("CAN_SLIM".into(), canslim_score);
    module_scores.insert("形态".into(), pattern_score);
    module_scores.insert("突破".into(), breakout_score);
    module_scores.insert("趋势".into(), trend_score);
    module_scores.insert("量价".into(), vp_score);

    // 综合分 = int(趋势25% + CAN20% + 突破20% + 量价20% + 形态15%)
    let score = (trend_score as f64 * 0.25
        + canslim_score as f64 * 0.20
        + breakout_score as f64 * 0.20
        + vp_score as f64 * 0.20
        + pattern_score as f64 * 0.15)
        .trunc() as i64;

    let action = if score >= 75 {
        "强烈买入"
    } else if score >= 60 {
        "买入"
    } else {
        "观望"
    };

    let qualified_count = module_scores.values().filter(|&&s| s >= 60).count() as i64;
    let confidence = (score as f64 * 0.8).trunc() as i64 + 12 * qualified_count - 40;
    let confidence = confidence.max(10);

    let (risk_level, signal_strength) =
        calc_risk_level(score, &trend, &vp, &canslim, &breakouts);

    // ---- 信号聚合 ----
    let mut buy_signals = Vec::new();
    let mut sell_signals = Vec::new();

    if trend.strength >= 65 {
        buy_signals.push(format!("趋势强势上升({}分)", trend.strength));
    } else if trend.strength >= 45 {
        buy_signals.push(format!("趋势上升({}分)", trend.strength));
    }
    for sig in &trend.signals {
        if !buy_signals.contains(sig) && !sig.starts_with("MA20") {
            buy_signals.push(sig.clone());
        }
    }

    for p in &patterns {
        if p.name == "头肩底" && p.direction == "看涨" {
            buy_signals.push(format!("{}({})", p.name, p.status));
        }
    }

    if vp.direction == "看涨" && vp.confidence >= 60 {
        buy_signals.push(format!("量价{}({}分)", vp.pattern, vp.confidence));
    }
    for sig in &vp.signals {
        if sig.contains("流出") {
            sell_signals.push(sig.clone());
        } else if sig.contains("净流入") {
            buy_signals.push(sig.clone());
        }
    }

    for b in &breakouts {
        if b.signal == "持仓" && b.entry_price.is_some() {
            buy_signals.push(format!(
                "{}持仓(N={:.2}，止损{:.2})",
                b.system, b.current_n, b.stop_loss
            ));
        } else if b.signal == "空头平仓" {
            buy_signals.push(format!("{}空头平仓@{}(偏多)", b.system, b.breakout_price));
        } else if b.signal == "多头止损" {
            sell_signals.push(format!("{}多头止损@{:.2}", b.system, b.stop_loss));
        }
    }

    for sig in &canslim.signals {
        if sig.contains("⚠️") {
            sell_signals.push(sig.clone());
        } else if !sig.starts_with("M(") {
            buy_signals.push(sig.clone());
        }
    }

    // ---- 风险提示 ----
    let mut risk_warnings = Vec::new();
    if canslim.m_score < 30 {
        risk_warnings.push("市场环境偏空".into());
    }
    if vp.direction == "看跌" {
        risk_warnings.push("量价配合不佳".into());
    }
    if trend.direction == "下降" {
        risk_warnings.push("处于下降趋势".into());
    }

    // ---- 关键价位 ----
    let mut key_levels = BTreeMap::new();
    let mut primary_pattern: Option<&PatternResult> = None;
    for prefix in &["头肩", "双底", "箱体"] {
        for p in &patterns {
            if p.name.starts_with(prefix) {
                primary_pattern = Some(p);
                break;
            }
        }
        if primary_pattern.is_some() {
            break;
        }
    }
    if let Some(p) = primary_pattern {
        for (label, &val) in &p.key_levels {
            key_levels.insert(format!("{}_{}", p.name, label), py_round(val, 2));
        }
    }
    for b in &breakouts {
        if b.stop_loss > 0.0 {
            key_levels.insert(format!("{}_止损", b.system), b.stop_loss);
        }
    }
    if let Some(ref ch) = canslim.cup_handle {
        key_levels.insert("杯柄买点".into(), ch.buy_point);
    }
    if let Some(ref tl) = trend.trendline {
        key_levels.insert("趋势线".into(), tl.current_price);
    }

    let trade_plan = build_trade_plan(action, score, &patterns, &breakouts, klines);

    let desc_parts = vec![
        format!("综合{}分", score),
        format!("趋势={}({})", trend.direction, trend_score),
        format!("量价={}({})", vp.pattern, vp_score),
        format!("突破={}", breakout_score),
        format!("CS={}({})", canslim.grade, canslim_score),
        format!("形态={}", pattern_score),
    ];
    let description = desc_parts.join(" | ");

    let plain_summary =
        build_plain_summary(action, &trend, &patterns, &vp, &canslim, &trade_plan);

    SignalEngineResult {
        action: action.into(),
        score,
        confidence,
        risk_level,
        signal_strength,
        trend: Some(trend),
        patterns,
        volume_price: Some(vp),
        breakouts,
        canslim: Some(canslim),
        module_scores,
        buy_signals,
        sell_signals,
        risk_warnings,
        key_levels,
        description,
        plain_summary,
        trade_plan,
        original_action: None,
        optimized_action: None,
        veto_reason: None,
        position_advice: None,
        risk_notes: Vec::new(),
        risk_reward: 0.0,
    }
}
