"""缠论日线/周线分析（明文版）。

管线：合并K线 → 分型 → 笔 → 中枢 → MACD背驰 → 买卖点信号。
所有算法与阈值均从加密版(analysis/chanlun_daily.py)黑盒反推，
并已通过 8 只基准股（600519/000001/600036/300750/601318/000858/600900/002594）
的 A/B 回归验证。
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import List, Optional, Tuple


@dataclass
class MergedDailyKline:
    date_start: str
    date_end: str
    high: float
    low: float
    direction: int
    raw_count: int


@dataclass
class DailyFractal:
    index: int
    type: str
    price: float
    date: str


@dataclass
class DailyStroke:
    direction: str
    start_price: float
    end_price: float
    start_date: str
    end_date: str
    start_idx: int
    end_idx: int
    macd_area: float = 0.0
    has_divergence: bool = False


@dataclass
class Zhongshu:
    start_date: str
    end_date: str
    zg: float
    zd: float
    zz: float
    stroke_start_idx: int = 0
    stroke_end_idx: int = 0
    is_broken: bool = False
    break_direction: str = ""


@dataclass
class ChanlunDailySignal:
    type: str
    price: float
    date: str
    description: str
    confidence: int = 50


@dataclass
class ChanlunDailyResult:
    kline_count: int
    merged_count: int
    fractal_count: int
    stroke_count: int
    zhongshu_count: int
    fractals: List[DailyFractal]
    strokes: List[DailyStroke]
    zhongshus: List[Zhongshu]
    signals: List[ChanlunDailySignal]
    macd_dif: List[float]
    macd_dea: List[float]
    macd_bar: List[float]
    current_state: str
    summary: str
    description: str
    chart_signals: List[dict]
    chart_fractals: List[dict]
    chart_zhongshus: List[dict]
    chart_strokes: List[dict]


def calc_daily_macd(closes: List[float]) -> Tuple[List[float], List[float], List[float]]:
    """MACD(12,26,9)，以 SMA12/SMA26 作为 EMA 初值（与加密版一致）。

    关键：EMA12 在索引 12 前保持 SMA12 恒定，EMA26 在索引 26 前保持 SMA26 恒定，
    DEA 在索引 9 前保持 SMA9(即前 9 个 DIF 的均值)恒定。这样前 12 项 DIF 恒为
    (SMA12 - SMA26)，笔面积能精确复现加密版的 0 起始值。
    """
    n = len(closes)
    if n == 0:
        return [], [], []
    s12 = sum(closes[:12]) / 12.0
    s26 = sum(closes[:26]) / 26.0
    ema12 = [s12] * n
    ema26 = [s26] * n
    # EMA12 从索引 12 开始递归（之前保持 SMA12 恒定）
    for i in range(12, n):
        ema12[i] = ema12[i - 1] + (closes[i] - ema12[i - 1]) * (2.0 / 13.0)
    # EMA26 从索引 26 开始递归（之前保持 SMA26 恒定）
    for i in range(26, n):
        ema26[i] = ema26[i - 1] + (closes[i] - ema26[i - 1]) * (2.0 / 27.0)
    dif = [ema12[i] - ema26[i] for i in range(n)]
    # DEA = EMA9 of DIF，以 SMA9 作为初值
    dea = [0.0] * n
    s_dea = sum(dif[:9]) / 9.0
    dea = [s_dea] * n
    for i in range(9, n):
        dea[i] = dea[i - 1] + (dif[i] - dea[i - 1]) * (2.0 / 10.0)
    bar = [(dif[i] - dea[i]) * 2.0 for i in range(n)]
    return dif, dea, bar


def merge_daily_klines(dates: List[str], highs: List[float], lows: List[float]) -> List[MergedDailyKline]:
    """合并K线：包含关系按当前方向取极值，方向由非包含K线高低点判定。"""
    if not dates:
        return []
    merged = []
    cur = MergedDailyKline(dates[0], dates[0], highs[0], lows[0], 0, 1)
    for i in range(1, len(dates)):
        h, l = highs[i], lows[i]
        # 包含判定：新K 完全位于当前段内 或 完全包裹当前段
        contained = (h <= cur.high and l >= cur.low) or (h >= cur.high and l <= cur.low)
        if not contained:
            # 非包含：闭合当前段，新段方向由高点比较决定
            merged.append(cur)
            direction = 1 if h > cur.high else -1
            cur = MergedDailyKline(dates[i], dates[i], h, l, direction, 1)
        else:
            # 包含：按当前方向扩展极值
            if cur.direction == 1:
                new_high = max(h, cur.high)
                new_low = max(l, cur.low)
            elif cur.direction == -1:
                new_high = min(h, cur.high)
                new_low = min(l, cur.low)
            else:
                new_high = max(h, cur.high)
                new_low = min(l, cur.low)
            cur = MergedDailyKline(cur.date_start, dates[i], new_high, new_low, cur.direction, cur.raw_count + 1)
    if cur is not None:
        merged.append(cur)
    return merged

def find_daily_fractals(merged: List[MergedDailyKline]) -> List[DailyFractal]:
    """在合并K线上识别顶/底分型。index 为合并K线索引。"""
    fractals = []
    n = len(merged)
    for i in range(n):
        if i == 0 or i == n - 1:
            continue
        left, cur, right = merged[i - 1], merged[i], merged[i + 1]
        # 顶分型：中间高点大于左右高点
        if cur.high > left.high and cur.high > right.high:
            fractals.append(DailyFractal(
                index=i, type="top", price=cur.high, date=cur.date_end))
        # 底分型：中间低点小于左右低点
        elif cur.low < left.low and cur.low < right.low:
            fractals.append(DailyFractal(
                index=i, type="bottom", price=cur.low, date=cur.date_end))
    return fractals


def find_daily_strokes(fractals: List[DailyFractal],
                       merged: List[MergedDailyKline]) -> List[DailyStroke]:
    """在分型序列上构建笔（含中间分型吸收与极值延伸）。

    规则（从加密版黑盒反推，已在 8 股 164 笔验证）：
    - 起点取首个分型，方向由首个分型类型决定（顶→向下笔，底→向上笔）。
    - 遇到与起点同类型的分型：若尚无可闭合的终点，则当它比当前起点更极端（向下笔
      更高的顶 / 向上笔更低的底）时更新起点；否则吸收。
    - 遇到相反类型的分型：当它与当前起点的『合并K线索引差』>= 4 时设为终点。
    - 一旦确定终点，闭合当前笔并反转方向，以终点作为下一笔起点继续。
    - start_idx/end_idx 记录的是分型列表中的位置（非合并K线索引）。
    """
    strokes: List[DailyStroke] = []
    n = len(fractals)
    if n == 0:
        return strokes

    start = fractals[0]
    start_pos = 0
    direction = "down" if start.type == "top" else "up"
    end = None
    end_pos = 0
    j = 1
    while j < n:
        f = fractals[j]
        if f.type == start.type:
            if end is not None:
                strokes.append(DailyStroke(
                    direction, start.price, end.price, start.date, end.date,
                    start_pos, end_pos))
                start = end
                start_pos = end_pos
                direction = "up" if direction == "down" else "down"
                end = None
            else:
                # 无已确定终点：若更极端则更新起点
                if (direction == "down" and f.price > start.price) or \
                   (direction == "up" and f.price < start.price):
                    start = f
                    start_pos = j
                j += 1
        else:
            # 相反类型：合并K线索引差达到阈值才可成为终点
            if f.index - start.index >= 4:
                end = f
                end_pos = j
            j += 1

    if end is not None:
        strokes.append(DailyStroke(
            direction, start.price, end.price, start.date, end.date,
            start_pos, end_pos))
    return strokes


def find_zhongshus(strokes: List[DailyStroke]) -> List[Zhongshu]:
    """构建中枢：相邻两笔的重叠区间，成对推进，落空时单步推进。"""
    zhongshus: List[Zhongshu] = []
    n = len(strokes)
    i = 0
    while i + 2 < n:
        a, b = strokes[i], strokes[i + 1]
        # 两笔的四个端点构成区间，重叠部分为 [max(两低位), min(两高位)]
        zd = max(min(a.start_price, a.end_price), min(b.start_price, b.end_price))
        zg = min(max(a.start_price, a.end_price), max(b.start_price, b.end_price))
        if zd < zg:
            zhongshus.append(Zhongshu(
                start_date=a.start_date, end_date=b.end_date,
                zg=zg, zd=zd, zz=(zg + zd) / 2.0,
                stroke_start_idx=i, stroke_end_idx=i + 1))
            i += 2
        else:
            i += 1

    # 确定每个中枢是否被后续笔突破及方向；突破时 end_date 取突破笔的起始日期
    for z in zhongshus:
        for s in strokes[z.stroke_end_idx + 1:]:
            if s.direction == "up" and s.end_price > z.zg:
                z.is_broken = True
                z.break_direction = "up"
                z.end_date = s.start_date
                break
            if s.direction == "down" and s.end_price < z.zd:
                z.is_broken = True
                z.break_direction = "down"
                z.end_date = s.start_date
                break
    return zhongshus


def detect_daily_divergence(strokes: List[DailyStroke],
                            macd_bar: List[float],
                            dates: List[str]) -> None:
    """计算每笔 MACD 面积并标注背驰（就地修改笔对象）。"""
    date_to_idx = {d: i for i, d in enumerate(dates)}
    for st in strokes:
        si = date_to_idx.get(st.start_date)
        ei = date_to_idx.get(st.end_date)
        if si is None or ei is None:
            st.macd_area = 0.0
            st.has_divergence = False
            continue
        st.macd_area = sum(abs(x) for x in macd_bar[si:ei + 1])

    for i, st in enumerate(strokes):
        prev = None
        for j in range(i - 1, -1, -1):
            if strokes[j].direction == st.direction:
                prev = strokes[j]
                break
        if prev is None:
            st.has_divergence = False
            continue
        area_less = st.macd_area < prev.macd_area
        new_extreme = (st.end_price < prev.end_price) if st.direction == "down" \
            else (st.end_price > prev.end_price)
        st.has_divergence = area_less and new_extreme


def _divergence_confidence(strokes: List[DailyStroke], idx: int,
                           direction: str) -> int:
    """背驰置信度 = clamp(round(94.5 - 35 * 面积比), 55, 92)。"""
    st = strokes[idx]
    prev = None
    for j in range(idx - 1, -1, -1):
        if strokes[j].direction == direction:
            prev = strokes[j]
            break
    if prev is None or prev.macd_area <= 0:
        return 55
    ratio = st.macd_area / prev.macd_area
    raw = 94.5 - 35.0 * ratio
    return max(55, min(92, int(round(raw))))


SIGNAL_STYLE = {
    "buy1": {"symbol": "triangle", "rotate": 0, "position": "bottom", "color": "#E24B4A"},
    "buy2": {"symbol": "triangle", "rotate": 0, "position": "bottom", "color": "#D85A30"},
    "buy3": {"symbol": "triangle", "rotate": 0, "position": "bottom", "color": "#BA7517"},
    "sell1": {"symbol": "pin", "rotate": 180, "position": "top", "color": "#639922"},
    "sell2": {"symbol": "pin", "rotate": 180, "position": "top", "color": "#3B6D11"},
    "sell3": {"symbol": "pin", "rotate": 180, "position": "top", "color": "#27500A"},
}


def generate_daily_signals(strokes: List[DailyStroke],
                           fractals: List[DailyFractal],
                           zhongshus: List[Zhongshu]) -> List[ChanlunDailySignal]:
    """生成买卖点信号。

    顺序：[一类点按日期交错] + [二类买] + [二类卖] + [三类卖] + [三类买]。
    一类点置信度为背驰置信度；二类点固定 70；三类点固定 75。
    """
    type1: List[ChanlunDailySignal] = []
    buy2_list: List[ChanlunDailySignal] = []
    sell2_list: List[ChanlunDailySignal] = []
    buy3_list: List[ChanlunDailySignal] = []
    sell3_list: List[ChanlunDailySignal] = []

    # ---- 一类买卖点：背驰笔的终点 ----
    for i, st in enumerate(strokes):
        if not st.has_divergence:
            continue
        if st.direction == "down":
            type1.append(ChanlunDailySignal(
                type="buy1", price=st.end_price, date=st.end_date,
                confidence=_divergence_confidence(strokes, i, "down"),
                description="一类买点：日线底背驰，MACD面积{0:.1f}较前笔衰减，空头力度衰竭".format(
                    st.macd_area)))
        else:
            type1.append(ChanlunDailySignal(
                type="sell1", price=st.end_price, date=st.end_date,
                confidence=_divergence_confidence(strokes, i, "up"),
                description="一类卖点：日线顶背驰，MACD面积{0:.1f}较前笔衰减，多头力度衰竭".format(
                    st.macd_area)))
    type1.sort(key=lambda s: s.date)

    # ---- 二类买卖点：一类点后的回调笔未破前低/前高 ----
    for i, st in enumerate(strokes):
        if st.has_divergence:
            idx2 = i + 2
            if idx2 < len(strokes):
                nxt = strokes[idx2]
                if st.direction == "down" and nxt.direction == "down" and nxt.end_price > st.end_price:
                    buy2_list.append(ChanlunDailySignal(
                        type="buy2", price=nxt.end_price, date=nxt.end_date,
                        confidence=70,
                        description="二类买点：一类买点后反弹再回落，未破前低{0:.2f}".format(st.end_price)))
                elif st.direction == "up" and nxt.direction == "up" and nxt.end_price < st.end_price:
                    sell2_list.append(ChanlunDailySignal(
                        type="sell2", price=nxt.end_price, date=nxt.end_date,
                        confidence=70,
                        description="二类卖点：一类卖点后回落再反弹，未破前高{0:.2f}".format(st.end_price)))
    buy2_list.sort(key=lambda s: s.date)
    sell2_list.sort(key=lambda s: s.date)

    # ---- 三类买卖点：中枢突破后的回抽笔（即中枢末笔 +2 的笔） ----
    for z in zhongshus:
        if not z.is_broken:
            continue
        recess_idx = z.stroke_end_idx + 2
        if recess_idx >= len(strokes):
            continue
        retro = strokes[recess_idx]
        if z.break_direction == "up":  # 向上突破 -> 三类买点（回踩笔为向下笔）
            if retro.direction == "down" and retro.end_price > z.zg:
                buy3_list.append(ChanlunDailySignal(
                    type="buy3", price=retro.end_price, date=retro.end_date,
                    confidence=75,
                    description="三类买点：中枢[{0:.2f}-{1:.2f}]向上突破后回踩，未回到中枢内".format(z.zd, z.zg)))
        else:  # 向下突破 -> 三类卖点（反弹笔为向上笔）
            if retro.direction == "up" and retro.end_price < z.zd:
                sell3_list.append(ChanlunDailySignal(
                    type="sell3", price=retro.end_price, date=retro.end_date,
                    confidence=75,
                    description="三类卖点：中枢[{0:.2f}-{1:.2f}]向下突破后反弹，未回到中枢内".format(z.zd, z.zg)))
    sell3_list.sort(key=lambda s: s.date)
    buy3_list.sort(key=lambda s: s.date)

    return type1 + buy2_list + sell2_list + sell3_list + buy3_list


def get_signal_type_name(sig_type: str) -> str:
    return {
        "buy1": "一类买点", "buy2": "二类买点", "buy3": "三类买点",
        "sell1": "一类卖点", "sell2": "二类卖点", "sell3": "三类卖点",
    }.get(sig_type, sig_type)


def _describe_state(strokes: List[DailyStroke],
                    zhongshus: List[Zhongshu],
                    signals: List[ChanlunDailySignal],
                    fractal_count: int) -> Tuple[str, str, str]:
    """生成 current_state / summary / description 三段文本。"""
    last_stroke = strokes[-1] if strokes else None
    is_up = bool(last_stroke and last_stroke.direction == "up")
    direction_cn = "向上" if is_up else "向下"
    bull_cn = "多头" if is_up else "空头"

    if zhongshus:
        last_z = zhongshus[-1]
        if last_z.is_broken:
            zs_text = "已向上突破" if last_z.break_direction == "up" else "已向下突破"
        else:
            zs_text = "[{0:.2f}-{1:.2f}]震荡中".format(last_z.zd, last_z.zg)
    else:
        zs_text = "无中枢"

    latest = signals[-1] if signals else None
    state = "处于{0}笔中，{1}延续，最近中枢{2}".format(direction_cn, bull_cn, zs_text)
    if latest:
        type_cn = get_signal_type_name(latest.type)
        state += "，最新信号：{0}@{1:.2f}".format(type_cn, latest.price)
        summary = "最新信号：{0}@{1:.2f}({2})".format(type_cn, latest.price, latest.date)
    else:
        state += "，无信号"
        summary = "无信号"

    description = "共{0}个分型、{1}笔、{2}个中枢、{3}个信号。{4}".format(
        fractal_count, len(strokes), len(zhongshus), len(signals), state)
    return state, summary, description


def _build_chart_overlay(fractals: List[DailyFractal],
                         strokes: List[DailyStroke],
                         zhongshus: List[Zhongshu],
                         signals: List[ChanlunDailySignal],
                         dates: List[str]) -> Tuple[List[dict], List[dict], List[dict], List[dict]]:
    """构建折线图分型/笔/中枢/信号覆盖层。分型坐标为原始 3 位小数价格。"""
    chart_fractals = [{
        "coord": [f.date, f.price],
        "symbol": "circle",
        "symbolSize": 7,
        "itemStyle": {"color": "transparent",
                      "borderColor": "#A32D2D" if f.type == "top" else "#3B6D11",
                      "borderWidth": 1.5},
        "fractal_type": f.type,
    } for f in fractals]

    chart_strokes = [{
        "coords": [[s.start_date, s.start_price], [s.end_date, s.end_price]],
        "lineStyle": {
            "color": "#639922" if s.direction == "down" else "#E24B4A",
            "width": 1.5,
            "type": "dashed" if s.has_divergence else "solid"},
        "has_divergence": s.has_divergence,
    } for s in strokes]

    chart_zhongshus = [{
        "xAxis": [z.start_date, z.end_date],
        "yAxis": [z.zd, z.zg],
        "itemStyle": {"color": "rgba(83, 74, 183, 0.08)",
                      "borderColor": "rgba(83, 74, 183, 0.4)"},
        "broken": z.is_broken,
        "break_direction": z.break_direction,
        "zg": z.zg,
        "zd": z.zd,
    } for z in zhongshus]

    chart_signals = []
    for s in signals:
        style = SIGNAL_STYLE.get(s.type, SIGNAL_STYLE["buy1"])
        type_cn = get_signal_type_name(s.type)
        chart_signals.append({
            "coord": [s.date, s.price],
            "symbol": style["symbol"],
            "symbolRotate": style["rotate"],
            "symbolSize": 14,
            "itemStyle": {"color": style["color"], "opacity": 0.9},
            "label": {"show": True, "position": style["position"],
                      "formatter": type_cn, "fontSize": 10, "color": style["color"]},
            "type_name": type_cn,
            "date": s.date,
            "price": round(s.price, 2),
            "confidence": s.confidence,
            "description": s.description,
        })
    return chart_signals, chart_fractals, chart_zhongshus, chart_strokes


def analyze_chanlun_daily(dates: List[str], opens: List[float], closes: List[float],
                          highs: List[float], lows: List[float],
                          volumes: List[float]) -> ChanlunDailyResult:
    """日线缠论完整分析管线。"""
    merged = merge_daily_klines(dates, highs, lows)
    fractals = find_daily_fractals(merged)
    strokes = find_daily_strokes(fractals, merged)
    zhongshus = find_zhongshus(strokes)
    dif, dea, bar = calc_daily_macd(closes)
    detect_daily_divergence(strokes, bar, dates)
    signals = generate_daily_signals(strokes, fractals, zhongshus)
    state, summary, description = _describe_state(strokes, zhongshus, signals, len(fractals))
    cs, cf, cz, cst = _build_chart_overlay(fractals, strokes, zhongshus, signals, dates)
    return ChanlunDailyResult(
        kline_count=len(dates), merged_count=len(merged), fractal_count=len(fractals),
        stroke_count=len(strokes), zhongshu_count=len(zhongshus),
        fractals=fractals, strokes=strokes, zhongshus=zhongshus, signals=signals,
        macd_dif=dif, macd_dea=dea, macd_bar=bar,
        current_state=state, summary=summary, description=description,
        chart_signals=cs, chart_fractals=cf, chart_zhongshus=cz, chart_strokes=cst)


def daily_result_to_dict(result: ChanlunDailyResult) -> dict:
    """把结果对象转为字典（供 A/B 对比与图表序列化）。"""
    return {
        "kline_count": result.kline_count,
        "merged_count": result.merged_count,
        "fractal_count": result.fractal_count,
        "stroke_count": result.stroke_count,
        "zhongshu_count": result.zhongshu_count,
        "fractals": [{"type": f.type,
                      "type_name": "顶分型" if f.type == "top" else "底分型",
                      "price": round(f.price, 2), "date": f.date} for f in result.fractals],
        "strokes": [{"direction": s.direction,
                     "start_price": round(s.start_price, 2),
                     "end_price": round(s.end_price, 2),
                     "start_date": s.start_date, "end_date": s.end_date,
                     "macd_area": round(s.macd_area, 2),
                     "has_divergence": s.has_divergence} for s in result.strokes],
        "zhongshus": [{"start_date": z.start_date, "end_date": z.end_date,
                       "zg": round(z.zg, 2), "zd": round(z.zd, 2),
                       "zz": round(z.zz, 2),
                       "is_broken": z.is_broken, "break_direction": z.break_direction}
                      for z in result.zhongshus],
        "signals": [{"type": s.type, "type_name": get_signal_type_name(s.type),
                     "price": round(s.price, 2), "date": s.date,
                     "confidence": s.confidence, "description": s.description}
                    for s in result.signals],
        "current_state": result.current_state,
        "summary": result.summary,
        "description": result.description,
        "chart_signals": result.chart_signals,
        "chart_fractals": result.chart_fractals,
        "chart_zhongshus": result.chart_zhongshus,
        "chart_strokes": result.chart_strokes,
    }
