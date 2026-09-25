"""搜索：东财suggest接口。"""
from __future__ import annotations

import json
from typing import Dict, List

from .session import SEARCH_HOST, UA_POOL, log, _cached, _set_cache


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
