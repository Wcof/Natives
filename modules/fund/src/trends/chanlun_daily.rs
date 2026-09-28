//! 缠论日线/周线分析（1:1 移植自 `engine/analysis/chanlun_daily.py` 明文版）。
//!
//! 管线：合并K线 → 分型 → 笔 → 中枢 → MACD背驰 → 买卖点信号。
//! 本模块的 `calc_daily_macd` 是 SMA-seed 特殊变体（EMA12/26 在索引 12/26 前恒为
//! SMA 值、DEA 在索引 9 前恒为前 9 个 DIF 的均值），与 `_indicators.py` 的
//! `macd_series` 及分钟版 `_ema_sma_seed` 是不同算法，禁止合并或抽共享。

use std::collections::HashMap;

use serde_json::json;

use super::{macd_area_value, py_round, py_round_i, KlineInput};

/// API 层数据门槛：engine `handle_chanlun_daily` 拒绝不足 10 根的 K 线
/// （app.py: `len(klines) < 10`）。该拒绝发生在接口层而非纯函数内，
/// P0 的分析函数与 Python 一致不对长度做限制；P3 接线时由路由层使用本常量。
pub const MIN_KLINE_COUNT: usize = 10;

/// 合并后 K 线（对应 Python `MergedDailyKline`）。
#[derive(Clone)]
struct MergedKline {
    date_start: String,
    date_end: String,
    high: f64,
    low: f64,
    direction: i32,
    raw_count: i32,
}

/// 分型（对应 Python `DailyFractal`）。`index` 为合并K线索引。
#[derive(Clone)]
struct Fractal {
    index: usize,
    kind: String, // "top" | "bottom"
    price: f64,
    date: String,
}

