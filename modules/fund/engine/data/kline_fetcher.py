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


# ---- 代码转换 ----
def symbol_to_secid(symbol: str) -> str:
    symbol = str(symbol).strip().zfill(6)
    if symbol.startswith("920"):
        return f"0.{symbol}"
    if symbol.startswith(("5", "6", "7", "9")):
        return f"1.{symbol}"
    return f"0.{symbol}"


def symbol_to_tencent(symbol: str) -> str:
    """转腾讯代码。6/5开头=沪市(sh)，其余=深市(sz)。"""
    symbol = symbol.strip()
    if symbol.startswith(("6", "5")):
        return f"sh{symbol}"
    return f"sz{symbol}"


# ---- K线 ----
@dataclass
class Kline:
    date: str
    open: float
    close: float
    high: float
    low: float
    volume: float  # 成交量(手)
    amount: float = 0.0  # 成交额(元)
    pct: float = 0.0  # 涨跌幅(%)
    turnover: float = 0.0  # 换手率(%)


def fetch_kline(symbol: str, count: int = 250, period: str = "day", adjust: str = "qfq") -> List[Kline]:
    """获取K线数据。多源fallback：腾讯前复权→新浪(不复权)→东财push2test。东财补充成交额/换手率。"""
    cache_key = f"kline_{symbol}_{count}_{period}_{adjust}"
    cached = _cached(cache_key)
    if cached:
        return cached

    klines: List[Kline] = []

    # 1. 优先腾讯API（前复权，价格准确）
    klines = _fetch_kline_tencent(symbol, count, period, adjust)
    if klines:
        log.debug(f"腾讯K线成功 {symbol}: {len(klines)}条")
    else:
        # 2. 腾讯失败(可能WAF拦截) → 东财API（前复权，与腾讯基准一致）
        #    注意：不能先落新浪——新浪不复权，与腾讯前复权基准不同，缓存过期后
        #    换源会导致整段K线价格基准切换，前端表现为图表跳变/假大阳线闪烁。
        klines = _fetch_kline_eastmoney(symbol, count, period)
        if klines:
            log.debug(f"东财K线成功 {symbol}: {len(klines)}条")
        else:
            # 3. 最后fallback新浪API（不复权但价格准确）
            klines = _fetch_kline_sina(symbol, count, period)
            if klines:
                log.debug(f"新浪K线成功 {symbol}: {len(klines)}条")

    if not klines or len(klines) < 10:
        log.error(f"所有K线源均失败 {symbol}, 获取{len(klines)}条")
        return klines if klines else []

    # 数据校验：过滤异常价格（价格<0或>10000的可能是脏数据）
    valid_klines = []
    for k in klines:
        if k.close > 0 and k.high > 0 and k.low > 0 and k.open > 0:
            if k.high >= k.low and k.high >= k.close and k.high >= k.open:
                if k.low <= k.close and k.low <= k.open:
                    if k.close < 10000:  # 合理价格上限
                        valid_klines.append(k)
                        continue
        log.warning(f"异常K线数据被过滤 {symbol} {k.date}: O={k.open} H={k.high} L={k.low} C={k.close}")
    if len(valid_klines) < len(klines):
        log.warning(f"过滤{len(klines)-len(valid_klines)}条异常K线 {symbol}")
    klines = valid_klines

    # 补充成交额和换手率（东财API，按日期匹配）
    _enrich_from_eastmoney(symbol, count, klines)

    _set_cache(cache_key, klines)
    return klines


def _fetch_kline_tencent(symbol: str, count: int, period: str, adjust: str) -> List[Kline]:
    """腾讯API前复权K线。价格准确，但无成交额。可能被WAF拦截。"""
    tc_symbol = symbol_to_tencent(symbol)
    fq = adjust if adjust else ""
    params = {"param": f"{tc_symbol},{period},,,{count},{fq}"}

    try:
        s = _get_session()
        r = s.get(TENCENT_KLINE, params=params, timeout=10)
        # WAF拦截检测：先尝试JSON解析，只有解析失败或返回HTML验证页才判定拦截
        # 腾讯API返回Content-Type=text/html但内容可能是有效JSON，不能仅靠Content-Type判断
        ct = r.headers.get("Content-Type", "")
        text = r.text.strip()
        # 1. 如果文本以<!DOCTYPE开头，必定是HTML验证页
        if text.startswith("<!DOCTYPE") or text.startswith("<html"):
            log.warning(f"腾讯K线被WAF拦截(HTML页面) {symbol}")
            return []
        # 2. 尝试JSON解析——即使Content-Type是text/html，数据也可能是有效JSON
        try:
            data = r.json()
        except (json.JSONDecodeError, ValueError):
            # JSON解析失败，检查是否是HTML验证页
            if "text/html" in ct or "<" in text[:50]:
                log.warning(f"腾讯K线被WAF拦截(JSON解析失败) {symbol}")
            else:
                log.error(f"腾讯K线JSON解析失败 {symbol}: {text[:100]}")
            return []

        # 3. JSON解析成功但code!=0，可能是API错误
        if data.get("code") != 0:
            log.warning(f"腾讯K线API返回code={data.get('code')} {symbol}")
            return []

        stock_data = data.get("data", {}).get(tc_symbol, {})
        key = f"{fq}day" if period == "day" else f"{fq}week" if period == "week" else f"{fq}month"
        rows = stock_data.get(key, stock_data.get("day", stock_data.get("week", [])))

        klines: List[Kline] = []
        for row in rows:
            if len(row) >= 6:
                vol_raw = row[5]
                vol = float(vol_raw) if isinstance(vol_raw, (str, int, float)) else 0.0
                # 腾讯K线volume：A股单位=手，ETF单位=份。统一存原始值
                # A股: vol=65181(手), 前端fmtVol→"6.5万手"
                # ETF: vol=97721858(份), 前端fmtVol→"9772万份"
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
        log.error(f"腾讯K线失败 {symbol}: {e}")
        return []


