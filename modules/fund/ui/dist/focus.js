(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：专注模式（Focus Workspace）。职责一句话——三栏工作台的
// 进入/退出、左栏上下文资产列表（来源切换/Sparkline/轮动）、全局检索弹窗、
// 左栏拖拽调宽与键盘上下键切换标的。
// import 方向：core（状态/api/取色）；跨域经 actions（selectSymbol/refreshWatchlist 等）。

// ---------- 状态 ----------
let focusActive = false;
let focusSource = "watchlist"; // watchlist | market
let focusWidth = 220;
let focusItems = []; // { symbol, name, price, changePct }
try {
  const w = parseInt(localStorage.getItem("natives-fund-focus-width"), 10);
  if (!isNaN(w)) focusWidth = w;
} catch (e) {}

// ---------- 节点搬移：进入时把现有图表/盘口区块搬入三栏，退出时搬回 ----------
// 搬移而非复制：保持 canvas 上下文与事件绑定不失效，DOM 唯一归属。
const MOVE_TO_CENTER = [
  ".chart-control-bar", ".chart-canvas-container", ".market-compare-card",
];
const MOVE_TO_RIGHT = [
  ".quote-summary-card", ".depth-card", ".metrics-card", ".alert-card", ".side-header", ".alert-box", ".depth-section", ".metrics-grid",
];

function openStockDetail(symbol, name) {
  if (!symbol) return;
  const cleanCode = symbol.replace(/^(sh|sz|bj)/i, "");
  // 1. 选中该标的
  actions.selectSymbol(symbol, name || symbol, true);
  // 2. 如果之前在专注模式，先安全退出
  if (focusActive) {
    exitFocus();
  }
  // 3. 切换至「趋势分析」个股全盘详情视口（K线、分时、指标、资金流与AI决策全屏呈现）
  const trendsTab = document.getElementById("tab-trends");
  if (trendsTab) {
    trendsTab.click();
  }
  // 4. 立即触发深度分析流水线
  if (window.analyze) {
    window.analyze(cleanCode);
  } else if (actions.loadTrendsStock) {
    actions.loadTrendsStock(cleanCode);
  }
  // 5. 确保右侧决策看板完全展开（AI 结论/操作计划/买卖信号等卡片在此）
  if (typeof expandSidePanel === "function") {
    expandSidePanel();
  } else if (actions.expandSidePanel) {
    actions.expandSidePanel();
  }
}
actions.openStockDetail = openStockDetail;

function enterFocus() {
  openStockDetail(state.currentSymbol || "600519", state.currentName || "贵州茅台");
}

function exitFocus() {
  if (!focusActive) return;
  focusActive = false;
  const ws = document.getElementById("focus-workspace");
  const center = document.getElementById("focus-center");
  const right = document.getElementById("focus-right");
  document.body.classList.remove("focus-mode");
  if (ws) ws.hidden = true;
  // 搬回原位：主栏区块回到 main-column 内 chart-control-bar 之前的顺序，
  // 盘口区块回到 side-column 内 side-fund-content 之前的顺序（趋势信号
  // right-panel 在 side-fund-content 之后，不受影响）。
  const mainCol = document.querySelector(".main-column");
  const sideCol = document.getElementById("side-fund-content") || document.getElementById("side-dashboard");
  const anchorMain = document.querySelector(".main-column .search-box, .main-column section");
  MOVE_TO_CENTER.forEach((sel) => {
    const node = center.querySelector(sel);
    if (node && mainCol) mainCol.insertBefore(node, anchorMain);
  });
  MOVE_TO_RIGHT.forEach((sel) => {
    const node = right.querySelector(sel);
    if (node && sideCol) sideCol.insertBefore(node, sideCol.firstChild);
  });
  actions.redrawChart();
}

// ---------- 左栏资产列表 ----------
function renderFocusList() {
  const list = document.getElementById("focus-asset-list");
  if (!list) return;
  const source = focusItems;
  list.replaceChildren();
  source.forEach(function(item) {
    const row = document.createElement("div");
    row.className = "focus-asset-row" + (item.symbol === state.currentSymbol ? " active" : "");
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(item.symbol === state.currentSymbol));
    row.dataset.symbol = item.symbol;

    const text = document.createElement("div");
    text.className = "fa-text";
    const nameEl = document.createElement("div");
    nameEl.className = "fa-name";
    nameEl.textContent = item.name;
    const codeEl = document.createElement("div");
    codeEl.className = "fa-code";
    codeEl.textContent = item.symbol;
    text.append(nameEl, codeEl);

    const spark = document.createElement("canvas");
    spark.width = 56; spark.height = 20;
    spark.className = "fa-spark";

    const quote = document.createElement("div");
    quote.className = "fa-quote";
    const pctEl = document.createElement("div");
    pctEl.className = "fa-pct " + (item.changePct > 0 ? "up" : item.changePct < 0 ? "down" : "");
    pctEl.textContent = (item.changePct > 0 ? "+" : "") + (item.changePct || 0).toFixed(2) + "%";
    const priceEl = document.createElement("div");
    priceEl.className = "fa-price";
    priceEl.textContent = item.price ? item.price.toFixed(2) : "–";
    quote.append(pctEl, priceEl);

    row.append(text, spark, quote);
    row.addEventListener("click", function() {
      actions.selectSymbol(item.symbol, item.name);
    });
    row.addEventListener("dblclick", function() { enterFocus(); });
    list.appendChild(row);
    drawSpark(spark, item);
  });
}

