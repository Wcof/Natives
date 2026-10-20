(() => {
const { state, actions, api, t, decimalToMinor, formatMinorExact } = globalThis.Fund;
// Fund 持仓页 controller（R2 T04 供体 Stocks.tsx 复刻）：全部/股票/基金 Tab、
// 账户分组（展开/汇总/拖拽排序＋键盘上移下移等价路径）、行情/净值列、
// 场外待确认专属状态、添加（搜索身份＋期初/买入分开）、卖出/转移
// （预览→expectedRevision→409 草稿保留）、详情/资讯/AI/规则快捷动作。
// 账本权威全部来自 GET /api/positions/detail 单快照；市值/盈亏为展示口径，
// 缺行情/缺净值显示 待核实，不伪造完整总额。场外过期净值通过共享调度
// 有界同步；净值缺失/币种未确认在行内
// 提供直达估值对话框的修复入口。自动刷新只经 marketSession 共享
// 调度（A24），本文件不创建任何定时器。

const home = () => globalThis.Fund.home;
const esc = (s) => String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const today = () => new Date(Date.now() + 8 * 3600000).toISOString().slice(0, 10);
const fmt = n => n == null ? t("fundHomeNeedsVerificationccf920") : typeof n === "bigint" ? formatMinorExact(n) : String(n).includes(".") ? String(n).padEnd(String(n).indexOf(".") + 3, "0") : String(n) + ".00";
function displayMode() {
  const h = home();
  const mode = h && typeof h.displayMode === "function" ? h.displayMode() : null;
  return ["amount", "percent", "hidden"].includes(mode) ? mode : "hidden";
}
const maskAmounts = () => displayMode() !== "amount";
const hideIdentity = () => displayMode() === "hidden";
const money = (v) => (maskAmounts() ? "****" : fmt(v));

// ---------- 快照状态 ----------
let snapshotDay = "";
let snap = null;           // GET /api/positions/detail：{dataRevision, accounts[], holdings[]}
let quotes = {};           // quoteSymbol -> 实时报价（仅场内）
let quoteFailed = false;
const POS_TABS = ["all", "stock", "etf", "otc"]; // DES-D01 展示分类：股票/场内基金/场外基金（旧 fund 语义兼容）
let tabKind = "all";
const collapsed = new Set();
let dragId = null;
let _renderSig = "";
let generation = 0, pending = null, ledgerLoading=false, resumeLedger=false;
function cancelRequests() { generation++; if (pending) pending.abort(); pending = null; }
actions.leavePositions = () => { Fund.positionMenus.close(); actions.leaveHoldingEditor?.(); Fund.fundClassification?.cancel(document.getElementById('view-positions'));cancelRequests();ledgerLoading=false;resumeLedger=false; };
window.addEventListener("pagehide", actions.leavePositions);
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {const interrupted=ledgerLoading;actions.leavePositions();resumeLedger=interrupted;}
  else if(!document.getElementById("view-positions").hidden){if(resumeLedger){resumeLedger=false;loadPositions();}else if(snapshotDay&&snapshotDay!==today())fetchQuotes();}
});

const accountName = (id) => (snap && snap.accounts.find(a => a.id === id) || {}).name || String(id);
const visibleAccounts = () => (snap ? snap.accounts.filter(a => !a.archived) : []);
// DES-D01 展示分类严格按 instrumentKind/venue：stock=股票；etf=场内基金；
// otc=场外基金（instrumentKind=fund）。未知组合进「类型待核对」不冒充基金。
const categoryOf = h => h.instrumentKind === "stock" ? "stock"
  : h.instrumentKind === "etf" && h.venue === "exchange" ? "etf"
  : h.instrumentKind === "fund" && h.venue === "otc" ? "otc" : "unknown";
const tabHoldings = () => (snap ? snap.holdings.filter(h => tabKind === "all" ? true : categoryOf(h) === tabKind) : []);
const realizedInCategory = r => tabKind === "all" ? true : categoryOf(r) === tabKind;

let navSyncing = false;
let retryNav = false;
async function syncCurrentNav(gen, signal) {
  const navs=(snap?.holdings||[]).filter(h=>h.venue==="otc").map(h=>({fundCode:h.fundCode,navDate:h.nav?.date}));
  navSyncing=true;
  const manual=retryNav;retryNav=false;
  try {const result=await Fund.syncHoldingNav(navs,signal,manual);return gen===generation&&result.failed;}
  finally {navSyncing=false;}
}

function loadPositions() {
  if(document.hidden){resumeLedger=true;return Promise.resolve();}
  resumeLedger=false;ledgerLoading=true;
  if (actions.loadLegacyHoldings) Promise.resolve(actions.loadLegacyHoldings()).catch(() => {});
  cancelRequests();
  const gen = generation, controller = pending = new AbortController();
  document.getElementById("holding-add").disabled = true;
  document.getElementById("positions-status").textContent = t("fundHomeLoading");
  return api("GET", "/api/positions/detail", undefined, { signal: controller.signal }).then(function (res) {
    if (gen !== generation) return;
    ledgerLoading=false;
    snap = res; snapshotDay = today();
    document.getElementById("holding-add").disabled = false;
    quotes = {}; _renderSig = "";
    quoteFailed = false;
    document.getElementById("positions-status").textContent = "";
    render();
    if (actions.refreshWatchlist) Promise.resolve(actions.refreshWatchlist()).catch(() => {});
    return fetchQuotes();
  }).catch(function (e) {
    if (gen !== generation || e.name === "AbortError") return;
    ledgerLoading=false;
    if (snap) {
      document.getElementById('holding-add').disabled = false;
      document.getElementById('positions-status').textContent = t('fundLedgerLoadFailed') + ' · ' + e.message;
      return;
    }
    const groups = document.getElementById("positions-groups");
    Fund.positionMenus.close();
    if (groups) groups.innerHTML = "<p class='err'>" + esc(t("fundLedgerLoadFailed") + (e && e.message ? " · " + e.message : e)) + "</p><button type='button' data-pos-retry>" + esc(t("fundLedgerRetry")) + "</button>";
    document.getElementById("positions-status").textContent = t("fundLedgerLoadFailed");
    groups?.querySelector("[data-pos-retry]")?.addEventListener("click", loadPositions);
  });
}
actions.loadPositions = loadPositions;
document.getElementById('pos-open-ledger')?.addEventListener('click',()=>document.getElementById('tab-tx').click());
actions.reloadPositionsLedger = loadPositions;

// 行情刷新：只刷当前 Tab 的场内标的；隐藏视图直接跳过（A24 双保险）。
function fetchQuotes() {
  const view = document.getElementById("view-positions");
  if (!view || view.hidden || document.hidden || navSyncing || ledgerLoading) return Promise.resolve();
  cancelRequests();
  const gen = generation, controller = pending = new AbortController();
  return syncCurrentNav(gen, controller.signal).then(async navFailed => {
    if (gen !== generation || view.hidden || document.hidden) return null;
    const res = await api("GET", "/api/positions/detail?refresh=1&kind=" + tabKind, undefined, { signal: controller.signal });
    res.navSyncFailed = navFailed;
    return res;
  })
    .then(function (res) {
      if (!res || gen !== generation || view.hidden || document.hidden) return;
      const map = {};
      res.holdings.forEach(h => { if (h.quote) map[h.quoteSymbol] = h.quote; });
      snap = res; snapshotDay = today(); quotes = map; quoteFailed = res.quoteFailed; _renderSig = ""; render();
      document.getElementById("positions-status").textContent = res.navSyncFailed ? t("fundLedgerNavSyncFailed") : "";
    }).catch(function (e) {
      if (gen !== generation || e.name === "AbortError") return;
      quotes = {}; quoteFailed = true;
      if (snap) snap.holdings.forEach(h => { if (h.venue !== "otc") h.marketValue = null; });
      _renderSig = ""; render();
    }).finally(() => { if (gen === generation) pending = null; });
}
actions.refreshPositionsQuotes = (manual = false) => { if (manual) retryNav=true; return fetchQuotes(); };

