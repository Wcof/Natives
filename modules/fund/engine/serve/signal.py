"""信号解析与序列化：count安全解析 + K线/行情/信号→dict + 信号引擎优化后处理。"""
from __future__ import annotations

import logging

from analysis.signal_engine import SignalEngineResult
from data.kline_fetcher import Kline, Quote

log = logging.getLogger("trend_app")

# count 参数安全解析上限，防止非法输入导致 500 或超大值放大网络请求
MAX_KLINE_COUNT = 10000
MAX_CHANLUN_COUNT = 10000


def _parse_count(params: dict, default: int = 250, max_count: int = MAX_KLINE_COUNT) -> int:
    """安全解析 count 查询参数：非法/超限时回退到默认值或钳制到上限。"""
    raw = params.get("count", [str(default)])[0]
    try:
        value = int(raw)
    except (TypeError, ValueError):
        return default
    if value <= 0:
        return default
    return min(value, max_count)


# ---- 数据序列化 ----
def kline_to_dict(k: Kline) -> dict:
    return {
        "date": k.date, "open": k.open, "close": k.close,
        "high": k.high, "low": k.low, "volume": k.volume,
        "amount": k.amount, "pct": k.pct, "turnover": k.turnover,
    }


def quote_to_dict(q: Quote) -> dict:
    return {
        "symbol": q.symbol, "name": q.name, "price": q.price, "pct": q.pct,
        "change": q.change, "high": q.high, "low": q.low, "open": q.open,
        "pre_close": q.pre_close, "volume": q.volume, "amount": q.amount,
        "turnover": q.turnover,
    }


def signal_to_dict(r: SignalEngineResult) -> dict:
    """将信号引擎结果序列化为JSON。"""
    data = {
        "action": r.action,
        "score": r.score,
        "confidence": r.confidence,
        "risk_level": r.risk_level,
        "signal_strength": r.signal_strength,
        "plain_summary": r.plain_summary,
        "trade_plan": r.trade_plan,
        "module_scores": r.module_scores,
        "buy_signals": r.buy_signals,
        "sell_signals": r.sell_signals,
        "risk_warnings": r.risk_warnings,
        "key_levels": r.key_levels,
        "description": r.description,
        "trend": None,
        "patterns": [],
        "volume_price": None,
        "breakouts": [],
        "canslim": None,
    }

    if r.trend:
        data["trend"] = {
            "direction": r.trend.direction,
            "strength": r.trend.strength,
            "stage": r.trend.stage,
            "ma_arrangement": r.trend.ma_arrangement,
            "ma_scores": r.trend.ma_scores,
            "trendline": r.trend.trendline,
            "signals": r.trend.signals,
        }

    for p in r.patterns:
        data["patterns"].append({
            "name": p.name,
            "direction": p.direction,
            "confidence": p.confidence,
            "status": p.status,
            "target_price": p.target_price,
            "key_levels": p.key_levels,
            "description": p.description,
        })

    if r.volume_price:
        data["volume_price"] = {
            "pattern": r.volume_price.pattern,
            "direction": r.volume_price.direction,
            "confidence": r.volume_price.confidence,
            "volume_ratio": r.volume_price.volume_ratio,
            "turnover": r.volume_price.turnover,
            "obv_trend": r.volume_price.obv_trend,
            "signals": r.volume_price.signals,
            "description": r.volume_price.description,
        }

    for b in r.breakouts:
        data["breakouts"].append({
            "system": b.system,
            "signal": b.signal,
            "breakout_price": b.breakout_price,
            "current_n": b.current_n,
            "stop_loss": b.stop_loss,
            "entry_price": b.entry_price,
            "position_units": b.position_units,
            "exit_price": b.exit_price,
            "channel_high": b.channel_high,
            "channel_low": b.channel_low,
            "next_add_price": b.next_add_price,
            "signals": b.signals,
            "description": b.description,
        })

    if r.canslim:
        data["canslim"] = {
            "c_score": r.canslim.c_score,
            "a_score": r.canslim.a_score,
            "n_score": r.canslim.n_score,
            "s_score": r.canslim.s_score,
            "l_score": r.canslim.l_score,
            "i_score": r.canslim.i_score,
            "m_score": r.canslim.m_score,
            "total": r.canslim.total,
            "grade": r.canslim.grade,
            "signals": r.canslim.signals,
            "cup_handle": r.canslim.cup_handle,
            "description": r.canslim.description,
        }

    return data