/// 笔（对应 Python `DailyStroke`）。`start_idx`/`end_idx` 为分型列表中的位置。
#[derive(Clone, Default)]
struct Stroke {
    direction: String,
    start_price: f64,
    end_price: f64,
    start_date: String,
    end_date: String,
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

/// 中枢（对应 Python `Zhongshu`）。
#[derive(Clone)]
struct Zhongshu {
    start_date: String,
    end_date: String,
    zg: f64,
    zd: f64,
    zz: f64,
    /// 与 Python 数据类字段一一对应保留（序列化契约不输出，P1 起消费）。
    #[allow(dead_code)]
    stroke_start_idx: usize,
    stroke_end_idx: usize,
    is_broken: bool,
    break_direction: String,
}

/// 买卖点信号（对应 Python `ChanlunDailySignal`）。
struct Signal {
    kind: String,
    price: f64,
    date: String,
    description: String,
    confidence: i64,
}

/// 完整分析结果（对应 Python `ChanlunDailyResult`）。
pub struct ChanlunDailyResult {
    kline_count: usize,
    merged_count: usize,
    fractal_count: usize,
    stroke_count: usize,
    zhongshu_count: usize,
    fractals: Vec<Fractal>,
    strokes: Vec<Stroke>,
    zhongshus: Vec<Zhongshu>,
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
    chart_signals: Vec<serde_json::Value>,
    chart_fractals: Vec<serde_json::Value>,
    chart_zhongshus: Vec<serde_json::Value>,
    chart_strokes: Vec<serde_json::Value>,
}

/// MACD(12,26,9)，以 SMA12/SMA26 作为 EMA 初值（与 Python `calc_daily_macd` 逐位一致）。
///
/// 关键：EMA12 在索引 12 前保持 SMA12 恒定，EMA26 在索引 26 前保持 SMA26 恒定，
/// DEA 在索引 9 前保持 SMA9（前 9 个 DIF 的均值）恒定；输入不足 12/26/9 个时
/// Python 按现有元素求和再除以固定分母，此处同样保留该边界行为。
fn calc_daily_macd(closes: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = closes.len();
    if n == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let s12 = closes.iter().take(12).sum::<f64>() / 12.0;
    let s26 = closes.iter().take(26).sum::<f64>() / 26.0;
    let mut ema12 = vec![s12; n];
    let mut ema26 = vec![s26; n];
    // EMA12 从索引 12 开始递推（之前保持 SMA12 恒定）
    for i in 12..n {
        ema12[i] = ema12[i - 1] + (closes[i] - ema12[i - 1]) * (2.0 / 13.0);
    }
    // EMA26 从索引 26 开始递推（之前保持 SMA26 恒定）
    for i in 26..n {
        ema26[i] = ema26[i - 1] + (closes[i] - ema26[i - 1]) * (2.0 / 27.0);
    }
    let dif: Vec<f64> = (0..n).map(|i| ema12[i] - ema26[i]).collect();
    // DEA = EMA9 of DIF，以 SMA9 作为初值
    let s_dea = dif.iter().take(9).sum::<f64>() / 9.0;
    let mut dea = vec![s_dea; n];
    for i in 9..n {
        dea[i] = dea[i - 1] + (dif[i] - dea[i - 1]) * (2.0 / 10.0);
    }
    let bar: Vec<f64> = (0..n).map(|i| (dif[i] - dea[i]) * 2.0).collect();
    (dif, dea, bar)
}

/// 合并K线：包含关系按当前方向取极值，方向由非包含K线高低点判定。
fn merge_daily_klines(dates: &[String], highs: &[f64], lows: &[f64]) -> Vec<MergedKline> {
    if dates.is_empty() {
        return Vec::new();
    }
    let mut merged: Vec<MergedKline> = Vec::new();
    let mut cur = MergedKline {
        date_start: dates[0].clone(),
        date_end: dates[0].clone(),
        high: highs[0],
        low: lows[0],
        direction: 0,
        raw_count: 1,
    };
    for i in 1..dates.len() {
        let (h, l) = (highs[i], lows[i]);
        // 包含判定：新K 完全位于当前段内 或 完全包裹当前段
        let contained = (h <= cur.high && l >= cur.low) || (h >= cur.high && l <= cur.low);
        if !contained {
            // 非包含：闭合当前段，新段方向由高点比较决定
            merged.push(cur.clone());
            let direction = if h > cur.high { 1 } else { -1 };
            cur = MergedKline {
                date_start: dates[i].clone(),
                date_end: dates[i].clone(),
                high: h,
                low: l,
                direction,
                raw_count: 1,
            };
        } else {
            // 包含：按当前方向扩展极值
            let (new_high, new_low) = match cur.direction {
                1 => (h.max(cur.high), l.max(cur.low)),
                -1 => (h.min(cur.high), l.min(cur.low)),
                _ => (h.max(cur.high), l.min(cur.low)),
            };
            cur = MergedKline {
                date_start: cur.date_start.clone(),
                date_end: dates[i].clone(),
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

/// 在合并K线上识别顶/底分型。index 为合并K线索引。
fn find_daily_fractals(merged: &[MergedKline]) -> Vec<Fractal> {
    let mut fractals = Vec::new();
    let n = merged.len();
    for i in 0..n {
        if i == 0 || i + 1 == n {
            continue;
        }
        let (left, cur, right) = (&merged[i - 1], &merged[i], &merged[i + 1]);
        // 顶分型：中间高点大于左右高点
        if cur.high > left.high && cur.high > right.high {
            fractals.push(Fractal {
                index: i,
                kind: "top".to_string(),
                price: cur.high,
                date: cur.date_end.clone(),
            });
        } else if cur.low < left.low && cur.low < right.low {
            // 底分型：中间低点小于左右低点
            fractals.push(Fractal {
                index: i,
                kind: "bottom".to_string(),
                price: cur.low,
                date: cur.date_end.clone(),
            });
        }
    }
    fractals
}

/// 在分型序列上构建笔（含中间分型吸收与极值延伸）。
///
/// 规则（与 Python `find_daily_strokes` 逐行对应）：
/// - 起点取首个分型，方向由首个分型类型决定（顶→向下笔，底→向上笔）；
/// - 遇到与起点同类型的分型：若尚无可闭合的终点，则当它比当前起点更极端
///   （向下笔更高的顶 / 向上笔更低的底）时更新起点，否则吸收；
/// - 遇到相反类型的分型：当它与当前起点的「合并K线索引差」>= 4 时设为终点；
/// - 一旦确定终点，闭合当前笔并反转方向，以终点作为下一笔起点继续——
///   注意与 Python 一致，此时不推进 j，同一分型立即按新起点重新判定；
/// - `start_idx`/`end_idx` 记录分型列表中的位置（非合并K线索引）。
fn find_daily_strokes(fractals: &[Fractal]) -> Vec<Stroke> {
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
                        start_date: start.date.clone(),
                        end_date: e.date.clone(),
                        start_idx: start_pos,
                        end_idx: end_pos,
                        ..Stroke::default()
                    });
                    start = e;
                    start_pos = end_pos;
                    direction = if direction == "down" { "up" } else { "down" };
                    end = None;
                    // 与 Python 一致：此处不 j += 1，同一分型按新起点重新判定
                }
                None => {
                    // 无已确定终点：若更极端则更新起点
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
            // 相反类型：合并K线索引差达到阈值才可成为终点
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
            start_date: start.date.clone(),
            end_date: e.date.clone(),
            start_idx: start_pos,
            end_idx: end_pos,
            ..Stroke::default()
        });
    }
    strokes
}