// ---------- 渲染 ----------
// 账户分组排序：default=用户自定义（拖拽/上移下移），其余为临时视图排序，
// 不写回 sortOrder，切换回默认即恢复自定义顺序。
let accountSort = "default";
const ACCOUNT_SORTS = [
  ["default", "fundLedgerSortDefault"],
  ["name", "fundLedgerSortName"],
  ["amountAsc", "fundLedgerSortAmountAsc"],
  ["amountDesc", "fundLedgerSortAmountDesc"],
  ["dayPnlDesc", "fundLedgerSortDayPnlDesc"],
  ["dayPnlAsc", "fundLedgerSortDayPnlAsc"],
];
function sortAccounts(pairs) {
  if (accountSort === "default") return pairs;
  const byName = (x, y) => x[0].name.localeCompare(y[0].name, state.language === "en" ? "en" : "zh-Hans-CN");
  const byAmount = (x, y) => {
    const vx = summarize(x[1], x[0].id).value, vy = summarize(y[1], y[0].id).value;
    if (vx === vy) return byName(x, y);
    if (vx == null) return 1; if (vy == null) return -1;
    return accountSort === "amountAsc" ? (vx < vy ? -1 : 1) : (vx > vy ? -1 : 1);
  };
  const byDayPnl = (x, y) => {
    const sx = summarize(x[1], x[0].id), sy = summarize(y[1], y[0].id);
    const dx = sx.hasDayPnl ? sx.dayPnl : null, dy = sy.hasDayPnl ? sy.dayPnl : null;
    if (dx === dy) return byName(x, y);
    if (dx == null) return 1; if (dy == null) return -1;
    return accountSort === "dayPnlAsc" ? (dx < dy ? -1 : 1) : (dx > dy ? -1 : 1);
  };
  if (accountSort === "name") return pairs.slice().sort(byName);
  if (accountSort.startsWith("dayPnl")) return pairs.slice().sort(byDayPnl);
  return pairs.slice().sort(byAmount);
}

function rowValue(h) {
  return h.marketValue == null ? null : decimalToMinor(h.marketValue);
}
function summarize(list, accountId = null) {
  let cost = 0n, realized = 0n, value = 0n, dayPnl = 0n, unknown = false, valued = list.length > 0;
  let hasDayPnl = false, missingDayPnl = 0, estimatedDayCount=0;
  list.forEach(h => {
    if (h.costUnknown) unknown = true; else cost += decimalToMinor(h.cost || "0");
    realized += decimalToMinor(h.realized || "0");
    const v = rowValue(h);
    if (v == null) valued = false; else value += v;
    const fact=Fund.positionIncome.fact(h,'day');
    const dp = fact?.date===today()&&fact.amount!=null?decimalToMinor(fact.amount):null;
    if(dp!=null&&fact.estimated)estimatedDayCount++;
    if (dp != null) {
      dayPnl += dp;
      hasDayPnl = true;
    } else missingDayPnl++;
  });
  if(Array.isArray(snap?.realized)){
    const ids=new Set(visibleAccounts().map(a=>a.id));
    const history=snap.realized.filter(r=>(tabKind==="all"||realizedInCategory(r)) && ids.has(r.accountId) && (accountId==null || r.accountId===accountId));
    realized=history.some(r=>r.value==null)?null:history.reduce((sum,r)=>sum+decimalToMinor(r.value),0n);
  }
  return { cost, realized, value, dayPnl, hasDayPnl, missingDayPnl, estimatedDayCount, unknown, valued, count: list.length };
}

// 行操作全部收进「更多」菜单，行外不再散落按钮。
function actionCell(h) {
  const id = h.accountId + ":" + h.fundId;
  const b = (act, key) => `<button type="button" role="menuitem" data-pos-act="${act}" data-pos-id="${id}">${esc(t(key))}</button>`;
  const watched = h.quoteSymbol && state.watchlistItems.some(i => i.symbol === h.quoteSymbol);
  const suffix = h.accountId + "-" + h.fundId;
  const name = hideIdentity() ? "****" : (h.fundName || h.fundCode) + " · " + h.fundCode;
  const group = (key, buttons) => `<div role="group" aria-label="${esc(t(key))}"><p class="pos-menu-group">${esc(t(key))}</p>${buttons}</div>`;
  // N09：场外基金走专属订单流程（加仓/减仓/转换/定投/编辑持仓），
  // 股票/场内基金保留按成交数量、价格、费用记账的既有入口。
  const otcGroup = h.venue === "otc"
    ? group("fundLedgerOtcMenu", b("otcBuy", "fundLedgerOtcBuyTitle") + b("otcSell", "fundLedgerOtcSellTitle") + b("otcConvert", "fundLedgerOtcConvertTitle") + b("otcPlans", "fundLedgerOtcPlansTitle") + b("otcEdit", "fundLedgerOtcEditTitle"))
    : group("fundLedgerMenuRecord", b("buy", "fundLedgerMenuRecordBuy") + b("sell", "fundLedgerMenuRecordSell") + b("transfer", "fundLedgerTransfer") + b("opening", "fundHomeOpeningHistory") + b("valuation", "fundHomeValuation"));
  return `<button type="button" id="pos-edit-${suffix}" data-pos-act="edit" data-pos-id="${id}" ${maskAmounts()?"disabled":""}>${esc(t("fundHomeEdit"))}</button><details class="pos-more" data-pos-menu-id="${id}"><summary id="pos-more-${suffix}" aria-haspopup="menu" aria-expanded="false" aria-controls="pos-menu-${suffix}" aria-label="${esc(t("fundLedgerMenuMore") + " · " + name)}">${esc(t("fundLedgerMenuMore"))}</summary>
    <div class="pos-action-menu" id="pos-menu-${suffix}" popover="auto" role="menu" aria-label="${esc(t("fundLedgerMenuMore") + " · " + name)}"><p class="pos-menu-name">${esc(name)}</p>
    ${otcGroup}
    ${group("fundLedgerMenuInspect", b("detail", "fundHomeDetails") + b("news", "fundLedgerNews") + b("ai", "fundLedgerAiAnalysis"))}
    ${group("fundLedgerMenuOther", (h.quoteSymbol ? b("watch", watched ? "fundLedgerWatchRemove" : "fundLedgerWatchAdd") : "") + b("external", "fundHomeOpenSource") + b("rules", "fundHomeRules"))}</div></details>`;
}

