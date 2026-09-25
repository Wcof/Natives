"""实时行情：东财stock/get，fltt=2价格不除100。2秒缓存保证秒级实时性。"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from .session import QUOTE_HOSTS, _RT_CACHE_TTL, log, _cache_get, _cache_set, _to_float, _get_json_eastmoney
from .symbols import symbol_to_secid


# ---- 实时行情 ----
@dataclass
class Quote:
    symbol: str
    name: str
    price: float
    pct: float
    change: float
    high: float
    low: float
    open: float
    pre_close: float
    volume: float  # 成交量(手)
    amount: float  # 成交额(元)
    turnover: float  # 换手率(%)
    timestamp: str = ""


def fetch_quote(symbol: str) -> Optional[Quote]:
    """实时行情。东财stock/get，fltt=2价格不除100。2秒缓存保证秒级实时性。"""
    cache_key = f"quote_{symbol}"
    cached = _cache_get(cache_key, _RT_CACHE_TTL)
    if cached:
        return cached

    secid = symbol_to_secid(symbol)
    params = {
        "secid": secid,
        "fields": "f43,f44,f45,f46,f47,f48,f57,f58,f60,f169,f170,f168",
        "fltt": "2",
        "invt": "2",
        "ut": "fa5fd1943c7b386f172d6893dbfba10b",
    }

    data = _get_json_eastmoney("/api/qt/stock/get", params, QUOTE_HOSTS)
    if data and data.get("data"):
        d = data["data"]
        q = Quote(
            symbol=symbol,
            name=d.get("f58", ""),
            price=_to_float(d.get("f43")) or 0,
            pct=_to_float(d.get("f170")) or 0,
            change=_to_float(d.get("f169")) or 0,
            high=_to_float(d.get("f44")) or 0,
            low=_to_float(d.get("f45")) or 0,
            open=_to_float(d.get("f46")) or 0,
            pre_close=_to_float(d.get("f60")) or 0,
            volume=_to_float(d.get("f47")) or 0,
            amount=_to_float(d.get("f48")) or 0,
            turnover=_to_float(d.get("f168")) or 0,
        )
        _cache_set(cache_key, q)
        return q
    return None
