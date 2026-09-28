//! 突破模块（1:1 移植自 `engine/analysis/breakout_module.py`）。
//!
//! 海龟交易法则：
//! 系统一：20日唐奇安通道；系统二：55日唐奇安通道。
//! 核心算法：
//!   TR = max(H-L, |H-PDC|, |L-PDC|)；N = 前N日TR的SMA；止损 = 入场价 ± 2N；
//!   加仓：每上涨 0.5N 加 1 单位。

use crate::trends::{py_round, KlineBar};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BreakoutResult {
    pub system: String,
    pub signal: String,
    pub breakout_price: f64,
    pub current_n: f64,
    pub stop_loss: f64,
    pub entry_price: Option<f64>,
    pub position_units: i64,
    pub exit_price: Option<f64>,
    pub channel_high: f64,
    pub channel_low: f64,
    pub next_add_price: Option<f64>,
    pub signals: Vec<String>,
    pub description: String,
}

pub fn calc_true_range(high: f64, low: f64, pre_close: f64) -> f64 {
    let hl = high - low;
    let hpc = (high - pre_close).abs();
    let lpc = (low - pre_close).abs();
    hl.max(hpc).max(lpc)
}

pub fn calc_n(klines: &[KlineBar], period: usize) -> f64 {
    if klines.len() < period + 1 {
        return 0.0;
    }
    let mut trs = Vec::with_capacity(period);
    for i in (klines.len() - period)..klines.len() {
        let k = &klines[i];
        let pre_close = klines[i - 1].close;
        trs.push(calc_true_range(k.high, k.low, pre_close));
    }
    let mean: f64 = trs.iter().sum::<f64>() / trs.len() as f64;
    py_round(mean, 4)
}

pub fn calc_donchian_channel(klines: &[KlineBar], period: usize) -> (f64, f64) {
    if klines.is_empty() {
        return (0.0, 0.0);
    }
    let window = if klines.len() <= period {
        if klines.len() > 1 {
            &klines[..klines.len() - 1]
        } else {
            klines
        }
    } else {
        &klines[klines.len() - period - 1..klines.len() - 1]
    };
    let high = window.iter().map(|k| k.high).fold(f64::NEG_INFINITY, f64::max);
    let low = window.iter().map(|k| k.low).fold(f64::INFINITY, f64::min);
    (high, low)
}

fn find_last_entry(klines: &[KlineBar], period: usize) -> Option<(&'static str, f64, usize)> {
    let n = klines.len();
    if n <= period {
        return None;
    }
    for i in (period..n).rev() {
        let window = &klines[i - period..i];
        if window.len() < period {
            continue;
        }
        let window_high = window.iter().map(|k| k.high).fold(f64::NEG_INFINITY, f64::max);
        let window_low = window.iter().map(|k| k.low).fold(f64::INFINITY, f64::min);
        let k = &klines[i];
        if k.high > window_high {
            return Some(("多", window_high, i));
        }
        if k.low < window_low {
            return Some(("空", window_low, i));
        }
    }
    None
}

fn analyze_system(klines: &[KlineBar], period: usize, system_name: &str) -> BreakoutResult {
    let n_val = calc_n(klines, 20);
    let (channel_high, channel_low) = calc_donchian_channel(klines, period);
    let last_entry = find_last_entry(klines, period);

    if n_val <= 0.0 || channel_high <= 0.0 || last_entry.is_none() {
        return BreakoutResult {
            system: system_name.to_string(),
            signal: "无信号".to_string(),
            breakout_price: py_round(channel_high, 2),
            current_n: n_val,
            stop_loss: 0.0,
            entry_price: None,
            position_units: 0,
            exit_price: None,
            channel_high: py_round(channel_high, 2),
            channel_low: py_round(channel_low, 2),
            next_add_price: None,
            signals: Vec::new(),
            description: format!("{}无突破信号", system_name),
        };
    }

    let (direction, entry, entry_idx) = last_entry.unwrap();
    let holding_days = klines.len() - 1 - entry_idx;

    let (signal, stop, units, next_add, exit_price, sig_text) = if direction == "多" {
        let high_since = if klines.len() > entry_idx + 1 {
            klines[entry_idx + 1..]
                .iter()
                .map(|k| k.high)
                .fold(f64::NEG_INFINITY, f64::max)
        } else {
            entry
        };
        let mut extra = 0i64;
        if high_since > entry + 0.5 * n_val {
            extra = ((high_since - entry) / (0.5 * n_val)).floor() as i64;
        }
        let units = 1 + extra;
        let last_add_price = entry + (units - 1) as f64 * 0.5 * n_val;
        let stop = last_add_price - 2.0 * n_val;
        let next_add = if units <= 4 && system_name != "系统二(55日)" {
            Some(entry + units as f64 * 0.5 * n_val)
        } else {
            None
        };
        let (signal, exit_price) = if klines.last().unwrap().close <= stop {
            ("卖出", Some(stop))
        } else {
            ("持仓", None)
        };
        let sig_text = if signal == "卖出" {
            format!("触及2N止损{:.2}，卖出", stop)
        } else {
            format!("持有多头{}日，止损{:.2}", holding_days, stop)
        };
        (signal, stop, units, next_add, exit_price, sig_text)
    } else {
        let stop = entry + 2.0 * n_val;
        let units = 1i64;
        let next_add = None;
        let exit_window = if system_name.contains("系统一") { 10 } else { 20 };
        let (high_exit_level, _) = calc_donchian_channel(klines, exit_window);
        let last = klines.last().unwrap();
        let (signal, exit_price, sig_text) = if last.high >= high_exit_level {
            (
                "空头平仓",
                Some(high_exit_level),
                format!("突破{}日高点{:.2}，空头平仓", exit_window, high_exit_level),
            )
        } else if last.close >= stop {
            (
                "空头平仓",
                Some(stop),
                format!("触及2N止损{:.2}，空头平仓", stop),
            )
        } else {
            (
                "持仓",
                None,
                format!("持有空头{}日，止损{:.2}", holding_days, stop),
            )
        };
        (signal, stop, units, next_add, exit_price, sig_text)
    };

    let desc_prefix = system_name.replace("(20日)", "").replace("(55日)", "");
    let description = format!(
        "{}入场={}@{:.2}，N={:.4}，持有{}日",
        desc_prefix, direction, entry, n_val, holding_days
    );

    BreakoutResult {
        system: system_name.to_string(),
        signal: signal.to_string(),
        breakout_price: py_round(channel_high, 2),
        current_n: n_val,
        stop_loss: py_round(stop, 2),
        entry_price: Some(py_round(entry, 2)),
        position_units: units,
        exit_price: exit_price.map(|p| py_round(p, 2)),
        channel_high: py_round(channel_high, 2),
        channel_low: py_round(channel_low, 2),
        next_add_price: next_add.map(|p| py_round(p, 2)),
        signals: vec![sig_text],
        description,
    }
}

pub fn analyze_breakout_system1(klines: &[KlineBar]) -> BreakoutResult {
    analyze_system(klines, 20, "系统一(20日)")
}

pub fn analyze_breakout_system2(klines: &[KlineBar]) -> BreakoutResult {
    analyze_system(klines, 55, "系统二(55日)")
}

pub fn analyze_breakout(klines: &[KlineBar]) -> Vec<BreakoutResult> {
    vec![
        analyze_breakout_system1(klines),
        analyze_breakout_system2(klines),
    ]
}