// 收益投影由账本 API 计算；同格显示金额与百分比，日期/持有天数由独立 renderer 负责。
// 每条账户持仓一张卡片；主指标常驻，四项期间收益和数量/成本/报价可展开。
const CATEGORY_LABEL = { stock: "fundHomeStocks", etf: "fundLedgerTypeEtf", otc: "fundLedgerTypeOtcFund", unknown: "fundLedgerCategoryUnknown" };
function holdingCard(h, weight, expanded) {
  const q = h.quoteSymbol ? quotes[h.quoteSymbol] : null;
  const hiddenAll = displayMode() === "hidden";
  const fixBtn = key => `<button type="button" class="pos-quality-btn" data-pos-act="valuation" data-pos-id="${h.accountId}:${h.fundId}">${esc(t(key))}</button>`;
  const pending = h.pendingCount > 0 ? `<span class="badge down" title="${esc(t("fundLedgerPendingCount").replace("{n}", h.pendingCount))}">${esc(t("fundLedgerStatePending"))}×${h.pendingCount}</span>` : "";
  const priceCell = h.venue === "otc" ? (h.nav ? `${esc(h.nav.nav)}<small>${esc(h.nav.date)}</small>${h.valuationQuality === "stale" ? `<small>${esc(t("fundHomeStaleValuation"))}</small>` : ""}${h.valuationQuality?.startsWith("currency_") ? fixBtn("fundHomeCurrencyNotConfirmed") : ""}` : fixBtn("fundLedgerNavMissing"))
    : (q && q.price != null ? `${esc(q.price)}<small>${esc(q.asOf || "–")}</small>` : `<small>${esc(quoteFailed ? t("fundLedgerQuoteFailed") : "–")}</small>`);
  const category = esc(t(CATEGORY_LABEL[categoryOf(h)] || "fundLedgerCategoryUnknown"));
  const id = h.accountId + ":" + h.fundId;
  const metric = (label, value) => `<div class="pos-metric"><small>${esc(t(label))}</small><b>${value}</b></div>`;
  const income = (key, label) => `<div class="pos-income" data-income="${key}"><small>${esc(t(Fund.positionIncome.label(h,key,label)))}</small>${Fund.positionIncome.inline(h,key)}</div>`;
  const trade = (act, label) => `<button type="button" data-pos-act="${act}" data-pos-id="${id}" ${maskAmounts()?"disabled":""}>${esc(t(label))}</button>`;
  return `<article class="pos-holding-card" data-holding-id="${id}" aria-labelledby="pos-name-${h.accountId}-${h.fundId}">
    <header class="pos-card-head"><div class="pos-card-avatar" aria-hidden="true">${hideIdentity() ? "·" : esc((h.fundName || h.fundCode).slice(0,1))}</div><div class="pos-identity"><h3 id="pos-name-${h.accountId}-${h.fundId}"><button type="button" class="holding-detail-link" id="pos-detail-${h.accountId}-${h.fundId}" data-pos-act="detail" data-pos-id="${id}">${hideIdentity() ? "****" : esc(h.fundName || h.fundCode)}</button></h3><span>${hideIdentity() ? "****" : esc(h.fundCode)}</span></div><span class="pos-venue">${category}</span></header>
    ${h.instrumentKind==='fund'||h.instrumentKind==='etf'?`<div class="fund-theme-tags" data-fund-classification="${hideIdentity()?'':esc(h.fundCode)}" ${hideIdentity()?'hidden':''}></div>`:''}
    <div class="pos-value"><small>${esc(t("fundLedgerMarketValue"))}</small><strong>${money(rowValue(h))}</strong><small>${hiddenAll ? "****" : weight == null ? "–" : weight.toFixed(1) + "% · " + esc(t("fundLedgerWeightAllScope"))}</small></div>
    <div class="pos-card-holding">${income("holding","fundLedgerColHoldPnl")}</div>
    <div class="pos-card-daily">${income("day","fundLedgerColDayPnl")}${income("yesterday","fundLedgerColYesterday")}</div>
    ${pending ? `<div class="pos-card-status">${pending}</div>` : ""}
    <details class="pos-card-details" data-metrics-for="${id}" ${expanded ? "open" : ""}><summary>${esc(t("fundLedgerHoldingDetails"))}</summary><div class="pos-metrics">
    ${metric("fundLedgerColUnits",maskAmounts() ? "****" : esc(h.quantity))}
    ${metric("fundLedgerThCost",money(h.costUnknown ? null : decimalToMinor(h.cost || "0")))}
    ${metric("fundLedgerColCostPrice",maskAmounts() ? "****" : esc(h.averageCost ?? "–"))}
    ${metric("fundLedgerColPrice",hiddenAll ? "****" : priceCell)}
    ${Object.entries({week:"fundLedgerColWeek",month:"fundLedgerColMonth",year:"fundLedgerColYear",cumulative:"fundLedgerColCumulative"}).map(([key,label])=>`<div class="pos-metric" data-income="${key}"><small>${esc(t(label))}</small>${Fund.positionIncome.inline(h,key)}</div>`).join("")}
    ${h.costUnknown ? `<p class="pos-income-gap">${esc(t("fundLedgerCostUnknown"))}</p>` : ""}
    ${h.venue === "otc" && (BigInt(h.otcPendingBuyMinor || "0") > 0n || BigInt(h.otcInTransitMinor || "0") > 0n) ? metric("fundLedgerOtcPendingBadge",[BigInt(h.otcPendingBuyMinor || "0") > 0n ? `${esc(t("fundLedgerOtcPendingBuy"))} ${money(BigInt(h.otcPendingBuyMinor))}` : "",BigInt(h.otcInTransitMinor || "0") > 0n ? `${esc(t("fundLedgerOtcInTransit"))} ${money(BigInt(h.otcInTransitMinor))}` : ""].filter(Boolean).join(" · ")) : ""}
    </div></details>
    <footer class="pos-actions">${trade(h.venue === "otc" ? "otcBuy" : "buy",h.venue === "otc" ? "fundLedgerOtcBuyTitle" : "fundLedgerMenuRecordBuy")}${trade(h.venue === "otc" ? "otcSell" : "sell",h.venue === "otc" ? "fundLedgerOtcSellTitle" : "fundLedgerMenuRecordSell")}${actionCell(h)}</footer>
  </article>`;
}

function accountCard(a, list, weightOf, expanded) {
  const s = summarize(list,a.id), isCollapsed = collapsed.has(a.id), vis = visibleAccounts(), idx = vis.findIndex(x => x.id === a.id);
  const dayPnlText = s.hasDayPnl ? (maskAmounts() ? "****" : (s.dayPnl > 0n ? "+" : "") + formatMinorExact(s.dayPnl)) + (s.missingDayPnl ? " · " + t("fundLedgerIncomePartial",{n:s.missingDayPnl}) : "") : "–";
  const dayPnlClass = s.hasDayPnl && !maskAmounts() ? (s.dayPnl > 0n ? "up" : s.dayPnl < 0n ? "down" : "") : "";
  return `<section class="pos-account" data-account="${a.id}"><header class="pos-account-head" draggable="true" data-drag-account="${a.id}"><button type="button" data-pos-toggle="${a.id}" aria-expanded="${String(!isCollapsed)}">${esc(isCollapsed ? t("fundLedgerExpand") : t("fundLedgerCollapse"))}</button><b class="pos-account-name">${esc(a.name)}</b><span class="pos-account-sum">${esc(t("fundLedgerPositionsCount").replace("{n}", s.count))} · ${esc(t("fundLedgerThCost"))} ${money(s.unknown ? null : s.cost)} · ${esc(t("fundLedgerMarketValue"))} ${money(s.valued ? s.value : null)} · ${esc(t("fundLedgerColDayPnl"))} <b class="${dayPnlClass}">${dayPnlText}</b> · ${esc(t("fundLedgerThRealized"))} ${money(s.realized)}</span><span class="pos-account-move"><button type="button" data-pos-order="up" data-account="${a.id}" ${idx <= 0 ? "disabled" : ""} title="${esc(t("fundHomeMoveUp"))}">↑</button><button type="button" data-pos-order="down" data-account="${a.id}" ${idx >= vis.length - 1 ? "disabled" : ""} title="${esc(t("fundHomeMoveDown"))}">↓</button></span></header>${isCollapsed ? "" : `<div class="pos-card-grid">${list.map(h => holdingCard(h, weightOf(h), expanded.has(h.accountId + ":" + h.fundId))).join("")}<button type="button" class="pos-holding-card pos-add-card" data-pos-add-account="${a.id}"><span class="pos-card-avatar pos-add-plus" aria-hidden="true">+</span><strong>${esc(t("fundLedgerAddHolding"))}</strong><span class="pos-add-description">${esc(t("fundLedgerAddHoldingHint"))}</span><span class="pos-add-action">${esc(t("fundLedgerAddHolding"))}<span aria-hidden="true"> →</span></span></button></div>`}</section>`;
}

