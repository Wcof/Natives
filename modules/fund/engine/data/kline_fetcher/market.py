"""全市场数据：市场宽度（涨跌家数）+ 全A股列表。"""
from __future__ import annotations

from typing import Dict, List, Optional

from .session import QUOTE_HOSTS, log, _get_session, _cache_get, _cache_set, _to_float


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