SINA_KLINE = "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/CN_MarketData.getKLineData"

def _sina_symbol(symbol: str) -> str:
    """转新浪代码前缀。6/5=sh, 920=bj, 其余=sz。"""
    symbol = symbol.strip()
    if symbol.startswith(("6", "5")):
        return f"sh{symbol}"
    if symbol.startswith("920"):
        return f"bj{symbol}"
    return f"sz{symbol}"


def _fetch_kline_sina(symbol: str, count: int, period: str) -> List[Kline]:
    """新浪API K线。不复权但价格准确，有成交量(股)无成交额。"""
    sina_sym = _sina_symbol(symbol)
    scale_map = {"day": "240", "week": "1200", "month": "7200"}
    scale = scale_map.get(period, "240")
    params = {"symbol": sina_sym, "scale": scale, "ma": "no", "datalen": str(count)}

    try:
        s = _get_session()
        r = s.get(SINA_KLINE, params=params, timeout=10)
        if r.status_code != 200:
            return []
        items = r.json()
        if not isinstance(items, list) or not items:
            return []

        klines: List[Kline] = []
        for item in items:
            d = item.get("day", "")
            o = _to_float(item.get("open"))
            c = _to_float(item.get("close"))
            h = _to_float(item.get("high"))
            lo = _to_float(item.get("low"))
            v = _to_float(item.get("volume"))
            if None in (o, c, h, lo) or not d:
                continue
            # 新浪volume：A股单位=股，ETF单位=份
            # A股: 7714770(股) → ÷100 = 77147.7(手)，和腾讯单位一致
            # ETF: 9965596311(份) → 直接存(份)，和腾讯单位一致
            if _is_etf(symbol):
                vol = v if v else 0.0  # ETF份直接存
            else:
                vol = v / 100.0 if v else 0.0  # A股股→手
            k = Kline(date=d, open=o, close=c, high=h, low=lo, volume=vol)
            if klines:
                k.pct = round((k.close - klines[-1].close) / klines[-1].close * 100, 2)
            klines.append(k)
        # 新浪返回的数据已是正序(旧→新)，无需反转
        return klines
    except Exception as e:
        log.error(f"新浪K线失败 {symbol}: {e}")
        return []


EM_KLINE_HOSTS = [
    "https://push2his.eastmoney.com",
    "https://82.push2his.eastmoney.com",
    "https://push2delay.eastmoney.com",
    "https://push2test.eastmoney.com",
]

def _fetch_kline_eastmoney(symbol: str, count: int, period: str) -> List[Kline]:
    """东财K线(push2test优先)。有成交额/换手率，但价格可能不准(仅做最后fallback)。"""
    secid = symbol_to_secid(symbol)
    klt = "101" if period == "day" else "102" if period == "week" else "103"
    params = {
        "secid": secid,
        "ut": "fa5fd1943c7b386f172d6893dbfba10b",
        "fields1": "f1,f2,f3,f4,f5,f6",
        "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
        "klt": klt,
        "fqt": "1",
        "lmt": str(count),
        "end": "20500101",
    }

    data = _get_json_eastmoney("/api/qt/stock/kline/get", params, EM_KLINE_HOSTS)
    if not data or not data.get("data") or not data["data"].get("klines"):
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
                volume=float(parts[5]),
                amount=_to_float(parts[6]) or 0.0,
            )
            if len(parts) >= 11:
                k.turnover = _to_float(parts[10]) or 0.0
            if klines:
                k.pct = round((k.close - klines[-1].close) / klines[-1].close * 100, 2)
            klines.append(k)
    return klines


