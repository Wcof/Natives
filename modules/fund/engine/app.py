"""趋势分析实时买卖点工具 - API服务器。

启动：python app.py
API端口：8795  |  看板端口：同端口（/ → index.html）

API：
  GET /api/analyze?symbol=600000          全量分析
  GET /api/quote?symbol=600000            实时行情
  GET /api/search?keyword=贵州             搜索股票
  GET /api/kline?symbol=600000&count=250  K线数据
"""
from __future__ import annotations

import json
import sys
import os
import logging
import threading
import time
import concurrent.futures
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs

# 确保项目根目录在path中
ROOT = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, ROOT)

from data.kline_fetcher import (
    fetch_kline, fetch_quote, fetch_fund_flow, search_stock, fetch_minute,
    fetch_realtime_flow, fetch_all_a_shares, fetch_index_kline, fetch_market_breadth,
    Kline, Quote, FundFlow, MinuteData, MinuteFlow
)
from analysis.signal_engine import run_analysis, SignalEngineResult
from analysis.chanlun_minute import analyze_chanlun_minute, signals_to_dict
from analysis.chanlun_daily import analyze_chanlun_daily, daily_result_to_dict

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
log = logging.getLogger("trend_app")

PORT = 8795
DASHBOARD_DIR = os.path.join(ROOT, "dashboard")
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


# ---- API处理 ----
def handle_analyze(params: dict) -> dict:
    symbol = params.get("symbol", [""])[0].strip()
    if not symbol:
        return {"error": "缺少symbol参数"}

    period = params.get("period", ["day"])[0].strip()
    log.info(f"开始分析 {symbol} (period={period})")

    # 获取数据
    klines = fetch_kline(symbol, count=250, period=period)
    if len(klines) < 30:
        return {"error": f"K线数据不足: {len(klines)}条"}

    quote = fetch_quote(symbol)
    flows = fetch_fund_flow(symbol, days=30)

    # 获取大盘指数（上证指数）用于CAN SLIM M维度
    from data.kline_fetcher import fetch_index_kline, fetch_market_breadth
    try:
        index_klines = fetch_index_kline("000001", count=60)
    except Exception:
        index_klines = None

    # 获取市场宽度（涨跌家数）用于M维度修正
    try:
        breadth = fetch_market_breadth()
    except Exception:
        breadth = None

    # 运行分析
    result = run_analysis(klines, quote, flows, index_klines)

    # 后处理：用市场宽度修正M评分（加密模块无法内部修改，在此修正）
    signal_data = signal_to_dict(result)
    if breadth and signal_data.get("canslim") and breadth.get("total", 0) >= 50:
        br = breadth.get("breadth_ratio", 0.5)
        up_n = breadth.get("up", 0)
        down_n = breadth.get("down", 0)
        pct_str = f"{br * 100:.0f}%"
        old_m = signal_data["canslim"]["m_score"]
        if br >= 0.7:
            bonus = 15
            br_label = "广度强"
        elif br >= 0.6:
            bonus = 10
            br_label = "偏多"
        elif br >= 0.5:
            bonus = 5
            br_label = "中性"
        elif br >= 0.4:
            bonus = -5
            br_label = "偏空"
        elif br >= 0.3:
            bonus = -10
            br_label = "广度弱"
        else:
            bonus = -15
            br_label = "普跌"
        new_m = max(0, min(100, old_m + bonus))
        signal_data["canslim"]["m_score"] = new_m
        br_signal = f"今日{up_n}涨/{down_n}跌，{pct_str}个股上涨({br_label})"
        if signal_data["canslim"].get("signals"):
            signal_data["canslim"]["signals"] = list(signal_data["canslim"]["signals"]) + [br_signal]
        if signal_data["canslim"].get("description"):
            signal_data["canslim"]["description"] = signal_data["canslim"]["description"] + f"；{br_signal}"

    # ---- 信号引擎优化：硬否决/软否决/分级体系/仓位管理/盈亏比 ----
    signal_data = _apply_signal_optimization(signal_data, klines, quote)

    # 构建大盘环境摘要
    market_env = ""
    if index_klines and len(index_klines) >= 20:
        idx_close = index_klines[-1].close
        idx_pct = index_klines[-1].pct
        idx_20d = (index_klines[-1].close - index_klines[-21].close) / index_klines[-21].close * 100 if len(index_klines) >= 21 else 0
        market_env = f"上证{idx_close:.1f}({idx_pct:+.2f}%) 20日{idx_20d:+.1f}%"
        if breadth:
            up_n = breadth.get("up", 0)
            down_n = breadth.get("down", 0)
            br = breadth.get("breadth_ratio", 0)
            market_env += f" | {up_n}涨{down_n}跌({br*100:.0f}%上涨)"

    return {
        "symbol": symbol,
        "name": quote.name if quote else "",
        "quote": quote_to_dict(quote) if quote else None,
        "signal": signal_data,
        "klines": [kline_to_dict(k) for k in klines[-120:]],  # 返回最近120条K线
        "flows": [{"date": f.date, "main_net": f.main_net, "super_large_net": f.super_large_net,
                    "large_net": f.large_net, "main_pct": f.main_pct} for f in flows] if flows else [],
        "market_env": market_env,  # 大盘环境摘要
        "breadth": breadth,  # 市场宽度（涨跌家数）
    }


