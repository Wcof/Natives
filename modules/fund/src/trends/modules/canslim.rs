//! CANSLIM 模块（1:1 移植自 `engine/analysis/canslim_module.py`）。
//!
//! 七维度评分：C 近期动量 / A 中期趋势 / N 新高形态 / S 供需 / L 领涨强度 / I 机构资金 / M 大盘环境。
//! 算法依据：加密版常量池（_calc_c_score.._calc_m_score/_detect_cup_handle/_calc_grade）+ 基准输出反推。
//! 杯柄目标公式已验证：target = buy_point + (cup_high - cup_low)。

use crate::trends::indicators::{pct_change, sma_series};
use crate::trends::{py_round, FundFlowInput, KlineBar, QuoteInput};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CupHandleResult {
    pub pattern: String,
    pub cup_high: f64,
    pub cup_low: f64,
    pub handle_high: f64,
    pub handle_low: f64,
    pub cup_depth: f64,
    pub handle_depth: f64,
    pub breakout: bool,
    pub buy_point: f64,
    pub target: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanslimResult {
    pub c_score: i64,
    pub a_score: i64,
    pub n_score: i64,
    pub s_score: i64,
    pub l_score: i64,
    pub i_score: i64,
    pub m_score: i64,
    pub total: i64,
    pub grade: String,
    pub signals: Vec<String>,
    pub cup_handle: Option<CupHandleResult>,
    pub description: String,
}

fn calc_c_score(klines: &[KlineBar]) -> (i64, String) {
    if klines.len() < 10 {
        return (50, String::new());
    }
    let closes: Vec<f64> = klines.iter().map(|k| k.close).collect();
    let n = closes.len();
    let gain20 = if n >= 21 {
        pct_change(closes[n - 21], closes[n - 1])
    } else {
        0.0
    };
    let gain5 = pct_change(closes[n - 6], closes[n - 1]);

    let score20 = if gain20 > 20.0 {
        90
    } else if gain20 > 15.0 {
        80
    } else if gain20 > 10.0 {
        70
    } else if gain20 > 5.0 {
        60
    } else if gain20 > 0.0 {
        50
    } else if gain20 > -5.0 {
        35
    } else {
        20
    };

    let score5 = if gain5 > 5.0 {
        80
    } else if gain5 > 2.0 {
        70
    } else if gain5 > 0.0 {
        60
    } else if gain5 > -5.0 {
        50
    } else {
        35
    };

    let score = score20.max(score5);
    (score, format!("C(近期动量){}分", score))
}

fn calc_a_score(klines: &[KlineBar]) -> (i64, String) {
    if klines.len() < 125 {
        return (50, String::new());
    }
    let n = klines.len();
    let gain = pct_change(klines[n - 121].close, klines[n - 1].close);
    let score = if gain > 150.0 {
        90
    } else if gain > 30.0 {
        75
    } else if gain > 15.0 {
        70
    } else if gain > 12.0 {
        60
    } else if gain > 5.0 {
        50
    } else if gain > -15.0 {
        35
    } else {
        20
    };
    (score, format!("A(中期趋势){}分", score))
}

fn detect_cup_handle(klines: &[KlineBar]) -> Option<CupHandleResult> {
    if klines.len() < 80 {
        return None;
    }
    let window = if klines.len() > 120 {
        &klines[klines.len() - 120..]
    } else {
        klines
    };
    let lows: Vec<f64> = window.iter().map(|k| k.low).collect();

    let mut cup_low_idx = 0;
    let mut cup_low = lows[0];
    for (i, &v) in lows.iter().enumerate() {
        if v < cup_low {
            cup_low = v;
            cup_low_idx = i;
        }
    }
    if cup_low_idx < 20 || cup_low_idx > window.len() - 20 {
        return None;
    }

    let left = &window[..cup_low_idx];
    if left.len() < 10 {
        return None;
    }
    let left_start = if left.len() > 20 { left.len() - 20 } else { 0 };
    let left_recent = &left[left_start..];
    let cup_high = left_recent
        .iter()
        .map(|k| k.high)
        .fold(f64::NEG_INFINITY, f64::max);
    if cup_high <= cup_low {
        return None;
    }

    let right = &window[cup_low_idx..];
    let mut handle_high = f64::NEG_INFINITY;
    for k in right {
        if k.high > handle_high {
            handle_high = k.high;
        }
    }
    if handle_high <= cup_low {
        return None;
    }

    let ws = if klines.len() >= 20 {
        klines.len() - 20
    } else {
        0
    };
    if ws + 2 >= klines.len() {
        return None;
    }
    let handle_low = klines[ws + 2..]
        .iter()
        .map(|k| k.low)
        .fold(f64::INFINITY, f64::min);
    if handle_low <= 0.0 || !handle_low.is_finite() {
        return None;
    }

    let cup_depth = (cup_high - cup_low) / cup_high * 100.0;
    let handle_depth = (handle_high - handle_low) / handle_high * 100.0;
    if !(5.0..=35.0).contains(&cup_depth) || handle_depth > 30.0 {
        return None;
    }

    let buy_point = handle_high;
    let target = buy_point + (cup_high - cup_low);
    let start30 = if klines.len() > 30 { klines.len() - 30 } else { 0 };
    let recent_highs_max = klines[start30..]
        .iter()
        .map(|k| k.high)
        .fold(f64::NEG_INFINITY, f64::max);
    let breakout = recent_highs_max >= buy_point;

    Some(CupHandleResult {
        pattern: "杯柄形态".into(),
        cup_high: py_round(cup_high, 2),
        cup_low: py_round(cup_low, 2),
        handle_high: py_round(handle_high, 2),
        handle_low: py_round(handle_low, 2),
        cup_depth: py_round(cup_depth, 1),
        handle_depth: py_round(handle_depth, 1),
        breakout,
        buy_point: py_round(buy_point, 2),
        target: py_round(target, 2),
    })
}

fn calc_n_score(klines: &[KlineBar]) -> (i64, String, Option<CupHandleResult>) {
    if klines.len() < 60 {
        return (50, String::new(), None);
    }
    let high_250 = if klines.len() >= 100 {
        let start = if klines.len() > 250 { klines.len() - 250 } else { 0 };
        klines[start..].iter().map(|k| k.high).fold(f64::NEG_INFINITY, f64::max)
    } else {
        klines.iter().map(|k| k.high).fold(f64::NEG_INFINITY, f64::max)
    };
    let price = klines.last().unwrap().close;
    let dist = if high_250 > 0.0 {
        (high_250 - price) / high_250 * 100.0
    } else {
        100.0
    };
    let cup_handle = detect_cup_handle(klines);

    let high_120_prev = if klines.len() >= 121 {
        let start = klines.len() - 121;
        let end = klines.len() - 1;
        klines[start..end].iter().map(|k| k.high).fold(f64::NEG_INFINITY, f64::max)
    } else {
        0.0
    };

    let score = if price >= high_120_prev {
        100
    } else if dist < 3.0 {
        70
    } else if let Some(ref ch) = cup_handle {
        if ch.breakout {
            if dist < 8.0 {
                90
            } else if dist < 12.0 {
                85
            } else {
                75
            }
        } else {
            40
        }
    } else {
        40
    };

    (score, format!("N(新高/形态){}分", score), cup_handle)
}

fn calc_s_score(klines: &[KlineBar], _quote: Option<&QuoteInput>) -> (i64, String) {
    let vol_ratio = if klines.len() >= 6 {
        let last_vol = klines.last().unwrap().volume;
        let prev5_sum: f64 = klines[klines.len() - 6..klines.len() - 1].iter().map(|k| k.volume).sum();
        let prev5_avg = prev5_sum / 5.0;
        if prev5_avg > 0.0 {
            last_vol / prev5_avg
        } else {
            1.0
        }
    } else {
        1.0
    };

    let mut score = 40i64;
    if vol_ratio < 0.5 {
        score -= 2;
    } else if vol_ratio >= 2.0 {
        score += 15;
    } else if vol_ratio >= 1.5 {
        score += 5;
    }
    (score, format!("S(供需关系){}分", score))
}

fn calc_l_score(klines: &[KlineBar]) -> (i64, String) {
    if klines.len() < 95 {
        return (50, String::new());
    }
    let closes: Vec<f64> = klines.iter().map(|k| k.close).collect();
    let n = closes.len();
    let gain60 = if n >= 61 {
        pct_change(closes[n - 61], closes[n - 1])
    } else {
        0.0
    };
    let gain250 = if n >= 251 {
        pct_change(closes[n - 251], closes[n - 1])
    } else {
        pct_change(closes[0], closes[n - 1])
    };

    let base = if gain60 >= 30.0 {
        70
    } else if gain60 >= 15.0 {
        60
    } else if gain60 >= 5.0 {
        50
    } else if gain60 >= 1.0 {
        43
    } else if gain60 >= -9.0 {
        30
    } else {
        20
    };

    let adj = if gain250 > 2.5 {
        18
    } else if gain250 > 0.0 {
        13
    } else if gain250 < -30.0 {
        -5
    } else {
        0
    };

    let score = (base + adj).clamp(0, 100);
    (score, format!("L(相对强度){}分", score))
}

fn calc_i_score(flows: Option<&[FundFlowInput]>) -> (i64, String) {
    let Some(flows) = flows else {
        return (50, String::new());
    };
    if flows.is_empty() {
        return (50, String::new());
    }
    let main_nets: Vec<f64> = flows.iter().map(|f| f.main_net).collect();
    if main_nets.is_empty() {
        return (50, String::new());
    }

    let mut streak = 0;
    let recent3_start = if main_nets.len() >= 3 { main_nets.len() - 3 } else { 0 };
    for &v in main_nets[recent3_start..].iter().rev() {
        if v > 0.0 {
            streak += 1;
        } else {
            break;
        }
    }

    let start5 = if main_nets.len() >= 5 { main_nets.len() - 5 } else { 0 };
    let sum5: f64 = main_nets[start5..].iter().sum();
    let last = *main_nets.last().unwrap();

    let score = if streak >= 3 {
        85
    } else if sum5 < 0.0 {
        10
    } else if last < -5e8 {
        45
    } else if sum5 > 0.0 {
        75
    } else {
        55
    };

    (score, format!("I(机构资金){}分", score))
}

fn calc_m_score(index_klines: Option<&[KlineBar]>, stock_klines: &[KlineBar]) -> (i64, String) {
    let (src, is_index) = match index_klines {
        Some(ik) if ik.len() >= 30 => (ik, true),
        _ => (stock_klines, false),
    };
    if src.len() < 30 {
        return (50, String::new());
    }
    let closes: Vec<f64> = src.iter().map(|k| k.close).collect();
    let ma20 = sma_series(&closes, 20);
    let ma60 = sma_series(&closes, 60);
    let ma20_val = ma20.last().copied().flatten();
    let ma60_val = ma60.last().copied().flatten();

    if ma20_val.is_none() || ma60_val.is_none() {
        return (50, String::new());
    }
    let ma20_v = ma20_val.unwrap();
    let ma60_v = ma60_val.unwrap();

    let up_days20 = (closes.len() - 20..closes.len())
        .filter(|&i| closes[i] > closes[i - 1])
        .count();

    let score = if ma20_v > ma60_v && up_days20 >= 13 {
        80
    } else if ma20_v > ma60_v && up_days20 >= 7 {
        70
    } else if ma20_v > ma60_v {
        60
    } else if is_index {
        15
    } else {
        35
    };

    (score, format!("M(市场环境){}分", score))
}

fn calc_grade(score: i64) -> &'static str {
    if score >= 85 {
        "A+"
    } else if score >= 70 {
        "A"
    } else if score >= 60 {
        "B+"
    } else if score >= 50 {
        "B"
    } else if score >= 40 {
        "C+"
    } else if score >= 30 {
        "C"
    } else {
        "D"
    }
}

