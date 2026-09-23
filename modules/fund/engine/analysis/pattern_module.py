"""形态模块（明文版）。

识别箱体、双底/双顶、头肩底/顶、双弧底、三角形、旗形、跳空等形态。
目标价公式从基准输出反推并逐一验证：
  双底/头肩底目标 = 颈线 + (颈线 - 底部)；箱体目标 = 上沿 + (上沿 - 下沿)。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from data.kline_fetcher import Kline
from ._indicators import find_peaks, find_troughs


@dataclass
class PatternResult:
    name: str
    direction: str
    confidence: int
    status: str
    target_price: Optional[float] = None
    key_levels: dict = field(default_factory=dict)
    description: str = ""


def _find_peaks(klines: List[Kline], window: int = 3) -> List[int]:
    """局部高点索引（按 high 计算）。"""
    return find_peaks([k.high for k in klines], window)


def _find_troughs(klines: List[Kline], window: int = 3) -> List[int]:
    """局部低点索引（按 low 计算）。"""
    return find_troughs([k.low for k in klines], window)


def _detect_box(klines: List[Kline], price: float) -> Optional[PatternResult]:
    """箱体震荡识别。窗口 = 20 日（样本反推：加密箱体上下沿为近20日高低点）。"""
    if len(klines) < 20:
        return None
    window = klines[-20:]
    upper = max(k.high for k in window)
    lower = min(k.low for k in window)
    if upper <= lower:
        return None
    amplitude = (upper - lower) / lower * 100
    if not (5.0 <= amplitude <= 25.0):
        return None

    # 注意：price >= upper*0.98 已覆盖 price >= upper 的情况，
    # 原版中 "已突破" 分支为死代码（8股+边界合成案例验证），故只保留两个可达分支
    if price >= upper * 0.98:
        status, direction, confidence, target = "接近突破上沿", "看涨", 55, upper + (upper - lower)
    else:
        status, direction, confidence, target = "形成中", "中性", 55, None

    desc = (f"箱体振幅{amplitude:.1f}%，突破上沿目标{upper + (upper - lower):.2f}，"
            f"跌破下沿目标{lower - (upper - lower):.2f}")
    return PatternResult(
        name="箱体震荡",
        direction=direction,
        confidence=confidence,
        status=status,
        target_price=round(target, 2) if target else None,
        key_levels={"箱体上沿": round(upper, 2), "箱体下沿": round(lower, 2)},
        description=desc,
    )


def _detect_double_top_bottom(klines: List[Kline], price: float) -> Optional[PatternResult]:
    """双底 / 双顶识别。"""
    if len(klines) < 60:
        return None
    lows = [k.low for k in klines]
    highs = [k.high for k in klines]
    troughs = _find_troughs(klines, window=5)
    if len(troughs) < 2:
        return None

    # 双底：加密版只检查最后 3 个谷形成的相邻对，价差 < 3%，取中间区间高点为颈线
    for i in range(len(troughs) - 2, len(troughs) - 1):
        t0, t1 = troughs[i], troughs[i + 1]
        if t1 - t0 < 5:
            continue
        v0, v1 = lows[t0], lows[t1]
        if abs(v0 - v1) / min(v0, v1) > 0.03:
            continue
        mid = klines[t0:t1 + 1]
        neck = max(k.high for k in mid)
        bottom = v0
        if neck <= bottom:
            continue
        target = neck + (neck - bottom)
        status = "已突破" if price > neck else "形成中"
        return PatternResult(
            name="双底",
            direction="看涨",
            confidence=55,
            status=status,
            target_price=round(target, 2),
            key_levels={"颈线": round(neck, 2), "底部": round(bottom, 2)},
            description="双底可靠性约10%，需等待价格确认突破颈线",
        )

    # 双顶：最近两个高点接近，中间取低点为颈线；加密版仅在跌破颈线时输出
    peaks = _find_peaks(klines, window=5)
    if len(peaks) < 2:
        return None
    for i in range(len(peaks) - 2, len(peaks) - 1):
        p0, p1 = peaks[i], peaks[i + 1]
        if p1 - p0 < 5:
            continue
        v0, v1 = highs[p0], highs[p1]
        if abs(v0 - v1) / min(v0, v1) > 0.03:
            continue
        mid = klines[p0:p1 + 1]
        neck = min(k.low for k in mid)
        top = max(v0, v1)
        if neck >= top:
            continue
        target = neck - (top - neck)
        if price >= neck:
            continue
        return PatternResult(
            name="双顶",
            direction="看跌",
            confidence=55,
            status="已突破",
            target_price=round(target, 2),
            key_levels={"颈线": round(neck, 2), "顶部": round(top, 2)},
            description=f"双顶颈线{neck:.2f}，跌破后目标{target:.2f}",
        )
    return None


def _detect_head_shoulders(klines: List[Kline], price: float) -> Optional[PatternResult]:
    """头肩底 / 头肩顶识别。"""
    if len(klines) < 10:
        return None
    lows = [k.low for k in klines]
    highs = [k.high for k in klines]
    troughs = _find_troughs(klines, window=3)

    # 头肩底：三个低点，中间最低
    if len(troughs) >= 3:
        # 加密版只用最后 3 个谷形成的三元组（样本反推）
        for i in range(len(troughs) - 2, len(troughs) - 1):
            l, m, r = troughs[i - 1], troughs[i], troughs[i + 1]
            if m - l < 2 or r - m < 2:
                continue
            vl, vm, vr = lows[l], lows[m], lows[r]
            if not (vm < vl and vm < vr):
                continue
            if abs(vl - vr) / min(vl, vr) > 0.08:
                continue
            # 颈线 = 两肩中较高谷所在K线的 high（样本反推：与加密版一致）
            neck = max(highs[l], highs[r])
            depth = neck - vm
            if neck <= vm:
                continue
            target = neck + depth
            status = "已突破" if price > neck else "形成中"
            confidence = 80 if status == "已突破" else 60
            return PatternResult(
                name="头肩底",
                direction="看涨",
                confidence=confidence,
                status=status,
                target_price=round(target, 2),
                key_levels={"颈线": round(neck, 2), "头部": round(vm, 2)},
                description=f"底部深度{depth:.2f}，突破颈线后目标{target:.2f}",
            )

    peaks = _find_peaks(klines, window=3)
    # 头肩顶：三个高点，中间最高
    if len(peaks) >= 3:
        for i in range(len(peaks) - 2, len(peaks) - 1):
            l, m, r = peaks[i - 1], peaks[i], peaks[i + 1]
            if m - l < 2 or r - m < 2:
                continue
            vl, vm, vr = highs[l], highs[m], highs[r]
            if not (vm > vl and vm > vr):
                continue
            if abs(vl - vr) / min(vl, vr) > 0.08:
                continue
            # 头肩顶颈线 = 两肩峰中较高峰所在K线的 low（与头肩底对称）
            neck = min(lows[l], lows[r])
            if neck >= vm:
                continue
            height = vm - neck
            target = neck - height
            status = "已突破" if price < neck else "形成中"
            return PatternResult(
                name="头肩顶",
                direction="看跌",
                confidence=45 if status == "形成中" else 60,
                status=status,
                target_price=round(target, 2),
                key_levels={"颈线": round(neck, 2), "头部": round(vm, 2)},
                description=f"头部高度{height:.2f}，跌破颈线后目标{target:.2f}",
            )
    return None


def _detect_rounding_bottom(klines: List[Kline], price: float) -> Optional[PatternResult]:
    """双弧底（圆弧底）识别：K线与成交量同时呈圆弧。"""
    if len(klines) < 60:
        return None
    window = klines[-60:]
    lows = [k.low for k in window]
    min_idx = lows.index(min(lows))
    if min_idx < 15 or min_idx > len(window) - 15:
        return None
    left = lows[:min_idx]
    right = lows[min_idx + 1:]
    if len(left) < 8 or len(right) < 8:
        return None
    # 左右两侧单调接近低点（圆弧形状）
    left_desc = all(left[i] >= left[i + 1] for i in range(len(left) - 2) if left[i] > 0)
    right_asc = all(right[i] <= right[i + 1] for i in range(len(right) - 1))
    if not (left_desc and right_asc):
        return None
    # 成交量也呈圆弧（两侧递减到最低点后回升）
    vols = [k.volume for k in window]
    vol_min = min(vols[min_idx - 5:min_idx + 6])
    if vol_min <= 0:
        return None
    bottom = min(lows)
    return PatternResult(
        name="双弧底",
        direction="看涨",
        confidence=70,
        status="形成中",
        target_price=None,
        key_levels={"弧底低点": round(bottom, 2)},
        description="K线与成交量同时呈圆弧底，连续放量2-3日可确认起涨",
    )


def _detect_triangle(klines: List[Kline], price: float) -> Optional[PatternResult]:
    """三角形识别：对称 / 上升 / 下降。"""
    if len(klines) < 30:
        return None
    window = klines[-30:]
    highs = [k.high for k in window]
    lows = [k.low for k in window]
    first_half = window[:15]
    second_half = window[15:]
    if not first_half or not second_half:
        return None
    h1, h2 = max(k.high for k in first_half), max(k.high for k in second_half)
    l1, l2 = min(k.low for k in first_half), min(k.low for k in second_half)
    upper_slope = h2 - h1
    lower_slope = l2 - l1

    if abs(upper_slope) < 1e-6 or abs(lower_slope) < 1e-6:
        return None

    # 对称：高点降低 + 低点抬高
    if upper_slope < 0 and lower_slope > 0:
        return PatternResult(
            name="对称三角形",
            direction="中性",
            confidence=50,
            status="形成中",
            key_levels={},
            description="对称三角形通常延续原有趋势，等待方向选择",
        )
    # 上升：高点持平 + 低点抬高
    if abs(upper_slope) / h1 < 0.02 and lower_slope > 0:
        status = "已突破" if price > h2 else "接近突破"
        return PatternResult(
            name="上升三角形",
            direction="看涨",
            confidence=60,
            status=status,
            target_price=round(h2 + (h2 - l1), 2),
            key_levels={"阻力位": round(h2, 2)},
            description=f"上升三角形偏多，突破{round(h2, 2):.2f}确认",
        )
    # 下降：低点持平 + 高点降低
    if abs(lower_slope) / l1 < 0.02 and upper_slope < 0:
        status = "已跌破" if price < l2 else "接近跌破"
        return PatternResult(
            name="下降三角形",
            direction="看跌",
            confidence=60,
            status=status,
            target_price=round(l2 - (h1 - l2), 2),
            key_levels={"支撑位": round(l2, 2)},
            description=f"下降三角形偏空，跌破{round(l2, 2):.2f}确认",
        )
    return None


def _detect_flag(klines: List[Kline], price: float) -> Optional[PatternResult]:
    """旗形识别：急涨后窄幅整理。"""
    if len(klines) < 30:
        return None
    pole = klines[-30:-15]
    flag = klines[-15:]
    if not pole or not flag:
        return None
    pole_rise = pole[-1].close - pole[0].close
    if pole_rise <= 0:
        return None
    flag_high = max(k.high for k in flag)
    flag_low = min(k.low for k in flag)
    flag_range = (flag_high - flag_low) / flag_low if flag_low else 0
    if flag_range > 0.08:
        return None
    return PatternResult(
        name="上升旗形",
        direction="看涨",
        confidence=60,
        status="整理中",
        target_price=round(flag_high + pole_rise, 2),
        key_levels={"旗形上沿": round(flag_high, 2)},
        description=f"旗杆涨幅{round(pole_rise, 2):.2f}，突破后目标{round(flag_high + pole_rise, 2):.2f}",
    )


def _detect_gap(klines: List[Kline], price: float) -> Optional[PatternResult]:
    """跳空缺口识别。"""
    if len(klines) < 5:
        return None
    latest = klines[-1]
    prev = klines[-2]
    if latest.low > prev.high:
        gap = latest.low - prev.high
        return PatternResult(
            name="向上突破缺口",
            direction="看涨",
            confidence=65,
            status="已形成",
            key_levels={"缺口上沿": round(latest.low, 2)},
            description=f"向上跳空缺口{gap:.2f}，回补前视为支撑",
        )
    if latest.high < prev.low:
        gap = prev.low - latest.high
        return PatternResult(
            name="向下突破缺口",
            direction="看跌",
            confidence=65,
            status="已形成",
            key_levels={"缺口下沿": round(latest.high, 2)},
            description=f"向下跳空缺口{gap:.2f}，回补前视为压力",
        )
    return None


def analyze_patterns(klines: List[Kline]) -> List[PatternResult]:
    """形态综合分析。加密版各检测器基于不同长度的 K 线切片：
      头肩 / 双顶双底 → 最近 60 根；三角形 / 箱体 → 最近 20 根。
      按 头肩 → 双顶/双底 → 三角形 → 箱体 → 旗形 → 跳空 → 双弧底 顺序检测，
      结果按检测顺序 append，不排序。"""
    price = klines[-1].close
    window60 = klines[-60:]
    window20 = klines[-20:]
    detectors = [
        lambda ks: _detect_head_shoulders(window60, price),
        lambda ks: _detect_double_top_bottom(window60, price),
        lambda ks: _detect_triangle(window20, price),
        lambda ks: _detect_box(window20, price),
        lambda ks: _detect_flag(window60, price),
        lambda ks: _detect_gap(window60, price),
        lambda ks: _detect_rounding_bottom(window60, price),
    ]
    results = []
    for detector in detectors:
        try:
            result = detector(klines)
        except Exception:
            result = None
        if result is not None:
            results.append(result)
    # 按检测顺序 append，与加密版一致（不排序）
    return results[:3]