def _apply_signal_optimization(signal_data: dict, klines: list, quote) -> dict:
    """信号引擎优化后处理：硬否决/软否决/分级体系/仓位管理/盈亏比检查。

    在加密signal_engine返回结果后，通过后处理实现股神级风险控制。
    """
    action = signal_data.get("action", "观望")
    score = signal_data.get("score", 0)
    confidence = signal_data.get("confidence", 0)
    module_scores = signal_data.get("module_scores", {})
    buy_signals = signal_data.get("buy_signals", [])
    sell_signals = signal_data.get("sell_signals", [])
    risk_warnings = list(signal_data.get("risk_warnings", []))
    canslim = signal_data.get("canslim") or {}
    m_score = canslim.get("m_score", 50)
    trade_plan = dict(signal_data.get("trade_plan") or {})

    original_action = action

    # ---- 1. 收集个股信号文本（排除大盘M信号）----
    stock_signals = []
    trend_data = signal_data.get("trend") or {}
    stock_signals.extend(trend_data.get("signals", []))
    vp_data = signal_data.get("volume_price") or {}
    stock_signals.extend(vp_data.get("signals", []))
    for s in buy_signals + sell_signals:
        if any(kw in s for kw in ("大盘", "空头环境", "今日", "上证")):
            continue
        stock_signals.append(s)
    all_signal_text = " ".join(stock_signals)

    # ---- 2. 硬否决检查（仅个股信号，不看大盘）----
    HARD_VETO = [
        ("跌破MA20", "价格跌破MA20，趋势已坏"),
        ("价跌量增", "价跌量增，恐慌抛售信号"),
        ("OBV下降", "OBV下降，量能走弱"),
        ("OBV走低", "OBV走低，量能走弱"),
        ("OBV下行", "OBV下行，量能走弱"),
    ]
    hard_veto_reason = None
    for kw, desc in HARD_VETO:
        if kw in all_signal_text:
            hard_veto_reason = desc
            break
    # 量价pattern字段单独检查
    vp_pattern = vp_data.get("pattern", "")
    if "价跌量增" in vp_pattern and not hard_veto_reason:
        hard_veto_reason = "价跌量增，恐慌抛售信号"

    # ---- 3. 软否决检查 ----
    SOFT_VETO = [
        ("MA20向下", "MA20向下，短期趋势偏弱"),
        ("MA20下行", "MA20下行，短期趋势偏弱"),
        ("受压60日", "受压60日决策线，上方压力大"),
    ]
    soft_veto_reason = None
    for kw, desc in SOFT_VETO:
        if kw in all_signal_text:
            soft_veto_reason = desc
            break

    # ---- 4. 分级体系重新评级 ----
    is_buy = action in ("买入", "强烈买入")
    is_sell = action in ("卖出", "强烈卖出")
    veto_reason = None

    # 模块一致性
    scores_list = [
        module_scores.get("趋势", 50),
        module_scores.get("CAN_SLIM", 50),
        module_scores.get("突破", 50),
        module_scores.get("量价", 50),
        module_scores.get("形态", 50),
    ]
    modules_above_55 = sum(1 for s in scores_list if s >= 55)

    if is_sell:
        # 卖出信号不拦截，顺势离场
        pass
    elif is_buy:
        if hard_veto_reason:
            action = "观望"
            veto_reason = f"硬否决：{hard_veto_reason}"
        else:
            # 分级评定
            if score >= 75 and confidence >= 60 and modules_above_55 >= 4:
                new_action = "强烈买入"
            elif score >= 65 and confidence >= 45 and modules_above_55 >= 3:
                new_action = "买入"
            elif score >= 60:
                new_action = "谨慎买入"
            else:
                new_action = "观望"

            # 软否决降一级
            if soft_veto_reason:
                if new_action == "强烈买入":
                    new_action = "买入"
                    veto_reason = f"软否决：{soft_veto_reason}"
                elif new_action == "买入":
                    new_action = "谨慎买入"
                    veto_reason = f"软否决：{soft_veto_reason}"

            action = new_action

    # ---- 5. M分驱动仓位管理 ----
    original_position = trade_plan.get("position_size", "")
    if action in ("买入", "强烈买入", "谨慎买入"):
        if m_score < 40:
            position_advice = "轻仓(1/4) — 大盘偏空，严格控制仓位"
            if action == "强烈买入":
                action = "买入"
                veto_reason = (veto_reason + "；" if veto_reason else "") + f"大盘M分{m_score}偏低，降级为买入"
            elif action == "买入":
                action = "谨慎买入"
                veto_reason = (veto_reason + "；" if veto_reason else "") + f"大盘M分{m_score}偏低，降级为谨慎买入"
        elif m_score < 55:
            position_advice = "半仓(1/2) — 大盘中性偏弱"
        elif m_score < 65:
            position_advice = original_position or "半仓(1/2)"
        else:
            position_advice = original_position or "正常仓位"
    else:
        position_advice = "空仓等待"

    # ---- 6. 盈亏比检查 ----
    entry = trade_plan.get("entry_price", 0) or 0
    stop = trade_plan.get("stop_loss", 0) or 0
    target = trade_plan.get("target_price", 0) or 0
    risk_reward = trade_plan.get("risk_reward_ratio", 0) or 0

    risk_notes = []
    if entry and stop and target and entry > 0:
        if not risk_reward:
            risk_amt = entry - stop
            reward_amt = target - entry
            if risk_amt > 0:
                risk_reward = round(reward_amt / risk_amt, 1)

        if risk_reward:
            if risk_reward < 1.0:
                risk_notes.append(f"盈亏比{risk_reward}倒挂，不建议入场")
                if action in ("买入", "强烈买入", "谨慎买入"):
                    action = "观望"
                    veto_reason = (veto_reason + "；" if veto_reason else "") + f"盈亏比{risk_reward}倒挂"
            elif risk_reward < 1.5:
                risk_notes.append(f"盈亏比{risk_reward}偏低，谨慎操作")
            elif risk_reward < 2.0:
                risk_notes.append(f"盈亏比{risk_reward}，勉强达标")
            else:
                risk_notes.append(f"盈亏比{risk_reward}，风险收益比良好")

    # ---- 7. 写回信号数据 ----
    signal_data["action"] = action
    signal_data["optimized_action"] = action
    signal_data["original_action"] = original_action
    if veto_reason:
        signal_data["veto_reason"] = veto_reason
        risk_warnings.insert(0, veto_reason)
    signal_data["risk_warnings"] = risk_warnings
    signal_data["position_advice"] = position_advice
    signal_data["risk_notes"] = risk_notes
    signal_data["risk_reward"] = risk_reward

    if trade_plan:
        trade_plan["position_size"] = position_advice
        signal_data["trade_plan"] = trade_plan

    # 更新大白话总结
    if veto_reason and action != original_action:
        prefix = f"[优化：{original_action}→{action}] {veto_reason}。"
        signal_data["plain_summary"] = prefix + signal_data.get("plain_summary", "")

    log.info(
        f"信号优化：{original_action}→{action} "
        f"score={score} conf={confidence} M={m_score} "
        f"硬否决={'是' if hard_veto_reason else '否'} "
        f"软否决={'是' if soft_veto_reason else '否'} "
        f"盈亏比={risk_reward} 仓位={position_advice}"
    )
    return signal_data