function render() {
  const groups = document.getElementById("positions-groups");
  if (!groups || !snap) return;
  const sig = JSON.stringify([tabKind, snap.dataRevision, hideIdentity(), maskAmounts(), [...collapsed].sort(), quoteFailed, Object.entries(quotes).map(([k, q]) => k + (q ? q.price + ":" + q.changePct : "")), (state.watchlistItems || []).map(i => i.symbol).join()]);
  if (sig === _renderSig) return;
  _renderSig = sig;
  const perAcct = sortAccounts(visibleAccounts().map(a => [a, tabHoldings().filter(h => h.accountId === a.id)]).filter(([, l]) => l.length > 0));
  const all = perAcct.flatMap(([, l]) => l);
  const s = summarize(all);
  // 持仓占比 = 单持仓市值 / 全部持仓市值；任一持仓缺市值则整体显式缺位。
  let totValue = 0n, allValued = snap.holdings.length > 0;
  snap.holdings.forEach(h => { const v = rowValue(h); if (v == null) allValued = false; else totValue += v; });
  const weightOf = (h) => {
    if (!allValued) return null;
    const v = rowValue(h);
    return v != null && totValue > 0n ? Number((v * 1000n) / totValue) / 10 : null;
  };
  document.getElementById("pos-total-cost").textContent = money(s.unknown ? null : s.cost);
  document.getElementById("pos-total-value").textContent = money(s.valued && all.length ? s.value : all.length ? null : 0n);
  // DES-P02：分类条标注条目数与已知市值（当前范围）；缺口不冒充完整总额。
  const tabMeta = document.getElementById("pos-tab-meta");
  if (tabMeta) {
    const inScope = tabHoldings();
    const valued = inScope.filter(h => rowValue(h) != null);
    const known = valued.reduce((acc, h) => acc + rowValue(h), 0n);
    const gap = inScope.length - valued.length;
    tabMeta.textContent = hideIdentity() ? "****" : esc(t("fundLedgerPositionsCount").replace("{n}", inScope.length))
      + " · " + esc(t("fundLedgerMarketValue")) + " " + (valued.length ? money(known) : "–")
      + (gap > 0 ? " · " + esc(t("fundLedgerPositionsGap").replace("{n}", gap)) : "");
    tabMeta.hidden = inScope.length === 0;
  }
  document.getElementById("pos-total-realized").textContent = money(s.realized);
  // 今日收益（全持仓今日盈亏合计）
  const totalDayPnlEl = document.getElementById("pos-total-day-pnl");
  const totalDayLabel=document.getElementById('pos-total-day-label');if(totalDayLabel)totalDayLabel.textContent=t(s.estimatedDayCount?'fundHomeEstimatedDailyIncome':'fundLedgerColDayPnl');
  if (totalDayPnlEl) {
    const dayPnlText = s.hasDayPnl ? (maskAmounts() ? "****" : (s.dayPnl > 0n ? "+" : "") + formatMinorExact(s.dayPnl)) + (s.missingDayPnl ? " · " + t("fundLedgerIncomePartial",{n:s.missingDayPnl}) : "") : "–";
    totalDayPnlEl.textContent = dayPnlText;
    totalDayPnlEl.classList.toggle("up", s.hasDayPnl && !maskAmounts() && s.dayPnl > 0n);
    totalDayPnlEl.classList.toggle("down", s.hasDayPnl && !maskAmounts() && s.dayPnl < 0n);
  }
  // 更新时间 = 本次持仓快照到达时刻（非估值业务日期；业务日期见各行 asOf）。
  const updated = document.getElementById("pos-total-updated");
  if (updated) updated.textContent = new Date(Date.now())
    .toLocaleTimeString(state.language === "en" ? "en-US" : "zh-CN", { timeZone: "Asia/Shanghai", hour: "2-digit", minute: "2-digit" });
  // 预计收益 = 市值 − 成本（未实现的浮动盈亏，卖出即转入已实现口径）。
  // 成本未知或任一持仓缺市值时显式缺位，不冒充完整汇总。
  const projected = document.getElementById("pos-total-projected");
  if (projected) {
    const computable = all.length > 0 && s.valued && !s.unknown && s.realized !== null;
    const floating = computable ? s.value - s.cost : null;
    projected.textContent = money(floating);
    projected.classList.toggle("up", floating != null && floating > 0n);
    projected.classList.toggle("down", floating != null && floating < 0n);
  }
  const expanded = new Set([...groups.querySelectorAll(".pos-card-details[open]")].map(el => el.dataset.metricsFor));
  const focused=document.activeElement,focusId=groups.contains(focused)?focused.id:null;
  const menus = Fund.positionMenus.capture();
  Fund.positionMenus.close();
  groups.innerHTML = (perAcct.length
    ? `<p class="pos-order-hint">${esc(t("fundLedgerOrderHint"))}</p>` + perAcct.map(([a, l]) => accountCard(a, l, weightOf, expanded)).join("")
    : `<p>${esc(t("fundLedgerNoPositions"))}</p>`);
  document.getElementById("pos-detailed").checked = all.length > 0 && all.every(h => expanded.has(h.accountId + ":" + h.fundId));
  Fund.positionMenus.bind(menus);
  Fund.fundClassification?.mount(document.getElementById('view-positions'));
  if(focusId&&!focused.isConnected&&!document.getElementById("view-positions").hidden)document.getElementById(focusId)?.focus({preventScroll:true});
  updatePositionsSidebar();
}
actions.renderPositions = render;

function updatePositionsSidebar() {
  const list = snap ? snap.holdings : [];
  let totCost = 0n, totRealized = 0n, unknownCost = false;
  list.forEach(function (p) {
    if (p.costUnknown) unknownCost = true; else totCost += decimalToMinor(p.cost || "0");
    totRealized += decimalToMinor(p.realized || "0");
  });
  const costEl = document.getElementById("side-pos-cost-val");
  const realizedEl = document.getElementById("side-pos-realized-val");
  const countEl = document.getElementById("side-pos-count-val");
  const distEl = document.getElementById("side-pos-dist-list");
  if (costEl) costEl.textContent = money(unknownCost ? null : totCost);
  if (realizedEl) {
    realizedEl.textContent = maskAmounts() ? "****" : (totRealized > 0n ? "+" : "") + formatMinorExact(totRealized);
    realizedEl.className = "num " + (totRealized > 0n ? "up" : totRealized < 0n ? "down" : "");
  }
  if (countEl) countEl.textContent = t("fundLedgerPositionsCount").replace("{n}", list.length);
  if (distEl) {
    if (list.length === 0) {
      distEl.innerHTML = "<div style='color:var(--natives-text-muted); text-align:center; padding:10px 0;'>" + esc(t("fundLedgerNoPositions")) + "</div>";
    } else {
      let html = "";
      list.slice(0, 5).forEach(function (p) {
        const cost = p.costUnknown ? null : decimalToMinor(p.cost || "0");
        const weight = !unknownCost && cost != null && totCost > 0n ? Number((cost * 100n + totCost / 2n) / totCost) : null;
        html += "<div style='margin-bottom:8px;'>" +
          "<div style='display:flex; justify-content:space-between; margin-bottom:2px;'>" +
            "<span><b>" + (hideIdentity() ? "****" : esc(p.fundName || p.fundCode)) + "</b> <span style='font-size:var(--natives-fs-secondary); color:var(--natives-text-muted);'>" + (hideIdentity() ? "****" : esc(p.fundCode)) + "</span></span>" +
            "<span class='num'>" + (hideIdentity() ? "****" : weight == null ? t("fundHomeNeedsVerificationccf920") : weight + "%" + (maskAmounts() ? "" : " (" + formatMinorExact(cost) + ")")) + "</span>" +
          "</div>" +
          "<div style='height:4px; background:var(--natives-surface-3); border-radius:2px; overflow:hidden;'>" +
            "<div style='height:100%; width:" + (hideIdentity() ? 0 : weight ?? 0) + "%; background:var(--natives-accent); border-radius:2px;'></div>" +
          "</div>" +
        "</div>";
      });
      distEl.innerHTML = html;
    }
  }
}
actions.updatePositionsSidebar = updatePositionsSidebar;