/// 构建中枢：相邻两笔的重叠区间，成对推进，落空时单步推进。
fn find_zhongshus(strokes: &[Stroke]) -> Vec<Zhongshu> {
    let mut zhongshus: Vec<Zhongshu> = Vec::new();
    let n = strokes.len();
    let mut i = 0usize;
    while i + 2 < n {
        let (a, b) = (&strokes[i], &strokes[i + 1]);
        // 两笔的四个端点构成区间，重叠部分为 [max(两低位), min(两高位)]
        let zd = a
            .start_price
            .min(a.end_price)
            .max(b.start_price.min(b.end_price));
        let zg = a
            .start_price
            .max(a.end_price)
            .min(b.start_price.max(b.end_price));
        if zd < zg {
            zhongshus.push(Zhongshu {
                start_date: a.start_date.clone(),
                end_date: b.end_date.clone(),
                zg,
                zd,
                zz: (zg + zd) / 2.0,
                stroke_start_idx: i,
                stroke_end_idx: i + 1,
                is_broken: false,
                break_direction: String::new(),
            });
            i += 2;
        } else {
            i += 1;
        }
    }

    // 确定每个中枢是否被后续笔突破及方向；突破时 end_date 取突破笔的起始日期
    for z in zhongshus.iter_mut() {
        for s in strokes.iter().skip(z.stroke_end_idx + 1) {
            if s.direction == "up" && s.end_price > z.zg {
                z.is_broken = true;
                z.break_direction = "up".to_string();
                z.end_date = s.start_date.clone();
                break;
            }
            if s.direction == "down" && s.end_price < z.zd {
                z.is_broken = true;
                z.break_direction = "down".to_string();
                z.end_date = s.start_date.clone();
                break;
            }
        }
    }
    zhongshus
}

