//! 缠论分钟线分析（1:1 移植自 `engine/analysis/chanlun_minute.py` 明文版）。
//!
//! 算法要点（与 Python 逐位对应）：
//! - `construct_5min_klines`：每 5 分钟聚合一根 K 线（区间首分钟 open、末分钟 close、
//!   high=max、low=min、volume=sum），time 取区间最后一分钟时间戳；时段断裂
//!   （与上一分钟非 1 分钟间隔，午休/隔日等）立即收拢当前组；
//! - `calc_macd`：`_ema_sma_seed` 的 SMA 种子 EMA（`x*k + prev*(1-k)` 写法），
//!   与日线版 `calc_daily_macd` 的递推写法浮点运算顺序不同，位模式不可互换，
//!   两个实现禁止合并或抽共享；
//! - merge / fractals / strokes 与日线版同构（笔端点合并K线索引差 >= 4）；
//! - detect_divergence：面积 = sum(abs(bar[si..=ei]))，time 映射到 5min K 线索引；
//! - generate_signals：背驰笔 -> buy1（向下笔）/ sell1（向上笔），
//!   confidence = clamp(round(100 - 40*ratio), 55, 90)。

use std::collections::HashMap;

use serde_json::json;

use super::{macd_area_value, py_round, py_round_i};

/// 5 分钟 K 线（对应 Python `MinuteKline`）。
#[derive(Clone)]
struct MinuteKline {
    time: String,
    /// 与 Python 数据类字段一一对应保留（P1 起消费）。
    #[allow(dead_code)]
    open: f64,
    close: f64,
    high: f64,
    low: f64,
    /// 与 Python 数据类字段一一对应保留（P1 起消费）。
    #[allow(dead_code)]
    volume: f64,
}

/// 包含关系合并后的 K 线（对应 Python `MergedKline`）。
#[derive(Clone)]
struct MergedKline {
    time_start: String,
    time_end: String,
    high: f64,
    low: f64,
    direction: i32,
    raw_count: i32,
}

/// 分型（对应 Python `Fractal`）。index 为合并 K 线索引。
#[derive(Clone)]
struct Fractal {
    index: usize,
    kind: String, // "top" | "bottom"
    price: f64,
    time: String,
}

/// 笔（对应 Python `Stroke`）。start_idx/end_idx 为分型列表中的位置。
#[derive(Clone, Default)]
struct Stroke {
    direction: String,
    start_price: f64,
    end_price: f64,
    start_time: String,
    end_time: String,
    /// 与 Python 数据类字段一一对应保留（序列化契约不输出，P1 起消费）。
    #[allow(dead_code)]
    start_idx: usize,
    /// 与 Python 数据类字段一一对应保留（序列化契约不输出，P1 起消费）。
    #[allow(dead_code)]
    end_idx: usize,
    macd_area: f64,
    has_divergence: bool,
    /// Python `sum()` 空切片返回 int 0 的 1:1 复现标记（见 [`super::macd_area_value`]）。
    area_int_zero: bool,
}

/// 买卖点信号（对应 Python `ChanlunSignal`）。
struct Signal {
    kind: String,
    price: f64,
    time: String,
    description: String,
    confidence: i64,
}

/// 缠论分钟线分析结果（对应 Python `ChanlunMinuteResult`）。
pub struct ChanlunMinuteResult {
    kline_count: usize,
    fractal_count: usize,
    fractals: Vec<Fractal>,
    stroke_count: usize,
    strokes: Vec<Stroke>,
    signals: Vec<Signal>,
    /// 与 Python 结果对象字段一一对应保留（dict 契约不含三线，P1 起消费）。
    #[allow(dead_code)]
    macd_dif: Vec<f64>,
    /// 与 Python 结果对象字段一一对应保留（dict 契约不含三线，P1 起消费）。
    #[allow(dead_code)]
    macd_dea: Vec<f64>,
    /// 与 Python 结果对象字段一一对应保留（dict 契约不含三线，P1 起消费）。
    #[allow(dead_code)]
    macd_bar: Vec<f64>,
    current_state: String,
    summary: String,
    description: String,
}

