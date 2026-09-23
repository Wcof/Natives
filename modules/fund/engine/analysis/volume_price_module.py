"""量价模块（明文版）。

功能：量价模式识别（量价齐升/缩量回调/放量滞涨等）、OBV 能量潮、资金流辅助。
算法依据：加密版常量池（_calc_obv/_classify_price_volume/_analyze_fund_flow）+ 基准输出反推。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from data.kline_fetcher import Kline, Quote, FundFlow
from ._indicators import last_sma, ma_direction, sma_series


@dataclass
class VolumePriceResult:
    pattern: str
    direction: str
    confidence: int
    volume_ratio: float
    turnover: float
    obv_trend: str
    signals: List[str] = field(default_factory=list)
    description: str = ""


def _sma(values: List[float], period: int) -> float:
    """返回最后一位 SMA。"""
    return last_sma(values, period)


def _calc_obv(klines: List[Kline]) -> List[float]:
    """计算 OBV（能量潮）序列。"""
    obv = []
    running = 0.0
    prev_close = None
    for k in klines:
        if prev_close is not None and k.close != prev_close:
            running += k.volume if k.close > prev_close else -k.volume
        obv.append(running)
        prev_close = k.close
    return obv


def _classify_price_volume(klines: List[Kline]) -> Tuple[str, str, int]:
    """量价模式分类。返回 (pattern, direction, base_confidence)。

    反推规则（9/9 样本匹配）：
      价格方向：8 日涨跌 (close[-1]-close[-8])/close[-8] > 2% → 涨；<-2% → 跌；否则平。
      量能方向：ma3(成交量) 相对前5日 (vols[-8:-3]) 均量变化 > 30% → 增；<-30% → 缩；否则平。
    """
    if len(klines) < 10:
        return "数据不足", "中性", 50
    closes = [k.close for k in klines]
    g8 = (closes[-1] - closes[-8]) / closes[-8] * 100 if closes[-8] else 0.0
    vols = [k.volume for k in klines]

    if g8 > 2.0:
        price_dir = "涨"
    elif g8 < -2.0:
        price_dir = "跌"
    else:
        price_dir = "平"

    # 量方向：最近3日均量 vs 前5日均量(不含最近3日)
    if len(vols) >= 8:
        ma3_vol = sum(vols[-3:]) / 3
        ma5_prev = sum(vols[-8:-3]) / 5
        vol_change = (ma3_vol - ma5_prev) / ma5_prev * 100 if ma5_prev else 0.0
    else:
        vol_change = 0.0
    if vol_change > 30.0:
        vol_dir = "增"
    elif vol_change < -30.0:
        vol_dir = "缩"
    else:
        vol_dir = "平"

    pattern = f"价{price_dir}量{vol_dir}"
    if price_dir == "涨":
        direction = "看涨"
    elif price_dir == "跌":
        direction = "看跌"
    else:
        direction = "中性"

    base_conf = {
        "价涨量增": 80, "价涨量平": 55, "价涨量缩": 60,
        "价平量增": 50, "价平量平": 20, "价平量缩": 35,
        "价跌量增": 75, "价跌量平": 50, "价跌量缩": 60,
    }.get(pattern, 50)
    return pattern, direction, base_conf


def _analyze_fund_flow(flows: Optional[List[FundFlow]]) -> Tuple[str, int]:
    """分析资金流。返回 (信号文案, 置信度修正)。

    反推规则（常量池 + 8/8 样本验证）：
      最近3日 main_net 全正 → 连续3日主力净流入(+15)；全负 → 连续3日主力净流出(-15)。
      否则按昨日均量基准对比单日净额：
        前两日均值 prev_avg = (main_net[-3] + main_net[-2]) / 2：
          若 prev_avg >= 0，阈值 thr = 0.5*prev_avg：
            last < -thr → 今日主力大幅流出(-10)
            last >  thr → 今日主力大幅流入(+10)
          若 prev_avg < 0 → 温合(0)。
    """
    if not flows:
        return "", 0
    recent = flows[-3:]
    main_nets = [f.main_net for f in recent if f.main_net is not None]
    if not main_nets:
        return "", 0

    if all(v > 0 for v in main_nets):
        return "连续3日主力净流入", 15
    if all(v < 0 for v in main_nets):
        return "连续3日主力净流出", -15

    last_net = main_nets[-1]
    prev_avg = (main_nets[0] + main_nets[1]) / 2
    if prev_avg >= 0:
        threshold = 0.5 * prev_avg
        if last_net < -threshold:
            return "今日主力大幅流出", -10
        if last_net > threshold:
            return "今日主力大幅流入", 10
    return "主力资金温和", 0


def _detect_limit_up_volume(klines: List[Kline]) -> Optional[str]:
    """检测放量涨停。"""
    if len(klines) < 6:
        return None
    latest = klines[-1]
    vols = [k.volume for k in klines[-6:-1]]
    avg_vol = sum(vols) / len(vols) if vols else 1.0
    if latest.pct >= 9.5 and avg_vol and latest.volume > avg_vol * 1.5:
        return f"放量涨停(pct={latest.pct:.1f}%)"
    return None


def _detect_volume_breakout(klines: List[Kline]) -> Optional[str]:
    """检测量能突破（当日量为近20日最大且超过1.5倍均值）。"""
    if len(klines) < 20:
        return None
    latest = klines[-1]
    vols = [k.volume for k in klines[-21:-1]]
    avg_vol = sum(vols) / len(vols) if vols else 1.0
    if latest.volume > avg_vol * 1.5 and latest.volume > max(vols):
        return "量能突破，资金活跃"
    return None


def analyze_volume_price(
    klines: List[Kline],
    quote: Optional[Quote] = None,
    flows: Optional[List[FundFlow]] = None,
) -> VolumePriceResult:
    """量价综合分析。返回 VolumePriceResult。"""
    pattern, direction, base = _classify_price_volume(klines)
    # 量比 = quote 现量 / 前5日均量（加密版实测口径）
    if quote and len(klines) >= 6:
        avg5 = sum(k.volume for k in klines[-6:-1]) / max(1, len(klines[-6:-1]))
        volume_ratio = round(quote.volume / avg5, 2) if avg5 else 1.0
    else:
        volume_ratio = 1.0

    turnover = (quote.turnover if quote and quote.turnover else klines[-1].turnover) or 0.0
    obv = _calc_obv(klines)
    # 加密版反推：OBV 方向用 lookback=8（600900 验证：5→上升 8→下降）
    obv_dir = ma_direction(obv, lookback=8)
    obv_trend = "上升" if obv_dir == "向上" else ("下降" if obv_dir == "向下" else "走平")

    fund_text, fund_delta = _analyze_fund_flow(flows)
    confidence = base
    # 量比修正：<0.5 → -3；0.5~1.5 → +2；1.5~2.0 → +7；≥2.0 → +12
    if volume_ratio < 0.5:
        confidence += -3
    elif volume_ratio < 1.5:
        confidence += 2
    elif volume_ratio < 2.0:
        confidence += 7
    else:
        confidence += 12
    confidence += fund_delta
    confidence = max(5, min(95, confidence))

    signals = []
    if fund_text:
        signals.append(fund_text)
    if obv_trend == "上升":
        signals.append("OBV上升")
    elif obv_trend == "下降":
        signals.append("OBV下降")

    limit_up = _detect_limit_up_volume(klines)
    if limit_up:
        signals.append(limit_up)
    vol_breakout = _detect_volume_breakout(klines)
    if vol_breakout and "量能突破" not in " ".join(signals):
        signals.append(vol_breakout)

    desc = f"量价模式={pattern}，量比={volume_ratio}，换手={turnover:.1f}%"
    if fund_text:
        desc += f"，{fund_text}"

    return VolumePriceResult(
        pattern=pattern,
        direction=direction,
        confidence=confidence,
        volume_ratio=volume_ratio,
        turnover=round(turnover, 2),
        obv_trend=obv_trend,
        signals=signals,
        description=desc,
    )
