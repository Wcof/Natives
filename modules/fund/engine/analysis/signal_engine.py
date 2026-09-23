"""信号引擎（明文版）——五模块聚合与决策。

综合分 = int(趋势×25% + CANSLIM×20% + 突破×20% + 量价×20% + 形态×15%)
形态分 = 50 + Σ(方向 × confidence × 0.2)   （方向：看涨+1 / 看跌-1 / 中性0）
以上公式均从加密版基准输出精确反推。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

from data.kline_fetcher import Kline, Quote, FundFlow
from .trend_module import TrendResult, analyze_trend
from .volume_price_module import VolumePriceResult, analyze_volume_price
from .pattern_module import PatternResult, analyze_patterns
from .breakout_module import BreakoutResult, analyze_breakout
from .canslim_module import CanslimResult, analyze_canslim


# 风险等级 / 信号强度判定的分档阈值（数值来源于加密版反推，勿改动）
RISK_HEAVY = 5      # risk_points>=5 → 高风险
RISK_MEDIUM = 3     # risk_points>=3 → 中风险
STRONG_SCORE = 75   # score>=75   → 强信号 / 正常仓位
MEDIUM_SCORE = 60   # score>=60   → 中信号


@dataclass
class SignalEngineResult:
    action: str
    score: int
    confidence: int
    risk_level: str
    signal_strength: str = ""
    trend: Optional[TrendResult] = None
    patterns: List[PatternResult] = field(default_factory=list)
    volume_price: Optional[VolumePriceResult] = None
    breakouts: List[BreakoutResult] = field(default_factory=list)
    canslim: Optional[CanslimResult] = None
    module_scores: Dict[str, int] = field(default_factory=dict)
    buy_signals: List[str] = field(default_factory=list)
    sell_signals: List[str] = field(default_factory=list)
    risk_warnings: List[str] = field(default_factory=list)
    key_levels: Dict[str, float] = field(default_factory=dict)
    description: str = ""
    plain_summary: str = ""
    trade_plan: Dict[str, Any] = field(default_factory=dict)


def _trend_to_score(trend: TrendResult) -> int:
    """趋势模块评分 = 各均线子项得分之和。"""
    return trend.strength


def _pattern_to_score(patterns: List[PatternResult]) -> int:
    """形态模块评分 = 50 + Σ(方向 × confidence × 0.2)。"""
    total = 50.0
    for p in patterns:
        sign = 1 if p.direction == "看涨" else (-1 if p.direction == "看跌" else 0)
        total += sign * p.confidence * 0.2
    return max(20, min(100, int(total)))


def _volume_price_to_score(vp: VolumePriceResult) -> int:
    """量价模块评分。看涨=confidence；中性=50；看跌=低分。"""
    if vp.direction == "看涨":
        return vp.confidence
    if vp.direction == "看跌":
        return max(20, 100 - vp.confidence)
    return 50


def _breakout_to_score(breakouts: List[BreakoutResult]) -> int:
    """突破模块评分。有突破信号 60 起，存在空头平仓（偏多）加 3 至 63（加密反推）。"""
    score = 50
    has_signal = False
    has_short_cover = False
    for b in breakouts:
        if b.signal in ("持仓", "持仓空头", "多头止损"):
            has_signal = True
        if b.signal == "空头平仓":
            has_short_cover = True
    if has_signal:
        score = 60
    elif has_short_cover:
        score = 60
    if has_short_cover:
        score += 3
    return min(100, score)


def _calc_risk_level(
    score: int,
    trend: TrendResult,
    vp: VolumePriceResult,
    canslim: CanslimResult,
    breakouts: List[BreakoutResult],
) -> Tuple[str, str]:
    """风险等级与信号强度（加密版反推）。

    信号强度 = 综合分分档：>=75 强，>=60 中，否则弱（与 action 同阈值）。
    风险等级基于看跌/看空信号累积计分。
    """
    risk_points = 0
    if trend.direction == "下降":
        risk_points += 2
    if vp.direction == "看跌":
        risk_points += 2
    if canslim.m_score < 30:
        risk_points += 1
    if any("止损" in s for b in breakouts for s in b.signals):
        risk_points += 1

    if risk_points >= RISK_HEAVY:
        risk_level = "高"
    elif risk_points >= RISK_MEDIUM:
        risk_level = "中"
    else:
        risk_level = "低"

    if score >= STRONG_SCORE:
        strength = "强"
    elif score >= MEDIUM_SCORE:
        strength = "中"
    else:
        strength = "弱"
    return risk_level, strength


def _build_trade_plan(
    action: str,
    score: int,
    risk_level: str,
    strength: str,
    trend: TrendResult,
    patterns: List[PatternResult],
    breakouts: List[BreakoutResult],
    canslim: CanslimResult,
    klines: List[Kline],
) -> Dict[str, Any]:
    """构建交易计划（加密版 8/8 A/B 反推）。

    - 止损 = 入场价 × 0.95（固定 5%，8/8 验证）
    - 目标 = 看涨形态按优先级 头肩底 > 双底 > 箱体，取首个高于现价的 target；
      无有效目标时回退箱体上沿（300750 验证 403.8）
    - 仓位：买入=半仓(1/2)，观望=空仓等待
    - 持仓周期：恒为中线(1-3月)
    - notes：仅第一个「持仓」系统的突破信息（用 entry_price 原值）
    """
    entry = klines[-1].close if klines else 0.0
    stop = round(entry * 0.95, 2)  # 固定 5% 止损（8/8 验证），先舍入再算盈亏比

    # 目标价：头肩底 > 双底 > 箱体，取首个 > entry 的 target
    priority_map = {"头肩底": 0, "双底": 1, "箱体": 2}
    ordered = sorted(
        (p for p in patterns if p.direction == "看涨" and p.target_price),
        key=lambda p: priority_map.get(p.name, 9),
    )
    target = next((p.target_price for p in ordered if p.target_price > entry), None)
    if target is None:
        for p in patterns:
            if "箱体上沿" in p.key_levels:
                target = p.key_levels["箱体上沿"]
                break
    if target is None:
        target = entry * 1.10

    risk_amt = entry - stop
    reward_amt = target - entry
    risk_reward = round(reward_amt / risk_amt, 1) if risk_amt > 0 else 0.0
    max_loss_pct = 5.0  # 固定（8/8 验证）

    if action == "观望":
        position_size = "空仓等待"
    elif score >= STRONG_SCORE:
        position_size = "正常仓位"
    else:
        position_size = "半仓(1/2)"  # 买入档恒为半仓（8/8 验证）

    holding_period = "中线(1-3月)"  # 恒为中线（8/8 验证）

    # notes：仅第一个「持仓」系统的突破信息，用 entry_price 原值（600519 显示 1362.0）
    notes = []
    for b in breakouts:
        if b.signal == "持仓" and b.entry_price:
            note = f"{b.system}：持仓中(突破价{b.entry_price})，止损{b.stop_loss:.2f}"
            if b.next_add_price:
                note += f"；加仓价{b.next_add_price:.2f}"
            notes.append(note)
            break

    return {
        "action": action,
        "entry_price": round(entry, 2),
        "stop_loss": round(stop, 2),
        "target_price": round(target, 2),
        "position_size": position_size,
        "holding_period": holding_period,
        "risk_reward_ratio": risk_reward,
        "max_loss_pct": max_loss_pct,
        "notes": "；".join(notes),
    }


def _build_plain_summary(
    action: str,
    score: int,
    strength: str,
    risk_level: str,
    trend: TrendResult,
    patterns: List[PatternResult],
    vp: VolumePriceResult,
    breakouts: List[BreakoutResult],
    canslim: CanslimResult,
    plan: Dict[str, Any],
) -> str:
    """大白话总结（加密版 8/8 A/B 反推）。

    买入：出现买入信号，处于{强势}上升趋势，[⚠️大盘偏空，][头肩底形态确认，][量价配合良好，]
          建议{仓位}入场，买入价{entry}，止损{stop}，目标{target}（盈亏比{RR}）。
    观望：建议观望，处于{方向}趋势，[主力资金流出，][大盘环境偏空。]建议耐心等待信号明确后再操作。
    """
    has_head_shoulder = any(p.name == "头肩底" for p in patterns)
    has_flow_out = any("流出" in s for s in (vp.signals if vp else []))

    # 量价配合良好：看涨且 confidence>=70（000858 价涨量增77 → 良好；000001 价涨量增67 → 否）
    volume_price_ok = bool(
        vp and vp.direction == "看涨" and vp.confidence >= 70
    )

    if action == "观望":
        desc_parts = [f"处于{trend.direction}趋势"]
        if has_flow_out:
            desc_parts.append("主力资金流出")
        if canslim.m_score < 30:
            desc_parts.append("大盘环境偏空")
        return "建议观望，" + "，".join(desc_parts) + "。建议耐心等待信号明确后再操作。"

    trend_desc = "强势上升趋势" if trend.strength >= 70 else "上升趋势"
    desc_parts = [f"处于{trend_desc}"]
    if canslim.m_score < 30:
        desc_parts.append("⚠️大盘偏空")
    if has_head_shoulder:
        desc_parts.append("头肩底形态确认")
    if volume_price_ok:
        desc_parts.append("量价配合良好")

    entry = plan.get("entry_price", 0.0)
    stop = plan.get("stop_loss", 0.0)
    target = plan.get("target_price", 0.0)
    rr = plan.get("risk_reward_ratio", 0)
    return (
        f"出现买入信号，" + "，".join(desc_parts) + "。"
        f"建议{plan.get('position_size', '')}入场，买入价{entry:.2f}，"
        f"止损{stop:.2f}，目标{target:.2f}（盈亏比{rr}）。"
    )


def run_analysis(
    klines: List[Kline],
    quote: Optional[Quote] = None,
    flows: Optional[List[FundFlow]] = None,
    index_klines: Optional[List[Kline]] = None,
) -> SignalEngineResult:
    """五模块综合分析入口。"""
    # 与加密版一致：未显式提供大盘指数时，内部获取上证指数（失败则回退个股均线）
    if index_klines is None:
        try:
            from data.kline_fetcher import fetch_index_kline
            index_klines = fetch_index_kline("000001", 60)
        except Exception:
            index_klines = None
    trend = analyze_trend(klines)
    patterns = analyze_patterns(klines)
    vp = analyze_volume_price(klines, quote, flows)
    breakouts = analyze_breakout(klines)
    canslim = analyze_canslim(klines, quote, flows, index_klines)

    trend_score = _trend_to_score(trend)
    pattern_score = _pattern_to_score(patterns)
    vp_score = _volume_price_to_score(vp)
    breakout_score = _breakout_to_score(breakouts)
    canslim_score = canslim.total

    module_scores = {
        "趋势": trend_score,
        "形态": pattern_score,
        "量价": vp_score,
        "突破": breakout_score,
        "CAN_SLIM": canslim_score,
    }
    # 综合分 = int(趋势25% + CAN20% + 突破20% + 量价20% + 形态15%)
    score = int(
        trend_score * 0.25 + canslim_score * 0.20 + breakout_score * 0.20
        + vp_score * 0.20 + pattern_score * 0.15
    )

    # 加密版 action 仅三档：>=75 强烈买入，>=60 买入，否则观望（8/8 A/B 反推，无"谨慎买入"档）
    if score >= 75:
        action = "强烈买入"
    elif score >= 60:
        action = "买入"
    else:
        action = "观望"

    # 置信度（加密版反推）：confidence = max(10, int(score*0.8) + 12*n - 40)
    # 其中 n = 五个模块中模块分>=60 的数量（达标模块越多，置信度越高）
    qualified_count = sum(
        1 for module_score in module_scores.values() if module_score >= 60
    )
    confidence = max(10, int(score * 0.8) + 12 * qualified_count - 40)

    risk_level, signal_strength = _calc_risk_level(
        score, trend, vp, canslim, breakouts
    )

    # ---- 信号聚合 ----
    buy_signals = []
    sell_signals = []

    if trend.strength >= 65:
        buy_signals.append(f"趋势强势上升({trend.strength}分)")
    elif trend.strength >= 45:
        buy_signals.append(f"趋势上升({trend.strength}分)")
    for sig in trend.signals:
        if sig not in buy_signals and not sig.startswith("MA20"):
            buy_signals.append(sig)

    # 加密版反推：仅拼接「头肩」形态信号（600036/601318/600900 验证），
    # 双底/箱体不拼接；头肩底在量价信号之前
    for p in patterns:
        if p.name == "头肩底" and p.direction == "看涨":
            buy_signals.append(f"{p.name}({p.status})")

    # 量价信号仅在看涨且 confidence>=60 时进入（600519 价涨量平52 不进入）
    if vp.direction == "看涨" and vp.confidence >= 60:
        buy_signals.append(f"量价{vp.pattern}({vp.confidence}分)")
    # vp.signals 仅拼接「净流入/流出」类（OBV上升/主力资金温和不拼接）
    for sig in vp.signals:
        if "流出" in sig:
            sell_signals.append(sig)
        elif "净流入" in sig:
            buy_signals.append(sig)

    for b in breakouts:
        if b.signal == "持仓" and b.entry_price:
            buy_signals.append(f"{b.system}持仓(N={b.current_n:.2f}，止损{b.stop_loss:.2f})")
        elif b.signal == "空头平仓":
            # 加密版反推：用 breakout_price 而非 exit_price（300750 验证 438.24）
            buy_signals.append(f"{b.system}空头平仓@{b.breakout_price}(偏多)")
        elif b.signal == "多头止损":
            sell_signals.append(f"{b.system}多头止损@{b.stop_loss:.2f}")

    for sig in canslim.signals:
        if "⚠️" in sig:
            sell_signals.append(sig)
        elif not sig.startswith("M("):
            buy_signals.append(sig)

    # ---- 风险提示 ----
    risk_warnings = []
    if canslim.m_score < 30:
        risk_warnings.append("市场环境偏空")
    if vp.direction == "看跌":
        risk_warnings.append("量价配合不佳")
    if trend.direction == "下降":
        risk_warnings.append("处于下降趋势")
    # ---- 关键价位（加密版反推：仅聚合最高优先级形态的 key_levels）----
    # 优先级：头肩（底/顶）> 双底 > 箱体
    key_levels = {}
    primary_pattern = None
    for priority, name_prefix in [
        (0, "头肩"),
        (1, "双底"),
        (2, "箱体"),
    ]:
        for p in patterns:
            if p.name.startswith(name_prefix):
                primary_pattern = p
                break
        if primary_pattern is not None:
            break
    if primary_pattern is not None:
        for label, val in primary_pattern.key_levels.items():
            key_levels[f"{primary_pattern.name}_{label}"] = round(val, 2)
    for b in breakouts:
        if b.stop_loss > 0:
            key_levels[f"{b.system}_止损"] = b.stop_loss
    if canslim.cup_handle:
        key_levels["杯柄买点"] = canslim.cup_handle["buy_point"]
    if trend.trendline:
        key_levels["趋势线"] = trend.trendline["current_price"]
    trade_plan = _build_trade_plan(
        action, score, risk_level, signal_strength,
        trend, patterns, breakouts, canslim, klines,
    )

    desc_parts = [f"综合{score}分"]
    if trend.direction:
        desc_parts.append(f"趋势={trend.direction}({trend_score})")
    if vp:
        desc_parts.append(f"量价={vp.pattern}({vp_score})")
    desc_parts.append(f"突破={breakout_score}")
    if canslim:
        desc_parts.append(f"CS={canslim.grade}({canslim_score})")
    if patterns:
        desc_parts.append(f"形态={pattern_score}")
    description = " | ".join(desc_parts)

    plain_summary = _build_plain_summary(
        action, score, signal_strength, risk_level,
        trend, patterns, vp, breakouts, canslim, trade_plan,
    )

    return SignalEngineResult(
        action=action,
        score=score,
        confidence=confidence,
        risk_level=risk_level,
        signal_strength=signal_strength,
        trend=trend,
        patterns=patterns,
        volume_price=vp,
        breakouts=breakouts,
        canslim=canslim,
        module_scores=module_scores,
        buy_signals=buy_signals,
        sell_signals=sell_signals,
        risk_warnings=risk_warnings,
        key_levels=key_levels,
        description=description,
        plain_summary=plain_summary,
        trade_plan=trade_plan,
    )