/// HH:MM -> 当日分钟数（对应 Python `_to_minutes`）。
///
/// Python 对畸形输入会抛 ValueError；这里按 0 兜底（fixture 与生产数据
/// 均为合法 HH:MM，不影响 A/B 契约）。
fn to_minutes(t: &str) -> i64 {
    let Some((hh, mm)) = t.split_once(':') else {
        return 0;
    };
    let h: i64 = hh.parse().unwrap_or(0);
    let m: i64 = mm.parse().unwrap_or(0);
    h * 60 + m
}

/// 根据组内分钟下标聚合一根 5 分钟 K 线（对应 Python `_make_kline`）。
fn make_kline(times: &[String], prices: &[f64], volumes: &[f64], group: &[usize]) -> MinuteKline {
    let g_prices: Vec<f64> = group.iter().map(|&i| prices[i]).collect();
    MinuteKline {
        time: times[*group.last().unwrap()].clone(),
        open: g_prices[0],
        close: g_prices[g_prices.len() - 1],
        high: g_prices.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        low: g_prices.iter().copied().fold(f64::INFINITY, f64::min),
        volume: group.iter().map(|&i| volumes[i]).sum(),
    }
}

/// 将分钟数据按 5 分钟区间聚合为 5 分钟 K 线（对应 Python `construct_5min_klines`）。
///
/// 按连续交易时段（上午/下午/其他）分别从时段第一分钟起每 5 分钟一组，
/// time 取组内最后一分钟的原始时间戳；任意分钟时间属于同一时段。
fn construct_5min_klines(times: &[String], prices: &[f64], volumes: &[f64]) -> Vec<MinuteKline> {
    let mut klines: Vec<MinuteKline> = Vec::new();
    let n = times.len();
    if n == 0 {
        return klines;
    }

    let mut group: Vec<usize> = Vec::new();
    let mut group_start_minute = to_minutes(&times[0]);
    for i in 0..n {
        let cur_minute = to_minutes(&times[i]);
        // 检测时段断裂：与上一分钟出现非 1 分钟间隔（午休等）
        if !group.is_empty() && cur_minute - to_minutes(&times[i - 1]) != 1 {
            klines.push(make_kline(times, prices, volumes, &group));
            group.clear();
            group_start_minute = cur_minute;
        }
        if group.is_empty() {
            group_start_minute = cur_minute;
        }
        group.push(i);
        // 满 5 分钟或已到最后一个且累计不足 5 时收拢
        if cur_minute - group_start_minute == 4 || (i == n - 1 && !group.is_empty()) {
            klines.push(make_kline(times, prices, volumes, &group));
            group.clear();
        }
    }
    klines
}

/// EMA：前 n 个值保持为 SMA(n)，之后按 EMA 递推（对应 Python `_ema_sma_seed`）。
///
/// 注意：递推写法是 `seq[i]*k + out[-1]*(1-k)`，与日线版
/// `prev + (x - prev)*(2/(n+1))` 的浮点运算顺序不同，位模式不可互换。
fn ema_sma_seed(seq: &[f64], n: usize) -> Vec<f64> {
    if seq.is_empty() {
        return Vec::new();
    }
    let k = 2.0 / (n as f64 + 1.0);
    let seed = seq.iter().take(n).sum::<f64>() / n as f64;
    let mut out = vec![seed; n.min(seq.len())];
    for i in n..seq.len() {
        let v = seq[i] * k + out[out.len() - 1] * (1.0 - k);
        out.push(v);
    }
    out
}

/// SMA 种子 EMA 的 MACD 计算（对应 Python `calc_macd`），返回 (dif, dea, bar)。
fn calc_macd(klines: &[MinuteKline]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let closes: Vec<f64> = klines.iter().map(|k| k.close).collect();
    let ema12 = ema_sma_seed(&closes, 12);
    let ema26 = ema_sma_seed(&closes, 26);
    let dif: Vec<f64> = ema12.iter().zip(ema26.iter()).map(|(a, b)| a - b).collect();
    let dea = ema_sma_seed(&dif, 9);
    let bar: Vec<f64> = dif
        .iter()
        .zip(dea.iter())
        .map(|(d, s)| (d - s) * 2.0)
        .collect();
    (dif, dea, bar)
}