// 迷你分时微线图：挂机拉 minute 数据太重，用报价走势代理（本地生成基于
// 涨跌幅的对称折线仅作方向示意不可取）。改为真实分钟数据但限流：
// 仅对当前可视且激活标的取真数据，其余行画零线占位，WS 更新时重绘当前行。
let sparkDataCache = {};
function drawSpark(canvas, item) {
  const cached = sparkDataCache[item.symbol];
  if (!cached) { drawFlatSpark(canvas, item); return; }
  drawSparkFromPoints(canvas, cached, item);
}
function drawFlatSpark(canvas, item) {
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.strokeStyle = themeColor("--natives-border");
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, canvas.height / 2);
  ctx.lineTo(canvas.width, canvas.height / 2);
  ctx.stroke();
}
function drawSparkFromPoints(canvas, pts, item) {
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  if (!pts || pts.length < 2) { drawFlatSpark(canvas, item); return; }
  let min = Infinity, max = -Infinity;
  pts.forEach(function(p) { min = Math.min(min, p.price); max = Math.max(max, p.price); });
  const range = (max - min) || 1;
  ctx.strokeStyle = (item.changePct >= 0) ? themeColor("--color-up") : themeColor("--color-down");
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  pts.forEach(function(p, i) {
    const x = (i / (pts.length - 1)) * (canvas.width - 2) + 1;
    const y = canvas.height - 2 - ((p.price - min) / range) * (canvas.height - 4);
    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
  });
  ctx.stroke();
}
// 激活标的的真实分时 Sparkline（限流：只拉当前选中标的）
function refreshActiveSpark() {
  const sym = state.currentSymbol;
  if (sparkDataCache["_ts_" + sym] && Date.now() - sparkDataCache["_ts_" + sym] < 60000) return;
  api("GET", "/api/market/minute?symbol=" + sym).then(function(res) {
    sparkDataCache[sym] = res.points || [];
    sparkDataCache["_ts_" + sym] = Date.now();
    if (focusActive) renderFocusList();
  }).catch(function() {});
}
actions.refreshActiveSpark = refreshActiveSpark;

