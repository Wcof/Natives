//! 形态模块（1:1 移植自 `engine/analysis/pattern_module.py`）。
//!
//! 识别箱体、双底/双顶、头肩底/顶、双弧底、三角形、旗形、跳空等形态。
//! 目标价公式从基准输出反推并逐一验证：
//!   双底/头肩底目标 = 颈线 + (颈线 - 底部)；箱体目标 = 上沿 + (上沿 - 下沿)。

use crate::trends::indicators::{find_peaks, find_troughs};
use crate::trends::{py_round, KlineBar};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatternResult {
    pub name: String,
    pub direction: String,
    pub confidence: i64,
    pub status: String,
    pub target_price: Option<f64>,
    pub key_levels: BTreeMap<String, f64>,
    pub description: String,
}

fn detect_box(klines: &[KlineBar], price: f64) -> Option<PatternResult> {
    if klines.len() < 20 {
        return None;
    }
    let window = &klines[klines.len() - 20..];
    let upper = window
        .iter()
        .map(|k| k.high)
        .fold(f64::NEG_INFINITY, f64::max);
    let lower = window.iter().map(|k| k.low).fold(f64::INFINITY, f64::min);
    if upper <= lower {
        return None;
    }
    let amplitude = (upper - lower) / lower * 100.0;
    if !(5.0..=25.0).contains(&amplitude) {
        return None;
    }

    let (status, direction, confidence, target) = if price >= upper * 0.98 {
        (
            "接近突破上沿".to_string(),
            "看涨".to_string(),
            55i64,
            Some(upper + (upper - lower)),
        )
    } else {
        ("形成中".to_string(), "中性".to_string(), 55i64, None)
    };

    let desc = format!(
        "箱体振幅{:.1}%，突破上沿目标{:.2}，跌破下沿目标{:.2}",
        amplitude,
        upper + (upper - lower),
        lower - (upper - lower)
    );

    let mut key_levels = BTreeMap::new();
    key_levels.insert("箱体上沿".into(), py_round(upper, 2));
    key_levels.insert("箱体下沿".into(), py_round(lower, 2));

    Some(PatternResult {
        name: "箱体震荡".into(),
        direction,
        confidence,
        status,
        target_price: target.map(|t| py_round(t, 2)),
        key_levels,
        description: desc,
    })
}

fn detect_double_top_bottom(klines: &[KlineBar], price: f64) -> Option<PatternResult> {
    if klines.len() < 60 {
        return None;
    }
    let lows: Vec<f64> = klines.iter().map(|k| k.low).collect();
    let highs: Vec<f64> = klines.iter().map(|k| k.high).collect();
    let troughs = find_troughs(&lows, 5);
    if troughs.len() < 2 {
        return None;
    }
    let i = troughs.len() - 2;
    let t0 = troughs[i];
    let t1 = troughs[i + 1];
    if t1 - t0 >= 5 {
        let v0 = lows[t0];
        let v1 = lows[t1];
        if (v0 - v1).abs() / v0.min(v1) <= 0.03 {
            let mid = &klines[t0..=t1];
            let neck = mid.iter().map(|k| k.high).fold(f64::NEG_INFINITY, f64::max);
            let bottom = v0;
            if neck > bottom {
                let target = neck + (neck - bottom);
                let status = if price > neck {
                    "已突破"
                } else {
                    "形成中"
                };
                let mut key_levels = BTreeMap::new();
                key_levels.insert("颈线".into(), py_round(neck, 2));
                key_levels.insert("底部".into(), py_round(bottom, 2));
                return Some(PatternResult {
                    name: "双底".into(),
                    direction: "看涨".into(),
                    confidence: 55,
                    status: status.into(),
                    target_price: Some(py_round(target, 2)),
                    key_levels,
                    description: "双底可靠性约10%，需等待价格确认突破颈线".into(),
                });
            }
        }
    }

    let peaks = find_peaks(&highs, 5);
    if peaks.len() >= 2 {
        let i = peaks.len() - 2;
        let p0 = peaks[i];
        let p1 = peaks[i + 1];
        if p1 - p0 >= 5 {
            let v0 = highs[p0];
            let v1 = highs[p1];
            if (v0 - v1).abs() / v0.min(v1) <= 0.03 {
                let mid = &klines[p0..=p1];
                let neck = mid.iter().map(|k| k.low).fold(f64::INFINITY, f64::min);
                let top = v0.max(v1);
                if neck < top {
                    let target = neck - (top - neck);
                    if price < neck {
                        let mut key_levels = BTreeMap::new();
                        key_levels.insert("颈线".into(), py_round(neck, 2));
                        key_levels.insert("顶部".into(), py_round(top, 2));
                        return Some(PatternResult {
                            name: "双顶".into(),
                            direction: "看跌".into(),
                            confidence: 55,
                            status: "已突破".into(),
                            target_price: Some(py_round(target, 2)),
                            key_levels,
                            description: format!("双顶颈线{:.2}，跌破后目标{:.2}", neck, target),
                        });
                    }
                }
            }
        }
    }
    None
}