/// 合并 K 线：包含关系按当前方向取极值（对应 Python `merge_klines`）。
fn merge_klines(klines: &[MinuteKline]) -> Vec<MergedKline> {
    if klines.is_empty() {
        return Vec::new();
    }
    let mut merged: Vec<MergedKline> = Vec::new();
    let mut cur = MergedKline {
        time_start: klines[0].time.clone(),
        time_end: klines[0].time.clone(),
        high: klines[0].high,
        low: klines[0].low,
        direction: 0,
        raw_count: 1,
    };
    for i in 1..klines.len() {
        let (h, l) = (klines[i].high, klines[i].low);
        let contained = (h <= cur.high && l >= cur.low) || (h >= cur.high && l <= cur.low);
        if !contained {
            merged.push(cur.clone());
            let direction = if h > cur.high { 1 } else { -1 };
            cur = MergedKline {
                time_start: klines[i].time.clone(),
                time_end: klines[i].time.clone(),
                high: h,
                low: l,
                direction,
                raw_count: 1,
            };
        } else {
            let (new_high, new_low) = match cur.direction {
                1 => (h.max(cur.high), l.max(cur.low)),
                -1 => (h.min(cur.high), l.min(cur.low)),
                _ => (h.max(cur.high), l.min(cur.low)),
            };
            cur = MergedKline {
                time_start: cur.time_start.clone(),
                time_end: klines[i].time.clone(),
                high: new_high,
                low: new_low,
                direction: cur.direction,
                raw_count: cur.raw_count + 1,
            };
        }
    }
    merged.push(cur);
    merged
}

/// 在合并 K 线上识别顶/底分型。index 为合并 K 线索引（对应 Python `find_fractals`）。
fn find_fractals(merged: &[MergedKline]) -> Vec<Fractal> {
    let mut fractals: Vec<Fractal> = Vec::new();
    let n = merged.len();
    for i in 1..n.saturating_sub(1) {
        let (left, cur, right) = (&merged[i - 1], &merged[i], &merged[i + 1]);
        if cur.high > left.high && cur.high > right.high {
            fractals.push(Fractal {
                index: i,
                kind: "top".to_string(),
                price: cur.high,
                time: cur.time_end.clone(),
            });
        } else if cur.low < left.low && cur.low < right.low {
            fractals.push(Fractal {
                index: i,
                kind: "bottom".to_string(),
                price: cur.low,
                time: cur.time_end.clone(),
            });
        }
    }
    fractals
}

/// 在分型序列上构建笔（与日线版同规则，对应 Python `find_strokes`）。
///
/// 与 Python 一致：闭合当前笔后不推进 j，同一分型立即按新起点重新判定。
fn find_strokes(fractals: &[Fractal]) -> Vec<Stroke> {
    let mut strokes: Vec<Stroke> = Vec::new();
    let n = fractals.len();
    if n == 0 {
        return strokes;
    }

    let mut start = &fractals[0];
    let mut start_pos = 0usize;
    let mut direction = if start.kind == "top" { "down" } else { "up" };
    let mut end: Option<&Fractal> = None;
    let mut end_pos = 0usize;
    let mut j = 1usize;
    while j < n {
        let f = &fractals[j];
        if f.kind == start.kind {
            match end {
                Some(e) => {
                    strokes.push(Stroke {
                        direction: direction.to_string(),
                        start_price: start.price,
                        end_price: e.price,
                        start_time: start.time.clone(),
                        end_time: e.time.clone(),
                        start_idx: start_pos,
                        end_idx: end_pos,
                        ..Stroke::default()
                    });
                    start = e;
                    start_pos = end_pos;
                    direction = if direction == "down" { "up" } else { "down" };
                    end = None;
                    // 与 Python 一致：此处不 j += 1
                }
                None => {
                    if (direction == "down" && f.price > start.price)
                        || (direction == "up" && f.price < start.price)
                    {
                        start = f;
                        start_pos = j;
                    }
                    j += 1;
                }
            }
        } else {
            if f.index >= start.index + 4 {
                end = Some(f);
                end_pos = j;
            }
            j += 1;
        }
    }

    if let Some(e) = end {
        strokes.push(Stroke {
            direction: direction.to_string(),
            start_price: start.price,
            end_price: e.price,
            start_time: start.time.clone(),
            end_time: e.time.clone(),
            start_idx: start_pos,
            end_idx: end_pos,
            ..Stroke::default()
        });
    }
    strokes
}