def handle_quote(params: dict) -> dict:
    symbol = params.get("symbol", [""])[0].strip()
    if not symbol:
        return {"error": "缺少symbol参数"}
    q = fetch_quote(symbol)
    return quote_to_dict(q) if q else {"error": "获取行情失败"}


def handle_search(params: dict) -> dict:
    keyword = params.get("keyword", [""])[0].strip()
    if not keyword:
        return {"error": "缺少keyword参数"}
    results = search_stock(keyword)
    return {"results": results}


def handle_kline(params: dict) -> dict:
    symbol = params.get("symbol", [""])[0].strip()
    count = _parse_count(params, max_count=MAX_KLINE_COUNT)
    period = params.get("period", ["day"])[0].strip()
    if not symbol:
        return {"error": "缺少symbol参数"}
    klines = fetch_kline(symbol, count=count, period=period)
    return {"klines": [kline_to_dict(k) for k in klines]}


def handle_minute(params: dict) -> dict:
    """分时数据接口。"""
    symbol = params.get("symbol", [""])[0].strip()
    if not symbol:
        return {"error": "缺少symbol参数"}
    md = fetch_minute(symbol)
    if not md:
        return {"error": "获取分时数据失败"}
    return {
        "symbol": symbol,
        "name": md.name,
        "pre_close": md.pre_close,
        "high": md.high,
        "low": md.low,
        "times": md.times,
        "prices": md.prices,
        "avg_prices": md.avg_prices,
        "volumes": md.volumes,
    }


def handle_chanlun_minute(params: dict) -> dict:
    """缠论分时分析接口。在分时数据上运行缠论分析，返回买卖点信号。"""
    symbol = params.get("symbol", [""])[0].strip()
    if not symbol:
        return {"error": "缺少symbol参数"}
    md = fetch_minute(symbol)
    if not md or not md.prices:
        return {"error": "获取分时数据失败"}
    # 运行缠论分析
    result = analyze_chanlun_minute(md.times, md.prices, md.volumes)
    return signals_to_dict(result)


def handle_chanlun_daily(params: dict) -> dict:
    """缠论日线/周线分析接口。在日K或周K线上运行完整缠论分析，返回买卖点、分型、笔、中枢及图表叠加数据。"""
    symbol = params.get("symbol", [""])[0].strip()
    if not symbol:
        return {"error": "缺少symbol参数"}
    count = _parse_count(params, max_count=MAX_CHANLUN_COUNT)
    period = params.get("period", ["day"])[0].strip()
    klines = fetch_kline(symbol, count=count, period=period)
    if not klines or len(klines) < 10:
        return {"error": f"K线数据不足（仅{len(klines) if klines else 0}根）"}
    dates = [k.date for k in klines]
    opens = [k.open for k in klines]
    closes = [k.close for k in klines]
    highs = [k.high for k in klines]
    lows = [k.low for k in klines]
    volumes = [k.volume for k in klines]
    result = analyze_chanlun_daily(dates, opens, closes, highs, lows, volumes)
    return daily_result_to_dict(result)


def handle_realtime_flow(params: dict) -> dict:
    """盘中实时分时资金流接口。返回当日1分钟级累计资金流。"""
    symbol = params.get("symbol", [""])[0].strip()
    if not symbol:
        return {"error": "缺少symbol参数"}
    flows = fetch_realtime_flow(symbol)
    if not flows:
        return {"error": "暂无实时资金流数据（非交易日或盘前）", "flows": []}
    # 最后一根是当日累计总值
    last = flows[-1]
    return {
        "symbol": symbol,
        "flows": [{"time": f.time, "main_net": f.main_net, "super_large_net": f.super_large_net,
                    "large_net": f.large_net, "medium_net": f.medium_net, "small_net": f.small_net} for f in flows],
        "summary": {
            "main_net": last.main_net,
            "super_large_net": last.super_large_net,
            "large_net": last.large_net,
            "medium_net": last.medium_net,
            "small_net": last.small_net,
        },
        "time_range": f"{flows[0].time} ~ {flows[-1].time}",
    }


