"""K线：腾讯前复权+东财补充成交额，多源fallback（腾讯→东财→新浪）。"""
from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Dict, List, Tuple

from .session import (
    TENCENT_KLINE,
    log,
    _get_session,
    _cached,
    _set_cache,
    _to_float,
    _get_json_eastmoney,
    _is_etf,
)
from .symbols import symbol_to_secid, symbol_to_tencent, _sina_symbol

SINA_KLINE = "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/CN_MarketData.getKLineData"

EM_KLINE_HOSTS = [
    "https://push2his.eastmoney.com",
    "https://82.push2his.eastmoney.com",
    "https://push2delay.eastmoney.com",
    "https://push2test.eastmoney.com",
]


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
