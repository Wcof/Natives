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
from serve.signal import (
    MAX_KLINE_COUNT, MAX_CHANLUN_COUNT,
    _parse_count, kline_to_dict, quote_to_dict, signal_to_dict,
    _apply_signal_optimization,
)
from serve.scan import (
    _scan_state, _scan_lock, _scan_one_stock, _run_scan, handle_scan,
)
from serve.handler import Handler

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
log = logging.getLogger("trend_app")

PORT = 8795
DASHBOARD_DIR = os.path.join(ROOT, "dashboard")


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