# ---- 扫描功能 ----
_scan_state = {
    "status": "idle",        # idle | running | done | error
    "stage": "",             # 当前阶段描述
    "progress": 0,           # 0-100
    "total": 0,
    "scanned": 0,
    "found": 0,
    "results": [],
    "error": "",
    "start_time": 0,
    "elapsed": 0,
}
_scan_lock = threading.Lock()


def _scan_one_stock(symbol: str, period: str, index_klines, breadth) -> dict:
    """分析单只股票，返回简化结果。供扫描调用。"""
    try:
        klines = fetch_kline(symbol, count=250, period=period)
        if len(klines) < 30:
            return None
        quote = fetch_quote(symbol)
        flows = fetch_fund_flow(symbol, days=30)

        result = run_analysis(klines, quote, flows, index_klines)
        signal_data = signal_to_dict(result)

        # M评分修正（与handle_analyze一致）
        if breadth and signal_data.get("canslim") and breadth.get("total", 0) >= 50:
            br = breadth.get("breadth_ratio", 0.5)
            old_m = signal_data["canslim"]["m_score"]
            if br >= 0.7:
                bonus = 15
            elif br >= 0.6:
                bonus = 10
            elif br >= 0.5:
                bonus = 5
            elif br >= 0.4:
                bonus = -5
            elif br >= 0.3:
                bonus = -10
            else:
                bonus = -15
            signal_data["canslim"]["m_score"] = max(0, min(100, old_m + bonus))

        # 信号引擎优化后处理
        signal_data = _apply_signal_optimization(signal_data, klines, quote)

        return {
            "symbol": symbol,
            "name": quote.name if quote else "",
            "price": quote.price if quote else 0,
            "action": signal_data.get("action", "观望"),
            "score": signal_data.get("score", 0),
            "confidence": signal_data.get("confidence", 0),
            "original_action": signal_data.get("original_action", ""),
            "veto_reason": signal_data.get("veto_reason", ""),
            "position_advice": signal_data.get("position_advice", ""),
            "risk_reward": signal_data.get("risk_reward", 0),
            "m_score": (signal_data.get("canslim") or {}).get("m_score", 50),
            "module_scores": signal_data.get("module_scores", {}),
            "risk_notes": signal_data.get("risk_notes", []),
        }
    except Exception as e:
        log.debug(f"扫描{symbol}({period})失败: {e}")
        return None


