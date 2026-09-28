//! 量价模块（1:1 移植自 `engine/analysis/volume_price_module.py`）。
//!
//! 功能：量价模式识别（量价齐升/缩量回调/放量滞涨等）、OBV 能量潮、资金流辅助。
//! 算法依据：加密版常量池（_calc_obv/_classify_price_volume/_analyze_fund_flow）+ 基准输出反推。

use crate::trends::indicators::ma_direction;
use crate::trends::{py_round, FundFlowInput, KlineBar, QuoteInput};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VolumePriceResult {
    pub pattern: String,
    pub direction: String,
    pub confidence: i64,
    pub volume_ratio: f64,
    pub turnover: f64,
    pub obv_trend: String,
    pub signals: Vec<String>,
    pub description: String,
}

pub fn calc_obv(klines: &[KlineBar]) -> Vec<f64> {
    let mut obv = Vec::with_capacity(klines.len());
    let mut running = 0.0;
    let mut prev_close: Option<f64> = None;
    for k in klines {
        if let Some(pc) = prev_close {
            if k.close != pc {
                if k.close > pc {
                    running += k.volume;
                } else {
                    running -= k.volume;
                }
            }
        }
        obv.push(running);
        prev_close = Some(k.close);
    }
    obv
}

fn classify_price_volume(klines: &[KlineBar]) -> (String, String, i64) {
    if klines.len() < 10 {
        return ("数据不足".into(), "中性".into(), 50);
    }
    let n = klines.len();
    let c_last = klines[n - 1].close;
    let c_prev8 = klines[n - 8].close;
    let g8 = if c_prev8 > 0.0 {
        (c_last - c_prev8) / c_prev8 * 100.0
    } else {
        0.0
    };

    let price_dir = if g8 > 2.0 {
        "涨"
    } else if g8 < -2.0 {
        "跌"
    } else {
        "平"
    };

    // 量方向：最近3日均量 vs 前5日均量(不含最近3日)
    let vol_change = if n >= 8 {
        let ma3_vol = (klines[n - 3].volume + klines[n - 2].volume + klines[n - 1].volume) / 3.0;
        let ma5_prev = klines[n - 8..n - 3].iter().map(|k| k.volume).sum::<f64>() / 5.0;
        if ma5_prev > 0.0 {
            (ma3_vol - ma5_prev) / ma5_prev * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let vol_dir = if vol_change > 30.0 {
        "增"
    } else if vol_change < -30.0 {
        "缩"
    } else {
        "平"
    };

    let pattern = format!("价{}量{}", price_dir, vol_dir);
    let direction = if price_dir == "涨" {
        "看涨".to_string()
    } else if price_dir == "跌" {
        "看跌".to_string()
    } else {
        "中性".to_string()
    };

    let base_conf = match pattern.as_str() {
        "价涨量增" => 80,
        "价涨量平" => 55,
        "价涨量缩" => 60,
        "价平量增" => 50,
        "价平量平" => 20,
        "价平量缩" => 35,
        "价跌量增" => 75,
        "价跌量平" => 50,
        "价跌量缩" => 60,
        _ => 50,
    };

    (pattern, direction, base_conf)
}

fn analyze_fund_flow(flows: Option<&[FundFlowInput]>) -> (String, i64) {
    let Some(flows) = flows else {
        return (String::new(), 0);
    };
    if flows.is_empty() {
        return (String::new(), 0);
    }
    let start = if flows.len() >= 3 { flows.len() - 3 } else { 0 };
    let recent = &flows[start..];
    let main_nets: Vec<f64> = recent.iter().map(|f| f.main_net).collect();
    if main_nets.is_empty() {
        return (String::new(), 0);
    }

    if main_nets.len() == 3 && main_nets.iter().all(|&v| v > 0.0) {
        return ("连续3日主力净流入".into(), 15);
    }
    if main_nets.len() == 3 && main_nets.iter().all(|&v| v < 0.0) {
        return ("连续3日主力净流出".into(), -15);
    }

    if main_nets.len() >= 2 {
        let last_net = *main_nets.last().unwrap();
        let prev_avg = (main_nets[0] + main_nets[1]) / 2.0;
        if prev_avg >= 0.0 {
            let threshold = 0.5 * prev_avg;
            if last_net < -threshold {
                return ("今日主力大幅流出".into(), -10);
            }
            if last_net > threshold {
                return ("今日主力大幅流入".into(), 10);
            }
        }
    }
    ("主力资金温和".into(), 0)
}

fn detect_limit_up_volume(klines: &[KlineBar]) -> Option<String> {
    if klines.len() < 6 {
        return None;
    }
    let latest = klines.last().unwrap();
    let prev5 = &klines[klines.len() - 6..klines.len() - 1];
    let avg_vol = prev5.iter().map(|k| k.volume).sum::<f64>() / prev5.len() as f64;
    if latest.pct >= 9.5 && avg_vol > 0.0 && latest.volume > avg_vol * 1.5 {
        return Some(format!("放量涨停(pct={:.1}%)", latest.pct));
    }
    None
}

fn detect_volume_breakout(klines: &[KlineBar]) -> Option<String> {
    if klines.len() < 20 {
        return None;
    }
    let latest = klines.last().unwrap();
    let start = if klines.len() >= 21 { klines.len() - 21 } else { 0 };
    let window = &klines[start..klines.len() - 1];
    let avg_vol = window.iter().map(|k| k.volume).sum::<f64>() / window.len() as f64;
    let max_vol = window.iter().map(|k| k.volume).fold(f64::NEG_INFINITY, f64::max);
    if latest.volume > avg_vol * 1.5 && latest.volume > max_vol {
        return Some("量能突破，资金活跃".into());
    }
    None
}

/// 量价综合分析主入口。
pub fn analyze_volume_price(
    klines: &[KlineBar],
    quote: Option<&QuoteInput>,
    flows: Option<&[FundFlowInput]>,
) -> VolumePriceResult {
    let (pattern, direction, base) = classify_price_volume(klines);

    let volume_ratio = if let Some(q) = quote {
        if klines.len() >= 6 {
            let prev5 = &klines[klines.len() - 6..klines.len() - 1];
            let avg5 = prev5.iter().map(|k| k.volume).sum::<f64>() / prev5.len().max(1) as f64;
            if avg5 > 0.0 {
                py_round(q.volume / avg5, 2)
            } else {
                1.0
            }
        } else {
            1.0
        }
    } else {
        1.0
    };

    let turnover = if let Some(q) = quote {
        if q.turnover > 0.0 {
            q.turnover
        } else {
            klines.last().map(|k| k.turnover).unwrap_or(0.0)
        }
    } else {
        klines.last().map(|k| k.turnover).unwrap_or(0.0)
    };

    let obv = calc_obv(klines);
    let obv_opt: Vec<Option<f64>> = obv.into_iter().map(Some).collect();
    let obv_dir = ma_direction(&obv_opt, 8);
    let obv_trend = match obv_dir {
        "向上" => "上升".to_string(),
        "向下" => "下降".to_string(),
        _ => "走平".to_string(),
    };

    let (fund_text, fund_delta) = analyze_fund_flow(flows);
    let mut confidence = base;
    if volume_ratio < 0.5 {
        confidence -= 3;
    } else if volume_ratio < 1.5 {
        confidence += 2;
    } else if volume_ratio < 2.0 {
        confidence += 7;
    } else {
        confidence += 12;
    }
    confidence += fund_delta;
    confidence = confidence.max(5).min(95);

    let mut signals = Vec::new();
    if !fund_text.is_empty() {
        signals.push(fund_text.clone());
    }
    if obv_trend == "上升" {
        signals.push("OBV上升".into());
    } else if obv_trend == "下降" {
        signals.push("OBV下降".into());
    }

    if let Some(limit_up) = detect_limit_up_volume(klines) {
        signals.push(limit_up);
    }
    if let Some(vol_breakout) = detect_volume_breakout(klines) {
        if !signals.iter().any(|s| s.contains("量能突破")) {
            signals.push(vol_breakout);
        }
    }

    let vr_str = if volume_ratio.fract() == 0.0 {
        format!("{:.1}", volume_ratio)
    } else {
        format!("{}", volume_ratio)
    };
    let mut desc = format!("量价模式={}，量比={}，换手={:.1}%", pattern, vr_str, turnover);
    if !fund_text.is_empty() {
        desc.push_str(&format!("，{}", fund_text));
    }

    VolumePriceResult {
        pattern,
        direction,
        confidence,
        volume_ratio,
        turnover: py_round(turnover, 2),
        obv_trend,
        signals,
        description: desc,
    }
}
