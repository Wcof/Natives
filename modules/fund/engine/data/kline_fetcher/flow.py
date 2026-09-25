"""资金流：东财fflow/daykline（历史）+ fflow/kline/get（盘中实时分时，push2delay）。"""
from __future__ import annotations

from dataclasses import dataclass
from typing import List

from .session import HIS_HOSTS, _RT_CACHE_TTL, log, _cached, _set_cache, _cache_get, _cache_set, _to_float, _get_json_eastmoney
from .symbols import symbol_to_secid


# ---- 资金流 ----
@dataclass
class FundFlow:
    date: str
    main_net: float
    super_large_net: float
    large_net: float
    medium_net: float
    small_net: float
    main_pct: float = 0.0


def fetch_fund_flow(symbol: str, days: int = 30) -> List[FundFlow]:
    """历史资金流。东财fflow/daykline。"""
    cache_key = f"flow_{symbol}_{days}"
    cached = _cached(cache_key)
    if cached:
        return cached

    secid = symbol_to_secid(symbol)
    params = {
        "lmt": str(days),
        "klt": "101",
        "secid": secid,
        "fields1": "f1,f2,f3,f7",
        "fields2": "f51,f52,f53,f54,f55,f56,f57",
        "ut": "fa5fd1943c7b386f172d6893dbfba10b",
    }

    data = _get_json_eastmoney("/api/qt/stock/fflow/daykline/get", params, HIS_HOSTS)
    if data and data.get("data") and data["data"].get("klines"):
        flows: List[FundFlow] = []
        for line in data["data"]["klines"]:
            parts = line.split(",")
            if len(parts) >= 7:
                f = FundFlow(
                    date=parts[0],
                    main_net=float(parts[1]),
                    small_net=float(parts[2]),
                    medium_net=float(parts[3]),
                    large_net=float(parts[4]),
                    super_large_net=float(parts[5]),
                    main_pct=float(parts[6]) if parts[6] else 0.0,
                )
                flows.append(f)
        if len(flows) >= 3:
            _set_cache(cache_key, flows)
            return flows
    return []


# ---- 盘中实时分时资金流 ----
# push2delay直连即可，不需要DNS重定向（push2test的fflow/kline返回0条数据）
RT_FLOW_HOSTS = [
    "https://push2delay.eastmoney.com",
    "https://push2.eastmoney.com",
    "https://82.push2.eastmoney.com",
    "https://90.push2.eastmoney.com",
]


@dataclass
class MinuteFlow:
    """盘中分时资金流（累计值）。"""
    time: str               # "09:31"
    main_net: float         # 累计主力净流入(元)
    small_net: float        # 累计小单净流入(元)
    medium_net: float       # 累计中单净流入(元)
    large_net: float        # 累计大单净流入(元)
    super_large_net: float  # 累计超大单净流入(元)


def fetch_realtime_flow(symbol: str) -> List[MinuteFlow]:
    """盘中实时分时资金流。东财fflow/kline/get，走push2delay，klt=1(1分钟)。

    返回当日所有1分钟K线的累计资金流，最后一根即为当日总净流入。
    非交易日或盘前返回空列表。
    """
    cache_key = f"rt_flow_{symbol}"
    cached = _cache_get(cache_key, _RT_CACHE_TTL)
    if cached is not None:
        return cached

    secid = symbol_to_secid(symbol)
    params = {
        "klt": "1",  # 1分钟
        "secid": secid,
        "fields1": "f1,f2,f3,f7",
        "fields2": "f51,f52,f53,f54,f55,f56,f57",
        "ut": "fa5fd1943c7b386f172d6893dbfba10b",
        "lmt": "300",
    }

    data = _get_json_eastmoney("/api/qt/stock/fflow/kline/get", params, RT_FLOW_HOSTS)
    if not data or not data.get("data") or not data["data"].get("klines"):
        _cache_set(cache_key, [])
        return []

    flows: List[MinuteFlow] = []
    for line in data["data"]["klines"]:
        parts = line.split(",")
        if len(parts) >= 6:
            t = parts[0].split(" ")[-1] if " " in parts[0] else parts[0]
            flows.append(MinuteFlow(
                time=t,
                main_net=_to_float(parts[1]) or 0.0,
                small_net=_to_float(parts[2]) or 0.0,
                medium_net=_to_float(parts[3]) or 0.0,
                large_net=_to_float(parts[4]) or 0.0,
                super_large_net=_to_float(parts[5]) or 0.0,
            ))
    _cache_set(cache_key, flows)
    return flows