fn detect_head_shoulders(klines: &[KlineBar], price: f64) -> Option<PatternResult> {
    if klines.len() < 10 {
        return None;
    }
    let lows: Vec<f64> = klines.iter().map(|k| k.low).collect();
    let highs: Vec<f64> = klines.iter().map(|k| k.high).collect();
    let troughs = find_troughs(&lows, 3);

    if troughs.len() >= 3 {
        let i = troughs.len() - 2;
        let l = troughs[i - 1];
        let m = troughs[i];
        let r = troughs[i + 1];
        if m - l >= 2 && r - m >= 2 {
            let vl = lows[l];
            let vm = lows[m];
            let vr = lows[r];
            if vm < vl && vm < vr && (vl - vr).abs() / vl.min(vr) <= 0.08 {
                let neck = highs[l].max(highs[r]);
                let depth = neck - vm;
                if neck > vm {
                    let target = neck + depth;
                    let status = if price > neck {
                        "已突破"
                    } else {
                        "形成中"
                    };
                    let confidence = if status == "已突破" { 80 } else { 60 };
                    let mut key_levels = BTreeMap::new();
                    key_levels.insert("颈线".into(), py_round(neck, 2));
                    key_levels.insert("头部".into(), py_round(vm, 2));
                    return Some(PatternResult {
                        name: "头肩底".into(),
                        direction: "看涨".into(),
                        confidence,
                        status: status.into(),
                        target_price: Some(py_round(target, 2)),
                        key_levels,
                        description: format!("底部深度{:.2}，突破颈线后目标{:.2}", depth, target),
                    });
                }
            }
        }
    }

    let peaks = find_peaks(&highs, 3);
    if peaks.len() >= 3 {
        let i = peaks.len() - 2;
        let l = peaks[i - 1];
        let m = peaks[i];
        let r = peaks[i + 1];
        if m - l >= 2 && r - m >= 2 {
            let vl = highs[l];
            let vm = highs[m];
            let vr = highs[r];
            if vm > vl && vm > vr && (vl - vr).abs() / vl.min(vr) <= 0.08 {
                let neck = lows[l].min(lows[r]);
                if neck < vm {
                    let height = vm - neck;
                    let target = neck - height;
                    let status = if price < neck {
                        "已突破"
                    } else {
                        "形成中"
                    };
                    let confidence = if status == "形成中" { 45 } else { 60 };
                    let mut key_levels = BTreeMap::new();
                    key_levels.insert("颈线".into(), py_round(neck, 2));
                    key_levels.insert("头部".into(), py_round(vm, 2));
                    return Some(PatternResult {
                        name: "头肩顶".into(),
                        direction: "看跌".into(),
                        confidence,
                        status: status.into(),
                        target_price: Some(py_round(target, 2)),
                        key_levels,
                        description: format!("头部高度{:.2}，跌破颈线后目标{:.2}", height, target),
                    });
                }
            }
        }
    }
    None
}