// ---------- 账户排序：拖动 + 键盘按钮等价路径 ----------
async function persistOrders(orderedList) {
  const hh = home();
  try {
    await api("POST", "/api/accounts/order", { items: orderedList.map(a => ({ id: a.id, expectedVersion: a.version })), requestId: hh.requestId() });
  } catch (e) {
    hh.status(e.message); // 409/网络：放弃本次本地序，重读权威快照
  }
  await loadPositions();
}
function moveByIndex(list, from, to) {
  if (to < 0 || to >= list.length) return list;
  const arr = list.slice();
  arr.splice(to, 0, ...arr.splice(from, 1));
  return arr;
}
document.addEventListener("click", async function (e) {
  const hh = home();
  const toggle = e.target.closest("[data-pos-toggle]");
  if (toggle) {
    Fund.positionMenus.close();
    const id = Number(toggle.dataset.posToggle);
    if (collapsed.has(id)) collapsed.delete(id); else collapsed.add(id);
    _renderSig = ""; render();
    return;
  }
  const order = e.target.closest("[data-pos-order]");
  if (!order || hh.sensitive()) return;
  const vis = visibleAccounts();
  const i = vis.findIndex(a => a.id === Number(order.dataset.account));
  const j = order.dataset.posOrder === "up" ? i - 1 : i + 1;
  if (i < 0 || j < 0 || j >= vis.length) return;
  await persistOrders(moveByIndex(vis, i, j));
});
document.getElementById("positions-groups").addEventListener("dragstart", function (e) {
  const head = e.target.closest("[data-drag-account]");
  if (!head || home().sensitive()) return;
  dragId = Number(head.dataset.dragAccount);
  e.dataTransfer.effectAllowed = "move";
});
document.getElementById("positions-groups").addEventListener("dragover", function (e) {
  if (dragId != null && e.target.closest("[data-drag-account]")) e.preventDefault();
});
document.getElementById("positions-groups").addEventListener("drop", async function (e) {
  const head = e.target.closest("[data-drag-account]");
  if (!head || dragId == null) return;
  e.preventDefault();
  const vis = visibleAccounts();
  const i = vis.findIndex(a => a.id === dragId);
  const j = vis.findIndex(a => a.id === Number(head.dataset.dragAccount));
  dragId = null;
  if (i < 0 || j < 0 || i === j) return;
  await persistOrders(moveByIndex(vis, i, j));
});

// ---------- 交易表单（买入/卖出） ----------
function txDialog(holding, type) {
  const hh = home(); if (hh.sensitive()) return;
  const key = hh.requestId();
  const otc = holding.venue === "otc";
  hh.panel(
    () => (type === "BUY" ? t("fundLedgerTypeBuy") : t("fundLedgerTypeSell")) + " · " + (hideIdentity() ? "****" : (holding.fundName || holding.fundCode)),
    () => `<form>
      ${hh.input("quantity", t("fundLedgerLabelQuantity"), type === "SELL" ? holding.quantity : "", "number", holding.instrumentKind === "stock" ? 'required min="1" step="1"' : 'required min="0.0001" step="0.0001"')}
      ${hh.input("price", t("fundLedgerLabelPrice"), "", "number", 'required min="0" step="0.0001"')}
      ${hh.input("fee", t("fundLedgerLabelFee"), "0.00", "number", 'required min="0" step="0.01"')}
      ${hh.input("tradeDate", t("fundLedgerLabelDate"), today(), "date", `required max="${today()}"`)}
      ${otc ? hh.select("state", t("fundLedgerThStatus"), [["pending", t("fundLedgerStatePending")], ["confirmed", t("fundLedgerStateConfirmed")]], "pending") + `<p>${esc(t("fundLedgerOtcConfirmHint"))}</p>` : ""}
      <p>${esc(t("fundLedgerThAccount"))}：${hideIdentity() ? "****" : esc(accountName(holding.accountId))}</p>
      <button type="submit">${t("fundHomeConfirm")}</button></form>`,
    root => hh.submit(root, async v => {
      await api("POST", "/api/transactions", {
        account: accountName(holding.accountId),
        fundCode: holding.fundCode, instrumentKind: holding.instrumentKind, market: holding.market,
        fundName: holding.fundName, type, state: v.state || "confirmed",
        quantity: v.quantity, price: v.price, fee: v.fee, tradeDate: v.tradeDate,
        source: "manual", requestId: key,
      });
      hh.closePanel(); await loadPositions();
    }),
  );
}

// ---------- 转移（草稿保留：预览与提交共用 draft） ----------
function transferDialog(holding) {
  const hh = home(); if (hh.sensitive()) return;
  const key = hh.requestId();
  const targets = visibleAccounts().filter(a => a.id !== holding.accountId);
  const draft = { toId: targets[0] ? targets[0].id : null, quantity: holding.quantity, preview: null };
  const previewHtml = () => {
    const p = draft.preview;
    if (!p) return "";
    const side = (label, o) => `<p>${esc(label)}：${esc(t("fundLedgerThQuantity"))} ${esc(o.quantity)} · ${esc(t("fundLedgerThCost"))} ${money(decimalToMinor(o.cost))}</p>`;
    return side(t("fundLedgerTransferFrom"), p.from) + side(t("fundLedgerTransferTo"), p.to) +
      `<p>${esc(t("fundLedgerTransferredCost"))} ${money(decimalToMinor(p.transferredCost))}</p>` +
      `<p>${esc(t("fundLedgerTransferCashNeutral"))} · ${esc(t("fundLedgerTransferFeePolicy"))}</p>`;
  };
  hh.panel(
    () => t("fundLedgerTransfer") + " · " + (hideIdentity() ? "****" : (holding.fundName || holding.fundCode)),
    () => `<form>
      ${hh.select("toId", t("fundLedgerTransferTo"), targets.map(a => [String(a.id), hideIdentity() ? "****" : a.name]), draft.toId == null ? "" : String(draft.toId))}
      ${hh.input("quantity", t("fundLedgerLabelQuantity"), draft.quantity, "number", 'required min="0.0001" step="0.0001"')}
      <button type="submit">${t("fundLedgerTransferPreview")}</button></form>
      <div data-preview role="status">${previewHtml()}</div>
      <button type="button" data-save ${draft.preview ? "" : "disabled"}>${t("fundHomeConfirm")}</button>`,
    root => {
      const show = () => {
        root.querySelector("[data-preview]").innerHTML = previewHtml();
        root.querySelector("[data-save]").disabled = !draft.preview;
      };
      root.querySelector("[name=toId]").onchange = e => { draft.toId = Number(e.target.value); draft.preview = null; show(); };
      root.querySelector("[name=quantity]").oninput = e => { draft.quantity = e.target.value; draft.preview = null; show(); };
      hh.submit(root, async v => {
        try {
          draft.toId = Number(v.toId); draft.quantity = v.quantity;
          draft.preview = await api("POST", "/api/positions/transfer/preview", {
            fromId: holding.accountId, toId: draft.toId, securityId: holding.fundId,
            quantity: draft.quantity, requestId: key + "-pv",
          });
          show();
        } catch (e) { draft.preview = null; show(); hh.failure(e); }
      });
      root.querySelector("[data-save]").onclick = async e => {
        if (!draft.preview) return;
        e.target.disabled = true;
        try {
          await api("POST", "/api/positions/transfer", {
            fromId: holding.accountId, toId: draft.toId, securityId: holding.fundId,
            quantity: draft.quantity, requestId: key, expectedRevision: draft.preview.dataRevision,
          });
          hh.closePanel(); await loadPositions();
        } catch (err) {
          // 409/幂等失败：草稿与预览保留，重新预览即可重试（A21）。
          hh.failure(new Error(err.message + " · " + t("fundLedgerConflictRetry")));
          e.target.disabled = false;
        }
      };
    },
  );
}