// ---------- 数据源 ----------
function loadFocusItems() {
  if (focusSource === "watchlist") {
    // 复用 watchlist 状态（app.js 的 refreshWatchlist 已维护）
    actions.refreshWatchlist();
    focusItems = state.watchlistItems.map(function(it) {
      const q = state.lastQuotesMap[it.symbol] || {};
      return { symbol: it.symbol, name: it.name, price: q.price, changePct: q.changePct };
    });
    renderFocusList();
  } else {
    api("GET", "/api/market/stocks").then(function(res) {
      focusItems = (res.quotes || []).slice(0, 50).map(function(q) {
        return { symbol: q.code || q.symbol, name: q.name, price: q.price, changePct: q.changePct };
      });
      renderFocusList();
    }).catch(function() {});
  }
}
actions.loadFocusItems = loadFocusItems;

const sourceSel = document.getElementById("focus-source");
sourceSel.addEventListener("change", function() {
  focusSource = sourceSel.value;
  loadFocusItems();
});

// ---------- WS 推流联动：刷新左栏报价 ----------
const origHandleWSTick = actions.handleWSTick;
// dashboard.js 已把报价写入 lastQuotesMap 并触发 renderWatchlistTable；
// 这里订阅同一数据源：专注模式激活时低频重绘左栏（节流 1s）。
let lastListRender = 0;
setInterval(function() {
  if (!focusActive) return;
  if (Date.now() - lastListRender < 1000) return;
  lastListRender = Date.now();
  if (focusSource === "watchlist") {
    let changed = false;
    focusItems.forEach(function(item) {
      const q = state.lastQuotesMap[item.symbol];
      if (q) {
        item.price = q.price;
        item.changePct = q.changePct;
        changed = true;
      }
    });
    if (changed) renderFocusList();
  }
}, 1000);

// ---------- 键盘：↑/↓ 切换标的，Esc 退出 ----------
document.addEventListener("keydown", function(e) {
  if (!focusActive) return;
  const tag = (document.activeElement && document.activeElement.tagName || "").toLowerCase();
  if (tag === "input" || tag === "textarea" || tag === "select") return;
  if (e.key === "Escape") { exitFocus(); return; }
  if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
  e.preventDefault();
  if (!focusItems.length) return;
  const idx = focusItems.findIndex(function(i) { return i.symbol === state.currentSymbol; });
  const next = e.key === "ArrowUp"
    ? (idx <= 0 ? focusItems.length - 1 : idx - 1)
    : (idx < 0 || idx >= focusItems.length - 1 ? 0 : idx + 1);
  const item = focusItems[next];
  if (item) actions.selectSymbol(item.symbol, item.name);
});

// ---------- 左栏拖拽调宽 ----------
function applyFocusWidth(w, persist) {
  focusWidth = Math.max(180, Math.min(360, w));
  document.documentElement.style.setProperty("--focus-left-width", focusWidth + "px");
  try { if (persist) localStorage.setItem("natives-fund-focus-width", String(focusWidth)); } catch (e) {}
  actions.redrawChart();
}
const focusResizer = document.getElementById("focus-resizer");
let focusResizing = false;
focusResizer.addEventListener("mousedown", function(e) {
  focusResizing = true;
  focusResizer.classList.add("dragging");
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
  e.preventDefault();
});
document.addEventListener("mousemove", function(e) {
  if (!focusResizing) return;
  applyFocusWidth(e.clientX, false); // 左栏靠左：clientX 即宽度
});
document.addEventListener("mouseup", function() {
  if (!focusResizing) return;
  focusResizing = false;
  focusResizer.classList.remove("dragging");
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
  applyFocusWidth(focusWidth, true);
});
focusResizer.addEventListener("keydown", function(e) {
  if (e.key === "ArrowLeft") { applyFocusWidth(focusWidth - 20, true); e.preventDefault(); }
  else if (e.key === "ArrowRight") { applyFocusWidth(focusWidth + 20, true); e.preventDefault(); }
  else if (e.key === "Home") { applyFocusWidth(180, true); e.preventDefault(); }
  else if (e.key === "End") { applyFocusWidth(360, true); e.preventDefault(); }
});
applyFocusWidth(focusWidth, false);

// ---------- 全局检索弹窗 ----------
const backdrop = document.getElementById("focus-search-backdrop");
const searchInput = document.getElementById("focus-search-input");
const searchResults = document.getElementById("focus-search-results");
let searchTimer = null;
let searchHits = [];