fn detect_rounding_bottom(klines: &[KlineBar], _price: f64) -> Option<PatternResult> {
    if klines.len() < 60 {
        return None;
    }
    let window = &klines[klines.len() - 60..];
    let lows: Vec<f64> = window.iter().map(|k| k.low).collect();

    let mut min_idx = 0;
    let mut min_val = lows[0];
    for (i, &v) in lows.iter().enumerate() {
        if v < min_val {
            min_val = v;
            min_idx = i;
        }
    }
    if min_idx < 15 || min_idx > window.len() - 15 {
        return None;
    }
    let left = &lows[..min_idx];
    let right = &lows[min_idx + 1..];
    if left.len() < 8 || right.len() < 8 {
        return None;
    }

    let left_desc = (0..left.len().saturating_sub(2)).all(|i| {
        if left[i] > 0.0 {
            left[i] >= left[i + 1]
        } else {
            true
        }
    });
    let right_asc = (0..right.len().saturating_sub(1)).all(|i| right[i] <= right[i + 1]);
    if !(left_desc && right_asc) {
        return None;
    }

    let vols: Vec<f64> = window.iter().map(|k| k.volume).collect();
    let vol_sub = &vols[min_idx.saturating_sub(5)..(min_idx + 6).min(vols.len())];
    let vol_min = vol_sub.iter().copied().fold(f64::INFINITY, f64::min);
    if vol_min <= 0.0 {
        return None;
    }
    let bottom = lows.iter().copied().fold(f64::INFINITY, f64::min);

    let mut key_levels = BTreeMap::new();
    key_levels.insert("弧底低点".into(), py_round(bottom, 2));

    Some(PatternResult {
        name: "双弧底".into(),
        direction: "看涨".into(),
        confidence: 70,
        status: "形成中".into(),
        target_price: None,
        key_levels,
        description: "K线与成交量同时呈圆弧底，连续放量2-3日可确认起涨".into(),
    })
}

fn detect_triangle(klines: &[KlineBar], price: f64) -> Option<PatternResult> {
    if klines.len() < 30 {
        return None;
    }
    let window = &klines[klines.len() - 30..];
    let first_half = &window[..15];
    let second_half = &window[15..];
    if first_half.is_empty() || second_half.is_empty() {
        return None;
    }
    let h1 = first_half
        .iter()
        .map(|k| k.high)
        .fold(f64::NEG_INFINITY, f64::max);
    let h2 = second_half
        .iter()
        .map(|k| k.high)
        .fold(f64::NEG_INFINITY, f64::max);
    let l1 = first_half
        .iter()
        .map(|k| k.low)
        .fold(f64::INFINITY, f64::min);
    let l2 = second_half
        .iter()
        .map(|k| k.low)
        .fold(f64::INFINITY, f64::min);
    let upper_slope = h2 - h1;
    let lower_slope = l2 - l1;

    if upper_slope.abs() < 1e-6 || lower_slope.abs() < 1e-6 {
        return None;
    }

    if upper_slope < 0.0 && lower_slope > 0.0 {
        return Some(PatternResult {
            name: "对称三角形".into(),
            direction: "中性".into(),
            confidence: 50,
            status: "形成中".into(),
            target_price: None,
            key_levels: BTreeMap::new(),
            description: "对称三角形通常延续原有趋势，等待方向选择".into(),
        });
    }
    if h1 > 0.0 && upper_slope.abs() / h1 < 0.02 && lower_slope > 0.0 {
        let status = if price > h2 {
            "已突破"
        } else {
            "接近突破"
        };
        let mut key_levels = BTreeMap::new();
        key_levels.insert("阻力位".into(), py_round(h2, 2));
        return Some(PatternResult {
            name: "上升三角形".into(),
            direction: "看涨".into(),
            confidence: 60,
            status: status.into(),
            target_price: Some(py_round(h2 + (h2 - l1), 2)),
            key_levels,
            description: format!("上升三角形偏多，突破{:.2}确认", py_round(h2, 2)),
        });
    }
    if l1 > 0.0 && lower_slope.abs() / l1 < 0.02 && upper_slope < 0.0 {
        let status = if price < l2 {
            "已跌破"
        } else {
            "接近跌破"
        };
        let mut key_levels = BTreeMap::new();
        key_levels.insert("支撑位".into(), py_round(l2, 2));
        return Some(PatternResult {
            name: "下降三角形".into(),
            direction: "看跌".into(),
            confidence: 60,
            status: status.into(),
            target_price: Some(py_round(l2 - (h1 - l2), 2)),
            key_levels,
            description: format!("下降三角形偏空，跌破{:.2}确认", py_round(l2, 2)),
        });
    }
    None
}

