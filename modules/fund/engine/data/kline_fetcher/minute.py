"""分时数据：东财trends2/get。"""
from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional

from .session import QUOTE_HOSTS, _RT_CACHE_TTL, log, _cache_get, _set_cache, _get_json_eastmoney
from .symbols import symbol_to_secid


# ---- 分时数据 ----
@dataclass
class MinuteData:
    times: List[str]       # 时间标签 "09:30"
    prices: List[float]    # 价格
    avg_prices: List[float] # 均价
    volumes: List[float]   # 成交量
    pre_close: float = 0.0
    name: str = ""
    high: float = 0.0
    low: float = 0.0


def fetch_minute(symbol: str) -> Optional[MinuteData]:
    """获取当日分时数据。东财trends2/get。"""
    cache_key = f"minute_{symbol}"
    cached = _cache_get(cache_key, _RT_CACHE_TTL)  # 用5秒短缓存，保证实时性
    if cached:
        return cached

    secid = symbol_to_secid(symbol)
    params = {
        "secid": secid,
        "fields1": "f1,f2,f3,f4,f5,f6,f7,f8,f9,f10,f11,f12,f13",
        "fields2": "f51,f52,f53,f54,f55,f56,f57,f58",
        "isccr": "1",
        "ndays": "1",
        "iscca": "0",
        "klt": "5",
        "fqt": "1",
        "ut": "fa5fd1943c7b386f172d6893dbfba10b",
    }

    data = _get_json_eastmoney("/api/qt/stock/trends2/get", params, QUOTE_HOSTS)
    if not data or not data.get("data"):
        return None

    d = data["data"]
    trends = d.get("trends", [])
    if not trends:
        return None

    times, prices, avg_prices, volumes = [], [], [], []
    high, low = 0.0, 999999.0
    for line in trends:
        parts = line.split(",")
        if len(parts) >= 8:
            t = parts[0].split(" ")[-1] if " " in parts[0] else parts[0]
            price = float(parts[2]) if parts[2] else 0.0
            avg = float(parts[7]) if parts[7] else 0.0
            vol = float(parts[5]) if parts[5] else 0.0
            times.append(t)
            prices.append(price)
            avg_prices.append(avg)
            volumes.append(vol)
            if price > 0:
                high = max(high, price)
                low = min(low, price)

    md = MinuteData(
        times=times, prices=prices, avg_prices=avg_prices, volumes=volumes,
        pre_close=float(d.get("preClose", 0) or 0),
        name=d.get("name", ""),
        high=high if high > 0 else 0.0,
        low=low if low < 999999 else 0.0,
    )
    _set_cache(cache_key, md)
    return md