def _enrich_from_eastmoney(symbol: str, count: int, klines: List[Kline]) -> None:
    """从东财K线API补充成交额和换手率。按日期匹配。请求比K线多50%以确保日期覆盖。"""
    secid = symbol_to_secid(symbol)
    # 多请求一些数据以确保日期覆盖（东财可能缺少部分历史数据）
    request_count = min(count + 60, 500)
    params = {
        "secid": secid,
        "ut": "fa5fd1943c7b386f172d6893dbfba10b",
        "fields1": "f1,f2,f3,f4,f5,f6",
        "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
        "klt": "101",
        "fqt": "0",  # 不复权（只取成交额和换手率，价格不用）
        "lmt": str(request_count),
        "end": "20500101",
    }

    data = _get_json_eastmoney("/api/qt/stock/kline/get", params, EM_KLINE_HOSTS)
    if not data or not data.get("data") or not data["data"].get("klines"):
        return

    # 构建日期→(amount, turnover)映射
    em_map: Dict[str, Tuple[float, float]] = {}
    for line in data["data"]["klines"]:
        parts = line.split(",")
        if len(parts) >= 11:
            date = parts[0]
            amount = _to_float(parts[6]) or 0.0
            turnover = _to_float(parts[10]) or 0.0
            em_map[date] = (amount, turnover)

    # 按日期匹配补充
    matched = 0
    for k in klines:
        if k.date in em_map:
            k.amount = em_map[k.date][0]
            k.turnover = em_map[k.date][1]
            matched += 1
    log.debug(f"东财补充成交额: {matched}/{len(klines)} 条匹配")


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

# 实时数据缓存，2秒——保证秒级刷新拿到最新数据
_RT_CACHE_TTL = 2


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


# ---- 搜索 ----
def search_stock(keyword: str, count: int = 10) -> List[Dict]:
    """搜索股票。东财suggest接口。"""
    if not keyword or len(keyword.strip()) < 1:
        return []

    keyword = keyword.strip()
    cache_key = f"search_{keyword}"
    cached = _cached(cache_key)
    if cached is not None:
        return cached

    # 纯数字6位代码直接返回
    if keyword.isdigit() and len(keyword) == 6:
        if keyword.startswith("6") or keyword.startswith("5"):
            market = "SH"
        elif keyword.startswith("920"):
            market = "BJ"
        else:
            market = "SZ"
        result = [{"code": keyword, "name": "", "market": market}]
        _set_cache(cache_key, result)
        return result

    params = {"input": keyword, "type": "14", "count": str(count), "ut": "fa5fd1943c7b386f172d6893dbfba10b"}
    try:
        # 用urllib而非requests，避免某些环境下API返回JSONP格式
        import urllib.request, urllib.parse
        query = urllib.parse.urlencode(params)
        url = f"{SEARCH_HOST}/api/suggest/get?{query}"
        req = urllib.request.Request(url, headers={
            "User-Agent": UA_POOL[0],
            "Referer": "https://quote.eastmoney.com/",
        })
        with urllib.request.urlopen(req, timeout=8) as resp:
            body = json.loads(resp.read().decode("utf-8"))
    except Exception as e:
        log.debug(f"搜索失败: {e}")
        return []

    items = body.get("QuotationCodeTable", {}).get("Data") or []
    results = []
    for item in items:
        code = item.get("Code", "")
        name = item.get("Name", "")
        classify = item.get("Classify", "")
        # A股 + ETF + 北交所
        is_valid = classify in ("AStock", "Fund") or (len(code) == 6 and code[0] in "036") or code.startswith("920") or (len(code) == 6 and code.startswith("5"))
        if is_valid and len(code) == 6:
            if code.startswith("6") or code.startswith("5"):
                mkt = "SH"
            elif code.startswith("920"):
                mkt = "BJ"
            else:
                mkt = "SZ"
            results.append({"code": code, "name": name, "market": mkt})
    _set_cache(cache_key, results[:count])
    return results[:count]


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