/// 计算每笔 MACD 面积并标注背驰（就地修改笔对象，对应 Python `detect_divergence`）。
fn detect_divergence(strokes: &mut [Stroke], macd_bar: &[f64], klines: &[MinuteKline]) {
    let time_to_idx: HashMap<&str, usize> = klines
        .iter()
        .enumerate()
        .map(|(i, k)| (k.time.as_str(), i))
        .collect();
    for st in strokes.iter_mut() {
        match (
            time_to_idx.get(st.start_time.as_str()),
            time_to_idx.get(st.end_time.as_str()),
        ) {
            (Some(&si), Some(&ei)) => {
                // Python：sum() 作用在空切片（跨交易日时间标签重复导致 si > ei）
                // 返回 int 0，序列化时须输出整数 0
                if si > ei {
                    st.macd_area = 0.0;
                    st.area_int_zero = true;
                } else {
                    st.macd_area = macd_bar[si..=ei].iter().map(|x| x.abs()).sum();
                    st.area_int_zero = false;
                }
            }
            _ => {
                st.macd_area = 0.0;
                st.area_int_zero = false;
            }
        }
    }

    for i in 0..strokes.len() {
        let mut prev: Option<usize> = None;
        for j in (0..i).rev() {
            if strokes[j].direction == strokes[i].direction {
                prev = Some(j);
                break;
            }
        }
        let diverged = match prev {
            None => false,
            Some(p) => {
                let area_less = strokes[i].macd_area < strokes[p].macd_area;
                let new_extreme = if strokes[i].direction == "down" {
                    strokes[i].end_price < strokes[p].end_price
                } else {
                    strokes[i].end_price > strokes[p].end_price
                };
                area_less && new_extreme
            }
        };
        strokes[i].has_divergence = diverged;
    }
}

/// 背驰置信度 = clamp(round(100 - 40 * 面积比), 55, 90)。
fn signal_confidence(area: f64, prev_area: f64) -> i64 {
    if prev_area <= 0.0 {
        return 55;
    }
    let ratio = area / prev_area;
    let raw = 100.0 - 40.0 * ratio;
    py_round_i(raw).clamp(55, 90)
}

/// 信号类型中文名（对应 Python `get_signal_type_name`，未知类型原样返回）。
fn get_signal_type_name(sig_type: &str) -> String {
    match sig_type {
        "buy1" => "一类买点",
        "sell1" => "一类卖点",
        other => return other.to_string(),
    }
    .to_string()
}

/// 从背驰笔生成一类买卖点信号（对应 Python `generate_signals`，按笔序不排序）。
fn generate_signals(strokes: &[Stroke]) -> Vec<Signal> {
    let mut signals: Vec<Signal> = Vec::new();
    for (i, st) in strokes.iter().enumerate() {
        if !st.has_divergence {
            continue;
        }
        let mut prev: Option<&Stroke> = None;
        for j in (0..i).rev() {
            if strokes[j].direction == st.direction {
                prev = Some(&strokes[j]);
                break;
            }
        }
        let Some(p) = prev else {
            continue;
        };
        let conf = signal_confidence(st.macd_area, p.macd_area);
        let (sig_type, desc) = if st.direction == "up" {
            (
                "sell1",
                format!(
                    "一类卖点：顶背驰，MACD面积{:.2}较前笔衰减，多头力度衰竭",
                    st.macd_area
                ),
            )
        } else {
            (
                "buy1",
                format!(
                    "一类买点：底背驰，MACD面积{:.2}较前笔衰减，空头力度衰竭",
                    st.macd_area
                ),
            )
        };
        signals.push(Signal {
            kind: sig_type.to_string(),
            price: st.end_price,
            time: st.end_time.clone(),
            description: desc,
            confidence: conf,
        });
    }
    signals
}