// ---------- 添加持仓（搜索身份 → 类型/登记方式；A20） ----------
const SUGGEST_MARKET = { sh: "fundLedgerMarketSse", sz: "fundLedgerMarketSzse", bj: "fundLedgerMarketBse", jj: "fundLedgerSuggestOtc" };
function addHoldingDialog(seed) {
  const hh = home(); if (hh.sensitive()) return;
  if (seed && seed.code !== undefined) { addHoldingForm(seed); return; }
  let results = [];
  const itemsHtml = () => results.map((r, i) => `<button type="button" data-pick="${i}">[${esc(t(SUGGEST_MARKET[r.market] || "fundHomeUnknown"))}] ${esc(r.code)} ${esc(r.name)}</button>`).join("");
  hh.panel(
    () => t("fundLedgerAddHolding"),
    () => `<form data-search>
      ${hh.input("q", t("fundLedgerSearchHint"), "", "search", 'required maxlength="40"')}
      <button type="submit">${t("fundLedgerSearch")}</button></form>
      <p data-err role="status" class="err"></p>
      <div data-results>${itemsHtml()}</div>
      <button type="button" data-manual>${t("fundLedgerManualEntry")}</button>`,
    root => {
      const bindPicks = () => root.querySelectorAll("[data-pick]").forEach(b => b.onclick = () => { hh.closePanel(); addHoldingDialog(results[Number(b.dataset.pick)]); });
      bindPicks();
      root.querySelector("[data-manual]").onclick = () => { hh.closePanel(); addHoldingDialog({ code: "", name: "", market: "sh" }); };
      hh.submit(root, async v => {
        const err = root.querySelector("[data-err]");
        err.textContent = "";
        try {
          const res = await api("GET", "/api/market/suggest?input=" + encodeURIComponent(v.q.trim()));
          results = res.suggestions || [];
          err.textContent = results.length ? "" : t("fundLedgerNoResults");
          root.querySelector("[data-results]").innerHTML = itemsHtml();
          bindPicks();
        } catch (e) { err.textContent = e.message; }
      });
    },
  );
}
function addHoldingForm(p) {
  const hh = home();
  const guessKind = p.market === "jj" ? "fund" : /^(5\d{5}|159\d{3})$/.test(p.code || "") ? "etf" : tabKind === "otc" && !p.code ? "fund" : tabKind === "etf" ? "etf" : "stock";
  const guessMarket = { sh: "SSE", sz: "SZSE", bj: "BSE" }[p.market] || "SSE";
  const requestId = hh.requestId();
  hh.panel(
    () => t("fundLedgerAddHolding") + (p.code ? " · " + (hideIdentity() ? "****" : esc(p.code) + " " + esc(p.name)) : ""),
    () => `<form>
      ${hh.select("account", t("fundLedgerThAccount"), visibleAccounts().map(a => [a.name, hideIdentity() ? "****" : a.name]), visibleAccounts()[0] ? visibleAccounts()[0].name : "")}
      ${hh.select("instrumentKind", t("fundHomeType"), [["stock", t("fundHomeStocks")], ["etf", t("fundLedgerTypeEtf")], ["fund", t("fundHomeFunds")]], guessKind)}
      <span data-market-row>${hh.select("market", t("fundHomeMarket"), [["SSE", "SSE"], ["SZSE", "SZSE"], ["BSE", "BSE"]], guessMarket)}</span>
      ${hh.input("fundCode", t("fundLedgerLabelCode"), p.code || "", "text", 'required pattern="[0-9]{6}" maxlength="6"')}
      ${hh.input("fundName", t("fundHomeName"), p.name || "", "text", 'maxlength="64"')}
      ${hh.select("method", t("fundLedgerAddMethod"), [["opening", t("fundHomeOpeningHolding")], ["buy", t("fundLedgerTypeBuy")], ["market", t("fundLedgerOtcAddMethod")]], "opening")}
      ${hh.input("quantity", t("fundLedgerLabelQuantity"), "", "number", 'required min="0.0001" step="0.0001"')}
      <span data-cost-row>${hh.input("totalCost", t("fundHomeCostOptional"), "", "number", 'min="0" step="0.01"')}</span>
      <span data-buy-row hidden>${hh.input("price", t("fundLedgerLabelPrice"), "", "number", 'min="0" step="0.0001"')}${hh.input("fee", t("fundLedgerLabelFee"), "0.00", "number", 'min="0" step="0.01" required')}${hh.select("state", t("fundLedgerThStatus"), [["pending", t("fundLedgerStatePending")], ["confirmed", t("fundLedgerStateConfirmed")]], "confirmed")}</span>
      ${hh.input("asOf", t("fundHomeActualEventDate"), today(), "date", `required max="${today()}"`)}
      <p>${esc(t("fundLedgerNoMarketNote"))} ${esc(t("fundLedgerQuantityPrecisionHint"))}</p>
      <button type="submit">${t("fundHomeSave")}</button></form>`,
    root => {
      const syncRows = () => {
        const kind = root.querySelector("[name=instrumentKind]").value;
        const buy = root.querySelector("[name=method]").value === "buy";
        const quantity = root.querySelector("[name=quantity]");
        quantity.min = kind === "stock" ? "1" : "0.0001";
        quantity.step = kind === "stock" ? "1" : "0.0001";
        const market = root.querySelector("[name=method]").value === "market";
        root.querySelector("[data-market-row]").hidden = kind === "fund";
        root.querySelector("[data-cost-row]").hidden = buy || market;
        root.querySelector("[data-buy-row]").hidden = !buy;
        quantity.required = !buy && !market;
        const st = root.querySelector("[data-buy-row] [name=state]");
        if (st) st.value = kind === "fund" ? "pending" : "confirmed";
      };
      root.querySelectorAll("[name=instrumentKind],[name=method]").forEach(n => n.onchange = syncRows);
      syncRows();
      hh.submit(root, async v => {
        const base = {
          account: v.account, fundCode: v.fundCode, fundName: v.fundName,
          instrumentKind: v.instrumentKind, market: v.instrumentKind === "fund" ? "CN" : v.market,
        };
        if (v.method === "market") {
          // N09：场外按市值登记（金额+收益 → 后端净值推导）。
          hh.closePanel();
          Fund.otcOps?.holdingNew({ account: v.account, fundCode: v.fundCode, fundName: v.fundName });
          return;
        }
        if (v.method === "opening") {
          const body = { ...base, quantity: v.quantity, asOf: v.asOf };
          if (v.totalCost) body.totalCost = v.totalCost;
          await api("POST", "/api/openings", body);
        } else {
          await api("POST", "/api/transactions", { ...base, type: "BUY", state: v.state, quantity: v.quantity, price: v.price, fee: v.fee || "0.00", tradeDate: v.asOf, source: "manual", requestId });
        }
        hh.closePanel(); await loadPositions();
      });
    },
  );
}

