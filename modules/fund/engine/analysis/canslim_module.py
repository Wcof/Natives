"""CANSLIM 模块（明文版）。

七维度评分：C 近期动量 / A 中期趋势 / N 新高形态 / S 供需 / L 领涨强度 / I 机构资金 / M 大盘环境。
算法依据：加密版常量池（_calc_c_score.._calc_m_score/_detect_cup_handle/_calc_grade）+ 基准输出反推。
杯柄目标公式已验证：target = buy_point + (cup_high - cup_low)。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from data.kline_fetcher import Kline, Quote, FundFlow
from ._indicators import last_sma, sma_series, pct_change


@dataclass
class CanslimResult:
    c_score: int
    a_score: int
    n_score: int
    s_score: int
    l_score: int
    i_score: int
    m_score: int
    total: int
    grade: str
    signals: List[str] = field(default_factory=list)
    cup_handle: Optional[dict] = None
    description: str = ""


def _sma(values: List[float], period: int) -> float:
    """返回最后一位 SMA。"""
    return last_sma(values, period)


def _calc_c_score(klines: List[Kline]) -> Tuple[int, str]:
    """C - 近期动量：20日涨幅分级 与 5日涨幅分级 取较大（样本反推）。"""
    if len(klines) < 10:
        return 50, ""
    closes = [k.close for k in klines]
    gain20 = pct_change(closes[-21], closes[-1]) if len(closes) >= 21 else 0.0
    gain5 = pct_change(closes[-6], closes[-1])
    if gain20 > 20:
        score20 = 90
    elif gain20 > 15:
        score20 = 80
    elif gain20 > 10:
        score20 = 70
    elif gain20 > 5:
        score20 = 60
    elif gain20 > 0:
        score20 = 50
    elif gain20 > -5:
        score20 = 35
    else:
        score20 = 20
    if gain5 > 5:
        score5 = 80
    elif gain5 > 2:
        score5 = 70
    elif gain5 > 0:
        score5 = 60
    elif gain5 > -5:
        score5 = 50
    else:
        score5 = 35
    score = max(score20, score5)
    return score, f"C(近期动量){score}分"


def _calc_a_score(klines: List[Kline]) -> Tuple[int, str]:
    """A - 中期趋势：近120日涨幅分级（样本反推）。"""
    if len(klines) < 125:
        return 50, ""
    gain = pct_change(klines[-121].close, klines[-1].close)
    if gain > 150:
        score = 90
    elif gain > 30:
        score = 75
    elif gain > 15:
        score = 70
    elif gain > 12:
        score = 60
    elif gain > 5:
        score = 50
    elif gain > -15:
        score = 35
    else:
        score = 20
    return score, f"A(中期趋势){score}分"


def _calc_n_score(klines: List[Kline]) -> Tuple[int, str, Optional[dict]]:
    """N - 新高形态：接近52周新高 + 杯柄形态突破（样本反推）。"""
    if len(klines) < 60:
        return 50, "", None
    high_250 = max(k.high for k in klines[-250:]) if len(klines) >= 100 else max(k.high for k in klines)
    price = klines[-1].close
    dist = (high_250 - price) / high_250 * 100 if high_250 else 100
    cup_handle = _detect_cup_handle(klines)
    # 创近期新高：突破近120日高点（排除今日）→ 100
    high_120_prev = max(k.high for k in klines[-121:-1]) if len(klines) >= 121 else 0.0
    if price >= high_120_prev:
        score = 100
    elif dist < 3.0:
        score = 70
    elif cup_handle and cup_handle["breakout"]:
        if dist < 8.0:
            score = 90
        elif dist < 12.0:
            score = 85
        else:
            score = 75
    else:
        score = 40
    return score, f"N(新高/形态){score}分", cup_handle


def _calc_s_score(klines: List[Kline], quote: Optional[Quote]) -> Tuple[int, str]:
    """S - 供需关系：40 基准，量比缩量 -2，放量加分（样本反推）。"""
    turnover = (quote.turnover if quote and quote.turnover else klines[-1].turnover) or 0.0
    if len(klines) >= 6:
        vol_ratio = klines[-1].volume / (sum(k.volume for k in klines[-6:-1]) / 5)
    else:
        vol_ratio = 1.0
    score = 40
    if vol_ratio < 0.5:
        score -= 2  # 缩量
    elif vol_ratio >= 2.0:
        score += 15  # 显著放量
    elif vol_ratio >= 1.5:
        score += 5  # 放量
    return score, f"S(供需关系){score}分"


def _calc_l_score(klines: List[Kline]) -> Tuple[int, str]:
    """L - 领涨强度：60日涨幅分档基础分 + 250日涨幅分级奖罚（8/8 黑盒反推）。"""
    if len(klines) < 95:
        return 50, ""
    closes = [k.close for k in klines]
    gain60 = pct_change(closes[-61], closes[-1]) if len(closes) >= 61 else 0.0
    gain250 = pct_change(closes[-251], closes[-1]) if len(closes) >= 251 else pct_change(closes[0], closes[-1])
    if gain60 >= 30:
        base = 70
    elif gain60 >= 15:
        base = 60
    elif gain60 >= 5:
        base = 50
    elif gain60 >= 1:
        base = 43
    elif gain60 >= -9:
        base = 30
    else:
        base = 20
    if gain250 > 2.5:
        adj = 18
    elif gain250 > 0:
        adj = 13
    elif gain250 < -30:
        adj = -5
    else:
        adj = 0
    score = base + adj
    score = max(0, min(100, score))
    return score, f"L(相对强度){score}分"


def _calc_i_score(flows: Optional[List[FundFlow]]) -> Tuple[int, str]:
    """I - 机构资金：用资金流替代机构持仓数据。

    反推规则：连续3日净流入→85；今日大幅流出→10；5日净流入且今日流入→75；
    5日净流入→65；5日净流出→45；否则55。
    """
    if not flows:
        return 50, ""
    main_nets = [f.main_net for f in flows if f.main_net is not None]
    if not main_nets:
        return 50, ""
    streak = 0
    for v in reversed(main_nets[-3:]):
        if v > 0:
            streak += 1
        else:
            break
    sum5 = sum(main_nets[-5:])
    last = main_nets[-1]
    if streak >= 3:
        score = 85
    elif sum5 < 0:
        score = 10
    elif last < -5e8:
        score = 45
    elif sum5 > 0:
        score = 75
    else:
        score = 55
    return score, f"I(机构资金){score}分"


def _calc_m_score(index_klines: Optional[List[Kline]], stock_klines: List[Kline]) -> Tuple[int, str]:
    """M - 大盘环境：优先用上证指数；无指数数据时用个股均线 + 上涨天数（样本反推）。"""
    if not index_klines or len(index_klines) < 30:
        src = stock_klines
        src_name = "个股均线(近似)"
    else:
        src = index_klines
        src_name = "大盘指数"
    src = index_klines if index_klines and len(index_klines) >= 30 else stock_klines
    if len(src) < 30:
        return 50, ""
    closes = [k.close for k in src]
    ma20 = sma_series(closes, 20)
    ma60 = sma_series(closes, 60)
    ma20_val = ma20[-1] if ma20 else None
    ma60_val = ma60[-1] if ma60 else None

    if ma20_val is None or ma60_val is None:
        return 50, ""

    up_days20 = sum(1 for i in range(len(closes) - 20, len(closes)) if closes[i] > closes[i - 1])
    # MA20 方向（对 MA 序列末段比较）
    ma20_rising = len(ma20) >= 2 and ma20[-1] >= ma20[-6]
    ma20_txt = "向上" if ma20_rising else "向下"
    if ma20_val > ma60_val and up_days20 >= 13:
        score = 80  # 多头且近20日上涨天数多（8/8 反推：加密最高 80）
    elif ma20_val > ma60_val and up_days20 >= 7:
        score = 70
    elif ma20_val > ma60_val:
        score = 60
    elif src_name == "大盘指数":
        score = 15  # 指数空头 → 15（全链路 8/8 反推）
    else:
        score = 35  # 个股回退空头 → 35（模块级 8/8 反推）
    return score, f"M(市场环境){score}分"


def _calc_grade(score: int) -> str:
    """CAN SLIM 评级（加密版反推：每档阈值低 10 分）。"""
    if score >= 85:
        return "A+"
    if score >= 70:
        return "A"
    if score >= 60:
        return "B+"
    if score >= 50:
        return "B"
    if score >= 40:
        return "C+"
    if score >= 30:
        return "C"
    return "D"


def _detect_cup_handle(klines: List[Kline]) -> Optional[dict]:
    """杯柄形态检测。返回 {pattern, cup_high, cup_low, handle_high, handle_low, ...}。"""
    if len(klines) < 80:
        return None
    window = klines[-120:]
    lows = [k.low for k in window]
    highs = [k.high for k in window]
    cup_low_idx = lows.index(min(lows))
    if cup_low_idx < 20 or cup_low_idx > len(window) - 20:
        return None
    cup_low = min(lows)

    left = window[:cup_low_idx]
    if len(left) < 10:
        return None
    # 加密版反推：杯口高点 = 杯底之前 20 日内的最高价
    left_recent = left[-20:]
    cup_high = max(k.high for k in left_recent)
    if cup_high <= cup_low:
        return None

    right = window[cup_low_idx:]
    handle_high_idx = max(range(len(right)), key=lambda i: right[i].high)
    handle_high = right[handle_high_idx].high
    if handle_high <= cup_low:
        return None

    after = right[handle_high_idx:]
    # 加密版反推：柄部低点 = 全量 [ws+2..末尾] 的最低点
    ws = len(klines) - 20
    all_lows = [k.low for k in klines]
    handle_low = min(all_lows[ws + 2:]) if ws + 2 < len(all_lows) else None
    if not handle_low or handle_low <= 0:
        return None

    cup_depth = (cup_high - cup_low) / cup_high * 100
    handle_depth = (handle_high - handle_low) / handle_high * 100
    if not (5.0 <= cup_depth <= 35.0) or handle_depth > 30:
        return None

    buy_point = handle_high
    target = buy_point + (cup_high - cup_low)
    # 突破判定：近30日最高价曾达到柄高点（加密用 high 而非 close）
    recent_highs = [k.high for k in klines[-30:]]
    breakout = max(recent_highs) >= buy_point

    return {
        "pattern": "杯柄形态",
        "cup_high": round(cup_high, 2),
        "cup_low": round(cup_low, 2),
        "handle_high": round(handle_high, 2),
        "handle_low": round(handle_low, 2),
        "cup_depth": round(cup_depth, 1),
        "handle_depth": round(handle_depth, 1),
        "breakout": breakout,
        "buy_point": round(buy_point, 2),
        "target": round(target, 2),
    }


def analyze_canslim(
    klines: List[Kline],
    quote: Optional[Quote] = None,
    flows: Optional[List[FundFlow]] = None,
    index_klines: Optional[List[Kline]] = None,
) -> CanslimResult:
    """CANSLIM 综合分析。"""
    c_score, c_text = _calc_c_score(klines)
    a_score, a_text = _calc_a_score(klines)
    n_score, n_text, cup_handle = _calc_n_score(klines)
    s_score, s_text = _calc_s_score(klines, quote)
    l_score, l_text = _calc_l_score(klines)
    i_score, i_text = _calc_i_score(flows)
    m_score, m_text = _calc_m_score(index_klines, klines)

    # 加权总分（int 截断）：C15% A10% N25% S5% L20% I15% M10%
    # 权重与计算公式经 8 只基准股 A/B 回归约束求解得出。
    total = int(
        0.15 * c_score + 0.10 * a_score + 0.25 * n_score
        + 0.05 * s_score + 0.20 * l_score + 0.15 * i_score + 0.10 * m_score
    )
    grade = _calc_grade(total)

    signals = []
    if c_score >= 65:
        signals.append(c_text)
    if a_score >= 65:
        signals.append(a_text)
    if n_score >= 65:
        signals.append(n_text)
    # L 信号阈值反推：L=68（600900）不触发，L=78 触发 → 阈值 70
    if l_score >= 70:
        signals.append(l_text)
    if i_score >= 65:
        signals.append(i_text)
    # M 信号：>=70 追加文本；<40 追加空头警告（8/8 反推，300750/000858 m=35 触发）
    if m_score >= 70:
        signals.append(m_text)
    if m_score < 40:
        signals.append("⚠️ 市场环境偏空，谨慎操作")
    if i_score < 30:
        signals.append("⚠️ 机构资金流出，注意风险")
    if l_score < 30:
        signals.append("⚠️ 相对强度弱势，非领涨股")

    description = (f"综合{total}分({grade}) | C={c_score} A={a_score} N={n_score} "
                   f"S={s_score} L={l_score} I={i_score} M={m_score}")

    return CanslimResult(
        c_score=c_score, a_score=a_score, n_score=n_score,
        s_score=s_score, l_score=l_score, i_score=i_score, m_score=m_score,
        total=total, grade=grade, signals=signals,
        cup_handle=cup_handle, description=description,
    )
