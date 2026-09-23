"""共享技术指标库（明文版）。

供 analysis_clear 下各模块复用。算法与加密版常量池中提取的描述一致。
"""
from __future__ import annotations

import math
from typing import List, Optional, Sequence


def sma_series(values: Sequence[float], period: int) -> List[float]:
    """简单移动平均序列，前 period-1 个位置返回 None 占位。"""
    if period <= 0 or not values:
        return []
    result: List[Optional[float]] = [None] * len(values)
    running = 0.0
    for i, v in enumerate(values):
        running += v
        if i >= period:
            running -= values[i - period]
        if i >= period - 1:
            result[i] = running / period
    return result  # type: ignore[return-value]


def ema_series(values: Sequence[float], period: int) -> List[float]:
    """指数移动平均序列。以第一个有效窗口均值为种子。"""
    if period <= 0 or not values:
        return []
    result: List[Optional[float]] = [None] * len(values)
    alpha = 2.0 / (period + 1)
    seed = sum(values[:period]) / period if len(values) >= period else None
    if seed is None:
        return result  # type: ignore[return-value]
    prev = seed
    result[period - 1] = prev
    for i in range(period, len(values)):
        prev = alpha * values[i] + (1 - alpha) * prev
        result[i] = prev
    return result  # type: ignore[return-value]


def last_sma(values: Sequence[float], period: int) -> float:
    """返回序列最后一个 SMA 值（数据不足返回 0）。"""
    if len(values) < period or period <= 0:
        return 0.0
    return sum(values[-period:]) / period


def ma_direction(ma_values: Sequence[float], lookback: int = 5) -> str:
    """判断均线方向：向上 / 向下 / 走平 / 未知。"""
    valid = [v for v in ma_values if v is not None]
    if len(valid) < lookback + 1:
        return "未知"
    recent = valid[-(lookback + 1):]
    slope = recent[-1] - recent[0]
    base = abs(recent[0])
    threshold = base * 0.002 if base else 1e-9
    if slope > threshold:
        return "向上"
    if slope < -threshold:
        return "向下"
    return "走平"


def find_peaks(values: Sequence[float], window: int = 3) -> List[int]:
    """查找局部高点索引。与加密版一致：窗口须完整且在数据内，不含首尾端点。"""
    peaks = []
    n = len(values)
    for i in range(window, n - window):
        lo = i - window
        hi = i + window
        if all(values[i] >= values[j] for j in range(lo, hi + 1) if j != i):
            # 排除区间内完全相等的平庸峰值
            if not all(values[j] == values[i] for j in range(lo, hi + 1)):
                peaks.append(i)
    return peaks


def find_troughs(values: Sequence[float], window: int = 3) -> List[int]:
    """查找局部低点索引。与加密版一致：窗口须完整且在数据内，不含首尾端点。"""
    troughs = []
    n = len(values)
    for i in range(window, n - window):
        lo = i - window
        hi = i + window
        if all(values[i] <= values[j] for j in range(lo, hi + 1) if j != i):
            if not all(values[j] == values[i] for j in range(lo, hi + 1)):
                troughs.append(i)
    return troughs


def fit_trendline(points_idx: List[int], values: Sequence[float]) -> Optional[float]:
    """对给定索引点做最小二乘线性拟合，返回斜率。数据不足返回 None。"""
    if len(points_idx) < 2:
        return None
    xs = [float(i) for i in points_idx]
    ys = [float(values[i]) for i in points_idx]
    n = len(xs)
    mean_x = sum(xs) / n
    mean_y = sum(ys) / n
    denom = sum((x - mean_x) ** 2 for x in xs)
    if denom == 0:
        return None
    slope = sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, ys)) / denom
    return slope


def macd_series(closes: Sequence[float], fast: int = 12, slow: int = 26, signal: int = 9):
    """MACD 三线：DIF / DEA / BAR(柱)。返回 (dif, dea, bar)。"""
    if len(closes) < slow:
        n = len(closes)
        return [0.0] * n, [0.0] * n, [0.0] * n
    ema_fast = ema_series(closes, fast)
    ema_slow = ema_series(closes, slow)
    dif = [ (ef if ef is not None else 0.0) - (es if es is not None else 0.0)
            for ef, es in zip(ema_fast, ema_slow) ]
    dea = ema_series(dif, signal)
    dea = [d if d is not None else 0.0 for d in dea]
    bar = [2.0 * (d - e) for d, e in zip(dif, dea)]
    return dif, dea, bar


def macd_area(macd_bar: Sequence[float], start: int, end: int) -> float:
    """计算 [start, end) 区间 MACD 柱面积（绝对值之和）。"""
    total = 0.0
    for i in range(max(0, start), min(end, len(macd_bar))):
        total += abs(macd_bar[i])
    return total


def round_price(value: float, digits: int = 2) -> float:
    """价格保留指定位数，避免浮点尾差。"""
    return round(value, digits)


def pct_change(start: float, end: float) -> float:
    """百分比变化。"""
    if not start:
        return 0.0
    return (end - start) / start * 100.0
