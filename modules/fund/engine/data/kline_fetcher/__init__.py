"""数据层：K线(腾讯前复权+东财补充成交额) + 实时行情(东财) + 资金流(东财) + 搜索(东财)。

参考《A股资金流向监控工具-个股增强版》的host池轮换、fltt=2、session复用设计。

数据源策略：
- K线价格/成交量：腾讯API前复权（价格准确，和行情一致）
- K线成交额/换手率：东财K线API（有值，按日期匹配补充）
- 实时行情：东财stock/get（fltt=2，价格不除100）
- 资金流：东财fflow/daykline（push2his优先，push2test备选）

包拆分：
- session.py：DNS重定向/host池/UA池/session复用/缓存/东财请求/ETF判断
- symbols.py：代码转换（东财secid/腾讯/新浪）
- kline.py：K线（多源fallback + 东财补充成交额/换手率）
- quote.py：实时行情
- flow.py：资金流（历史 + 盘中实时分时）
- search.py：搜索
- minute.py：分时数据
- index.py：大盘指数K线
- market.py：市场宽度 + 全A股列表
"""
from __future__ import annotations

# 先加载session（含requests fallback与libs路径处理），再加载其余子模块
from .session import (
    _PUSH2DELAY_IP, _original_getaddrinfo, _dns_redirected,
    _patch_dns, QUOTE_HOSTS, HIS_HOSTS, SEARCH_HOST, TENCENT_KLINE, UA_POOL,
    _session, _ua_idx, _cache, _CACHE_TTL, _RT_CACHE_TTL,
    _get_session, _rotate_ua, _cached, _set_cache, _cache_get, _cache_set,
    _to_float, _get_json_eastmoney, _is_etf,
)
from .symbols import symbol_to_secid, symbol_to_tencent, _sina_symbol
from .kline import (
    SINA_KLINE, EM_KLINE_HOSTS,
    Kline, fetch_kline, _fetch_kline_tencent, _fetch_kline_sina,
    _fetch_kline_eastmoney, _enrich_from_eastmoney,
)
from .quote import Quote, fetch_quote
from .flow import (
    FundFlow, fetch_fund_flow,
    RT_FLOW_HOSTS, MinuteFlow, fetch_realtime_flow,
)
from .search import search_stock
from .minute import MinuteData, fetch_minute
from .index import fetch_index_kline, _fetch_kline_tencent_index
from .market import fetch_market_breadth, fetch_all_a_shares

__all__ = [
    "_PUSH2DELAY_IP", "_original_getaddrinfo", "_dns_redirected",
    "_patch_dns", "QUOTE_HOSTS", "HIS_HOSTS", "SEARCH_HOST", "TENCENT_KLINE",
    "UA_POOL", "_session", "_ua_idx", "_cache", "_CACHE_TTL",
    "_get_session", "_rotate_ua", "_cached", "_set_cache", "_cache_get",
    "_cache_set", "_to_float", "_get_json_eastmoney", "_is_etf",
    "symbol_to_secid", "symbol_to_tencent", "_sina_symbol",
    "SINA_KLINE", "EM_KLINE_HOSTS",
    "Kline", "fetch_kline", "_fetch_kline_tencent", "_fetch_kline_sina",
    "_fetch_kline_eastmoney", "_enrich_from_eastmoney",
    "_RT_CACHE_TTL", "Quote", "fetch_quote",
    "FundFlow", "fetch_fund_flow",
    "RT_FLOW_HOSTS", "MinuteFlow", "fetch_realtime_flow",
    "search_stock",
    "MinuteData", "fetch_minute",
    "fetch_index_kline", "_fetch_kline_tencent_index",
    "fetch_market_breadth", "fetch_all_a_shares",
]