/// 生成 current_state / summary / description 三段文本（对应 Python `_describe_state`）。
fn describe_state(
    strokes: &[Stroke],
    signals: &[Signal],
    fractal_count: usize,
) -> (String, String, String) {
    if strokes.is_empty() {
        return (
            "笔形成中".to_string(),
            "暂无买卖信号".to_string(),
            format!("共{}个分型、0笔。笔形成中", fractal_count),
        );
    }

    let last_stroke = &strokes[strokes.len() - 1];
    let is_up = last_stroke.direction == "up";
    let direction_cn = if is_up { "向上" } else { "向下" };
    let bull_cn = if is_up { "多头" } else { "空头" };
    let latest = signals.last();

    let state;
    let summary;
    if last_stroke.has_divergence {
        // 末笔背驰 -> 一类买卖点风险提示
        let mut s = if last_stroke.direction == "up" {
            "向上笔顶背驰，注意一类卖点风险".to_string()
        } else {
            "向下笔底背驰，注意一类买点风险".to_string()
        };
        match latest {
            Some(l) => s.push_str(&format!("，最近{}信号在{}", l.kind, l.time)),
            None => {}
        }
        summary = match latest {
            Some(l) => format!("最新信号：{}@{:.2}", get_signal_type_name(&l.kind), l.price),
            None => "暂无买卖信号".to_string(),
        };
        state = s;
    } else {
        let mut s = format!("处于{}笔中，{}延续", direction_cn, bull_cn);
        match latest {
            Some(l) => {
                s.push_str(&format!("，最近{}信号在{}", l.kind, l.time));
                summary = format!("最新信号：{}@{:.2}", get_signal_type_name(&l.kind), l.price);
            }
            None => {
                summary = "暂无买卖信号".to_string();
            }
        }
        state = s;
    }

    let description = if !signals.is_empty() {
        format!(
            "共{}个分型、{}笔、{}个信号。{}",
            fractal_count,
            strokes.len(),
            signals.len(),
            state
        )
    } else {
        format!("共{}个分型、{}笔。{}", fractal_count, strokes.len(), state)
    };
    (state, summary, description)
}

/// 分钟缠论完整分析管线（对应 Python `analyze_chanlun_minute`）。
pub fn analyze_chanlun_minute(
    times: &[String],
    prices: &[f64],
    volumes: &[f64],
) -> ChanlunMinuteResult {
    let klines = construct_5min_klines(times, prices, volumes);
    let merged = merge_klines(&klines);
    let fractals = find_fractals(&merged);
    let mut strokes = find_strokes(&fractals);
    let (dif, dea, bar) = calc_macd(&klines);
    detect_divergence(&mut strokes, &bar, &klines);
    let signals = generate_signals(&strokes);
    let (state, summary, description) = describe_state(&strokes, &signals, fractals.len());
    ChanlunMinuteResult {
        kline_count: klines.len(),
        fractal_count: fractals.len(),
        fractals,
        stroke_count: strokes.len(),
        strokes,
        signals,
        macd_dif: dif,
        macd_dea: dea,
        macd_bar: bar,
        current_state: state,
        summary,
        description,
    }
}

/// 将分析结果序列化为 JSON（对应 Python `signals_to_dict`，macd_bar 保留 6 位小数）。
pub fn signals_to_dict(result: &ChanlunMinuteResult) -> serde_json::Value {
    json!({
        "kline_count": result.kline_count,
        "fractal_count": result.fractal_count,
        "stroke_count": result.stroke_count,
        "current_state": result.current_state,
        "summary": result.summary,
        "description": result.description,
        "signals": result
            .signals
            .iter()
            .map(|s| {
                json!({
                    "type": s.kind,
                    "type_name": get_signal_type_name(&s.kind),
                    "price": py_round(s.price, 2),
                    "time": s.time,
                    "description": s.description,
                    "confidence": s.confidence,
                })
            })
            .collect::<Vec<_>>(),
        "fractals": result
            .fractals
            .iter()
            .map(|f| {
                let type_name = if f.kind == "top" { "顶分型" } else { "底分型" };
                json!({
                    "type": f.kind,
                    "type_name": type_name,
                    "price": py_round(f.price, 2),
                    "time": f.time,
                })
            })
            .collect::<Vec<_>>(),
        "strokes": result
            .strokes
            .iter()
            .map(|s| {
                json!({
                    "direction": s.direction,
                    "start_price": py_round(s.start_price, 2),
                    "end_price": py_round(s.end_price, 2),
                    "start_time": s.start_time,
                    "end_time": s.end_time,
                    "macd_area": macd_area_value(s.macd_area, s.area_int_zero, 4),
                    "has_divergence": s.has_divergence,
                })
            })
            .collect::<Vec<_>>(),
        "macd_bar": result
            .macd_bar
            .iter()
            .map(|x| json!(py_round(*x, 6)))
            .collect::<Vec<_>>(),
    })
}
