"""代码转换：东财secid / 腾讯 / 新浪代码前缀。"""
from __future__ import annotations

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


def _sina_symbol(symbol: str) -> str:
    """转新浪代码前缀。6/5=sh, 920=bj, 其余=sz。"""
    symbol = symbol.strip()
    if symbol.startswith(("6", "5")):
        return f"sh{symbol}"
    if symbol.startswith("920"):
        return f"bj{symbol}"
    return f"sz{symbol}"