def _run_scan(max_stocks: int = 1000):
    """后台扫描全A股，日K+周K双周期买入筛选。"""
    global _scan_state
    try:
        with _scan_lock:
            _scan_state.update({
                "status": "running",
                "stage": "获取A股列表...",
                "progress": 0,
                "scanned": 0,
                "found": 0,
                "results": [],
                "error": "",
                "start_time": time.time(),
                "elapsed": 0,
            })

        # ---- 1. 获取全A股列表 ----
        all_stocks = fetch_all_a_shares()
        if not all_stocks:
            with _scan_lock:
                _scan_state["status"] = "error"
                _scan_state["error"] = "获取A股列表失败"
            return

        # ---- 2. 预过滤：排除ST/退市/停牌(价格=0) ----
        filtered = []
        for s in all_stocks:
            name = s.get("name", "")
            price = s.get("price", 0)
            # 排除ST、退市
            if "ST" in name or "退" in name:
                continue
            # 排除停牌/无报价的股票
            if not price or price <= 0:
                continue
            filtered.append(s)

        total_stage1 = len(filtered)
        log.info(f"扫描开始: 全A股{len(all_stocks)}只 → 过滤后{total_stage1}只（排除ST/退市）")

        with _scan_lock:
            _scan_state["total"] = total_stage1
            _scan_state["stage"] = f"日K扫描({total_stage1}只)..."

        # ---- 3. 预获取共享数据 ----
        index_klines = None
        try:
            index_klines = fetch_index_kline("000001", count=60)
        except Exception:
            pass

        breadth = None
        try:
            breadth = fetch_market_breadth()
        except Exception:
            pass

        # ---- 4. 并发日K扫描 ----
        daily_buy = []
        scanned_count = 0

        def scan_daily(stock):
            nonlocal scanned_count
            code = stock["code"]
            r = _scan_one_stock(code, "day", index_klines, breadth)
            with _scan_lock:
                scanned_count += 1
                _scan_state["scanned"] = scanned_count
                _scan_state["progress"] = round(scanned_count / max(total_stage1, 1) * 50, 1)
            if r and r["action"] in ("强烈买入", "买入", "谨慎买入"):
                r["daily_name"] = stock.get("name", "")
                r["daily_pct"] = stock.get("pct", 0)
                return r
            return None

        with concurrent.futures.ThreadPoolExecutor(max_workers=20) as executor:
            futures = {executor.submit(scan_daily, s): s for s in filtered}
            for f in concurrent.futures.as_completed(futures):
                try:
                    r = f.result()
                    if r:
                        daily_buy.append(r)
                        with _scan_lock:
                            _scan_state["found"] = len(daily_buy)
                except Exception:
                    pass

        log.info(f"日K扫描完成: {total_stage1}只 → {len(daily_buy)}只有买入信号")

        # ---- 5. 对日K买入的股票，扫描周K ----
        with _scan_lock:
            _scan_state["total"] = len(daily_buy)
            _scan_state["scanned"] = 0
            _scan_state["stage"] = f"周K验证({len(daily_buy)}只)..."

        weekly_scanned = 0
        dual_buy = []

        def scan_weekly(stock):
            nonlocal weekly_scanned
            code = stock["symbol"]
            r = _scan_one_stock(code, "week", index_klines, breadth)
            with _scan_lock:
                weekly_scanned += 1
                _scan_state["scanned"] = weekly_scanned
                _scan_state["progress"] = 50 + round(weekly_scanned / max(len(daily_buy), 1) * 50, 1)
            if r and r["action"] in ("强烈买入", "买入", "谨慎买入"):
                return r
            return None

        with concurrent.futures.ThreadPoolExecutor(max_workers=15) as executor:
            futures = {executor.submit(scan_weekly, s): s for s in daily_buy}
            for f in concurrent.futures.as_completed(futures):
                try:
                    r = f.result()
                    if r:
                        # 找到对应的日K数据
                        daily = next((d for d in daily_buy if d["symbol"] == r["symbol"]), {})
                        dual_buy.append({
                            "symbol": r["symbol"],
                            "name": daily.get("daily_name", r.get("name", "")),
                            "price": daily.get("price", 0),
                            "daily_pct": daily.get("daily_pct", 0),
                            "daily_action": daily.get("action", ""),
                            "daily_score": daily.get("score", 0),
                            "daily_confidence": daily.get("confidence", 0),
                            "weekly_action": r["action"],
                            "weekly_score": r["score"],
                            "weekly_confidence": r["confidence"],
                            "combined_score": daily.get("score", 0) + r["score"],
                            "position_advice": daily.get("position_advice", ""),
                            "risk_reward": daily.get("risk_reward", 0),
                            "veto_reason": daily.get("veto_reason", ""),
                            "m_score": daily.get("m_score", 50),
                            "risk_notes": daily.get("risk_notes", []),
                        })
                        with _scan_lock:
                            _scan_state["found"] = len(dual_buy)
                except Exception:
                    pass

        # ---- 6. 排序取前20 ----
        dual_buy.sort(key=lambda x: x["combined_score"], reverse=True)
        results = dual_buy[:20]

        elapsed = round(time.time() - _scan_state["start_time"], 1)
        with _scan_lock:
            _scan_state.update({
                "status": "done",
                "stage": f"完成: {len(dual_buy)}只双周期买入，取前{len(results)}",
                "progress": 100,
                "results": results,
                "elapsed": elapsed,
            })
        log.info(f"扫描完成: {total_stage1}→{len(daily_buy)}→{len(dual_buy)}→TOP{len(results)}, 耗时{elapsed}s")

    except Exception as e:
        with _scan_lock:
            _scan_state["status"] = "error"
            _scan_state["error"] = str(e)
        log.error(f"扫描失败: {e}", exc_info=True)


