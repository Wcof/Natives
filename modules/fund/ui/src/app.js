(() => {
const { state, actions, api, themeColor, themeRgba, applyLanguage } = globalThis.Fund;
// Fund 页面 controller：负责装配与生命周期（握手、页签、看板折叠、配色切换、
// selectSymbol 编排、独立小窗）。R-E3 import 方向：controller → core / actions。
// 领域能力（行情表、图表、盘口、记账）经 actions 注册表协作，不直接互相 import。

// ---------- 配色模式切换（红涨绿跌 vs 绿涨红跌） ----------
// 按钮为纯图标（红/绿双色箭头），颜色绑定 --color-up/--color-down 变量，
// 模式切换时变量自动互换、图标颜色随之反转，无需重渲染；仅更新语义属性。
const colorBtn = document.getElementById("btn-color-mode");
function applyColorModeLabel() {
  const cn = state.colorMode === "cn";
  colorBtn.title = Fund.t(cn ? "fundHomeSettingscn" : "fundHomeSettingsintl");
  colorBtn.setAttribute("aria-label", colorBtn.title);
}
colorBtn.addEventListener("click", async function() {
  colorBtn.disabled=true;
  try { await actions.setFinancialColorMode(state.colorMode === "cn" ? "intl" : "cn"); }
  catch { Fund.home.status(Fund.t("fundHomeSettingsWriteFailed")); }
  finally { colorBtn.disabled=false; }
});
document.addEventListener("fund:financial-display-changed",applyColorModeLabel);
document.addEventListener("fund:language-changed",applyColorModeLabel);
applyColorModeLabel();

// ---------- 双侧栏折叠联动（左侧边栏与右侧看板各自拥有独立的 collapse-btn） ----------

// 1. 左侧侧边栏折叠：控制外壳 app-sidebar（与 space-toggle-sidebar-btn 一致，彻底隐藏无残留条）
let leftSidebarCollapsed = false;
const leftSidebarBtn = document.getElementById("btn-toggle-left-sidebar");
function applyLeftSidebarCollapsed(notify = true) {
  if (leftSidebarBtn) {
    leftSidebarBtn.setAttribute("aria-pressed", String(leftSidebarCollapsed));
    leftSidebarBtn.innerHTML = '<svg class="svg-icon" aria-hidden="true"><use href="#i-panel" /></svg>';
    leftSidebarBtn.title = Fund.t(leftSidebarCollapsed ? "fundHomeSidebarExpand" : "fundHomeSidebarCollapse");
    leftSidebarBtn.setAttribute("aria-label",leftSidebarBtn.title);
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
  sideBtn.innerHTML = '<svg class="svg-icon" aria-hidden="true"><use href="#i-panel-right" /></svg>';
  sideBtn.title = Fund.t(sideCollapsed ? "fundHomeDashboardExpand" : "fundHomeDashboardCollapse") + " (⌘B)";
  sideBtn.setAttribute("aria-label",sideBtn.title);
  sideBtn.classList.toggle("active", sideCollapsed);
  try { localStorage.setItem("natives-fund-side-collapsed", sideCollapsed ? "1" : "0"); } catch (e) {}
  if (actions.redrawChart) actions.redrawChart();
  if (actions.resizeStockDetailCharts) actions.resizeStockDetailCharts();
  window.dispatchEvent(new Event("resize"));
  // 图表重绘经图表模块公开动作（mode/T05），不直接遍历其内部实例集合。
  actions.resizeTrendsCharts();
}
// 顶栏唯一折叠入口：个股详情激活时托管右侧看板，其余场景折叠外层侧栏
function toggleSidePanel() {
  if (actions.isStockDetailActive()) {
    actions.toggleStockDetailSide();
    return;
  }
  sideCollapsed = !sideCollapsed;
  applySideCollapsed();
}
sideBtn.addEventListener("click", toggleSidePanel);
// 供标的切换流程调用：展开看板并记忆用户展开偏好
function expandSidePanel() {
  if (!sideCollapsed) return;
  sideCollapsed = false;
  applySideCollapsed();
}
document.addEventListener("keydown", function(e) {
  if ((e.metaKey || e.ctrlKey) && (e.key === "b" || e.key === "B")) { e.preventDefault(); toggleSidePanel(); }
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

// ---------- 标签页切换与数字键 1-9 直达（0=首页） ----------
const tabs = ["watchlist", "market", "trends", "positions", "assets", "tx", "import", "nav", "news", "history"];
// 首页是默认视图但不占 1-9 快捷键位（方案 01 §3）；allViews 驱动视图切换循环。
const allViews = ["home", "settings"].concat(tabs);
function setTabSelected(el, selected) {
  if (!el) return;
  // DES 02 R03：主导航为 tab（aria-selected）；工具菜单项为 menuitem（data-active）。
  if (el.getAttribute("role") === "tab") el.setAttribute("aria-selected", selected ? "true" : "false");
  else el.toggleAttribute("data-active", selected);
}
actions.openFinanceSettings=()=>{
  allViews.forEach(other=>{
    document.getElementById("view-"+other)?.toggleAttribute("hidden",other!=="settings");
    setTabSelected(document.getElementById("tab-"+other), false);
  });
  sideBody.classList.remove("stock-detail-active","trends-active","home-active");
  document.getElementById("view-stock-detail").hidden=true;
  Fund.leaveFundDetail?.();
  Fund.holdingSummary?.clear();
  const fdView = document.getElementById("view-fund-detail");
  if (fdView) fdView.hidden = true;
  document.body.classList.remove("fund-detail-active");
  actions.pauseStockDetail?.();actions.leavePositions?.();actions.leaveHome?.();actions.leaveAssets?.();actions.leaveTx?.();
  syncNavFocus();
  switchSidePane("settings");actions.loadFinanceSettings();
  document.getElementById("financial-settings-title")?.focus();
};

// ---------- 右侧看板随 Tab 切换联动（自选/行情/趋势/持仓/流水/导入/净值） ----------
function switchSidePane(tabName) {
  const panes = document.querySelectorAll("#side-dashboard .side-tab-pane");
  panes.forEach(function(p) {
    const isTarget = p.dataset.tab === tabName;
    p.classList.toggle("active", isTarget);
    p.style.display = isTarget ? "flex" : "none";
  });
  // 触发对应子模块的侧栏数据同步
  if (tabName === "market" && actions.updateSectorSidebar) {
    actions.updateSectorSidebar();
  } else if (tabName === "positions" && actions.updatePositionsSidebar) {
    actions.updatePositionsSidebar();
  } else if (tabName === "tx" && actions.updateTxSidebar) {
    actions.updateTxSidebar();
  } else if (tabName === "nav" && actions.updateNavSidebar) {
    actions.updateNavSidebar();
  }
  // 切换页签可能导致主区域尺寸变化，重绘图表
  window.dispatchEvent(new Event("resize"));
  // 图表重绘经图表模块公开动作（mode/T05），不直接遍历其内部实例集合。
  actions.resizeTrendsCharts();
}
actions.switchSidePane = switchSidePane;

tabs.forEach(function(t) {
  const btn = document.getElementById("tab-" + t);
  if (btn) {
    btn.addEventListener("click", function() {
      // 切换常规 Tab 时，关闭个股与基金详情视口并暂停轮询
      const sdView = document.getElementById("view-stock-detail");
      if (sdView) sdView.hidden = true;
      Fund.leaveFundDetail?.();
  Fund.holdingSummary?.clear();
  const fdView = document.getElementById("view-fund-detail");
      if (fdView) fdView.hidden = true;
      sideBody.classList.remove("stock-detail-active");
      document.body.classList.remove("fund-detail-active");
      if (actions.pauseStockDetail) actions.pauseStockDetail();

      allViews.forEach(function(other) {
        const el = document.getElementById("tab-" + other);
        setTabSelected(el, other === t);
        const v = document.getElementById("view-" + other);
        if (v) v.hidden = (other !== t);
      })
      document.body.classList.toggle("market-active", t === "market");
      updateRefreshScope(t);;
      if (t !== "watchlist") actions.leaveWatchlist?.();
      if (t === "trends") {
        sideBody.classList.add("trends-active");
        const ensure = actions.ensureEcharts || globalThis.Fund?.ensureEcharts;
        if (ensure) {
          ensure.call(globalThis.Fund).catch(function() {}).then(function() { actions.refreshTrends(); });
        } else {
          actions.refreshTrends();
        }
      } else {
        sideBody.classList.remove("trends-active");
        actions.pauseTrends?.();
      }
      if (t === "home") {
        sideBody.classList.add("home-active");
      } else {
        sideBody.classList.remove("home-active");
      }
      switchSidePane(t);
      if (t === "watchlist") actions.refreshWatchlist();
      if (t === "market") actions.refreshMarket();
      else actions.leaveMarket?.();
      if (t === "positions") actions.loadPositions();
      else if (actions.leavePositions) actions.leavePositions();
      if (actions.leaveHome) actions.leaveHome();
      if (t === "assets") actions.loadAssets();
      else actions.leaveAssets?.();
      if (t === "tx") actions.loadTx();
      else actions.leaveTx?.();
      actions.leaveFinanceSettings?.();
      if(t!=='import')actions.leaveImport?.();
      actions.leaveNewsPage?.();
      actions.leaveDecisionHistory?.();
      if(t==='news')actions.loadNewsPage?.();
      if(t==='history')actions.loadDecisionHistory?.();
    });
  }
});

// 首页：默认视图；点击仅切换视图（快照已在 initApp 时加载，手动刷新按钮重取）。
const homeBtn = document.getElementById("tab-home");
document.getElementById('tab-ai')?.addEventListener('click',()=>{homeBtn.click();document.getElementById('home-ai-panel')?.scrollIntoView({block:'center'});document.getElementById('home-ai-title')?.focus({preventScroll:true});});
if (homeBtn) {
  homeBtn.addEventListener("click", function() {
    Fund.leaveFundDetail?.();
    Fund.holdingSummary?.clear();
    actions.leaveFinanceSettings?.();
    actions.leaveDecisionHistory?.();
    actions.leaveNewsPage?.();
    actions.leaveAssets?.();actions.leaveTx?.();
    if (actions.leavePositions) actions.leavePositions();
    const sdView = document.getElementById("view-stock-detail");
    if (sdView) sdView.hidden = true;
    sideBody.classList.remove("stock-detail-active");
    if (actions.pauseStockDetail) actions.pauseStockDetail();
    allViews.forEach(function(other) {
      const el = document.getElementById("tab-" + other);
      setTabSelected(el, other === "home");
      const v = document.getElementById("view-" + other);
      if (v) v.hidden = (other !== "home");
    });
    document.body.classList.remove("market-active");
    updateRefreshScope("home");
    sideBody.classList.add("home-active");
    sideBody.classList.remove("trends-active");
    switchSidePane("home");
    actions.leaveWatchlist?.();actions.pauseTrends?.();
    if (actions.loadHome) actions.loadHome();
  });
}

// 主导航按视觉顺序漫游；既有数字快捷键保持兼容。
const mainNav=document.querySelector("nav.main-nav");
const navButtons=()=>[...mainNav.querySelectorAll("[role=tab]")];
const syncNavFocus=()=>navButtons().forEach(button=>{button.tabIndex=button.getAttribute("aria-selected")==="true"?0:-1;});
navButtons().forEach(button=>{
  button.setAttribute("aria-controls","view-"+button.id.slice(4));
  const view=document.getElementById("view-"+button.id.slice(4));view?.setAttribute("role","tabpanel");view?.setAttribute("aria-labelledby",button.id);
});
mainNav.addEventListener("click",syncNavFocus);
mainNav.addEventListener("keydown",e=>{
  if(!["ArrowLeft","ArrowRight","Home","End"].includes(e.key)||e.metaKey||e.ctrlKey||e.altKey)return;
  const buttons=navButtons(),index=buttons.indexOf(document.activeElement);if(index<0)return;e.preventDefault();
  const next=e.key==="Home"?0:e.key==="End"?buttons.length-1:(index+(e.key==="ArrowLeft"?-1:1)+buttons.length)%buttons.length;
  buttons[next].click();buttons[next].focus();
});
syncNavFocus();
document.addEventListener("fund:language-changed",()=>{applyLeftSidebarCollapsed(false);sideBtn.title=Fund.t(sideCollapsed?"fundHomeDashboardExpand":"fundHomeDashboardCollapse")+" (⌘B)";sideBtn.setAttribute("aria-label",sideBtn.title);});
// 数字键 0=首页、1-9 直达页签（同花顺快捷键风格；输入框聚焦时不触发）
document.addEventListener("keydown", function(e) {
  if (e.metaKey || e.ctrlKey || e.altKey || document.querySelector("dialog[open]")) return;
  const tag = (document.activeElement && document.activeElement.tagName || "").toLowerCase();
  if (tag === "input" || tag === "textarea" || tag === "select") return;
  if (e.key === "0") {
    const btn = document.getElementById("tab-home");
    if (btn) btn.click();
    return;
  }
  const idx = "123456789".indexOf(e.key);
  if (idx < 0) return;
  const btn = document.getElementById("tab-" + tabs[idx]);
  if (btn) btn.click();
});

// ---------- 工具菜单与刷新范围（DES 02 R03） ----------
// 刷新按钮标注当前刷新范围；视图切换时更新（休市也可手动拉取的既有语义不变）。
const refreshScopeKeys = {
  home: "fundHomeHome", positions: "fundLedgerPagePositions", assets: "fundLedgerNavAssets",
  tx: "fundLedgerPageTransactions", watchlist: "fundLedgerNavWatchlist", market: "fundLedgerNavMarket",
  trends: "fundLedgerNavTrends", import: "fundLedgerNavImport", nav: "fundLedgerNavNav", settings: "fundHomeFinanceSettings",
  news:'fundHomeNews',history:'fundHomeDecisionHistory',
};
function updateRefreshScope(viewName) {
  const btn = document.getElementById("btn-manual-refresh");
  if (!btn) return;
  const scope = refreshScopeKeys[viewName] ? Fund.t(refreshScopeKeys[viewName]) : "";
  const base = Fund.t("fundHomeRefresh");
  btn.title = base + (scope ? " · " + scope : "");
  btn.setAttribute("aria-label", btn.title);
}
actions.updateRefreshScope = updateRefreshScope;

// 工具菜单：原生 menu 语义，Esc／外部点击／再次点击关闭；菜单项复用 tab-* 视图切换。
const toolsWrap = document.querySelector(".tools-wrap");
const toolsBtn = document.getElementById("btn-tools-menu");
const toolsMenu = document.getElementById("tools-menu");
function closeToolsMenu(refocus) {
  if (!toolsMenu || toolsMenu.hidden) return;
  toolsMenu.hidden = true;
  toolsBtn?.setAttribute("aria-expanded", "false");
  if (refocus) toolsBtn?.focus();
}
function openToolsMenu() {
  if (!toolsMenu) return;
  toolsMenu.hidden = false;
  toolsBtn?.setAttribute("aria-expanded", "true");
  toolsMenu.querySelector("button")?.focus();
}
if (toolsBtn && toolsMenu) {
  toolsBtn.addEventListener("click", function() {
    if (toolsMenu.hidden) openToolsMenu(); else closeToolsMenu(true);
  });
  toolsMenu.addEventListener("click", function(e) {
    // 激活任一菜单项即关闭菜单（DES R05：菜单最多一条展开，激活后收起）。
    if (e.target.closest("button")) closeToolsMenu(false);
  });
  toolsMenu.addEventListener("keydown", function(e) {
    const items = [...toolsMenu.querySelectorAll("button")];
    const idx = items.indexOf(document.activeElement);
    if (e.key === "Escape") { e.stopPropagation(); closeToolsMenu(true); }
    else if (e.key === "ArrowDown") { e.preventDefault(); items[(idx + 1) % items.length]?.focus(); }
    else if (e.key === "ArrowUp") { e.preventDefault(); items[(idx - 1 + items.length) % items.length]?.focus(); }
  });
  document.addEventListener("click", function(e) {
    if (toolsMenu.hidden) return;
    if (!toolsWrap?.contains(e.target)) closeToolsMenu(false);
  });
  document.addEventListener("fund:language-changed", function() {
    if (toolsBtn) { toolsBtn.textContent = Fund.t("fundHomeToolsMenu"); }
    if (toolsMenu) toolsMenu.setAttribute("aria-label", Fund.t("fundHomeToolsMenu"));
  });
}

function safeDecode(str) {
  if (!str) return '';
  try {
    return decodeURIComponent(str);
  } catch (e) {
    return str;
  }
}

// ---------- 深度看板联动：标的切换编排 ----------
function selectSymbol(symbol, name, userTriggered = false) {
  state.currentSymbol = symbol;
  state.currentName = safeDecode(name || symbol);
  // 用户主动点击行时才根据需要展开侧栏
  if (userTriggered) {
    expandSidePanel();
  }
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
  if (actions.loadTrendsStock) {
    const item=state.watchlistItems.find(item=>item.symbol===symbol);
    actions.loadTrendsStock(symbol,name,item?.assetType);
  }
  if (actions.updateSectorStockDetail) {
    actions.updateSectorStockDetail(symbol, name);
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

document.getElementById('btn-toggle-fav').addEventListener('click',async function() {
  if(this.disabled || !state.currentSymbol)return;this.disabled=true;
  try {await actions.setWatchlistMember({symbol:state.currentSymbol,name:state.currentName},!state.watchlistItems.some(item=>item.symbol===state.currentSymbol));}
  catch(error){document.getElementById('dash-name').textContent=error.message || Fund.t('fundHomeRequestFailed');}
  finally {this.disabled=false;}
});

function applyGlobalTheme(appearanceOrTheme) {
  // DES-D07：仅接受已知值（light/archive/dark/volt）；缺偏好或非法值回
  // Archive，不落到深色。已知值才写入模块缓存（缓存仅启动兜底）。
  const known = ["light", "archive", "dark", "volt"].includes(appearanceOrTheme);
  const isLight = appearanceOrTheme === "light" || appearanceOrTheme === "archive";
  document.documentElement.dataset.theme = known && !isLight ? "volt" : "archive";
  try {
    if (known) localStorage.setItem("natives-theme", isLight ? "archive" : "volt");
  } catch (e) {}
  if (actions.redrawChart) {
    actions.redrawChart();
  }
  if (actions.reloadStockDetailTheme) {
    actions.reloadStockDetailTheme();
  }
  if (globalThis.Fund.trends && globalThis.Fund.trends.reloadTheme) {
    globalThis.Fund.trends.reloadTheme();
  }
}

// 优先继承本地记忆的主题模式
try {
  const saved = localStorage.getItem("natives-theme");
  if (saved === "archive" || saved === "light") {
    document.documentElement.dataset.theme = "archive";
  }
} catch (e) {}

// ---------- 握手与初始化自启 ----------
// ADR-0036 深链：Shell 转发的纯导航意图（白名单 view），未知值回到默认视图。
function applyNavigateView(view) {
  if (typeof view !== "string") return null;
  if(view==="settings"){actions.openFinanceSettings();return "settings";}
  // P02：目标与配置 Tab 已撤销，goal/allocation 深链回首页（不自动打开编辑弹窗）。
  const target = { holdings: "positions", assets: "assets", goal: "home", allocation: "home" }[view];
  if (!target) return null; // 未知目标：保持当前/默认视图
  const btn = document.getElementById("tab-" + target);
  if (btn && btn.getAttribute("aria-selected")!=="true") btn.click();
  return target;
}

// 早于 welcome 到达的深链导航意图先保存，initApp 完成初始化后应用（方案 01 §3）。
let pendingInitialView = null;
let initialized = false;
let sharedDataDirty = false;
function refreshSharedData() {
  if (!sharedDataDirty || document.hidden || !initialized || document.querySelector('dialog[open]') || document.activeElement?.matches('input,textarea,select')) return;
  const view = document.querySelector('.main-column > section:not([hidden])')?.id;
  const refresh = {
    'view-home': actions.refreshHomeValuation,
    'view-positions': actions.loadPositions,
    'view-assets': actions.loadAssets,
    'view-tx': actions.loadTx,
    'view-watchlist': actions.refreshWatchlist,
    'view-news': () => actions.loadNewsPage?.(false),
    'view-history': actions.loadDecisionHistory,
    'view-fund-detail': () => Fund.refreshFundDetail?.(),
    'view-stock-detail': () => Fund.holdingSummary?.refresh(),
  }[view];
  // Settings/import editors retain their draft; navigation reads fresh state.
  if (!refresh) return;
  sharedDataDirty = false;
  Promise.resolve(refresh()).catch(error => Fund.home?.failure(error));
}
actions.invalidateSharedData = () => { sharedDataDirty = true; refreshSharedData(); };
document.addEventListener('visibilitychange', refreshSharedData);
document.addEventListener('focusout', () => queueMicrotask(refreshSharedData));
const sharedDialogObserver = new MutationObserver(refreshSharedData);
sharedDialogObserver.observe(document.body, {childList:true});
window.addEventListener('pagehide', () => sharedDialogObserver.disconnect());
window.addEventListener('pageshow', () => sharedDialogObserver.observe(document.body, {childList:true}));

window.addEventListener("message", function(e) {
  if (e.source !== window.parent) return;
  const msg = e.data;
  if (!msg || typeof msg !== "object") return;
  if (msg.type === "init") {
    // 计划 §31.3：init 上下文携带外观偏好（volt=dark / archive=light），
    // 首次启用即跟随主应用主题；此后以 theme 消息为准。
    applyGlobalTheme(msg.appearance || msg.theme);
    window.parent.postMessage({ type: "hello", generation: msg.generation, challenge: msg.challenge }, "*");
  } else if (msg.type === "welcome") {
    // 首次 welcome 走完整初始化；续期（renew-session 换发）只更新 token，
    // 不重复 initApp，避免重启轮询定时器/重复拉数据。
    const firstInit = !initialized;
    state.surfaceGeneration = msg.generation;
    state.surfaceChallenge = msg.challenge;
    state.token = msg.token;
    if (firstInit) initApp(msg.view || pendingInitialView);
    pendingInitialView = null;
  } else if (msg.type === "theme") {
    applyGlobalTheme(msg.appearance || msg.theme);
  } else if (msg.type === "language") {
    applyLanguage(msg.language);
  } else if (msg.type === "navigate") {
    // 覆盖初次打开与已打开后的二次导航：会话存活期间直接切页。
    // 初始化未完成（无 token）时先保存意图，initApp 后应用，避免被默认视图覆盖。
    if (!state.token) {
      pendingInitialView = msg.view;
      return;
    }
    applyNavigateView(msg.view);
  } else if (msg.type === "natives:data-invalidated" && msg.generation === state.surfaceGeneration && msg.challenge === state.surfaceChallenge) {
    actions.invalidateSharedData?.();
  }
});

function initApp(initialView) {
  // 深链优先（holdings/assets/goal/allocation 等），否则默认进入首页；
  // 隐藏视图的数据刷新由页签点击按需触发，初始化不再无条件启动自选与趋势。
  if (initialized || !state.token) return;
  initialized = true;
  if(initialView!=="settings")actions.loadFinancialDisplay();
  const startView = { holdings: "positions", assets: "assets", goal: "home", allocation: "home" }[initialView] || "home";
  if(initialView==="settings"){
    actions.openFinanceSettings();
  } else {
  const startBtn = document.getElementById("tab-" + startView);
  if (startBtn) {
    startBtn.click();
  } else {
    switchSidePane(startView);
  }
  }


  // 顶部跑马灯全局可见，保留市场调度；各页签数据按「当前可见页签」刷新——
  // 隐藏视图不发请求（方案 05 G1.1），可见视图保留交易时段轮询兜底。
  let watchlistPollCount = 0;
  let positionsPollCount = 0;
  const session = globalThis.Fund.marketSession;
  session.startScheduler(function(tick) {
    const detailView=document.getElementById('view-fund-detail');
    if(document.visibilityState==='visible'&&detailView&&!detailView.hidden){Fund.tickFundDetail(tick);return;}

    const shouldWork = tick.open || tick.manual || tick.reason === "init";
    if (!shouldWork) {
      if (document.visibilityState === "visible" && !document.getElementById("view-positions").hidden) actions.refreshPositionsQuotes();
      if (document.visibilityState === "visible" && !document.getElementById("view-home").hidden && !Fund.home?.inlineAIActive?.()) actions.refreshHomeValuation(true);
      return;
    }
    if (document.visibilityState !== "visible" && tick.reason !== "init") return;
    if (actions.isStockDetailActive()) return;
    actions.refreshTickers();
    const active = document.querySelector('.main-column > section:not([hidden])')?.id.replace('view-','')||'home';
    if (active === "home" && (tick.manual || !Fund.home?.inlineAIActive?.())) {
      if (tick.manual) actions.refreshHome();
      else if (tick.reason !== "init") actions.refreshHomeValuation(true);
    }
    if (active === "trends") {
      const selection = Fund.trends?.coordinator?.selection();
      if (selection?.symbol) Fund.trends.coordinator.refreshQuote(selection.symbol);
    }
    if (active === "watchlist") actions.loadDetailQuote();
    if (active === "watchlist" && (tick.open || tick.manual || tick.reason === "init")) {
      if (tick.manual || tick.reason === "init" || (tick.open && watchlistPollCount++ % 2 === 0)) {
        actions.refreshWatchlist();
      }
    }
    // 持仓页行情共享同一调度（A24）：账本快照只在切页/手动重载时取，
    // tick 刷当前 Tab 的场内报价并检查过期 NAV，开市时段降频、盘后 30 分钟检查。
    if (active === "positions") {
      if (tick.manual || tick.reason === "init" || (tick.open && positionsPollCount++ % 2 === 0)) {
        actions.refreshPositionsQuotes(tick.manual);
      }
    }
  });
  actions.requestFundRefresh = session.requestManualRefresh;
  const manualRefreshBtn = document.getElementById("btn-manual-refresh");
  if (manualRefreshBtn) {
    manualRefreshBtn.addEventListener("click", function() {
      session.requestManualRefresh();
    });
  }
}



})();