/// 计算每笔 MACD 面积并标注背驰（就地修改笔对象）。
fn detect_daily_divergence(strokes: &mut [Stroke], macd_bar: &[f64], dates: &[String]) {
    let date_to_idx: HashMap<&str, usize> = dates
        .iter()
        .enumerate()
        .map(|(i, d)| (d.as_str(), i))
        .collect();
    for st in strokes.iter_mut() {
        match (
            date_to_idx.get(st.start_date.as_str()),
            date_to_idx.get(st.end_date.as_str()),
        ) {
            (Some(&si), Some(&ei)) => {
                // Python：sum() 作用在空切片（si > ei，重复日期极端场景）返回 int 0
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

/// 背驰置信度 = clamp(round(94.5 - 35 * 面积比), 55, 92)。
fn divergence_confidence(strokes: &[Stroke], idx: usize, direction: &str) -> i64 {
    let st = &strokes[idx];
    let mut prev: Option<&Stroke> = None;
    for j in (0..idx).rev() {
        if strokes[j].direction == direction {
            prev = Some(&strokes[j]);
            break;
        }
    }
    match prev {
        Some(p) if p.macd_area > 0.0 => {
            let ratio = st.macd_area / p.macd_area;
            let raw = 94.5 - 35.0 * ratio;
            py_round_i(raw).clamp(55, 92)
        }
        _ => 55,
    }
}

/// 信号类型中文名（对应 Python `get_signal_type_name`，未知类型原样返回）。
fn get_signal_type_name(sig_type: &str) -> String {
    match sig_type {
        "buy1" => "一类买点",
        "buy2" => "二类买点",
        "buy3" => "三类买点",
        "sell1" => "一类卖点",
        "sell2" => "二类卖点",
        "sell3" => "三类卖点",
        other => return other.to_string(),
    }
    .to_string()
}

/// 生成买卖点信号。
///
/// 顺序：[一类点按日期交错] + [二类买] + [二类卖] + [三类卖] + [三类买]。
/// 一类点置信度为背驰置信度；二类点固定 70；三类点固定 75。
fn generate_daily_signals(strokes: &[Stroke], zhongshus: &[Zhongshu]) -> Vec<Signal> {
    let mut type1: Vec<Signal> = Vec::new();
    let mut buy2_list: Vec<Signal> = Vec::new();
    let mut sell2_list: Vec<Signal> = Vec::new();
    let mut buy3_list: Vec<Signal> = Vec::new();
    let mut sell3_list: Vec<Signal> = Vec::new();

    // ---- 一类买卖点：背驰笔的终点 ----
    for (i, st) in strokes.iter().enumerate() {
        if !st.has_divergence {
            continue;
        }
        if st.direction == "down" {
            type1.push(Signal {
                kind: "buy1".to_string(),
                price: st.end_price,
                date: st.end_date.clone(),
                confidence: divergence_confidence(strokes, i, "down"),
                description: format!(
                    "一类买点：日线底背驰，MACD面积{:.1}较前笔衰减，空头力度衰竭",
                    st.macd_area
                ),
            });
        } else {
            type1.push(Signal {
                kind: "sell1".to_string(),
                price: st.end_price,
                date: st.end_date.clone(),
                confidence: divergence_confidence(strokes, i, "up"),
                description: format!(
                    "一类卖点：日线顶背驰，MACD面积{:.1}较前笔衰减，多头力度衰竭",
                    st.macd_area
                ),
            });
        }
    }
    type1.sort_by(|a, b| a.date.cmp(&b.date));

    // ---- 二类买卖点：一类点后的回调笔未破前低/前高 ----
    for (i, st) in strokes.iter().enumerate() {
        if st.has_divergence {
            let idx2 = i + 2;
            if idx2 < strokes.len() {
                let nxt = &strokes[idx2];
                if st.direction == "down" && nxt.direction == "down" && nxt.end_price > st.end_price
                {
                    buy2_list.push(Signal {
                        kind: "buy2".to_string(),
                        price: nxt.end_price,
                        date: nxt.end_date.clone(),
                        confidence: 70,
                        description: format!(
                            "二类买点：一类买点后反弹再回落，未破前低{:.2}",
                            st.end_price
                        ),
                    });
                } else if st.direction == "up"
                    && nxt.direction == "up"
                    && nxt.end_price < st.end_price
                {
                    sell2_list.push(Signal {
                        kind: "sell2".to_string(),
                        price: nxt.end_price,
                        date: nxt.end_date.clone(),
                        confidence: 70,
                        description: format!(
                            "二类卖点：一类卖点后回落再反弹，未破前高{:.2}",
                            st.end_price
                        ),
                    });
                }
            }
        }
    }
    buy2_list.sort_by(|a, b| a.date.cmp(&b.date));
    sell2_list.sort_by(|a, b| a.date.cmp(&b.date));

    // ---- 三类买卖点：中枢突破后的回抽笔（即中枢末笔 +2 的笔） ----
    for z in zhongshus.iter() {
        if !z.is_broken {
            continue;
        }
        let recess_idx = z.stroke_end_idx + 2;
        if recess_idx >= strokes.len() {
            continue;
        }
        let retro = &strokes[recess_idx];
        if z.break_direction == "up" {
            // 向上突破 -> 三类买点（回踩笔为向下笔）
            if retro.direction == "down" && retro.end_price > z.zg {
                buy3_list.push(Signal {
                    kind: "buy3".to_string(),
                    price: retro.end_price,
                    date: retro.end_date.clone(),
                    confidence: 75,
                    description: format!(
                        "三类买点：中枢[{:.2}-{:.2}]向上突破后回踩，未回到中枢内",
                        z.zd, z.zg
                    ),
                });
            }
        } else {
            // 向下突破 -> 三类卖点（反弹笔为向上笔）
            if retro.direction == "up" && retro.end_price < z.zd {
                sell3_list.push(Signal {
                    kind: "sell3".to_string(),
                    price: retro.end_price,
                    date: retro.end_date.clone(),
                    confidence: 75,
                    description: format!(
                        "三类卖点：中枢[{:.2}-{:.2}]向下突破后反弹，未回到中枢内",
                        z.zd, z.zg
                    ),
                });
            }
        }
    }
    sell3_list.sort_by(|a, b| a.date.cmp(&b.date));
    buy3_list.sort_by(|a, b| a.date.cmp(&b.date));

    type1.extend(buy2_list);
    type1.extend(sell2_list);
    type1.extend(sell3_list);
    type1.extend(buy3_list);
    type1
}

/// 生成 current_state / summary / description 三段文本。
fn describe_state(
    strokes: &[Stroke],
    zhongshus: &[Zhongshu],
    signals: &[Signal],
    fractal_count: usize,
) -> (String, String, String) {
    let last_stroke = strokes.last();
    let is_up = matches!(last_stroke, Some(s) if s.direction == "up");
    let direction_cn = if is_up { "向上" } else { "向下" };
    let bull_cn = if is_up { "多头" } else { "空头" };

    let zs_text = match zhongshus.last() {
        Some(z) => {
            if z.is_broken {
                if z.break_direction == "up" {
                    "已向上突破".to_string()
                } else {
                    "已向下突破".to_string()
                }
            } else {
                format!("[{:.2}-{:.2}]震荡中", z.zd, z.zg)
            }
        }
        None => "无中枢".to_string(),
    };

    let mut state = format!("处于{direction_cn}笔中，{bull_cn}延续，最近中枢{zs_text}");
    let summary;
    match signals.last() {
        Some(latest) => {
            let type_cn = get_signal_type_name(&latest.kind);
            state.push_str(&format!("，最新信号：{}@{:.2}", type_cn, latest.price));
            summary = format!("最新信号：{}@{:.2}({})", type_cn, latest.price, latest.date);
        }
        None => {
            state.push_str("，无信号");
            summary = "无信号".to_string();
        }
    }

    let description = format!(
        "共{}个分型、{}笔、{}个中枢、{}个信号。{}",
        fractal_count,
        strokes.len(),
        zhongshus.len(),
        signals.len(),
        state
    );
    (state, summary, description)
}

/// 构建折线图分型/笔/中枢/信号覆盖层。分型坐标为原始价格（不做舍入）。
#[allow(clippy::type_complexity)]
fn build_chart_overlay(
    fractals: &[Fractal],
    strokes: &[Stroke],
    zhongshus: &[Zhongshu],
    signals: &[Signal],
) -> (
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
) {
    let chart_fractals: Vec<serde_json::Value> = fractals
        .iter()
        .map(|f| {
            let border_color = if f.kind == "top" {
                "#A32D2D"
            } else {
                "#3B6D11"
            };
            json!({
                "coord": [f.date, f.price],
                "symbol": "circle",
                "symbolSize": 7,
                "itemStyle": {
                    "color": "transparent",
                    "borderColor": border_color,
                    "borderWidth": 1.5,
                },
                "fractal_type": f.kind,
            })
        })
        .collect();

    let chart_strokes: Vec<serde_json::Value> = strokes
        .iter()
        .map(|s| {
            let color = if s.direction == "down" {
                "#639922"
            } else {
                "#E24B4A"
            };
            let line_type = if s.has_divergence { "dashed" } else { "solid" };
            json!({
                "coords": [[s.start_date, s.start_price], [s.end_date, s.end_price]],
                "lineStyle": {
                    "color": color,
                    "width": 1.5,
                    "type": line_type,
                },
                "has_divergence": s.has_divergence,
            })
        })
        .collect();

    let chart_zhongshus: Vec<serde_json::Value> = zhongshus
        .iter()
        .map(|z| {
            json!({
                "xAxis": [z.start_date, z.end_date],
                "yAxis": [z.zd, z.zg],
                "itemStyle": {
                    "color": "rgba(83, 74, 183, 0.08)",
                    "borderColor": "rgba(83, 74, 183, 0.4)",
                },
                "broken": z.is_broken,
                "break_direction": z.break_direction,
                "zg": z.zg,
                "zd": z.zd,
            })
        })
        .collect();

    let chart_signals: Vec<serde_json::Value> = signals
        .iter()
        .map(|s| {
            // SIGNAL_STYLE.get(s.type, SIGNAL_STYLE["buy1"]) 的等价展开
            let (symbol, rotate, position, color) = match s.kind.as_str() {
                "buy2" => ("triangle", 0, "bottom", "#D85A30"),
                "buy3" => ("triangle", 0, "bottom", "#BA7517"),
                "sell1" => ("pin", 180, "top", "#639922"),
                "sell2" => ("pin", 180, "top", "#3B6D11"),
                "sell3" => ("pin", 180, "top", "#27500A"),
                _ => ("triangle", 0, "bottom", "#E24B4A"),
            };
            let type_cn = get_signal_type_name(&s.kind);
            json!({
                "coord": [s.date, s.price],
                "symbol": symbol,
                "symbolRotate": rotate,
                "symbolSize": 14,
                "itemStyle": { "color": color, "opacity": 0.9 },
                "label": {
                    "show": true,
                    "position": position,
                    "formatter": type_cn,
                    "fontSize": 10,
                    "color": color,
                },
                "type_name": type_cn,
                "date": s.date,
                "price": py_round(s.price, 2),
                "confidence": s.confidence,
                "description": s.description,
            })
        })
        .collect();
    (
        chart_signals,
        chart_fractals,
        chart_zhongshus,
        chart_strokes,
    )
}

/// 日线缠论完整分析管线（对应 Python `analyze_chanlun_daily`）。
pub fn analyze_chanlun_daily(klines: &KlineInput) -> ChanlunDailyResult {
    let merged = merge_daily_klines(&klines.dates, &klines.highs, &klines.lows);
    let fractals = find_daily_fractals(&merged);
    let mut strokes = find_daily_strokes(&fractals);
    let zhongshus = find_zhongshus(&strokes);
    let (dif, dea, bar) = calc_daily_macd(&klines.closes);
    detect_daily_divergence(&mut strokes, &bar, &klines.dates);
    let signals = generate_daily_signals(&strokes, &zhongshus);
    let (state, summary, description) =
        describe_state(&strokes, &zhongshus, &signals, fractals.len());
    let (cs, cf, cz, cst) = build_chart_overlay(&fractals, &strokes, &zhongshus, &signals);
    ChanlunDailyResult {
        kline_count: klines.dates.len(),
        merged_count: merged.len(),
        fractal_count: fractals.len(),
        stroke_count: strokes.len(),
        zhongshu_count: zhongshus.len(),
        fractals,
        strokes,
        zhongshus,
        signals,
        macd_dif: dif,
        macd_dea: dea,
        macd_bar: bar,
        current_state: state,
        summary,
        description,
        chart_signals: cs,
        chart_fractals: cf,
        chart_zhongshus: cz,
        chart_strokes: cst,
    }
}

/// 把结果对象转为 JSON（对应 Python `daily_result_to_dict`，供 A/B 对比与图表序列化）。
pub fn daily_result_to_dict(result: &ChanlunDailyResult) -> serde_json::Value {
    json!({
        "kline_count": result.kline_count,
        "merged_count": result.merged_count,
        "fractal_count": result.fractal_count,
        "stroke_count": result.stroke_count,
        "zhongshu_count": result.zhongshu_count,
        "fractals": result
            .fractals
            .iter()
            .map(|f| {
                let type_name = if f.kind == "top" { "顶分型" } else { "底分型" };
                json!({
                    "type": f.kind,
                    "type_name": type_name,
                    "price": py_round(f.price, 2),
                    "date": f.date,
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
                    "start_date": s.start_date,
                    "end_date": s.end_date,
                    "macd_area": macd_area_value(s.macd_area, s.area_int_zero, 2),
                    "has_divergence": s.has_divergence,
                })
            })
            .collect::<Vec<_>>(),
        "zhongshus": result
            .zhongshus
            .iter()
            .map(|z| {
                json!({
                    "start_date": z.start_date,
                    "end_date": z.end_date,
                    "zg": py_round(z.zg, 2),
                    "zd": py_round(z.zd, 2),
                    "zz": py_round(z.zz, 2),
                    "is_broken": z.is_broken,
                    "break_direction": z.break_direction,
                })
            })
            .collect::<Vec<_>>(),
        "signals": result
            .signals
            .iter()
            .map(|s| {
                json!({
                    "type": s.kind,
                    "type_name": get_signal_type_name(&s.kind),
                    "price": py_round(s.price, 2),
                    "date": s.date,
                    "confidence": s.confidence,
                    "description": s.description,
                })
            })
            .collect::<Vec<_>>(),
        "current_state": result.current_state,
        "summary": result.summary,
        "description": result.description,
        "chart_signals": result.chart_signals,
        "chart_fractals": result.chart_fractals,
        "chart_zhongshus": result.chart_zhongshus,
        "chart_strokes": result.chart_strokes,
    })
}