def handle_scan(params: dict) -> dict:
    """扫描API：启动扫描或返回进度/结果。"""
    action = params.get("action", ["status"])[0]
    max_stocks = 0  # 0 = 全量扫描，不设上限

    if action == "start":
        with _scan_lock:
            if _scan_state["status"] == "running":
                return {"status": "running", "message": "扫描进行中，请等待..."}
            # 重置状态
            _scan_state.update({
                "status": "idle", "stage": "", "progress": 0,
                "total": 0, "scanned": 0, "found": 0,
                "results": [], "error": "", "elapsed": 0,
            })
        # 启动后台线程
        t = threading.Thread(target=_run_scan, args=(max_stocks,), daemon=True)
        t.start()
        return {"status": "started", "message": "扫描已启动"}

    # 默认返回当前状态
    with _scan_lock:
        state = dict(_scan_state)
    elapsed = state.get("elapsed", 0)
    if state["status"] == "running" and state.get("start_time"):
        elapsed = round(time.time() - state["start_time"], 1)
    return {
        "status": state["status"],
        "stage": state["stage"],
        "progress": state["progress"],
        "total": state["total"],
        "scanned": state["scanned"],
        "found": state["found"],
        "results": state["results"],
        "error": state.get("error", ""),
        "elapsed": elapsed,
    }


# ---- HTTP Handler ----
class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        pass  # 静默日志，避免刷屏

    def _send_no_cache_headers(self):
        """防止浏览器缓存旧版页面"""
        self.send_header("Cache-Control", "no-cache, no-store, must-revalidate")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")

    def _json(self, data: dict, status: int = 200):
        body = json.dumps(data, ensure_ascii=False, default=str).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Access-Control-Allow-Origin", "*")
        self._send_no_cache_headers()
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _html(self, content: bytes, content_type: str = "text/html"):
        self.send_response(200)
        self.send_header("Content-Type", f"{content_type}; charset=utf-8")
        self._send_no_cache_headers()
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path
        params = parse_qs(parsed.query)

        # API路由
        if path.startswith("/api/"):
            try:
                if path == "/api/health":
                    self._json({"status": "ok", "time": time.strftime("%H:%M:%S")})
                elif path == "/api/analyze":
                    self._json(handle_analyze(params))
                elif path == "/api/quote":
                    self._json(handle_quote(params))
                elif path == "/api/search":
                    self._json(handle_search(params))
                elif path == "/api/kline":
                    self._json(handle_kline(params))
                elif path == "/api/minute":
                    self._json(handle_minute(params))
                elif path == "/api/chanlun_minute":
                    self._json(handle_chanlun_minute(params))
                elif path == "/api/chanlun_daily":
                    self._json(handle_chanlun_daily(params))
                elif path == "/api/realtime_flow":
                    self._json(handle_realtime_flow(params))
                elif path == "/api/scan":
                    self._json(handle_scan(params))
                else:
                    self._json({"error": "未知API"}, 404)
            except Exception as e:
                log.error(f"API错误: {e}", exc_info=True)
                self._json({"error": str(e)}, 500)
            return

        # 静态文件（看板）
        if path == "/" or path == "/index.html":
            filepath = os.path.join(DASHBOARD_DIR, "index.html")
        else:
            # 安全处理静态文件路径
            safe_path = path.lstrip("/")
            filepath = os.path.normpath(os.path.join(DASHBOARD_DIR, safe_path))
            if not filepath.startswith(DASHBOARD_DIR):
                self._json({"error": "禁止访问"}, 403)
                return

        if os.path.isfile(filepath):
            ext = os.path.splitext(filepath)[1].lower()
            ct = {
                ".html": "text/html", ".js": "application/javascript",
                ".css": "text/css", ".png": "image/png", ".jpg": "image/jpeg",
                ".svg": "image/svg+xml", ".ico": "image/x-icon",
            }.get(ext, "application/octet-stream")
            with open(filepath, "rb") as f:
                self._html(f.read(), ct)
        else:
            self._json({"error": "文件不存在"}, 404)

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()


def main():
    import argparse
    parser = argparse.ArgumentParser(description="趋势分析实时买卖点工具 - API服务")
    parser.add_argument("--port", type=int, default=PORT, help="服务端口")
    args = parser.parse_args()
    port = args.port

    os.makedirs(DASHBOARD_DIR, exist_ok=True)
    # ThreadingHTTPServer: 多线程处理，浏览器并发请求不会卡死
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    log.info(f"趋势分析实时买卖点工具启动 → http://127.0.0.1:{port}")
    log.info(f"API: /api/analyze?symbol=600000")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        log.info("服务停止")
    finally:
        # 无论正常还是异常退出，都释放监听端口与资源
        server.shutdown()
        server.server_close()
        log.info("服务资源已释放")


if __name__ == "__main__":
    main()