/// CANSLIM 综合分析主入口。
pub fn analyze_canslim(
    klines: &[KlineBar],
    quote: Option<&QuoteInput>,
    flows: Option<&[FundFlowInput]>,
    index_klines: Option<&[KlineBar]>,
) -> CanslimResult {
    let (c_score, c_text) = calc_c_score(klines);
    let (a_score, a_text) = calc_a_score(klines);
    let (n_score, n_text, cup_handle) = calc_n_score(klines);
    let (s_score, _s_text) = calc_s_score(klines, quote);
    let (l_score, l_text) = calc_l_score(klines);
    let (i_score, i_text) = calc_i_score(flows);
    let (m_score, m_text) = calc_m_score(index_klines, klines);

    // 加权总分（int 截断）：C15% A10% N25% S5% L20% I15% M10%
    let total = (0.15 * c_score as f64
        + 0.10 * a_score as f64
        + 0.25 * n_score as f64
        + 0.05 * s_score as f64
        + 0.20 * l_score as f64
        + 0.15 * i_score as f64
        + 0.10 * m_score as f64)
        .trunc() as i64;
    let grade = calc_grade(total).to_string();

    let mut signals = Vec::new();
    if c_score >= 65 {
        signals.push(c_text);
    }
    if a_score >= 65 {
        signals.push(a_text);
    }
    if n_score >= 65 {
        signals.push(n_text);
    }
    if l_score >= 70 {
        signals.push(l_text);
    }
    if i_score >= 65 {
        signals.push(i_text);
    }
    if m_score >= 70 {
        signals.push(m_text);
    }
    if m_score < 40 {
        signals.push("⚠️ 市场环境偏空，谨慎操作".into());
    }
    if i_score < 30 {
        signals.push("⚠️ 机构资金流出，注意风险".into());
    }
    if l_score < 30 {
        signals.push("⚠️ 相对强度弱势，非领涨股".into());
    }

    let description = format!(
        "综合{}分({}) | C={} A={} N={} S={} L={} I={} M={}",
        total, grade, c_score, a_score, n_score, s_score, l_score, i_score, m_score
    );

    CanslimResult {
        c_score,
        a_score,
        n_score,
        s_score,
        l_score,
        i_score,
        m_score,
        total,
        grade,
        signals,
        cup_handle,
        description,
    }
}
