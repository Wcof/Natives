"""大盘指数K线（上证指数/深证成指/创业板指）。"""
from __future__ import annotations

import json
from typing import List

from .session import TENCENT_KLINE, log, _get_session, _cached, _set_cache, _to_float, _get_json_eastmoney
from .kline import Kline, EM_KLINE_HOSTS


# ---- 大盘指数 ----
def fetch_index_kline(index_code: str = "000001", count: int = 60) -> List[Kline]:
    """获取大盘指数K线数据（上证指数/深证成指等）。

    index_code:
      - "000001" → 上证指数 (secid=1.000001, 腾讯=sh000001)
      - "399001" → 深证成指 (secid=0.399001, 腾讯=sz399001)
      - "399006" → 创业板指 (secid=0.399006, 腾讯=sz399006)
    """
    cache_key = f"index_{index_code}_{count}"
    cached = _cached(cache_key)
    if cached:
        return cached

    # 指数的secid: 上证=1.000001, 深证/创业板=0.399xxx
    if index_code.startswith("399"):
        secid = f"0.{index_code}"
    else:
        secid = f"1.{index_code}"

    # 指数无前复权概念(fqt=0)，用EM_KLINE_HOSTS(push2his优先+空klines检测)
    params = {
        "secid": secid,
        "ut": "fa5fd1943c7b386f172d6893dbfba10b",
        "fields1": "f1,f2,f3,f4,f5,f6",
        "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
        "klt": "101",
        "fqt": "0",
        "lmt": str(count),
        "end": "20500101",
    }

    data = _get_json_eastmoney("/api/qt/stock/kline/get", params, EM_KLINE_HOSTS)
    if not data or not data.get("data") or not data["data"].get("klines"):
        # 东财失败 → 腾讯尝试
        klines = _fetch_kline_tencent_index(index_code, count)
        if klines:
            _set_cache(cache_key, klines)
            return klines
        return []

    klines: List[Kline] = []
    for line in data["data"]["klines"]:
        parts = line.split(",")
        if len(parts) >= 7:
            k = Kline(
                date=parts[0],
                open=float(parts[1]),
                close=float(parts[2]),
                high=float(parts[3]),
                low=float(parts[4]),
                volume=float(parts[5]) if parts[5] else 0.0,
                amount=_to_float(parts[6]) or 0.0,
            )
            if len(parts) >= 11:
                k.turnover = _to_float(parts[10]) or 0.0
            if klines:
                k.pct = round((k.close - klines[-1].close) / klines[-1].close * 100, 2)
            klines.append(k)

    if len(klines) >= 10:
        _set_cache(cache_key, klines)
        return klines
    return []


def _fetch_kline_tencent_index(index_code: str, count: int) -> List[Kline]:
    """腾讯API获取指数K线（上证/深证/创业板）。"""
    if index_code.startswith("399"):
        tc_symbol = f"sz{index_code}"
    else:
        tc_symbol = f"sh{index_code}"

    params = {"param": f"{tc_symbol},day,,,{count},"}
    try:
        s = _get_session()
        r = s.get(TENCENT_KLINE, params=params, timeout=10)
        text = r.text.strip()
        if text.startswith("<!DOCTYPE") or text.startswith("<html"):
            return []
        try:
            data = r.json()
        except (json.JSONDecodeError, ValueError):
            return []
        if data.get("code") != 0:
            return []

        stock_data = data.get("data", {}).get(tc_symbol, {})
        # 指数没有qfq/day，只有day
        rows = stock_data.get("day", [])
        klines: List[Kline] = []
        for row in rows:
            if len(row) >= 6:
                vol = float(row[5]) if isinstance(row[5], (str, int, float)) else 0.0
                k = Kline(
                    date=str(row[0]),
                    open=float(row[1]),
                    close=float(row[2]),
                    high=float(row[3]),
                    low=float(row[4]),
                    volume=vol,
                )
                if klines:
                    k.pct = round((k.close - klines[-1].close) / klines[-1].close * 100, 2)
                klines.append(k)
        return klines
    except Exception as e:
        log.error(f"腾讯指数K线失败 {index_code}: {e}")
        return []
