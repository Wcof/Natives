(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：自选分组与自选行情表。职责一句话——维护自选分组页签、
// 自选表（表头排序/涨速/闪烁/sparkline）与添加自选流程。
// 更新策略：行复用 + 属性级更新——轮询只改单元格 textContent/class，
// 不重建 DOM，页面零闪烁；仅成员增删时创建/移除对应行。
// import 方向：core（状态/api/主题取色）；跨域经 actions（app.selectSymbol 等）。

// ---------- 自选分组：顶部页签切换 ----------
const wTabs = document.querySelectorAll("#watchlist-groups .group-tab");
wTabs.forEach(function(tab) {
  tab.addEventListener("click", function() {
    wTabs.forEach(function(t) { t.classList.remove("active"); });
    tab.classList.add("active");
    state.activeGroup = tab.dataset.id;
    refreshWatchlist();
  });
});

function refreshWatchlist() {
  api("GET", "/api/watchlists").then(function(res) {
    const allItems = res.items || [];
    // 空列表就是空：不填充演示标的，不引导强制输入（用户明确要求）。
    state.watchlistItems = allItems.filter(function(it) { return it.watchlistId === state.activeGroup; });
    if (!state.currentSymbol && state.watchlistItems.length > 0) {
      actions.selectSymbol(state.watchlistItems[0].symbol, state.watchlistItems[0].name);
    }
    actions.syncWSSubscriptions();
    renderWatchlistTable();
    actions.updateFavButtonState();
    actions.updateKPI();
  }).catch(function(err) {
    // WS 永远离线（runtime 无 /ws 端点）时这里是唯一数据入口，失败必须可见并可重试。
    // 清空行情指纹：恢复后即使行情无变化也要重渲染，避免卡在错误态。
    _lastQuoteSig = null;
    resetRows();
    const tbody = document.querySelector("#watchlist-table tbody");
    if (tbody) {
      tbody.innerHTML = "<tr><td colspan='13' class='err'>自选加载失败：" + (err && err.message ? err.message : "未知错误") +
        "（<a href='#' id='retry-watchlist'>重试</a>）</td></tr>";
      const retry = document.getElementById("retry-watchlist");
      if (retry) retry.addEventListener("click", function(e) { e.preventDefault(); refreshWatchlist(); });
    }
  });
}
actions.refreshWatchlist = refreshWatchlist;
actions.renderWatchlistTable = renderWatchlistTable;

// ---------- 表头排序（同花顺风格：首次点击降序，再点切换升序；字符串列升序优先） ----------
let sortState = { key: null, asc: false };
// 最近一次行情快照缓存：排序点击只重排内存数据，不重复请求（R-P4 页面级去重）
let lastQuoteRows = null;

document.querySelectorAll("#watchlist-table th.sortable").forEach(function(th) {
  th.addEventListener("click", function() {
    const key = th.dataset.key;
    if (sortState.key === key) { sortState.asc = !sortState.asc; }
    else { sortState = { key: key, asc: ["symbol", "name"].indexOf(key) >= 0 }; }
    document.querySelectorAll("#watchlist-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    th.classList.add(sortState.asc ? "sort-asc" : "sort-desc");
    // 只重排内存快照，不重新请求行情（R-P4 页面级去重）
    if (lastQuoteRows) renderSortedRows(lastQuoteRows);
    else renderWatchlistTable();
  });
});

function sortKeyOf(item) {
  const q = item._q || {};
  switch (sortState.key) {
    case "symbol": return item.symbol;
    case "name": return item.name;
    case "price": return q.price;
    case "changePct": return q.changePct;
    case "change": return q.change;
    case "speed": return item._speed;
    case "volume": return q.volume;
    case "turnover": return q.turnover;
    case "amplitude": return q.amplitude;
    case "volumeRatio": return q.volumeRatio;
    default: return null;
  }
}

function sortRows(rows) {
  const sorted = rows.slice();
  if (sortState.key) {
    const dir = sortState.asc ? 1 : -1;
    sorted.sort(function(a, b) {
      const ka = sortKeyOf(a), kb = sortKeyOf(b);
      if (ka == null && kb == null) return 0;
      if (ka == null) return 1;
      if (kb == null) return -1;
      return ka < kb ? -dir : ka > kb ? dir : 0;
    });
  }
  return sorted;
}

// ---------- 行复用索引：symbol → <tr>（属性级更新的核心状态，唯一归属本模块） ----------
const rowIndex = new Map();

function resetRows() {
  rowIndex.forEach(function(tr) { tr.remove(); });
  rowIndex.clear();
}

// ---------- 行情取数：拉报价 → 计算涨速 → 增量落表 ----------
function renderWatchlistTable() {
  const tbody = document.querySelector("#watchlist-table tbody");
  if (!tbody) return;
  const symbols = state.watchlistItems.map(function(i) { return i.symbol; }).join(",");
  if (!symbols) { resetRows(); lastQuoteRows = null; return; }

  api("GET", "/api/market/quotes?codes=" + symbols).then(function(res) {
    const quotes = res.quotes || [];
    const quoteMap = {};
    quotes.forEach(function(q) { quoteMap[q.code] = q; });

    // 行情无变化时跳过更新：轮询期间数据没变就不触碰 DOM（二级守卫，
    // 即使数据变了也只做属性级更新，不会闪烁）。
    // 指纹包含自选成员列表：增删自选/切分组后签名必然变化，不会漏渲染。
    const sig = symbols + "#" + quotes.map(function(q) {
      return [q.code, q.price, q.changePct, q.volume, q.turnover, q.amplitude, q.volumeRatio].join("|");
    }).join(";");
    if (sig === _lastQuoteSig) return;
    _lastQuoteSig = sig;

    // 涨速：与上次快照的涨跌幅差（近似 1 分钟涨速，同花顺口径）
    const rows = state.watchlistItems.map(function(item) {
      const q = quoteMap[item.symbol] || {};
      const prevPct = state.lastQuotesMap[item.symbol + ":pct"];
      if (q.changePct != null) state.lastQuotesMap[item.symbol + ":pct"] = q.changePct;
      item._q = q;
      item._speed = (q.changePct != null && prevPct != null) ? (q.changePct - prevPct) : null;
      return item;
    });
    // 缓存快照供排序点击复用：只重排内存数据，不重复请求（R-P4 页面级去重）
    lastQuoteRows = rows.slice();
    renderSortedRows(rows);
  }).catch(function(err) {
    lastQuoteRows = null;
    // 清空行情指纹：网络恢复后即使行情无变化也要重渲染，避免卡在错误态。
    _lastQuoteSig = null;
    resetRows();
    const tbody = document.querySelector("#watchlist-table tbody");
    if (tbody) tbody.innerHTML = "<tr><td colspan='13' class='err'>行情获取失败：" + (err && err.message ? err.message : "未知错误") + "</td></tr>";
  });
}
// 上次行情指纹：与上次完全一致则跳过 DOM 更新
let _lastQuoteSig = null;

// 按当前 sortState 排序并增量落表（供取数路径与表头点击共用）
function renderSortedRows(rows) {
  const tbody = document.querySelector("#watchlist-table tbody");
  if (!tbody) return;
  const sorted = sortRows(rows);
  const seen = new Set();
  sorted.forEach(function(item) {
    let tr = rowIndex.get(item.symbol);
    if (!tr) { tr = buildRow(item); rowIndex.set(item.symbol, tr); }
    // 先入 DOM 再更新：sparkline 需按 id 找到已挂载的 canvas，
    // 闪烁动画的 reflow 重启也要求节点在文档中
    tbody.appendChild(tr);
    updateRow(tr, item);
    seen.add(item.symbol);
  });
  // 移除已删成员的行与历史错误/加载占位行
  rowIndex.forEach(function(tr, sym) {
    if (!seen.has(sym)) { tr.remove(); rowIndex.delete(sym); }
  });
  Array.from(tbody.children).forEach(function(tr) {
    if (!rowIndex.has(tr.dataset.symbol)) tr.remove();
  });
  actions.updateKPI();
}

// ---------- 行构建（每 symbol 仅一次；点击/删除事件绑定一次） ----------
function buildRow(item) {
  const tr = document.createElement("tr");
  tr.className = "clickable";
  tr.dataset.symbol = item.symbol;
  tr.dataset.name = item.name;

  const tdSym = document.createElement("td");
  const b = document.createElement("b");
  b.textContent = item.symbol;
  tdSym.appendChild(b);
  const tdName = document.createElement("td");
  tdName.textContent = item.name;
  tr.append(tdSym, tdName);
  // 13 列：0代码 1名称 2价 3幅 4额 5速 6spark 7量 8换手 9振幅 10量比 11预警 12删除
  for (let i = 0; i < 11; i++) {
    const td = document.createElement("td");
    if (i === 4) {
      td.className = "spark-cell";
      const canvas = document.createElement("canvas");
      canvas.className = "sparkline";
      canvas.id = "spark-" + item.symbol;
      td.appendChild(canvas);
    } else if (i === 9) {
      // 预警列：颜色固定 accent，更新时只改 textContent
      td.style.color = "var(--natives-accent)";
    } else if (i === 10) {
      const del = document.createElement("button");
      del.className = "btn-del";
      del.title = "移除自选";
      del.dataset.symbol = item.symbol;
      del.textContent = "×";
      td.appendChild(del);
    } else {
      td.className = "num";
    }
    tr.appendChild(td);
  }

  tr.addEventListener("click", function(e) {
    if (e.target.classList.contains("btn-del")) {
      e.stopPropagation();
      api("DELETE", "/api/watchlists/items?watchlistId=" + state.activeGroup + "&symbol=" + tr.dataset.symbol).then(function() {
        refreshWatchlist();
      });
      return;
    }
    actions.selectSymbol(tr.dataset.symbol, tr.dataset.name, true);
  });
  tr.addEventListener("dblclick", function() {
    if (actions.openStockDetail) {
      actions.openStockDetail(tr.dataset.symbol, tr.dataset.name);
    }
  });
  return tr;
}

// ---------- 行更新：只改变化的 textContent / class，不触碰 DOM 结构 ----------
function setNum(td, text, cls) {
  if (td.textContent !== text) td.textContent = text;
  const want = "num" + (cls ? " " + cls : "");
  if (td.className !== want) td.className = want;
}

function updateRow(tr, item) {
  const q = item._q || {};
  const tds = tr.children;
  const cls = q.changePct > 0 ? "up" : q.changePct < 0 ? "down" : "";

  // 闪烁：价格相对上次快照变化时加 flash 类（CSS 动画，800ms 后移除）。
  // 行复用下需先移除类再强制 reflow 才能重启动画。
  const lastPrice = state.lastQuotesMap[item.symbol];
  if (lastPrice != null && q.price != null && q.price !== lastPrice) {
    tr.classList.remove("flash-up", "flash-down");
    void tr.offsetWidth;
    tr.classList.add(q.price > lastPrice ? "flash-up" : "flash-down");
    setTimeout(function() { tr.classList.remove("flash-up", "flash-down"); }, 800);
  }
  if (q.price != null) state.lastQuotesMap[item.symbol] = q.price;
  if (q.changePct != null) item._lastChangePct = q.changePct;

  setNum(tds[2], q.price != null ? q.price.toFixed(2) : "–", cls);
  setNum(tds[3], q.changePct != null ? (q.changePct > 0 ? "+" : "") + q.changePct.toFixed(2) + "%" : "–", cls);
  setNum(tds[4], q.change != null ? (q.change > 0 ? "+" : "") + q.change.toFixed(2) : "–", cls);
  setNum(tds[5], item._speed != null ? (item._speed > 0 ? "+" : "") + item._speed.toFixed(2) : "–",
         item._speed > 0 ? "up" : item._speed < 0 ? "down" : "");
  drawSparkline(item.symbol, (q.changePct || 0) >= 0);
  setNum(tds[7], q.volume || "–", null);
  setNum(tds[8], q.turnover != null ? q.turnover.toFixed(2) : "–", null);
  setNum(tds[9], q.amplitude != null ? q.amplitude.toFixed(2) : "–", null);
  setNum(tds[10], q.volumeRatio != null ? q.volumeRatio.toFixed(2) : "–", null);
  const hasAlert = state.alertRules[item.symbol] ? state.alertRules[item.symbol] + "%" : "–";
  if (tds[11].textContent !== hasAlert) tds[11].textContent = hasAlert;
  tr.classList.toggle("selected", item.symbol === state.currentSymbol);
}

// ---------- Sparkline（60px 原生 Canvas 微缩走势） ----------
// sparkline 数据缓存：同一标的 60s 内复用，避免每行每刷各拉一次 minute 打满上游。
const sparkCache = {};
function drawSparkline(symbol, isUp) {
  const canvas = document.getElementById("spark-" + symbol) || document.getElementById("m-spark-" + symbol);
  if (!canvas) return;
  canvas.width = 60;
  canvas.height = 20;
  const ctx = canvas.getContext("2d");
  const draw = function(pts) {
    if (!pts || pts.length < 2) return;
    const prices = pts.map(function(p) { return p.price; });
    const min = Math.min.apply(null, prices);
    const max = Math.max.apply(null, prices);
    const range = (max - min) || 1;

    ctx.clearRect(0, 0, 60, 20);
    ctx.beginPath();
    ctx.strokeStyle = isUp ? themeColor("--color-up") : themeColor("--color-down");
    ctx.lineWidth = 1.2;
    for (let i = 0; i < pts.length; i++) {
      const x = (i / (pts.length - 1)) * 58 + 1;
      const y = 19 - ((prices[i] - min) / range) * 16;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  };
  const cached = sparkCache[symbol];
  if (cached && Date.now() - cached.ts < 60000) { draw(cached.pts); return; }
  api("GET", "/api/market/minute?symbol=" + symbol).then(function(res) {
    const pts = res.points || [];
    sparkCache[symbol] = { ts: Date.now(), pts: pts };
    draw(pts);
  }).catch(function() {});
}

// ---------- 添加自选：经 /api/market/suggest 搜索确认（代码/名称/拼音均可） ----------
// 输入代码或名称后先查联想接口拿真实 symbol+name，再入自选；多命中时取第一个。
// 失败给出明确提示（此前静默失败导致"无法搜索"的表象）。
function addSymbolFromInput() {
  const input = document.getElementById("add-symbol-input");
  const msg = document.getElementById("add-symbol-msg");
  const code = input.value.trim();
  const notify = function(text, isError) {
    if (!msg) return;
    msg.textContent = text || "";
    msg.style.color = isError ? "var(--natives-danger, #e07b7b)" : "var(--natives-text-muted)";
  };
  if (!code) return;
  notify("搜索中…", false);
  api("GET", "/api/market/suggest?input=" + encodeURIComponent(code)).then(function(res) {
    const hits = res.suggestions || [];
    if (hits.length === 0) {
      notify("未找到匹配的标的，请确认代码或名称。", true);
      return;
    }
    const hit = hits[0];
    return api("POST", "/api/watchlists/items", {
      watchlistId: state.activeGroup,
      symbol: hit.symbol,
      name: hit.name
    }).then(function() {
      input.value = "";
      notify("已添加 " + hit.name + "（" + hit.symbol + "）", false);
      refreshWatchlist();
      actions.selectSymbol(hit.symbol, hit.name);
    });
  }).catch(function(err) {
    notify("搜索失败：" + (err && err.message ? err.message : "未知错误"), true);
  });
}
document.getElementById("add-symbol-btn").addEventListener("click", addSymbolFromInput);
document.getElementById("add-symbol-input").addEventListener("keydown", function(e) {
  if (e.key === "Enter") { e.preventDefault(); addSymbolFromInput(); }
});

})();