function openSearch() {
  backdrop.hidden = false;
  backdrop.removeAttribute("hidden");
  searchInput.value = "";
  searchResults.replaceChildren();
  searchInput.focus();
}
function closeSearch() {
  backdrop.hidden = true;
  backdrop.setAttribute("hidden", "");
  searchInput.value = "";
  searchResults.replaceChildren();
}

document.getElementById("focus-add-btn").addEventListener("click", openSearch);
document.getElementById("focus-add-row-btn").addEventListener("click", openSearch);
backdrop.addEventListener("click", function(e) { if (e.target === backdrop) closeSearch(); });

searchInput.addEventListener("input", function() {
  clearTimeout(searchTimer);
  const q = searchInput.value.trim();
  if (!q) { searchResults.replaceChildren(); return; }
  searchTimer = setTimeout(function() {
    api("GET", "/api/market/suggest?input=" + encodeURIComponent(q)).then(function(res) {
      searchHits = res.suggestions || [];
      searchResults.replaceChildren();
      if (!searchHits.length) {
        const empty = document.createElement("div");
        empty.className = "focus-search-empty";
        empty.textContent = "未找到匹配的标的";
        searchResults.appendChild(empty);
        return;
      }
      searchHits.forEach(function(hit) {
        const row = document.createElement("div");
        row.className = "focus-search-item";
        const nameSpan = document.createElement("span");
        nameSpan.textContent = hit.name;
        const symSpan = document.createElement("span");
        symSpan.className = "fs-sym";
        symSpan.textContent = hit.market + " " + hit.symbol;
        row.append(nameSpan, symSpan);
        row.addEventListener("click", function() {
          closeSearch();
          api("POST", "/api/watchlists/items", {
            watchlistId: state.activeGroup, symbol: hit.symbol, name: hit.name,
          }).then(function() {
            focusSource = "watchlist";
            if (sourceSel) sourceSel.value = "watchlist";
            actions.refreshWatchlist();
            loadFocusItems();
            actions.selectSymbol(hit.symbol, hit.name);
          }).catch(function(err) {
            console.error("Failed to add to watchlist:", err);
          });
        });
        searchResults.appendChild(row);
      });
    }).catch(function(err) {
      searchResults.replaceChildren();
      const empty = document.createElement("div");
      empty.className = "focus-search-empty";
      empty.textContent = "搜索失败：" + (err && err.message ? err.message : "未知错误");
      searchResults.appendChild(empty);
    });
  }, 250);
});
searchInput.addEventListener("keydown", function(e) {
  if (e.key === "Escape") closeSearch();
  if (e.key === "Enter" && searchHits.length) {
    searchResults.firstChild && searchResults.firstChild.click();
  }
});

// ---------- 入口 ----------
document.getElementById("focus-exit-btn").addEventListener("click", exitFocus);
// selectSymbol 编排后同步专注模式状态（badge/列表高亮/激活 Sparkline）
const origSelect = actions.selectSymbol;
actions.selectSymbol = function(symbol, name, userTriggered) {
  origSelect(symbol, name, userTriggered);
  if (focusActive) {
    document.getElementById("focus-badge").textContent = "专注模式 · " + (name || symbol);
    renderFocusList();
    refreshActiveSpark();
  }
};
// 双击自选表行/成分股行 = 进入该标的的个股详情视口（趋势分析全盘呈现）
document.addEventListener("dblclick", function(e) {
  const row = e.target.closest && e.target.closest("#watchlist-table tbody tr, #sector-stocks-table tbody tr, .focus-asset-row");
  if (!row) return;
  const symbol = row.dataset.symbol || row.dataset.code || (row.querySelector("td:nth-child(2)")?.textContent?.trim());
  const name = row.dataset.name || (row.querySelector("td:nth-child(3)")?.textContent?.trim()) || (row.querySelector("td:nth-child(2)")?.textContent?.trim());
  if (symbol) {
    openStockDetail(symbol, name);
  }
});

})();