// ---------- 账户管理（改名/归档/恢复/新建） ----------
async function accountsDialog(offset = 0) {
  const hh = home(); if (hh.sensitive()) return;
  let data;
  try { data = await api("GET", "/api/accounts/detail?view=all&limit=50&offset=" + offset); } catch (e) { hh.status(e.message); return; }
  const rows = () => (data.items || []).map(a => `<article class="pos-acct-row${a.archived ? " is-archived" : ""}">
    <div class="pos-acct-id"><b>${esc(a.name)}</b>
      <span class="pos-acct-badges">${a.archived ? `<span class="otc-badge otc-badge-paused">${esc(t("fundHomeArchived"))}</span>` : ""}${a.cash.quality === "unbooked" ? `<span class="otc-badge">${esc(t("fundLedgerCashUnbooked"))}</span>` : ""}</span>
    </div>
    <div class="pos-acct-cash"><small>${esc(t("fundLedgerThCash"))}</small><b class="num">${money(a.cash.quality === "recorded" ? a.cash.amount : null)}</b></div>
    <div class="pos-acct-ops">
      <button type="button" data-rename="${a.id}">${esc(t("fundHomeEdit"))}</button>
      ${a.archived
        ? `<button type="button" data-toggle-archive="${a.id}" data-v="${a.version}" data-op="restore">${esc(t("fundHomeRestore"))}</button>`
        : `<button type="button" class="pos-acct-danger" data-toggle-archive="${a.id}" data-v="${a.version}" data-op="archive">${esc(t("fundHomeArchive"))}</button>`}
    </div>
  </article>`).join("");
  hh.panel(
    () => t("fundLedgerAccountManage"),
    () => `<p class="pos-acct-head-note">${esc(t("fundLedgerAccountsCount").replace("{n}", data.total ?? (data.items || []).length))}</p>
      <div class="pos-acct-list">${rows() || `<p class="otc-empty">${esc(t("fundLedgerOtcOrdersEmpty"))}</p>`}</div>
      <button type="button" class="btn btn-accent pos-acct-create" data-create>${t("fundLedgerNewAccount")}</button>
      <div class="pos-acct-pager"><button type="button" data-account-prev ${offset === 0 ? "disabled" : ""}>${t("fundHomePrevious")}</button><button type="button" data-account-next ${offset + 50 >= data.total ? "disabled" : ""}>${t("fundHomeNext")}</button></div>`,
    root => {
      root.querySelector("[data-account-prev]").onclick = () => accountsDialog(Math.max(0, offset - 50));
      root.querySelector("[data-account-next]").onclick = () => accountsDialog(offset + 50);
      root.querySelectorAll("[data-rename]").forEach(b => b.onclick = () => {
        const a = data.items.find(x => x.id === Number(b.dataset.rename));
        hh.panel(() => t("fundHomeEdit"), () => `<form>${hh.input("name", t("fundHomeName"), a.name, "text", 'required maxlength="64"')}<button type="submit">${t("fundHomeSave")}</button></form>`,
          r2 => hh.submit(r2, async v => {
            await api("POST", "/api/accounts/update", { id: a.id, name: v.name, expectedVersion: a.version, requestId: hh.requestId() });
            hh.closePanel(); await loadPositions();
          }));
      });
      root.querySelectorAll("[data-toggle-archive]").forEach(b => b.onclick = async () => {
        const archive = b.dataset.op === "archive";
        try {
          if (!await hh.confirm(archive ? t("fundLedgerArchiveConfirm") : t("fundLedgerRestoreConfirm"))) return;
          await api("POST", archive ? "/api/accounts/archive" : "/api/accounts/restore", { id: Number(b.dataset.toggleArchive), expectedVersion: Number(b.dataset.v), requestId: hh.requestId() });
          hh.closePanel(); await loadPositions();
        } catch (e) { hh.failure(e); }
      });
      root.querySelector("[data-create]").onclick = () => {
        hh.panel(() => t("fundLedgerNewAccount"), () => `<form>${hh.input("name", t("fundHomeName"), "", "text", 'required maxlength="64"')}<button type="submit">${t("fundHomeSave")}</button></form>`,
          r2 => hh.submit(r2, async v => {
            await api("POST", "/api/accounts", { name: v.name });
            hh.closePanel(); await loadPositions();
          }));
      };
    },
  );
}
actions.manageAccounts=accountsDialog;

// ---------- 资讯 / 能力矩阵 ----------
async function newsDialog(holding) {
  const hh = home(); if (hh.sensitive()) return;
  const ref = (holding.venue === "otc" ? "security:CN:fund:" : "security:CN:stock:") + holding.fundCode;
  try {
    const res = await api("GET", "/api/information/items?scope=holdings&asset=" + encodeURIComponent(ref));
    const items = res.items || [];
    hh.panel(
      () => t("fundLedgerNews") + " · " + (hideIdentity() ? "****" : (holding.fundName || holding.fundCode)),
      () => `<p class="pos-info-coverage">${esc(res.coverage || "")}</p>` + (items.length
        ? items.slice(0, 30).map(i => `<article><button type="button" data-info="${i.id}">${esc(i.title)}</button><small>${esc(i.publishedAt ? new Date(Number(i.publishedAt) * 1000).toLocaleString(state.language) : "–")}</small></article>`).join("")
        : `<p>${esc(t("fundLedgerNoNews"))}</p>`),
      root => root.querySelectorAll("[data-info]").forEach(b => b.onclick = () => { hh.closePanel(); hh.openInformationDetail(Number(b.dataset.info)); }),
      true,
    );
  } catch (e) { hh.status(e.message); }
}

function holdingDetail(holding) {
  const hh = home();
  const fields = [["fundLedgerThCode", holding.fundCode], ["fundLedgerThAccount", accountName(holding.accountId)],
    ["fundLedgerThQuantity", holding.quantity], ["fundLedgerThCost", money(holding.costUnknown ? null : decimalToMinor(holding.cost))],
    ["fundLedgerThUnitNav", holding.nav?.nav || "–"], ["fundLedgerNavDate", holding.nav?.date || "–"],
    ["fundLedgerMarketValue", money(rowValue(holding))], ["fundLedgerThAsOf", holding.asOf]];
  hh.panel(() => t("fundHomeDetails"), () => `<dl>${fields.map(([key, value]) => `<dt>${esc(t(key))}</dt><dd>${esc(value)}</dd>`).join("")}</dl>`, null, true);
}

function capabilityDialog() {
  const hh = home();
  const ok = t("fundHomeAvailable"), no = t("fundHomeDisabled"), partial = t("fundLedgerCapPartial");
  const row = (key, status, noteKey) => `<tr><td>${esc(t(key))}</td><td>${esc(status)}</td><td>${esc(t(noteKey))}</td></tr>`;
  hh.panel(
    () => t("fundLedgerCapabilityMatrix"),
    () => `<table class="data-table"><thead><tr><th>${esc(t("fundHomeType"))}</th><th>${esc(t("fundLedgerStatus"))}</th><th>${esc(t("fundLedgerCapNote"))}</th></tr></thead><tbody>` +
      row("fundLedgerCapAStock", ok, "fundLedgerCapAStockNote") +
      row("fundLedgerCapEtf", ok, "fundLedgerCapEtfNote") +
      row("fundLedgerCapOtcFund", ok, "fundLedgerCapOtcFundNote") +
      row("fundLedgerCapHkUs", partial, "fundLedgerCapHkUsNote") +
      row("fundLedgerCapFx", no, "fundLedgerCapFxNote") +
      `</tbody></table><p>${esc(t("fundLedgerCapFootnote"))}</p>`,
    null, true,
  );
}

// ---------- 行操作分发 ----------
document.getElementById("positions-groups").addEventListener("click", async function (e) {
  const hh = home();
  const btn = e.target.closest("[data-pos-act]");
  if (!btn) return;
  if (btn.closest(".pos-more")) Fund.positionMenus.close(true);
  if (hh.sensitive()) return;
  const [accId, fundId] = btn.dataset.posId.split(":").map(Number);
  const holding = snap && snap.holdings.find(h => h.accountId === accId && h.fundId === fundId);
  if (!holding) return;
  const withName = { ...holding, account: accountName(holding.accountId), dataRevision: snap.dataRevision };
  switch (btn.dataset.posAct) {
    case "detail":
      if (holding.instrumentKind === "stock") {
        if (holding.quoteSymbol && actions.openStockDetail) actions.openStockDetail(holding.quoteSymbol, holding.fundName,holding.instrumentKind,withName);
        else holdingDetail(holding);
      } else {
        Fund.openFundDetail?.(withName);
      }
      break;
    case "edit": actions.openHoldingEditor({...withName,returnFocusId:`pos-edit-${accId}-${fundId}`}); break;
    case "otcBuy": Fund.otcOps?.buy(withName); break;
    case "otcSell": Fund.otcOps?.sell(withName); break;
    case "otcConvert": Fund.otcOps?.convert(withName); break;
    case "otcPlans": Fund.otcOps?.plans(withName); break;
    case "otcEdit": Fund.otcOps?.holdingEdit(withName); break;
    case "buy": txDialog(holding, "BUY"); break;
    case "sell": txDialog(holding, "SELL"); break;
    case "transfer": transferDialog(holding); break;
    case "opening": if (actions.openOpeningDetail) actions.openOpeningDetail(withName); break;
    case "valuation": if (actions.openValuationDialog) actions.openValuationDialog(withName); break;
    case "external": if(hh.openAssetLink)hh.openAssetLink(holding).catch(hh.failure);break;
    case "news": newsDialog(holding); break;
    case "ai": if (hh.openAI) hh.openAI({ asset: `fund:${holding.accountId}:${holding.fundId}` }); break;
    case "rules": if (hh.manageRules) hh.manageRules(); break;
    case "watch": {
      try {
        await actions.setWatchlistMember({symbol:holding.quoteSymbol,name:holding.fundName},!state.watchlistItems.some(i=>i.symbol===holding.quoteSymbol));
        _renderSig = ""; render();
      } catch (err) { hh.status(err.message); }
      break;
    }
  }
});

