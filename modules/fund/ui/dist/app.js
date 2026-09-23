(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 页面 controller：负责装配与生命周期（握手、页签、看板折叠、配色切换、
// selectSymbol 编排、独立小窗）。R-E3 import 方向：controller → core / actions。
// 领域能力（行情表、图表、盘口、记账）经 actions 注册表协作，不直接互相 import。

// ---------- 配色模式切换（红涨绿跌 vs 绿涨红跌） ----------
const colorBtn = document.getElementById("btn-color-mode");
colorBtn.addEventListener("click", function() {
  if (state.colorMode === "cn") {
    state.colorMode = "intl";
    document.documentElement.setAttribute("data-color-mode", "intl");
    colorBtn.innerHTML = '<svg class="svg-icon" aria-hidden="true"><use href="#i-palette" /></svg> 绿涨红跌';
  } else {
    state.colorMode = "cn";
    document.documentElement.removeAttribute("data-color-mode");
    colorBtn.innerHTML = '<svg class="svg-icon" aria-hidden="true"><use href="#i-palette" /></svg> 红涨绿跌';
  }
  actions.redrawChart();
});

// ---------- 双侧栏折叠联动（左侧边栏与右侧看板各自拥有独立的 collapse-btn） ----------

// 1. 左侧侧边栏折叠：控制外壳 app-sidebar（与 space-toggle-sidebar-btn 一致，彻底隐藏无残留条）
let leftSidebarCollapsed = false;
const leftSidebarBtn = document.getElementById("btn-toggle-left-sidebar");
function applyLeftSidebarCollapsed(notify = true) {
  if (leftSidebarBtn) {
    leftSidebarBtn.setAttribute("aria-pressed", String(leftSidebarCollapsed));
    leftSidebarBtn.innerHTML = '<svg class="svg-icon" aria-hidden="true"><use href="#i-panel" /></svg>' + (leftSidebarCollapsed ? "" : '<span class="btn-text"> 折叠侧栏</span>');
    leftSidebarBtn.title = leftSidebarCollapsed ? "展开左侧栏" : "折叠左侧栏";
    leftSidebarBtn.classList.toggle("active", leftSidebarCollapsed);
  }
  if (notify && window.parent && window.parent !== window) {
    window.parent.postMessage({ type: "toggle-sidebar", collapsed: leftSidebarCollapsed }, "*");
  }
}
if (leftSidebarBtn) {
  leftSidebarBtn.addEventListener("click", function() {
    leftSidebarCollapsed = !leftSidebarCollapsed;
    applyLeftSidebarCollapsed(true);
  });
}
// 监听外壳发送的侧栏状态同步
window.addEventListener("message", function(e) {
  if (e.data && e.data.type === "sidebar-state" && typeof e.data.collapsed === "boolean") {
    leftSidebarCollapsed = e.data.collapsed;
    applyLeftSidebarCollapsed(false);
  }
});
if (window.parent && window.parent !== window) {
  window.parent.postMessage({ type: "get-sidebar-state" }, "*");
}

// 2. 右侧看板侧栏折叠：与文档管理一致（按钮 / ⌘B，状态记忆，彻底折叠不留残条）
// 默认折叠：仅选中标的（点击基金/股票行）后才自动展开；用户手动切换后按记忆值。
// 沙箱 iframe 内 localStorage 可能被禁，降级为内存态。
let sideCollapsed = true;
try {
  // "0" = 用户曾展开过（沿用）；无记录或 "1" = 默认折叠
  sideCollapsed = localStorage.getItem("natives-fund-side-collapsed") !== "0";
} catch (e) {}
const sideBody = document.querySelector(".workspace-body");
const sideBtn = document.getElementById("btn-toggle-side");
function applySideCollapsed() {
  sideBody.classList.toggle("side-collapsed", sideCollapsed);
  sideBtn.setAttribute("aria-pressed", String(sideCollapsed));
  sideBtn.innerHTML = '<svg class="svg-icon" aria-hidden="true"><use href="#i-panel-right" /></svg>' + (sideCollapsed ? "" : '<span class="btn-text"> 折叠看板</span>');
  sideBtn.title = (sideCollapsed ? "展开看板" : "折叠看板") + " (⌘B)";
  sideBtn.classList.toggle("active", sideCollapsed);
  try { localStorage.setItem("natives-fund-side-collapsed", sideCollapsed ? "1" : "0"); } catch (e) {}
  actions.redrawChart();
}
sideBtn.addEventListener("click", function() { sideCollapsed = !sideCollapsed; applySideCollapsed(); });
// 供标的切换流程调用：展开看板并记忆用户展开偏好
function expandSidePanel() {
  if (!sideCollapsed) return;
  sideCollapsed = false;
  applySideCollapsed();
}
document.addEventListener("keydown", function(e) {
  if ((e.metaKey || e.ctrlKey) && (e.key === "b" || e.key === "B")) { e.preventDefault(); sideCollapsed = !sideCollapsed; applySideCollapsed(); }
});
if (sideCollapsed) applySideCollapsed();

// ---------- 看板拖拽调宽：side-resizer 拖动调整 side-column 宽度，localStorage 记忆 ----------
// 沙箱 iframe 内 localStorage 可能被禁，降级为内存态；与折叠状态用同一降级策略。
let sideWidth = 390;
try {
  const storedWidth = parseInt(localStorage.getItem("natives-fund-side-width"), 10);
  if (!isNaN(storedWidth)) sideWidth = storedWidth;
} catch (e) {}
function applySideWidth(w, persist) {
  sideWidth = Math.max(280, Math.min(560, w));
  document.documentElement.style.setProperty("--fund-side-width", sideWidth + "px");
  try { if (persist) localStorage.setItem("natives-fund-side-width", String(sideWidth)); } catch (e) {}
  actions.redrawChart();
}
const sideResizer = document.getElementById("side-resizer");
let sideResizing = false;
sideResizer.addEventListener("mousedown", function(e) {
  sideResizing = true;
  sideResizer.classList.add("dragging");
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
  e.preventDefault();
});
document.addEventListener("mousemove", function(e) {
  if (!sideResizing) return;
  // side-column 靠右：向左拖（clientX 变小）加宽
  applySideWidth(window.innerWidth - e.clientX, false);
});
document.addEventListener("mouseup", function() {
  if (!sideResizing) return;
  sideResizing = false;
  sideResizer.classList.remove("dragging");
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
  applySideWidth(sideWidth, true);
});
sideResizer.addEventListener("keydown", function(e) {
  if (e.key === "ArrowLeft") { applySideWidth(sideWidth + 20, true); e.preventDefault(); }
  else if (e.key === "ArrowRight") { applySideWidth(sideWidth - 20, true); e.preventDefault(); }
  else if (e.key === "Home") { applySideWidth(280, true); e.preventDefault(); }
  else if (e.key === "End") { applySideWidth(560, true); e.preventDefault(); }
});
applySideWidth(sideWidth, false);

// ---------- 标签页切换与数字键 1-6 直达 ----------
const tabs = ["watchlist", "market", "trends", "positions", "tx", "import", "nav"];
tabs.forEach(function(t) {
  const btn = document.getElementById("tab-" + t);
  if (btn) {
    btn.addEventListener("click", function() {
      tabs.forEach(function(other) {
        document.getElementById("tab-" + other).setAttribute("aria-selected", other === t ? "true" : "false");
        const v = document.getElementById("view-" + other);
        if (v) v.hidden = (other !== t);
      });
      if (t === "trends") {
        sideBody.classList.add("trends-active");
        actions.refreshTrends();
      } else {
        sideBody.classList.remove("trends-active");
      }
      if (t === "watchlist") actions.refreshWatchlist();
      if (t === "market") actions.refreshMarket();
      if (t === "positions") actions.loadPositions();
      if (t === "tx") actions.loadTx();
    });
  }
});

// 数字键 1-7 直达页签（同花顺快捷键风格；输入框聚焦时不触发）
document.addEventListener("keydown", function(e) {
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const tag = (document.activeElement && document.activeElement.tagName || "").toLowerCase();
  if (tag === "input" || tag === "textarea" || tag === "select") return;
  const idx = "1234567".indexOf(e.key);
  if (idx < 0) return;
  const btn = document.getElementById("tab-" + tabs[idx]);
  if (btn) btn.click();
});

// ---------- 深度看板联动：标的切换编排 ----------
function selectSymbol(symbol, name) {
  state.currentSymbol = symbol;
  state.currentName = name || symbol;
  // 点击基金/股票后才展示右栏看板（默认折叠，见上方折叠逻辑）
  expandSidePanel();
  document.getElementById("dash-name").textContent = state.currentName;
  document.getElementById("dash-code").textContent = state.currentSymbol;

  const rows = document.querySelectorAll("#watchlist-table tbody tr");
  rows.forEach(function(r) {
    r.classList.toggle("selected", r.dataset.symbol === symbol);
  });

  updateFavButtonState();
  actions.loadDetailQuote();
  actions.loadChartData();
  actions.loadCompareIndex();
  actions.loadDrawings();
  actions.syncWSSubscriptions();
  if (actions.loadTrendsStock) {
    const clean = symbol.replace(/^(sh|sz|bj)/i, "");
    actions.loadTrendsStock(clean);
  }
}
actions.selectSymbol = selectSymbol;

function updateFavButtonState() {
  const btn = document.getElementById("btn-toggle-fav");
  const isFav = state.watchlistItems.some(function(i) { return i.symbol === state.currentSymbol; });
  btn.innerHTML = isFav ? '<svg class="svg-icon" aria-hidden="true"><use href="#i-star" /></svg> 已自选' : '<svg class="svg-icon" aria-hidden="true"><use href="#i-star" /></svg> 自选';
  btn.classList.toggle("btn-accent", !isFav);
}
actions.updateFavButtonState = updateFavButtonState;

document.getElementById("btn-toggle-fav").addEventListener("click", function() {
  const isFav = state.watchlistItems.some(function(i) { return i.symbol === state.currentSymbol; });
  if (isFav) {
    api("DELETE", "/api/watchlists/items?watchlistId=" + state.activeGroup + "&symbol=" + state.currentSymbol).then(function() {
      actions.refreshWatchlist();
    });
  } else {
    api("POST", "/api/watchlists/items", {
      watchlistId: state.activeGroup,
      symbol: state.currentSymbol,
      name: state.currentName
    }).then(function() {
      actions.refreshWatchlist();
    });
  }
});

// ---------- 握手与初始化自启 ----------
window.addEventListener("message", function(e) {
  const msg = e.data;
  if (!msg || typeof msg !== "object") return;
  if (msg.type === "init") {
    // 计划 §31.3：init 上下文携带外观偏好（volt=dark / archive=light），
    // 首次启用即跟随主应用主题；此后以 theme 消息为准。
    document.documentElement.dataset.theme = msg.appearance === "light" ? "archive" : "volt";
    window.parent.postMessage({ type: "hello", generation: msg.generation, challenge: msg.challenge }, "*");
  } else if (msg.type === "welcome") {
    state.token = msg.token;
    initApp();
  } else if (msg.type === "theme") {
    document.documentElement.dataset.theme = msg.appearance === "light" ? "archive" : "volt";
    actions.redrawChart();
  }
});

function initApp() {
  actions.initWebSocket();
  actions.refreshTickers();
  actions.refreshWatchlist();
  if (state.currentSymbol) {
    selectSymbol(state.currentSymbol, state.currentName);
  }
  if (actions.initTrends) {
    actions.initTrends();
  }

  // 视口感知调度：前台时轮询作为 WS 的弹性备份
  // runtime 后端无 /ws 端点，WS 实际永远离线：自选表行情必须走轮询兜底，
  // 否则表格只在启动取一次数，此后永不更新（表现为"没数据/不动"）。
  let watchlistPollCount = 0;
  setInterval(function() {
    if (document.visibilityState === "visible") {
      actions.refreshTickers();
      if (!state.wsConnected) {
        actions.loadDetailQuote();
        // 自选表 6s 一刷（每两次轮询一次），与 tickers/详情的 3s 错开减负
        if (watchlistPollCount++ % 2 === 0) actions.refreshWatchlist();
      }
    }
  }, 3000);
}

setTimeout(function() {
  if (!state.token) initApp();
}, 300);

})();
