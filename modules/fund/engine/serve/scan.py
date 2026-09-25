"""扫描功能：全A股日K+周K双周期买入筛选（后台线程+进度状态）。"""
from __future__ import annotations

import concurrent.futures
import logging
import threading
import time

from analysis.signal_engine import run_analysis
from data.kline_fetcher import (
    fetch_all_a_shares, fetch_fund_flow, fetch_index_kline,
    fetch_kline, fetch_market_breadth, fetch_quote,
)
from serve.signal import _apply_signal_optimization, signal_to_dict

log = logging.getLogger("trend_app")

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
