"""数据层：K线(腾讯前复权+东财补充成交额) + 实时行情(东财) + 资金流(东财) + 搜索(东财)。

参考《A股资金流向监控工具-个股增强版》的host池轮换、fltt=2、session复用设计。

数据源策略：
- K线价格/成交量：腾讯API前复权（价格准确，和行情一致）
- K线成交额/换手率：东财K线API（有值，按日期匹配补充）
- 实时行情：东财stock/get（fltt=2，价格不除100）
- 资金流：东财fflow/daykline（push2his优先，push2test备选）
"""
from __future__ import annotations

import json
import os
import sys
import time
import socket
import logging
from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Tuple

try:
    import requests
except ImportError:
    _LIBS_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "libs")
    if os.path.isdir(_LIBS_DIR):
        sys.path.insert(0, _LIBS_DIR)
    import requests

log = logging.getLogger("trend_data")

# ---- DNS重定向：push2his/push2 → push2delay IP ----
# push2his服务器拒绝直连，但push2delay CDN按SNI返回数据
# 这是东财API的已知行为，参考A股资金流向监控工具的实现
_PUSH2DELAY_IP = "117.184.45.167"
_original_getaddrinfo = socket.getaddrinfo
_dns_redirected = False


def _patch_dns():
    """Monkey-patch socket.getaddrinfo，将push2his/push2重定向到push2delay IP。"""
    global _dns_redirected, _original_getaddrinfo
    if _dns_redirected:
        return
    _dns_redirected = True

    def _patched_getaddrinfo(host, port, family=0, type=0, proto=0, flags=0):
        if host and ("push2his.eastmoney.com" in host or host == "push2.eastmoney.com"):
            # 直接返回push2delay的IP，SNI保持原host
            return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", (_PUSH2DELAY_IP, port or 443))]
        return _original_getaddrinfo(host, port, family, type, proto, flags)

    socket.getaddrinfo = _patched_getaddrinfo
    log.debug("DNS重定向已启用: push2his/push2 → push2delay IP")

# ---- host池 ----
QUOTE_HOSTS = [
    "https://push2delay.eastmoney.com",
    "https://push2test.eastmoney.com",
    "https://push2.eastmoney.com",
]
HIS_HOSTS = [
    "https://push2his.eastmoney.com",
    "https://push2test.eastmoney.com",
    "https://82.push2his.eastmoney.com",
    "https://90.push2his.eastmoney.com",
    "https://push2delay.eastmoney.com",
]
SEARCH_HOST = "https://searchapi.eastmoney.com"
TENCENT_KLINE = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get"

UA_POOL = [
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:120.0) Gecko/20100101",
]

# ---- 模块级session ----
_session: Optional[requests.Session] = None
_ua_idx = 0

_cache: Dict[str, Tuple[Any, float]] = {}
_CACHE_TTL = 15


def _get_session() -> requests.Session:
    global _session
    if _session is None:
        _patch_dns()  # 启用DNS重定向
        _session = requests.Session()
        _session.trust_env = False
        _session.proxies = {"http": None, "https": None}
        _session.headers.update({
            "User-Agent": UA_POOL[0],
            "Accept": "*/*",
            "Accept-Language": "zh-CN,zh;q=0.9",
            "Referer": "https://quote.eastmoney.com/",
            "Connection": "keep-alive",
        })
    return _session


def _rotate_ua() -> None:
    global _ua_idx
    _ua_idx = (_ua_idx + 1) % len(UA_POOL)
    _get_session().headers["User-Agent"] = UA_POOL[_ua_idx]


def _cached(key: str) -> Optional[Any]:
    e = _cache.get(key)
    if e and time.time() - e[1] < _CACHE_TTL:
        return e[0]
    return None


def _set_cache(key: str, val: Any) -> None:
    _cache[key] = (val, time.time())


def _cache_get(key: str, ttl: float) -> Optional[Any]:
    """带自定义TTL的缓存读取，None表示未命中。"""
    e = _cache.get(key)
    if e and time.time() - e[1] < ttl:
        return e[0]
    return None


def _cache_set(key: str, val: Any) -> None:
    _cache[key] = (val, time.time())


def _to_float(v: Any) -> Optional[float]:
    if v is None or v == "-" or v == "":
        return None
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def _get_json_eastmoney(path: str, params: dict, host_pool: List[str]) -> Optional[dict]:
    """东财API请求，带host池轮换。path如/api/qt/stock/get"""
    s = _get_session()
    current_url = host_pool[0] + path
    for attempt in range(len(host_pool)):
        try:
            r = s.get(current_url, params=params, timeout=8)
            if r.status_code == 200:
                data = r.json()
                if data and data.get("data") is not None:
                    # 检查klines是否为空列表（push2delay对kline接口返回空klines）
                    d = data["data"]
                    if isinstance(d, dict) and "klines" in d and not d["klines"]:
                        log.debug(f"host返回空klines，尝试下一个: {current_url}")
                    else:
                        return data
        except Exception as e:
            log.debug(f"请求失败 try={attempt+1} {current_url}: {e}")
        _rotate_ua()
        current_url = host_pool[(attempt + 1) % len(host_pool)] + path
        time.sleep(0.3)
    return None


def _is_etf(symbol: str) -> bool:
    """判断是否为ETF/LOF基金。5开头=沪市ETF，1开头(非000/002/300)=深市ETF/LOF，159开头=深市ETF。"""
    s = symbol.strip().zfill(6)
    # 沪市：5开头(ETF/LOF)
    if s.startswith("5"):
        return True
    # 深市：159开头(ETF)、18开头(封闭式基金)
    if s.startswith(("159", "18")):
        return True
    # 深市：1开头但非A股(000/002/003/300)
    if s.startswith("1") and not s.startswith(("000", "002", "003", "100", "110", "120", "130", "140", "150", "160", "170", "180", "200", "300")):
        return True
    return False


# 实时数据缓存，2秒——保证秒级刷新拿到最新数据（quote/flow/minute共享）
_RT_CACHE_TTL = 2
