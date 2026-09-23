"""突破模块（明文版）——海龟交易法则。

系统一：20日唐奇安通道；系统二：55日唐奇安通道。
核心算法（常量池确认）：
  TR = max(H-L, |H-PDC|, |L-PDC|)；N = 前N日TR的SMA；止损 = 入场价 ± 2N；
  加仓：每上涨 0.5N 加 1 单位。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from data.kline_fetcher import Kline
from ._indicators import last_sma


@dataclass
class BreakoutResult:
    system: str
    signal: str
    breakout_price: float
    current_n: float
    stop_loss: float
    entry_price: Optional[float] = None
    position_units: int = 0
    exit_price: Optional[float] = None
    channel_high: float = 0.0
    channel_low: float = 0.0
    next_add_price: Optional[float] = None
    signals: List[str] = field(default_factory=list)
    description: str = ""


def calc_true_range(high: float, low: float, pre_close: float) -> float:
    """真实波幅 TR = max(H-L, |H-PDC|, |L-PDC|)。"""
    return max(high - low, abs(high - pre_close), abs(low - pre_close))


def calc_n(klines: List[Kline], period: int = 20) -> float:
    """N 值 = 前 period 日 TR 的简单移动平均。"""
    if len(klines) < period + 1:
        return 0.0
    trs = []
    for i in range(len(klines) - period, len(klines)):
        k = klines[i]
        pre_close = klines[i - 1].close
        trs.append(calc_true_range(k.high, k.low, pre_close))
    return round(sum(trs) / len(trs), 4)


def calc_donchian_channel(klines: List[Kline], period: int) -> Tuple[float, float]:
    """唐奇安通道：(最高高点, 最低低点)，不含当日（加密反推）。"""
    if len(klines) <= period:
        window = klines[:-1] if len(klines) > 1 else klines
    else:
        window = klines[-period - 1:-1]
    return max(k.high for k in window), min(k.low for k in window)


def _find_last_entry(klines: List[Kline], period: int) -> Optional[Tuple[str, float, int]]:
    """查找最近一次突破入场。返回 (方向, 入场价, 入场日索引)。"""
    n = len(klines)
    for i in range(n - 1, period - 1, -1):
        window = klines[i - period:i]
        if len(window) < period:
            continue
        window_high = max(k.high for k in window)
        window_low = min(k.low for k in window)
        k = klines[i]
        if k.high > window_high:
            return ("多", window_high, i)
        if k.low < window_low:
            return ("空", window_low, i)
    return None


def _analyze_system(klines: List[Kline], period: int, system_name: str) -> BreakoutResult:
    """单个海龟系统分析。"""
    n_val = calc_n(klines, 20)
    channel_high, channel_low = calc_donchian_channel(klines, period)
    last_entry = _find_last_entry(klines, period)

    if n_val <= 0 or channel_high <= 0 or last_entry is None:
        return BreakoutResult(
            system=system_name, signal="无信号",
            breakout_price=round(channel_high, 2), current_n=n_val,
            stop_loss=0.0, channel_high=round(channel_high, 2),
            channel_low=round(channel_low, 2),
            description=f"{system_name}无突破信号",
        )

    direction, entry, entry_idx = last_entry
    holding_days = len(klines) - 1 - entry_idx

    if direction == "多":
        # 入场后最高价决定加仓单位数
        high_since = max(k.high for k in klines[entry_idx + 1:]) if len(klines) > entry_idx + 1 else entry
        extra = 0
        if high_since > entry + 0.5 * n_val:
            extra = int((high_since - entry) // (0.5 * n_val))
        units = 1 + extra
        # 加仓后止损上移至最后加仓价：stop = (entry+(units-1)*0.5N) - 2N
        last_add_price = entry + (units - 1) * 0.5 * n_val
        stop = last_add_price - 2 * n_val
        next_add = entry + units * 0.5 * n_val if (units <= 4 and system_name != "系统二(55日)") else None
        # 多头退出：仅看最后一日收盘是否跌破止损
        if klines[-1].close <= stop:
            signal = "卖出"
            exit_price = stop
        else:
            signal = "持仓"
            exit_price = None
        sig_text = (f"触及2N止损{stop:.2f}，卖出" if signal == "卖出"
                    else f"持有多头{holding_days}日，止损{stop:.2f}")
    else:
        stop = entry + 2 * n_val
        units = 1
        next_add = None
        # 空头平仓：仅看最后一日。系统一用10日高点，系统二用20日高点。
        # 20日(10日)高点突破优先，其次 2N 止损；两者均只看当日。
        exit_window = 10 if "系统一" in system_name else 20
        high_exit_level = calc_donchian_channel(klines, exit_window)[0]
        last = klines[-1]
        if last.high >= high_exit_level:
            signal = "空头平仓"
            exit_price = high_exit_level
            sig_text = f"突破{exit_window}日高点{high_exit_level:.2f}，空头平仓"
        elif last.close >= stop:
            signal = "空头平仓"
            exit_price = stop
            sig_text = f"触及2N止损{stop:.2f}，空头平仓"
        else:
            signal = "持仓"
            exit_price = None
            sig_text = f"持有空头{holding_days}日，止损{stop:.2f}"

    return BreakoutResult(
        system=system_name,
        signal=signal,
        breakout_price=round(channel_high, 2),
        current_n=n_val,
        stop_loss=round(stop, 2),
        entry_price=round(entry, 2),
        position_units=units,
        exit_price=round(exit_price, 2) if exit_price else None,
        channel_high=round(channel_high, 2),
        channel_low=round(channel_low, 2),
        next_add_price=round(next_add, 2) if next_add else None,
        signals=[sig_text],
        description=f"{system_name.replace('(20日)', '').replace('(55日)', '')}"
                    f"入场={direction}@{entry:.2f}，N={n_val:.4f}，持有{holding_days}日",
    )


def analyze_breakout_system1(klines: List[Kline]) -> BreakoutResult:
    """系统一（20日通道）。"""
    return _analyze_system(klines, 20, "系统一(20日)")


def analyze_breakout_system2(klines: List[Kline]) -> BreakoutResult:
    """系统二（55日通道）。"""
    return _analyze_system(klines, 55, "系统二(55日)")


def analyze_breakout(klines: List[Kline]) -> List[BreakoutResult]:
    """突破综合分析。返回两个系统的结果列表。"""
    return [analyze_breakout_system1(klines), analyze_breakout_system2(klines)]