fn detect_flag(klines: &[KlineBar], _price: f64) -> Option<PatternResult> {
    if klines.len() < 30 {
        return None;
    }
    let n = klines.len();
    let pole = &klines[n - 30..n - 15];
    let flag = &klines[n - 15..];
    if pole.is_empty() || flag.is_empty() {
        return None;
    }
    let pole_rise = pole.last().unwrap().close - pole.first().unwrap().close;
    if pole_rise <= 0.0 {
        return None;
    }
    let flag_high = flag
        .iter()
        .map(|k| k.high)
        .fold(f64::NEG_INFINITY, f64::max);
    let flag_low = flag.iter().map(|k| k.low).fold(f64::INFINITY, f64::min);
    let flag_range = if flag_low > 0.0 {
        (flag_high - flag_low) / flag_low
    } else {
        0.0
    };
    if flag_range > 0.08 {
        return None;
    }

    let mut key_levels = BTreeMap::new();
    key_levels.insert("旗形上沿".into(), py_round(flag_high, 2));

    Some(PatternResult {
        name: "上升旗形".into(),
        direction: "看涨".into(),
        confidence: 60,
        status: "整理中".into(),
        target_price: Some(py_round(flag_high + pole_rise, 2)),
        key_levels,
        description: format!(
            "旗杆涨幅{:.2}，突破后目标{:.2}",
            py_round(pole_rise, 2),
            py_round(flag_high + pole_rise, 2)
        ),
    })
}

fn detect_gap(klines: &[KlineBar], _price: f64) -> Option<PatternResult> {
    if klines.len() < 5 {
        return None;
    }
    let latest = &klines[klines.len() - 1];
    let prev = &klines[klines.len() - 2];
    if latest.low > prev.high {
        let gap = latest.low - prev.high;
        let mut key_levels = BTreeMap::new();
        key_levels.insert("缺口上沿".into(), py_round(latest.low, 2));
        return Some(PatternResult {
            name: "向上突破缺口".into(),
            direction: "看涨".into(),
            confidence: 65,
            status: "已形成".into(),
            target_price: None,
            key_levels,
            description: format!("向上跳空缺口{:.2}，回补前视为支撑", gap),
        });
    }
    if latest.high < prev.low {
        let gap = prev.low - latest.high;
        let mut key_levels = BTreeMap::new();
        key_levels.insert("缺口下沿".into(), py_round(latest.high, 2));
        return Some(PatternResult {
            name: "向下突破缺口".into(),
            direction: "看跌".into(),
            confidence: 65,
            status: "已形成".into(),
            target_price: None,
            key_levels,
            description: format!("向下跳空缺口{:.2}，回补前视为压力", gap),
        });
    }
    None
}

/// 形态综合分析主入口。
pub fn analyze_patterns(klines: &[KlineBar]) -> Vec<PatternResult> {
    if klines.is_empty() {
        return Vec::new();
    }
    let price = klines.last().unwrap().close;
    let window60 = if klines.len() > 60 {
        &klines[klines.len() - 60..]
    } else {
        klines
    };
    let window20 = if klines.len() > 20 {
        &klines[klines.len() - 20..]
    } else {
        klines
    };

    let mut results = Vec::new();
    if let Some(r) = detect_head_shoulders(window60, price) {
        results.push(r);
    }
    if let Some(r) = detect_double_top_bottom(window60, price) {
        results.push(r);
    }
    if let Some(r) = detect_triangle(window20, price) {
        results.push(r);
    }
    if let Some(r) = detect_box(window20, price) {
        results.push(r);
    }
    if let Some(r) = detect_flag(window60, price) {
        results.push(r);
    }
    if let Some(r) = detect_gap(window60, price) {
        results.push(r);
    }
    if let Some(r) = detect_rounding_bottom(window60, price) {
        results.push(r);
    }

    if results.len() > 3 {
        results.truncate(3);
    }
    results
}
