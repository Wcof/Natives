//! 信号引擎优化后处理（1:1 移植自 `engine/serve/signal.py` 中的 `_apply_signal_optimization`）。
//!
//! 包含：
//! - 硬否决检查（跌破MA20、价跌量增、OBV下降）
//! - 软否决检查（MA20向下、受压60日线）
//! - 分级体系重新评级
//! - M 分驱动仓位管理
//! - 盈亏比倒挂拦截与提示

use crate::trends::engine::SignalEngineResult;
use crate::trends::{py_round, KlineBar, QuoteInput};

pub fn apply_signal_optimization(
    r: &mut SignalEngineResult,
    _klines: &[KlineBar],
    _quote: Option<&QuoteInput>,
) {
    let original_action = r.action.clone();
    let score = r.score;
    let confidence = r.confidence;

    let m_score = r.canslim.as_ref().map(|c| c.m_score).unwrap_or(50);

    // ---- 1. 收集个股信号文本（排除大盘M信号）----
    let mut stock_signals = Vec::new();
    if let Some(ref t) = r.trend {
        stock_signals.extend(t.signals.clone());
    }
    if let Some(ref vp) = r.volume_price {
        stock_signals.extend(vp.signals.clone());
    }
    for s in r.buy_signals.iter().chain(r.sell_signals.iter()) {
        if s.contains("大盘") || s.contains("空头环境") || s.contains("今日") || s.contains("上证") {
            continue;
        }
        stock_signals.push(s.clone());
    }
    let all_signal_text = stock_signals.join(" ");

    // ---- 2. 硬否决检查（仅个股信号，不看大盘）----
    let hard_vetos = [
        ("跌破MA20", "价格跌破MA20，趋势已坏"),
        ("价跌量增", "价跌量增，恐慌抛售信号"),
        ("OBV下降", "OBV下降，量能走弱"),
        ("OBV走低", "OBV走低，量能走弱"),
        ("OBV下行", "OBV下行，量能走弱"),
    ];
    let mut hard_veto_reason = None;
    for (kw, desc) in hard_vetos {
        if all_signal_text.contains(kw) {
            hard_veto_reason = Some(desc);
            break;
        }
    }
    if let Some(ref vp) = r.volume_price {
        if vp.pattern.contains("价跌量增") && hard_veto_reason.is_none() {
            hard_veto_reason = Some("价跌量增，恐慌抛售信号");
        }
    }

    // ---- 3. 软否决检查 ----
    let soft_vetos = [
        ("MA20向下", "MA20向下，短期趋势偏弱"),
        ("MA20下行", "MA20下行，短期趋势偏弱"),
        ("受压60日", "受压60日决策线，上方压力大"),
    ];
    let mut soft_veto_reason = None;
    for (kw, desc) in soft_vetos {
        if all_signal_text.contains(kw) {
            soft_veto_reason = Some(desc);
            break;
        }
    }

    // ---- 4. 分级体系重新评级 ----
    let is_buy = original_action == "买入" || original_action == "强烈买入";
    let is_sell = original_action == "卖出" || original_action == "强烈卖出";
    let mut veto_reason: Option<String> = None;
    let mut action = original_action.clone();

    let scores_list = [
        r.module_scores.get("趋势").copied().unwrap_or(50),
        r.module_scores.get("CAN_SLIM").copied().unwrap_or(50),
        r.module_scores.get("突破").copied().unwrap_or(50),
        r.module_scores.get("量价").copied().unwrap_or(50),
        r.module_scores.get("形态").copied().unwrap_or(50),
    ];
    let modules_above_55 = scores_list.iter().filter(|&&s| s >= 55).count();

    if is_sell {
        // 卖出信号不拦截，顺势离场
    } else if is_buy {
        if let Some(hvr) = hard_veto_reason {
            action = "观望".to_string();
            veto_reason = Some(format!("硬否决：{}", hvr));
        } else {
            let mut new_action = if score >= 75 && confidence >= 60 && modules_above_55 >= 4 {
                "强烈买入"
            } else if score >= 65 && confidence >= 45 && modules_above_55 >= 3 {
                "买入"
            } else if score >= 60 {
                "谨慎买入"
            } else {
                "观望"
            };

            // 软否决降一级
            if let Some(svr) = soft_veto_reason {
                if new_action == "强烈买入" {
                    new_action = "买入";
                    veto_reason = Some(format!("软否决：{}", svr));
                } else if new_action == "买入" {
                    new_action = "谨慎买入";
                    veto_reason = Some(format!("软否决：{}", svr));
                }
            }
            action = new_action.to_string();
        }
    }

    // ---- 5. M分驱动仓位管理 ----
    let original_position = r
        .trade_plan
        .get("position_size")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let position_advice = if action == "买入" || action == "强烈买入" || action == "谨慎买入" {
        if m_score < 40 {
            if action == "强烈买入" {
                action = "买入".to_string();
                let note = format!("大盘M分{}偏低，降级为买入", m_score);
                veto_reason = Some(match veto_reason {
                    Some(prev) => format!("{}；{}", prev, note),
                    None => note,
                });
            } else if action == "买入" {
                action = "谨慎买入".to_string();
                let note = format!("大盘M分{}偏低，降级为谨慎买入", m_score);
                veto_reason = Some(match veto_reason {
                    Some(prev) => format!("{}；{}", prev, note),
                    None => note,
                });
            }
            "轻仓(1/4) — 大盘偏空，严格控制仓位".to_string()
        } else if m_score < 55 {
            "半仓(1/2) — 大盘中性偏弱".to_string()
        } else if m_score < 65 {
            if !original_position.is_empty() {
                original_position.clone()
            } else {
                "半仓(1/2)".to_string()
            }
        } else if !original_position.is_empty() {
            original_position.clone()
        } else {
            "正常仓位".to_string()
        }
    } else {
        "空仓等待".to_string()
    };

    // ---- 6. 盈亏比检查 ----
    let entry = r
        .trade_plan
        .get("entry_price")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let stop = r
        .trade_plan
        .get("stop_loss")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let target = r
        .trade_plan
        .get("target_price")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let mut risk_reward = r
        .trade_plan
        .get("risk_reward_ratio")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let mut risk_notes = Vec::new();
    if entry > 0.0 && stop > 0.0 && target > 0.0 {
        if risk_reward == 0.0 {
            let risk_amt = entry - stop;
            let reward_amt = target - entry;
            if risk_amt > 0.0 {
                risk_reward = py_round(reward_amt / risk_amt, 1);
            }
        }
        if risk_reward > 0.0 {
            let rr_str = if risk_reward.fract() == 0.0 {
                format!("{:.1}", risk_reward)
            } else {
                format!("{}", risk_reward)
            };
            if risk_reward < 1.0 {
                risk_notes.push(format!("盈亏比{}倒挂，不建议入场", rr_str));
                if action == "买入" || action == "强烈买入" || action == "谨慎买入" {
                    action = "观望".to_string();
                    let note = format!("盈亏比{}倒挂", rr_str);
                    veto_reason = Some(match veto_reason {
                        Some(prev) => format!("{}；{}", prev, note),
                        None => note,
                    });
                }
            } else if risk_reward < 1.5 {
                risk_notes.push(format!("盈亏比{}偏低，谨慎操作", rr_str));
            } else if risk_reward < 2.0 {
                risk_notes.push(format!("盈亏比{}，勉强达标", rr_str));
            } else {
                risk_notes.push(format!("盈亏比{}，风险收益比良好", rr_str));
            }
        }
    }

    // ---- 7. 写回数据 ----
    r.action = action.clone();
    r.optimized_action = Some(action.clone());
    r.original_action = Some(original_action.clone());
    r.position_advice = Some(position_advice.clone());
    r.risk_notes = risk_notes;
    r.risk_reward = risk_reward;
    r.veto_reason = veto_reason.clone();

    r.trade_plan
        .insert("position_size".into(), serde_json::json!(position_advice));

    if let Some(ref vr) = veto_reason {
        r.risk_warnings.insert(0, vr.clone());
    }

    if let Some(ref vr) = veto_reason {
        if r.action != original_action {
            let prefix = format!("[优化：{}→{}] {}。", original_action, r.action, vr);
            r.plain_summary = prefix + &r.plain_summary;
        }
    }
}
