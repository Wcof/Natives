"""缠论分钟线分析（明文版）。
与加密版 analysis/chanlun_minute.py 接口完全一致：
  analyze_chanlun_minute(times, prices, volumes) -> ChanlunMinuteResult
  signals_to_dict(result) -> dict
算法要点（均从加密版黑盒反推并 A/B 验证）：
- construct_5min_klines：每 5 分钟聚合成一根 K 线（区间首分钟 open、末分钟 close、
  high=max、low=min、volume=sum），time 取区间最后一分钟时间戳。
- calc_macd：SMA12/SMA26 种子 EMA，DEA=SMA9(dif) 种子，bar=(dif-dea)*2。
- merge_klines / find_fractals / find_strokes：与日线版同构（笔端点索引差 >= 4）。
- detect_divergence：面积 = sum(abs(bar[si:ei+1]))，time 映射到 5min K 线索引；
  背驰 = 面积较同方向前笔衰减 且 价格创新极值。
- generate_signals：背驰笔 -> buy1（向下笔）/ sell1（向上笔），
  confidence = clamp(round(100 - 40*ratio), 55, 90)。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple


@dataclass
class MinuteKline:
    """5 分钟 K 线。"""
    time: str
    open: float
    close: float
    high: float
    low: float
    volume: float


@dataclass
class MergedKline:
    """包含关系合并后的 K 线。"""
    time_start: str
    time_end: str
    high: float
    low: float
    direction: int
    raw_count: int


@dataclass
class Fractal:
    """分型。index 为合并 K 线索引。"""
    index: int
    type: str  # "top" | "bottom"
    price: float
    time: str


@dataclass
class Stroke:
    """笔。start_idx/end_idx 为分型列表中的位置。"""
    direction: str  # "up" | "down"
    start_price: float
    end_price: float
    start_time: str
    end_time: str
    start_idx: int
    end_idx: int
    macd_area: float = 0.0
    has_divergence: bool = False


@dataclass
class ChanlunSignal:
    """买卖点信号。"""
    type: str
    price: float
    time: str
    description: str
    confidence: int


@dataclass
class ChanlunMinuteResult:
    """缠论分钟线分析结果。"""
    kline_count: int
    fractal_count: int
    fractals: List[Fractal]
    stroke_count: int
    strokes: List[Stroke]
    signals: List[ChanlunSignal]
    macd_dif: List[float]
    macd_dea: List[float]
    macd_bar: List[float]
    current_state: str
    summary: str
    description: str


def construct_5min_klines(times: List[str], prices: List[float],
                          volumes: List[float]) -> List[MinuteKline]:
    """将分钟数据按 5 分钟区间聚合为 5 分钟 K 线。"""
    klines: List[MinuteKline] = []
    n = len(times)
    if n == 0:
        return klines

    # 按连续交易时段（上午/下午/其他）分别从时段第一分钟起每 5 分钟一组，
    # time 取组内最后一分钟的原始时间戳。任意分钟时间属于同一时段。
    group = []
    group_start_minute = _to_minutes(times[0])
    for i in range(n):
        cur_minute = _to_minutes(times[i])
        # 检测时段断裂：与上一分钟出现非 1 分钟间隔（午休等）
        if group and cur_minute - _to_minutes(times[i - 1]) != 1:
            klines.append(_make_kline(times, prices, volumes, group))
            group = []
            group_start_minute = cur_minute
        if not group:
            group_start_minute = cur_minute
        group.append(i)
        # 满 5 分钟或已到最后一个且累计不足 5 时收拢
        if cur_minute - group_start_minute == 4 or (i == n - 1 and group):
            klines.append(_make_kline(times, prices, volumes, group))
            group = []
    return klines


def _to_minutes(t: str) -> int:
    """HH:MM -> 当日分钟数。"""
    hh, mm = t.split(":")
    return int(hh) * 60 + int(mm)


def _make_kline(times: List[str], prices: List[float], volumes: List[float],
                group: List[int]) -> MinuteKline:
    """根据组内分钟下标聚合一根 5 分钟 K 线。"""
    g_times = [times[i] for i in group]
    g_prices = [prices[i] for i in group]
    return MinuteKline(
        time=g_times[-1],
        open=g_prices[0],
        close=g_prices[-1],
        high=max(g_prices),
        low=min(g_prices),
        volume=sum(volumes[i] for i in group),
    )


def calc_macd(klines: List[MinuteKline]) -> Tuple[List[float], List[float], List[float]]:
    """SMA 种子 EMA 的 MACD 计算（与日线版一致），返回 (dif, dea, bar)。"""
    closes = [k.close for k in klines]
    ema12 = _ema_sma_seed(closes, 12)
    ema26 = _ema_sma_seed(closes, 26)
    dif = [a - b for a, b in zip(ema12, ema26)]
    dea = _ema_sma_seed(dif, 9)
    bar = [(d - s) * 2 for d, s in zip(dif, dea)]
    return dif, dea, bar


def _ema_sma_seed(seq: List[float], n: int) -> List[float]:
    """EMA：前 n 个值保持为 SMA(n)，之后按 EMA 递推。"""
    if not seq:
        return []
    k = 2.0 / (n + 1)
    seed = sum(seq[:n]) / n
    out = [seed] * min(n, len(seq))
    for i in range(n, len(seq)):
        out.append(seq[i] * k + out[-1] * (1 - k))
    return out


def merge_klines(klines: List[MinuteKline]) -> List[MergedKline]:
    """合并 K 线：包含关系按当前方向取极值。"""
    if not klines:
        return []
    merged: List[MergedKline] = []
    cur = MergedKline(klines[0].time, klines[0].time, klines[0].high, klines[0].low, 0, 1)
    for i in range(1, len(klines)):
        h, l = klines[i].high, klines[i].low
        contained = (h <= cur.high and l >= cur.low) or (h >= cur.high and l <= cur.low)
        if not contained:
            merged.append(cur)
            direction = 1 if h > cur.high else -1
            cur = MergedKline(klines[i].time, klines[i].time, h, l, direction, 1)
        else:
            if cur.direction == 1:
                new_high = max(h, cur.high)
                new_low = max(l, cur.low)
            elif cur.direction == -1:
                new_high = min(h, cur.high)
                new_low = min(l, cur.low)
            else:
                new_high = max(h, cur.high)
                new_low = min(l, cur.low)
            cur = MergedKline(cur.time_start, klines[i].time, new_high, new_low,
                              cur.direction, cur.raw_count + 1)
    merged.append(cur)
    return merged


def find_fractals(merged: List[MergedKline]) -> List[Fractal]:
    """在合并 K 线上识别顶/底分型。index 为合并 K 线索引。"""
    fractals: List[Fractal] = []
    n = len(merged)
    for i in range(1, n - 1):
        left, cur, right = merged[i - 1], merged[i], merged[i + 1]
        if cur.high > left.high and cur.high > right.high:
            fractals.append(Fractal(index=i, type="top", price=cur.high, time=cur.time_end))
        elif cur.low < left.low and cur.low < right.low:
            fractals.append(Fractal(index=i, type="bottom", price=cur.low, time=cur.time_end))
    return fractals


def find_strokes(fractals: List[Fractal], merged: List[MergedKline]) -> List[Stroke]:
    """在分型序列上构建笔（与日线版同规则）。"""
    strokes: List[Stroke] = []
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
                strokes.append(Stroke(
                    direction, start.price, end.price, start.time, end.time,
                    start_pos, end_pos))
                start = end
                start_pos = end_pos
                direction = "up" if direction == "down" else "down"
                end = None
            else:
                if (direction == "down" and f.price > start.price) or \
                   (direction == "up" and f.price < start.price):
                    start = f
                    start_pos = j
                j += 1
        else:
            if f.index - start.index >= 4:
                end = f
                end_pos = j
            j += 1

    if end is not None:
        strokes.append(Stroke(
            direction, start.price, end.price, start.time, end.time,
            start_pos, end_pos))
    return strokes


def detect_divergence(strokes: List[Stroke], macd_bar: List[float],
                      klines: List[MinuteKline]) -> None:
    """计算每笔 MACD 面积并标注背驰（就地修改笔对象）。"""
    time_to_idx = {k.time: i for i, k in enumerate(klines)}
    for st in strokes:
        si = time_to_idx.get(st.start_time)
        ei = time_to_idx.get(st.end_time)
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


def _signal_confidence(area: float, prev_area: float) -> int:
    """背驰置信度 = clamp(round(100 - 40 * 面积比), 55, 90)。"""
    if prev_area <= 0:
        return 55
    ratio = area / prev_area
    raw = 100 - 40.0 * ratio
    return max(55, min(90, int(round(raw))))


def get_signal_type_name(sig_type: str) -> str:
    """信号类型中文名。"""
    names = {"buy1": "一类买点", "sell1": "一类卖点"}
    return names.get(sig_type, sig_type)


def generate_signals(strokes: List[Stroke], fractals: List[Fractal]) -> List[ChanlunSignal]:
    """从背驰笔生成一类买卖点信号。"""
    signals: List[ChanlunSignal] = []
    for i, st in enumerate(strokes):
        if not st.has_divergence:
            continue
        prev = None
        for j in range(i - 1, -1, -1):
            if strokes[j].direction == st.direction:
                prev = strokes[j]
                break
        if prev is None:
            continue
        conf = _signal_confidence(st.macd_area, prev.macd_area)
        if st.direction == "up":
            sig_type = "sell1"
            desc = "一类卖点：顶背驰，MACD面积{0:.2f}较前笔衰减，多头力度衰竭".format(st.macd_area)
        else:
            sig_type = "buy1"
            desc = "一类买点：底背驰，MACD面积{0:.2f}较前笔衰减，空头力度衰竭".format(st.macd_area)
        signals.append(ChanlunSignal(
            type=sig_type, price=st.end_price, time=st.end_time,
            description=desc, confidence=conf))
    return signals


def _describe_state(strokes: List[Stroke],
                    signals: List[ChanlunSignal],
                    fractal_count: int) -> Tuple[str, str, str]:
    """生成 current_state / summary / description 三段文本。"""
    if not strokes:
        return "笔形成中", "暂无买卖信号", "共{0}个分型、0笔。笔形成中".format(fractal_count)

    last_stroke = strokes[-1]
    is_up = last_stroke.direction == "up"
    direction_cn = "向上" if is_up else "向下"
    bull_cn = "多头" if is_up else "空头"
    latest = signals[-1] if signals else None

    # 末笔背驰 -> 一类买卖点风险提示
    if last_stroke.has_divergence:
        if last_stroke.direction == "up":
            state = "向上笔顶背驰，注意一类卖点风险"
        else:
            state = "向下笔底背驰，注意一类买点风险"
        if latest:
            state += "，最近{0}信号在{1}".format(latest.type, latest.time)
        summary = "最新信号：{0}@{1:.2f}".format(
            get_signal_type_name(latest.type), latest.price) if latest else "暂无买卖信号"
    else:
        state = "处于{0}笔中，{1}延续".format(direction_cn, bull_cn)
        if latest:
            state += "，最近{0}信号在{1}".format(latest.type, latest.time)
            summary = "最新信号：{0}@{1:.2f}".format(
                get_signal_type_name(latest.type), latest.price)
        else:
            summary = "暂无买卖信号"

    if signals:
        description = "共{0}个分型、{1}笔、{2}个信号。{3}".format(
            fractal_count, len(strokes), len(signals), state)
    else:
        description = "共{0}个分型、{1}笔。{2}".format(
            fractal_count, len(strokes), state)
    return state, summary, description


def analyze_chanlun_minute(times: List[str], prices: List[float],
                           volumes: List[float]) -> ChanlunMinuteResult:
    """分钟缠论完整分析管线。"""
    klines = construct_5min_klines(times, prices, volumes)
    merged = merge_klines(klines)
    fractals = find_fractals(merged)
    strokes = find_strokes(fractals, merged)
    dif, dea, bar = calc_macd(klines)
    detect_divergence(strokes, bar, klines)
    signals = generate_signals(strokes, fractals)
    state, summary, description = _describe_state(strokes, signals, len(fractals))
    return ChanlunMinuteResult(
        kline_count=len(klines), fractal_count=len(fractals),
        fractals=fractals, stroke_count=len(strokes), strokes=strokes,
        signals=signals, macd_dif=dif, macd_dea=dea, macd_bar=bar,
        current_state=state, summary=summary, description=description)


def signals_to_dict(result: ChanlunMinuteResult) -> dict:
    """将分析结果序列化为 dict（macd_bar 保留 6 位小数）。"""
    return {
        "kline_count": result.kline_count,
        "fractal_count": result.fractal_count,
        "stroke_count": result.stroke_count,
        "current_state": result.current_state,
        "summary": result.summary,
        "description": result.description,
        "signals": [{
            "type": s.type,
            "type_name": get_signal_type_name(s.type),
            "price": round(s.price, 2),
            "time": s.time,
            "description": s.description,
            "confidence": s.confidence,
        } for s in result.signals],
        "fractals": [{
            "type": f.type,
            "type_name": "顶分型" if f.type == "top" else "底分型",
            "price": round(f.price, 2),
            "time": f.time,
        } for f in result.fractals],
        "strokes": [{
            "direction": s.direction,
            "start_price": round(s.start_price, 2),
            "end_price": round(s.end_price, 2),
            "start_time": s.start_time,
            "end_time": s.end_time,
            "macd_area": round(s.macd_area, 4),
            "has_divergence": s.has_divergence,
        } for s in result.strokes],
        "macd_bar": [round(x, 6) for x in result.macd_bar],
    }
