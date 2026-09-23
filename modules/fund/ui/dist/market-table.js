(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：自选分组与行情表。职责一句话——维护自选分组、自选表
// （表头排序/涨速/闪烁/sparkline）与全景行情表的取数渲染。
// import 方向：core（状态/api/主题取色）；跨域经 actions（app.selectSymbol 等）。

// ---------- 自选分组：顶部页签 + 左侧行情树，双向联动 ----------
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

function renderWatchlistTable() {
  const tbody = document.querySelector("#watchlist-table tbody");
  tbody.innerHTML = "";
  const symbols = state.watchlistItems.map(function(i) { return i.symbol; }).join(",");
  if (!symbols) return;

  api("GET", "/api/market/quotes?codes=" + symbols).then(function(res) {
    const quotes = res.quotes || [];
    const quoteMap = {};
    quotes.forEach(function(q) { quoteMap[q.code] = q; });

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
    const tbody = document.querySelector("#watchlist-table tbody");
    if (tbody) tbody.innerHTML = "<tr><td colspan='13' class='err'>行情获取失败：" + (err && err.message ? err.message : "未知错误") + "</td></tr>";
  });
}

// 按当前 sortState 排序并渲染行（供取数路径与表头点击共用）
function renderSortedRows(rows) {
  const tbody = document.querySelector("#watchlist-table tbody");
  tbody.innerHTML = "";
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

  sorted.forEach(function(item) {
    const q = item._q || {};
    const tr = document.createElement("tr");
    tr.className = "clickable" + (item.symbol === state.currentSymbol ? " selected" : "");
    tr.dataset.symbol = item.symbol;
    tr.dataset.name = item.name;

    const lastPrice = state.lastQuotesMap[item.symbol];
    if (lastPrice != null && q.price != null) {
      if (q.price > lastPrice) tr.classList.add("flash-up");
      else if (q.price < lastPrice) tr.classList.add("flash-down");
      setTimeout(function() { tr.classList.remove("flash-up", "flash-down"); }, 800);
    }
    if (q.price != null) state.lastQuotesMap[item.symbol] = q.price;
    if (q.changePct != null) item._lastChangePct = q.changePct;

    const price = q.price != null ? q.price.toFixed(2) : "–";
    const changePct = q.changePct != null ? (q.changePct > 0 ? "+" : "") + q.changePct.toFixed(2) + "%" : "–";
    const change = q.change != null ? (q.change > 0 ? "+" : "") + q.change.toFixed(2) : "–";
    const cls = q.changePct > 0 ? "up" : q.changePct < 0 ? "down" : "";
    const hasAlert = state.alertRules[item.symbol] ? state.alertRules[item.symbol] + "%" : "–";

    tr.innerHTML = "<td><b>" + item.symbol + "</b></td>" +
                   "<td>" + item.name + "</td>" +
                   "<td class='num " + cls + "'>" + price + "</td>" +
                   "<td class='num " + cls + "'>" + changePct + "</td>" +
                   "<td class='num " + cls + "'>" + change + "</td>" +
                   "<td class='num " + (item._speed > 0 ? "up" : item._speed < 0 ? "down" : "") + "'>" + (item._speed != null ? (item._speed > 0 ? "+" : "") + item._speed.toFixed(2) : "–") + "</td>" +
                   "<td class='spark-cell'><canvas class='sparkline' id='spark-" + item.symbol + "'></canvas></td>" +
                   "<td class='num'>" + (q.volume || "–") + "</td>" +
                   "<td class='num'>" + (q.turnover != null ? q.turnover.toFixed(2) : "–") + "</td>" +
                   "<td class='num'>" + (q.amplitude != null ? q.amplitude.toFixed(2) : "–") + "</td>" +
                   "<td class='num'>" + (q.volumeRatio != null ? q.volumeRatio.toFixed(2) : "–") + "</td>" +
                   "<td><span style='color:var(--natives-accent)'>" + hasAlert + "</span></td>" +
                   "<td><button class='btn-del' title='移除自选' data-symbol='" + item.symbol + "'>×</button></td>";

    tr.addEventListener("click", function(e) {
      if (e.target.classList.contains("btn-del")) {
        e.stopPropagation();
        api("DELETE", "/api/watchlists/items?watchlistId=" + state.activeGroup + "&symbol=" + item.symbol).then(function() {
          refreshWatchlist();
        });
        return;
      }
      actions.selectSymbol(item.symbol, item.name);
    });

    tbody.appendChild(tr);
    drawSparkline(item.symbol, q.changePct >= 0);
  });
  actions.updateKPI();
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
// 失败给出明确提示（此前静默失败导致“无法搜索”的表象）。
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

// ---------- 热门板块与成分股联动模块（去除分钟K线，上下双表联动） ----------
let currentSectorType = "all";
let currentSelectedSector = null;
let lastSectorsData = [];
let lastStocksData = [];
let sectorSort = { key: "changePct", asc: false };
let stockSort = { key: "changePct", asc: false };

const sTabs = document.querySelectorAll("#sector-category-tabs .group-tab");
sTabs.forEach(function(tab) {
  tab.addEventListener("click", function() {
    sTabs.forEach(function(t) { t.classList.remove("active"); });
    tab.classList.add("active");
    currentSectorType = tab.dataset.type;
    refreshSectors();
  });
});

function refreshSectors() {
  const tbody = document.querySelector("#sectors-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "<tr><td colspan='9' style='text-align:center; padding: 20px;'>加载热门板块数据中…</td></tr>";

  api("GET", "/api/market/sectors?type=" + currentSectorType).then(function(res) {
    lastSectorsData = res.sectors || res.quotes || [];
    renderSectorsTable();
  }).catch(function(err) {
    tbody.innerHTML = "<tr><td colspan='9' class='err' style='text-align:center; padding: 20px;'>获取板块行情失败：" + err.message + "</td></tr>";
  });
}
actions.refreshSectors = refreshSectors;
actions.refreshMarket = refreshSectors;

function renderSectorsTable() {
  const tbody = document.querySelector("#sectors-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "";

  const items = lastSectorsData.slice();
  if (sectorSort.key) {
    items.sort(function(a, b) {
      const va = a[sectorSort.key] != null ? a[sectorSort.key] : -999999;
      const vb = b[sectorSort.key] != null ? b[sectorSort.key] : -999999;
      return sectorSort.asc ? (va > vb ? 1 : -1) : (va < vb ? 1 : -1);
    });
  }

  if (items.length === 0) {
    tbody.innerHTML = "<tr><td colspan='9' style='text-align:center; padding: 20px;'>暂无板块数据</td></tr>";
    return;
  }

  items.forEach(function(s, idx) {
    const tr = document.createElement("tr");
    tr.className = "clickable";
    if (currentSelectedSector && currentSelectedSector.code === s.code) {
      tr.classList.add("selected-sector");
    }

    const pct = s.changePct != null ? (s.changePct > 0 ? "+" : "") + s.changePct.toFixed(2) + "%" : "–";
    const pctCls = s.changePct > 0 ? "up" : s.changePct < 0 ? "down" : "";
    const speed = s.speed != null ? s.speed.toFixed(2) : "–";
    const speedCls = s.speed > 0 ? "up" : s.speed < 0 ? "down" : "";
    const topName = s.topStockName || "–";
    const topPrice = s.topStockPrice != null ? s.topStockPrice.toFixed(2) : "–";
    const topPct = s.topStockChangePct != null ? (s.topStockChangePct > 0 ? "+" : "") + s.topStockChangePct.toFixed(2) + "%" : "–";
    const topCls = s.topStockChangePct > 0 ? "up" : s.topStockChangePct < 0 ? "down" : "";
    const pct5 = s.changePct5 != null ? (s.changePct5 > 0 ? "+" : "") + s.changePct5.toFixed(2) + "%" : "–";
    const pct5Cls = s.changePct5 > 0 ? "up" : s.changePct5 < 0 ? "down" : "";
    const pct20 = s.changePct20 != null ? (s.changePct20 > 0 ? "+" : "") + s.changePct20.toFixed(2) + "%" : "–";
    const pct20Cls = s.changePct20 > 0 ? "up" : s.changePct20 < 0 ? "down" : "";

    tr.innerHTML = "<td style='color:var(--natives-text-muted);'>" + (idx + 1) + "</td>" +
                   "<td><b>" + s.name + "</b></td>" +
                   "<td class='num " + pctCls + "'>" + pct + "</td>" +
                   "<td class='num " + speedCls + "'>" + speed + "</td>" +
                   "<td>" + topName + "</td>" +
                   "<td class='num " + topCls + "'>" + topPrice + "</td>" +
                   "<td class='num " + topCls + "'>" + topPct + "</td>" +
                   "<td class='num " + pct5Cls + "'>" + pct5 + "</td>" +
                   "<td class='num " + pct20Cls + "'>" + pct20 + "</td>";

    tr.addEventListener("click", function() {
      selectSector(s);
    });

    tbody.appendChild(tr);
  });

  // 首次默认选中第 1 个板块
  if (!currentSelectedSector && items.length > 0) {
    selectSector(items[0]);
  }
}

function selectSector(s) {
  currentSelectedSector = s;
  document.querySelectorAll("#sectors-table tbody tr").forEach(function(r) {
    r.classList.remove("selected-sector");
  });
  renderSectorsTableSelection();
  loadSectorStocks(s);
}

function renderSectorsTableSelection() {
  const rows = document.querySelectorAll("#sectors-table tbody tr");
  rows.forEach(function(r) {
    const nameCell = r.querySelector("td:nth-child(2)");
    if (nameCell && currentSelectedSector && nameCell.textContent.trim() === currentSelectedSector.name) {
      r.classList.add("selected-sector");
    }
  });
}

function loadSectorStocks(sector) {
  const titleEl = document.getElementById("sector-stocks-title");
  if (titleEl) {
    titleEl.innerHTML = "<b>" + sector.name + "</b> 板块个股";
  }
  const tbody = document.querySelector("#sector-stocks-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "<tr><td colspan='10' style='text-align:center; padding: 20px;'>加载板块成分股中…</td></tr>";

  const url = "/api/market/sector/stocks?name=" + encodeURIComponent(sector.name) +
              "&code=" + encodeURIComponent(sector.code || "") +
              "&top=" + encodeURIComponent(sector.topStockCode || "");

  api("GET", url).then(function(res) {
    lastStocksData = res.quotes || [];
    if (titleEl) {
      titleEl.innerHTML = "<b>" + sector.name + "</b> 板块个股（共 " + lastStocksData.length + " 只）";
    }
    renderSectorStocksTable();
  }).catch(function(err) {
    tbody.innerHTML = "<tr><td colspan='10' class='err' style='text-align:center; padding: 20px;'>获取成分股失败：" + err.message + "</td></tr>";
  });
}

function renderSectorStocksTable() {
  const tbody = document.querySelector("#sector-stocks-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "";

  const items = lastStocksData.slice();
  if (stockSort.key) {
    items.sort(function(a, b) {
      const va = a[stockSort.key] != null ? a[stockSort.key] : -999999;
      const vb = b[stockSort.key] != null ? b[stockSort.key] : -999999;
      return stockSort.asc ? (va > vb ? 1 : -1) : (va < vb ? 1 : -1);
    });
  }

  if (items.length === 0) {
    tbody.innerHTML = "<tr><td colspan='10' style='text-align:center; padding: 20px;'>暂无成分股数据</td></tr>";
    return;
  }

  items.forEach(function(st, idx) {
    const tr = document.createElement("tr");
    tr.className = "clickable";
    if (state.currentSymbol === st.code) {
      tr.classList.add("selected");
    }

    const price = st.price != null ? st.price.toFixed(2) : "–";
    const pct = st.changePct != null ? (st.changePct > 0 ? "+" : "") + st.changePct.toFixed(2) + "%" : "–";
    const change = st.change != null ? (st.change > 0 ? "+" : "") + st.change.toFixed(2) : "–";
    const cls = st.changePct > 0 ? "up" : st.changePct < 0 ? "down" : "";
    const vol = st.volume != null ? (st.volume >= 10000 ? (st.volume / 10000).toFixed(2) + "万" : st.volume.toFixed(0)) : "–";
    const turnover = st.turnover != null ? st.turnover.toFixed(2) : "–";
    const amp = st.amplitude != null ? st.amplitude.toFixed(2) : "–";
    const vratio = st.volumeRatio != null ? st.volumeRatio.toFixed(2) : "–";

    tr.innerHTML = "<td style='color:var(--natives-text-muted);'>" + (idx + 1) + "</td>" +
                   "<td><b>" + st.code + "</b></td>" +
                   "<td>" + st.name + "</td>" +
                   "<td class='num " + cls + "'>" + price + "</td>" +
                   "<td class='num " + cls + "'>" + pct + "</td>" +
                   "<td class='num " + cls + "'>" + change + "</td>" +
                   "<td class='num'>" + vol + "</td>" +
                   "<td class='num'>" + turnover + "</td>" +
                   "<td class='num'>" + amp + "</td>" +
                   "<td class='num'>" + vratio + "</td>";

    tr.addEventListener("click", function() {
      document.querySelectorAll("#sector-stocks-table tbody tr").forEach(function(r) { r.classList.remove("selected"); });
      tr.classList.add("selected");
      actions.selectSymbol(st.code, st.name);
    });

    tbody.appendChild(tr);
  });
}

// 表头排序绑定
document.querySelectorAll("#sectors-table th.sortable").forEach(function(th) {
  th.addEventListener("click", function() {
    const key = th.dataset.key;
    if (sectorSort.key === key) { sectorSort.asc = !sectorSort.asc; }
    else { sectorSort = { key: key, asc: false }; }
    document.querySelectorAll("#sectors-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    th.classList.add(sectorSort.asc ? "sort-asc" : "sort-desc");
    renderSectorsTable();
  });
});

document.querySelectorAll("#sector-stocks-table th.sortable").forEach(function(th) {
  th.addEventListener("click", function() {
    const key = th.dataset.key;
    if (stockSort.key === key) { stockSort.asc = !stockSort.asc; }
    else { stockSort = { key: key, asc: false }; }
    document.querySelectorAll("#sector-stocks-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    th.classList.add(stockSort.asc ? "sort-asc" : "sort-desc");
    renderSectorStocksTable();
  });
});

})();
