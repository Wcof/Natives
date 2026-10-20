//! 共享技术指标库（1:1 移植自 `engine/analysis/_indicators.py`）。
//!
//! 供 trends::modules 下各模块复用。算法与常量池描述严格对齐。

/// 简单移动平均序列。
/// 前 period - 1 个位置返回 None 占位。
pub fn sma_series(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return Vec::new();
    }
    let mut result = vec![None; values.len()];
    let mut running = 0.0;
    for (i, &v) in values.iter().enumerate() {
        running += v;
        if i >= period {
            running -= values[i - period];
        }
        if i >= period - 1 {
            result[i] = Some(running / period as f64);
        }
    }
    result
}

/// 指数移动平均序列。以第一个有效窗口均值为种子。
pub fn ema_series(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return Vec::new();
    }
    let mut result = vec![None; values.len()];
    if values.len() < period {
        return result;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let seed: f64 = values[..period].iter().sum::<f64>() / period as f64;
    let mut prev = seed;
    result[period - 1] = Some(prev);
    for i in period..values.len() {
        prev = alpha * values[i] + (1.0 - alpha) * prev;
        result[i] = Some(prev);
    }
    result
}

/// 返回序列最后一个 SMA 值（数据不足返回 0.0）。
pub fn last_sma(values: &[f64], period: usize) -> f64 {
    if values.len() < period || period == 0 {
        return 0.0;
    }
    values[values.len() - period..].iter().sum::<f64>() / period as f64
}

/// 判断均线方向：向上 / 向下 / 走平 / 未知。
pub fn ma_direction(ma_values: &[Option<f64>], lookback: usize) -> &'static str {
    let valid: Vec<f64> = ma_values.iter().filter_map(|&v| v).collect();
    if valid.len() < lookback + 1 {
        return "未知";
    }
    let recent = &valid[valid.len() - (lookback + 1)..];
    let slope = recent[recent.len() - 1] - recent[0];
    let base = recent[0].abs();
    let threshold = if base > 0.0 { base * 0.002 } else { 1e-9 };
    if slope > threshold {
        "向上"
    } else if slope < -threshold {
        "向下"
    } else {
        "走平"
    }
}

/// 查找局部高点索引（窗口须完整且在数据内，不含首尾端点）。
pub fn find_peaks(values: &[f64], window: usize) -> Vec<usize> {
    let mut peaks = Vec::new();
    let n = values.len();
    if n < 2 * window + 1 {
        return peaks;
    }
    for i in window..(n - window) {
        let lo = i - window;
        let hi = i + window;
        let is_max = (lo..=hi)
            .filter(|&j| j != i)
            .all(|j| values[i] >= values[j]);
        if is_max {
            let all_equal = (lo..=hi).all(|j| values[j] == values[i]);
            if !all_equal {
                peaks.append(&mut vec![i]);
            }
        }
    }
    peaks
}

/// 查找局部低点索引（窗口须完整且在数据内，不含首尾端点）。
pub fn find_troughs(values: &[f64], window: usize) -> Vec<usize> {
    let mut troughs = Vec::new();
    let n = values.len();
    if n < 2 * window + 1 {
        return troughs;
    }
    for i in window..(n - window) {
        let lo = i - window;
        let hi = i + window;
        let is_min = (lo..=hi)
            .filter(|&j| j != i)
            .all(|j| values[i] <= values[j]);
        if is_min {
            let all_equal = (lo..=hi).all(|j| values[j] == values[i]);
            if !all_equal {
                troughs.append(&mut vec![i]);
            }
        }
    }
    troughs
}

/// 对给定索引点做最小二乘线性拟合，返回斜率。数据不足返回 None。
pub fn fit_trendline(points_idx: &[usize], values: &[f64]) -> Option<f64> {
    if points_idx.len() < 2 {
        return None;
    }
    let xs: Vec<f64> = points_idx.iter().map(|&i| i as f64).collect();
    let ys: Vec<f64> = points_idx.iter().map(|&i| values[i]).collect();
    let n = xs.len() as f64;
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = ys.iter().sum::<f64>() / n;
    let denom: f64 = xs.iter().map(|&x| (x - mean_x).powi(2)).sum();
    if denom == 0.0 {
        return None;
    }
    let slope = xs
        .iter()
        .zip(ys.iter())
        .map(|(&x, &y)| (x - mean_x) * (y - mean_y))
        .sum::<f64>()
        / denom;
    Some(slope)
}

/// MACD 三线：DIF / DEA / BAR(柱)。返回 (dif, dea, bar)。
///
/// 注意：此实现为标准 EMA 模式，与 `chanlun_daily.rs` / `chanlun_minute.rs` 的
/// SMA-seed 变体不同，不可混淆。
pub fn macd_series(
    closes: &[f64],
    fast: usize,
    slow: usize,
    signal: usize,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = closes.len();
    if n < slow {
        return (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    }
    let ema_fast = ema_series(closes, fast);
    let ema_slow = ema_series(closes, slow);
    let dif: Vec<f64> = ema_fast
        .iter()
        .zip(ema_slow.iter())
        .map(|(ef, es)| ef.unwrap_or(0.0) - es.unwrap_or(0.0))
        .collect();
    let dea_raw = ema_series(&dif, signal);
    let dea: Vec<f64> = dea_raw.into_iter().map(|d| d.unwrap_or(0.0)).collect();
    let bar: Vec<f64> = dif
        .iter()
        .zip(dea.iter())
        .map(|(&d, &e)| 2.0 * (d - e))
        .collect();
    (dif, dea, bar)
}

/// 计算 [start, end) 区间 MACD 柱面积（绝对值之和）。
pub fn macd_area(macd_bar: &[f64], start: usize, end: usize) -> f64 {
    let s = start.min(macd_bar.len());
    let e = end.min(macd_bar.len());
    if s >= e {
        return 0.0;
    }
    macd_bar[s..e].iter().map(|b| b.abs()).sum()
}

/// 百分比变化：(end - start) / start * 100.0。
pub fn pct_change(start: f64, end: f64) -> f64 {
    if start == 0.0 {
        0.0
    } else {
        (end - start) / start * 100.0
    }
}