// ---------- 顶栏绑定 ----------
function switchTab(kind) {
  Fund.positionMenus.close();
  if (!ledgerLoading) cancelRequests();
  tabKind = kind;
  POS_TABS.forEach(k => {
    const btn = document.getElementById("pos-tab-" + k);
    btn.setAttribute("aria-selected", String(k === kind));
    btn.tabIndex = k === kind ? 0 : -1;
  });
  document.getElementById("positions-groups").setAttribute("aria-labelledby", "pos-tab-" + kind);
  quotes = {}; // 切 Tab 清空行情缓存：杜绝旧行情残影（A19）
  _renderSig = "";
  render();
  if (!ledgerLoading) fetchQuotes();
}
POS_TABS.forEach(k => document.getElementById("pos-tab-" + k).addEventListener("click", () => switchTab(k)));
document.querySelector(".pos-tabs").addEventListener("keydown", e => {
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return;
  e.preventDefault();
  const idx = POS_TABS.indexOf(tabKind);
  const next = e.key === "Home" ? 0 : e.key === "End" ? POS_TABS.length - 1
    : e.key === "ArrowLeft" ? (idx - 1 + POS_TABS.length) % POS_TABS.length
    : (idx + 1) % POS_TABS.length;
  switchTab(POS_TABS[next]); document.getElementById("pos-tab-" + POS_TABS[next]).focus();
});
document.getElementById("holding-add").addEventListener("click", () => addHoldingDialog());
document.getElementById("holding-history").addEventListener("click", () => actions.openOpeningList());
document.getElementById("pos-account-sort").addEventListener("change", (e) => { accountSort = e.target.value; _renderSig = ""; render(); });
document.getElementById("positions-groups").addEventListener("click", (e) => {
  const add = e.target.closest("[data-pos-add-account]");
  if (!add) return;
  Fund.positionMenus.close(true);
  addHoldingDialog();
});
document.getElementById("pos-accounts").addEventListener("click", () => accountsDialog());
document.getElementById("pos-capability").addEventListener("click", capabilityDialog);
document.getElementById("pos-refresh").addEventListener("click", () => { retryNav=true; loadPositions(); });
document.getElementById("pos-detailed").onchange = e => { Fund.positionMenus.close(); document.querySelectorAll(".pos-card-details").forEach(detail => { detail.open = e.target.checked; }); };

// 显示开关：开＝显示数字（amount），关＝星号（hidden）。经 home.applyDisplayMode
// 单一 owner 持久化并广播，percent 态仍从首页三态入口进入。
// 本文件在 bundle 中先于 home.js 执行：home 尚未注册时按遮蔽态渲染，
// 偏好加载后的 fund:privacy-changed 事件会再次同步。
function syncDisplayToggle() {
  const btn = document.getElementById("pos-display-toggle");
  if (!btn) return;
  const h = home();
  const amount = !!(h && typeof h.displayMode === "function" && h.displayMode() === "amount");
  btn.textContent = t(amount ? "fundHomeDisplayHidden" : "fundHomeDisplayAmount");
  btn.setAttribute("aria-checked", String(amount));
  btn.classList.toggle("pos-display-on", amount);
}
document.getElementById("pos-display-toggle").addEventListener("click", () => {
  const h = home();
  if (!h || !h.applyDisplayMode) return;
  h.applyDisplayMode(h.displayMode() === "amount" ? "hidden" : "amount");
});
syncDisplayToggle();

// 隐私三态/语言切换：仅按缓存重绘（不重取数据、不重置 Tab/折叠/草稿；R-I5）
document.addEventListener("fund:privacy-changed", () => { Fund.positionMenus.close(); _renderSig = ""; render(); syncDisplayToggle(); });
document.addEventListener("fund:language-changed", () => { _renderSig = ""; render(); syncDisplayToggle(); });

actions.openTxDialog = (h, type) => txDialog(h, type);


actions.openHoldingMore = function(holding, anchorEl) {
  if (!holding) return;
  const hh = home();
  if (hh.sensitive()) return;
  const withName = { ...holding, account: holding.account || accountName(holding.accountId), dataRevision: holding.dataRevision || snap?.dataRevision };
  const suffix = holding.accountId + "-" + holding.fundId;
  const name = hideIdentity() ? "****" : (holding.fundName || holding.fundCode);
  const otc = holding.venue === "otc";

  hh.panel(
    () => t("fundLedgerMenuMore") + " · " + name,
    () => `<div class="pos-dialog-actions" style="display:flex;flex-direction:column;gap:8px;padding:8px 0">
      <button type="button" class="btn" data-act="edit">${esc(t("fundHomeEdit"))}</button>
      ${otc ? `
        <button type="button" class="btn" data-act="otcConvert">${esc(t("fundLedgerOtcConvertTitle"))}</button>
        <button type="button" class="btn" data-act="otcPlans">${esc(t("fundLedgerOtcPlansTitle"))}</button>
        <button type="button" class="btn" data-act="otcEdit">${esc(t("fundLedgerOtcEditTitle"))}</button>
      ` : `
        <button type="button" class="btn" data-act="transfer">${esc(t("fundLedgerTransfer"))}</button>
        <button type="button" class="btn" data-act="opening">${esc(t("fundHomeOpeningHistory"))}</button>
        <button type="button" class="btn" data-act="valuation">${esc(t("fundHomeValuation"))}</button>
      `}
      <button type="button" class="btn" data-act="news">${esc(t("fundLedgerNews"))}</button>
      <button type="button" class="btn" data-act="ai">${esc(t("fundLedgerAiAnalysis"))}</button>
      ${holding.quoteSymbol ? `<button type="button" class="btn" data-act="watch">${esc(t("fundTrendsUi4"))}</button>` : ""}
    </div>`,
    root => {
      root.querySelector("[data-act=edit]")?.addEventListener("click", () => {
        hh.closePanel();
        actions.openHoldingEditor?.({ ...withName, returnFocusId: anchorEl?.id || `pos-edit-${suffix}` });
      });
      root.querySelector("[data-act=otcConvert]")?.addEventListener("click", () => {
        hh.closePanel();
        Fund.otcOps?.convert(withName);
      });
      root.querySelector("[data-act=otcPlans]")?.addEventListener("click", () => {
        hh.closePanel();
        Fund.otcOps?.plans(withName);
      });
      root.querySelector("[data-act=otcEdit]")?.addEventListener("click", () => {
        hh.closePanel();
        Fund.otcOps?.holdingEdit(withName);
      });
      root.querySelector("[data-act=transfer]")?.addEventListener("click", () => {
        hh.closePanel();
        transferDialog(holding);
      });
      root.querySelector("[data-act=opening]")?.addEventListener("click", () => {
        hh.closePanel();
        actions.openOpeningDetail?.(withName);
      });
      root.querySelector("[data-act=valuation]")?.addEventListener("click", () => {
        hh.closePanel();
        actions.openValuationDialog?.(withName);
      });
      root.querySelector("[data-act=news]")?.addEventListener("click", () => {
        hh.closePanel();
        newsDialog(holding);
      });
      root.querySelector("[data-act=ai]")?.addEventListener("click", () => {
        hh.closePanel();
        hh.openAI?.({ asset: `fund:${holding.accountId}:${holding.fundId}` });
      });
      root.querySelector("[data-act=watch]")?.addEventListener("click", async () => {
        try {
          await actions.setWatchlistMember({symbol:holding.quoteSymbol,name:holding.fundName},!state.watchlistItems.some(item=>item.symbol===holding.quoteSymbol));
          hh.closePanel();
        } catch (error) { hh.status(error.message); }
      });
    },
    true
  );
};

})();