# ---- 市场宽度（涨跌家数）----
def fetch_market_breadth() -> Optional[dict]:
    """获取A股全市场宽度（涨跌家数）。

    全量抓取所有A股（约5800+只），10线程并发，约0.3秒完成。
    push2/push2delay每页最多100条，需要约59页。
    120秒缓存（涨跌家数分钟级变化不大）。
    """
    cache_key = "market_breadth"
    cached = _cache_get(cache_key, 120)
    if cached:
        return cached

    import concurrent.futures

    CLIST_PATH = "/api/qt/clist/get"
    base_params = {
        "po": "1",
        "np": "1",
        "fltt": "2",
        "fields": "f3",
        "fs": "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048",
        "pz": "100",
    }

    def _fetch_page(pn: int) -> list:
        params = dict(base_params, pn=str(pn))
        s = _get_session()
        for host in QUOTE_HOSTS:
            try:
                url = host + CLIST_PATH
                r = s.get(url, params=params, timeout=8)
                if r.status_code == 200:
                    data = r.json()
                    if data and data.get("data"):
                        return data["data"].get("diff") or []
            except Exception:
                continue
        return []

    # 先取第一页，拿到总数算页数
    first_page = _fetch_page(1)
    if not first_page:
        log.warning("市场宽度获取失败: 第一页为空")
        return None

    # 从第一页响应中拿total
    total_stocks = 0
    for host in QUOTE_HOSTS:
        try:
            s = _get_session()
            r = s.get(host + CLIST_PATH, params=dict(base_params, pn="1"), timeout=8)
            if r.status_code == 200:
                total_stocks = r.json().get("data", {}).get("total", 0)
                if total_stocks:
                    break
        except Exception:
            continue

    total_pages = (total_stocks + 99) // 100 if total_stocks else 59
    log.info(f"市场宽度: {total_stocks}只A股, {total_pages}页, 开始全量抓取")

    # 全量并发抓取（第1页已取，第2页~末页并发）
    up = down = flat = 0

    # 统计第一页
    for d in first_page:
        pct = _to_float(d.get("f3"))
        if pct is None or pct == 0:
            flat += 1
        elif pct > 0:
            up += 1
        else:
            down += 1

    if total_pages > 1:
        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
            futures = [executor.submit(_fetch_page, pn) for pn in range(2, total_pages + 1)]
            for f in concurrent.futures.as_completed(futures):
                diff = f.result()
                for d in diff:
                    pct = _to_float(d.get("f3"))
                    if pct is None or pct == 0:
                        flat += 1
                    elif pct > 0:
                        up += 1
                    else:
                        down += 1

    total = up + down + flat
    if total < 100:
        log.warning(f"市场宽度数据不足: {total}只")
        return None

    breadth_ratio = up / max(up + down, 1) if (up + down) > 0 else 0.5
    result = {
        "up": up,
        "down": down,
        "flat": flat,
        "total": total,
        "breadth_ratio": round(breadth_ratio, 3),
    }
    log.info(f"市场宽度(全量{total}只): {up}涨/{down}跌/{flat}平, ratio={breadth_ratio:.1%}")
    _cache_set(cache_key, result)
    return result


def fetch_all_a_shares() -> List[Dict]:
    """获取全A股列表（代码+名称+价格+涨跌幅+成交额）。

    用于扫描功能预过滤。全量抓取~5800只，10线程并发，约0.5秒完成。
    60秒缓存。
    """
    cache_key = "all_a_shares"
    cached = _cache_get(cache_key, 60)
    if cached:
        return cached

    import concurrent.futures

    CLIST_PATH = "/api/qt/clist/get"
    base_params = {
        "po": "1",
        "np": "1",
        "fltt": "2",
        "fields": "f2,f3,f6,f12,f14",
        "fs": "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048",
        "pz": "100",
    }

    def _fetch_page(pn: int) -> tuple:
        """返回 (page_data, total)"""
        params = dict(base_params, pn=str(pn))
        s = _get_session()
        for host in QUOTE_HOSTS:
            try:
                url = host + CLIST_PATH
                r = s.get(url, params=params, timeout=8)
                if r.status_code == 200:
                    data = r.json()
                    if data and data.get("data"):
                        diff = data["data"].get("diff") or []
                        total = data["data"].get("total", 0)
                        return diff, total
            except Exception:
                continue
        return [], 0

    # 第一页拿总数
    first_diff, total_stocks = _fetch_page(1)
    if not first_diff:
        log.warning("获取A股列表失败: 第一页为空")
        return []

    total_pages = (total_stocks + 99) // 100 if total_stocks else 59
    log.info(f"A股列表: {total_stocks}只, {total_pages}页, 开始全量抓取")

    all_stocks = []

    def _parse_diff(diff: list) -> list:
        result = []
        for d in diff:
            code = str(d.get("f12", "")).strip()
            name = str(d.get("f14", "")).strip()
            price = _to_float(d.get("f2")) or 0
            pct = _to_float(d.get("f3")) or 0
            amount = _to_float(d.get("f6")) or 0
            if code and len(code) == 6:
                result.append({
                    "code": code,
                    "name": name,
                    "price": price,
                    "pct": pct,
                    "amount": amount,
                })
        return result

    all_stocks.extend(_parse_diff(first_diff))

    if total_pages > 1:
        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
            futures = [executor.submit(_fetch_page, pn) for pn in range(2, total_pages + 1)]
            for f in concurrent.futures.as_completed(futures):
                diff, _ = f.result()
                all_stocks.extend(_parse_diff(diff))

    log.info(f"A股列表获取完成: {len(all_stocks)}只")
    _cache_set(cache_key, all_stocks)
    return all_stocks
