// Fund R2 首页：快照、目标、概览、市场与隐私的页面 owner。
// 管理弹窗与 AI 分别由 home-manage.js / home-evidence.js 拥有。
(() => {
const { state, actions, authFetch, api, t, formatMinorExact } = globalThis.Fund;
const home = (globalThis.Fund.home = globalThis.Fund.home || {});
const H = { snapshot: null, range: "month", from: "", to: "", privacy: true, detailOpen: null, marketTimer: null };

function fmtMinor(minor) {
  if (minor == null) return null;
  return formatMinorExact(minor);
}
function fmtMinorFull(minor) {
  if (minor == null) return "–";
  return formatMinorExact(minor);
}
function mask(text) {
  // 三态（方案 11 §5）：percent/hidden 均隐藏金额；仅 hidden 才遮名称等私密文字。
  return hideAmounts() ? "****" : text;
}
// percent 与 hidden 都不显示金额或可反推金额的组合。
function hideAmounts() { return H.displayMode !== "amount"; }
// hidden 独有：遮分组名称、账户名及可反推数据的图形与辅助文案。
function hidePrivate() { return H.displayMode === "hidden"; }
function setStatus(text) {
  H.snapshotFailure = null;
  const bar = document.getElementById("home-status-bar");
  if (!bar) return;
  if (!text) { bar.style.display = "none"; bar.textContent = ""; return; }
  bar.style.display = "block";
  bar.textContent = text;
}

// ---------- 快照加载 ----------
let snapshotGen = 0; // R06：区间快速切换时丢弃迟到响应
let snapshotController = null;
async function loadSnapshot(manual = false, valuationOnly = false, refreshEstimates = false) {
  if (document.hidden) return;
  const rangeStatus=document.getElementById("home-range-status");
  if(rangeStatus)rangeStatus.textContent=t("fundHomeRangeLoading");
  const asof = document.getElementById("home-asof");
  const gen = ++snapshotGen;
  snapshotController?.abort();
  const controller = new AbortController();
  snapshotController = controller;
  try {
    const query = new URLSearchParams({ range: H.range });
    if(manual||refreshEstimates)query.set("refreshEstimates","1");
    if (H.range === "custom") { query.set("from", H.from); query.set("to", H.to); }
    const request=()=>api(valuationOnly?"GET":"POST", `/api/home/snapshot${valuationOnly?"":"/generate"}?${query}`, valuationOnly?undefined:{}, {signal:controller.signal});
    const publish=data=>{
      const focus=document.activeElement;
      const attributes=focus?[...focus.attributes].filter(a=>a.name==='id'||a.name==='name'||(a.name.startsWith('data-')&&a.name!=='data-i18n')):[];
      const selector=attributes.length?focus.tagName.toLowerCase()+attributes.map(a=>`[${CSS.escape(a.name)}="${CSS.escape(a.value)}"]`).join(''):null;
      if(data.financialDisplay&&Number(data.financialDisplay.version)>=Number(state.financialSettingsVersion||0)){
        const intervalChanged=state.quoteRefreshSeconds!==data.financialDisplay.quoteRefreshSeconds;
        state.benchmarkSymbol=data.financialDisplay.benchmarkSymbol;state.quoteRefreshSeconds=data.financialDisplay.quoteRefreshSeconds;state.financialSettingsVersion=data.financialDisplay.version;
        if(intervalChanged)Fund.marketSession?.reschedule();
      }
      H.snapshot=freezeSnapshot(data);
      Fund.homeRange?.update(data.requestedRange);
      renderSnapshot();
      document.dispatchEvent(new Event('fund:home-snapshot-updated'));
      if(valuationOnly&&selector&&!focus.isConnected)(document.querySelector(selector)||document.getElementById("home-refresh-btn"))?.focus();
    };
    let data=await request();
    if(gen!==snapshotGen||controller.signal.aborted)return;
    if(data.error)throw new Error(t("fundHomeRequestFailed"));
    publish(data);
    const nav=await Fund.syncHoldingNav(data.portfolio?.ledger?.navs||[],controller.signal,manual);
    if(gen!==snapshotGen||controller.signal.aborted)return;
    if(nav.changed){data=await request();if(gen!==snapshotGen||controller.signal.aborted)return;publish(data);}
    setStatus(nav.failed?t("fundLedgerNavSyncFailed"):"");
    loadBenchmark(data);
    return H.snapshot;

  } catch (e) {
    if (gen !== snapshotGen || controller.signal.aborted) return;
    setStatus(`${t("fundHomeHomeSnapshotCouldNotLoad")}${Fund.localizeApiError(e.status || 500, e.code, "")}`);
    H.snapshotFailure = {status:e.status || 500,code:e.code};
  } finally {
    if (snapshotController === controller) snapshotController = null;
    if (gen === snapshotGen && asof) {
      const stamp = H.snapshot && H.snapshot.generatedAt
        ? new Date(Number(H.snapshot.generatedAt) * 1000).toLocaleTimeString(state.language, { timeZone: "Asia/Shanghai" })
        : "";
      asof.textContent = stamp ? `${t("fundHomeDataAsOf")}${stamp}` : "–";
      if(H.snapshotFailure)Fund.homeRange?.update(H.snapshot?.requestedRange);
      if(rangeStatus)rangeStatus.textContent=H.snapshotFailure?t("fundHomeRangeFailed"):"";
    }
  }
}

// ---------- 渲染 ----------
function renderSnapshot() {
  const s = H.snapshot;
  if (!s) return;
  renderWealth(s.wealth);
  home.renderOverview?.(s);
  home.renderOpportunities?.(s.opportunities);
  home.renderBenchmark?.(H.benchmark);
  renderGoal(s.goals);
  renderAllocation(s.allocation);
  renderGroupAllocation(s.groupAllocation);
  renderEvidence(s);
  if (H.actions) renderActions(H.actions);
  if (actions.homeRenderActions) actions.homeRenderActions(s);
}

// ---------- 自定义资金分组（方案 11 §3.1/§4：A 分母展示，旧 B 不混用） ----------
function renderGroupAllocation(ga) {
  if (home.renderGroupAllocation) home.renderGroupAllocation(ga);
}

function renderWealth(w) {
  const total = document.getElementById("hc-total-value");
  const invested = document.getElementById("hc-invested-value");
  const ret = document.getElementById("hc-return-value");
  const cash = document.getElementById("hc-cash-value");
  // 质量 → 副文案；未知不显示 0。
  const setCard = (el, minor, quality, issues) => {
    if (quality === "empty" || minor == null) {
      el.textContent = t("fundHomeNeedsVerificationccf920");
      el.classList.remove("hidden-amount");
      return;
    }
    el.textContent = mask(fmtMinor(minor) ?? "–");
    el.classList.toggle("hidden-amount", hideAmounts());
  };

  const q = w.quality || "empty";
  const partial = w.totalA == null && w.knownA != null;
  setCard(total, partial ? w.knownA : w.totalA, q);
  const totalLabel = document.querySelector("#hc-total .hc-label span");
  if (totalLabel) totalLabel.textContent = t(partial ? "fundHomeValuedAssets" : "fundHomeManagedAssets1025ab");
  const totalSub = document.getElementById("hc-total-sub");
  // N04：总额缺口时显示「已估值资产」口径与缺失项计数，不伪造完整金额。
  const valuation = w.valuation || {};
  const gapParts = [];
  if (valuation.missingQuotes > 0) gapParts.push(t("fundHomeValuationMissingQuotes").replace("{n}", valuation.missingQuotes));
  if (valuation.missingNavs > 0) gapParts.push(t("fundHomeValuationMissingNavs").replace("{n}", valuation.missingNavs));
  if (valuation.unknownCosts > 0) gapParts.push(t("fundHomeValuationUnknownCosts").replace("{n}", valuation.unknownCosts));
  if (valuation.quoteFailed) gapParts.push(t("fundHomeValuationFailed"));
  if (valuation.stale) gapParts.push(t("fundHomeValuationStale"));
  totalSub.textContent = q === "empty"
    ? t("fundHomeNoAssetsRecordedYet")
    : w.totalA == null && w.knownA != null
      ? `${t("fundHomeWealthKnownValue")}${mask(fmtMinor(w.knownA) ?? "–")}${gapParts.length ? " · " + gapParts.join(" · ") : ""}`
      : q === "ready"
        ? `${t("fundHomeCompleteValuationLastValued")}${w.latestValuation || "–"}`
        : `${t("fundHomeSomeDataNeedsVerification")}${w.issues.map(i => reasonText(i.code)).join("、") || t("fundHomeMissingData")}）${gapParts.length ? " · " + gapParts.join(" · ") : ""}`;
  document.getElementById("home-currency-manage").hidden = H.displayMode !== "amount" || !(valuation.unconfirmedCurrencies > 0);

  setCard(invested, w.investedI, q);
  document.getElementById("hc-invested-sub").textContent =
    t("fundHomeCurrentNoncashValueSeeHoldingsAndAssets");

  const r = w.returns || {};
  const retEl = document.getElementById("hc-return-value");
  const retSub = document.getElementById("hc-return-sub");
  if (["ready","partial","reference"].includes(r.status) && r.netReturnMinor != null) {
    const v = r.netReturnMinor;
    retEl.textContent = mask((v > 0 ? "+" : "") + (fmtMinor(v) ?? "0"));
    retEl.classList.toggle("hidden-amount", hideAmounts());
    retEl.style.color = hideAmounts() ? "" : v > 0 ? "var(--home-color-up, var(--color-up))" : v < 0 ? "var(--home-color-down, var(--color-down))" : "";
    retSub.textContent = `${r.from||""} -- ${r.to||""} · ${t(r.status==="reference"?"fundHomeRangeReference":"fundHomeRangeChecked")}${r.partialCoverage?" · "+t("fundHomeRangePartial"):""} · ${t("fundHomeNetContributionsExcluded")}`;
  } else {
    retEl.textContent = t("fundHomeNoCompleteReturnRecord");
    retEl.classList.add("hidden-amount");
    retSub.textContent = r.reason === "missing_baseline"
      ? t("fundHomeMissingOpeningBaselineSaveAValuationSnapshotf0dbb6")
      : t("fundHomeOpeningOrClosingValuationIsIncomplete");
  }

  const c = w.cash || {};
  const cashEl = document.getElementById("hc-cash-value");
  const cashSub = document.getElementById("hc-cash-sub");
  if (c.cashMinor == null) {
    cashEl.textContent = t("fundHomeNotRecorded");
    cashEl.classList.add("hidden-amount");
    cashSub.textContent = t("fundHomeNoCashAssetsRecorded");
  } else if (c.reserveState === "unconfigured") {
    cashEl.textContent = t("fundHomePurposeNotConfirmed");
    cashEl.classList.add("hidden-amount");
    cashSub.textContent = `${t("fundHomeSettledCash0c36f8")}${mask(fmtMinor(c.cashMinor) ?? "–")}${t("fundHomeReserveNotConfirmed")}`;
  } else if (c.availableMinor != null) {
    cashEl.textContent = mask(fmtMinor(c.availableMinor) ?? "0");
    cashEl.classList.toggle("hidden-amount", hideAmounts());
    cashSub.textContent = c.shortfallMinor > 0
      ? `${t("fundHomeCashShortfall")}${mask(fmtMinor(c.shortfallMinor) ?? "")}${t("fundHomeCashIsBelowReservesPlusCommitments")}`
      : `${t("fundHomeSettled")}${mask(fmtMinor(c.cashMinor) ?? "")}${t("fundHomeReserve190ba2")}${mask(fmtMinor(c.reservedMinor) ?? "0")}${t("fundHomeCommitment2aa056")}${mask(fmtMinor(c.reservedByOrdersMinor) ?? "0")}`;
  } else {
    cashEl.textContent = t("fundHomeNeedsVerificationccf920"); cashEl.classList.remove("hidden-amount"); cashSub.textContent = t("fundHomeSomeAccountReservesAreNotConfirmed");
  }
}

// 百分比动态小数：最多 4 位并去尾零。后端 ppm（0.0001%）粒度保证任何
// 非零进度不会塌缩显示为 0（0.001% 显示 0.001，40% 显示 40）。
function pctText(pct) {
  if (!Number.isFinite(pct)) return "–";
  const trimmed = pct.toFixed(4).replace(/\.?0+$/, "");
  return trimmed === "" || trimmed === "-" ? "0" : trimmed;
}

function renderGoal(g) {
  const card = document.querySelector(".home-goal");
  const body = document.getElementById("home-goal-body");
  if (!body) return;
  if (card) card.classList.add("home-goal-water");
  const goal = g?.goal || {}, p = g?.progress || {};
  const empty = !g || g.status === "empty";
  // 进度精度源：优先 ppm（0.0001%），旧快照回退 tenths（0.1%）。
  // null 与 undefined 都表示「无值」，不得经 Number() 归零。
  const ppm = p.ratioPpm != null ? Number(p.ratioPpm) : NaN;
  const tenths = p.ratioTenths != null ? Number(p.ratioTenths) : NaN;
  const ratioNum = Number.isFinite(ppm) ? ppm / 10000 : Number.isFinite(tenths) ? tenths / 10 : NaN;
  const valid = !empty && p.computable && Number.isFinite(ratioNum) && ratioNum >= 0;
  const ratio = valid ? ratioNum : null;
  const level = valid && !hidePrivate() ? Math.min(100, Math.max(0, ratio)) : null;
  const lineTop = level == null ? 100 : Math.max(0, Math.min(100, 100 - level));
  const label = hidePrivate() ? t("fundHomeGoalProgressHidden") : valid ? t("fundHomeGoalProgress") : empty ? t("fundHomeCreateGoal") : t("fundHomeNeedsVerificationccf920");

  const ticks = Array.from({length:21},(_,index)=>{const value=index*5;return `<span class="goal-tick ${value%25===0?'major':''}" style="--tick-top:${100-value}%">${value%25===0?value+'%':''}</span>`;}).join('');
  const waterBg = level == null ? "" : `
    <div class="goal-water" style="--line-top:calc(${lineTop}% + ${((level / 100) * 2 - 1) * 16}px)" aria-hidden="true">
      <div class="goal-water-fill"></div>
      <div class="goal-water-line"></div>
      <svg class="goal-water-wave" viewBox="0 0 100 12" preserveAspectRatio="none"><path class="goal-wave-path" d="M0,6 Q25,0 50,6 T100,6"/></svg>
    </div>
    <div class="goal-scale left" aria-hidden="true">${ticks}</div>
    <div class="goal-scale right" aria-hidden="true">${ticks}</div>`;

  if (empty) {
    document.getElementById("home-goal-progress")?.replaceChildren();
    body.innerHTML = `${waterBg}
      <div class="goal-water-content">
        <p class="home-empty-hint">${t("fundHomeCreateAFinancialGoalToSeeCoverage")}</p>
        <button class="btn" id="home-goal-create">${t("fundHomeCreateGoal")}</button>
      </div>`;
    document.getElementById("home-goal-create")?.addEventListener("click", () => openGoalManage());
    return;
  }

  const percentDisplay = hidePrivate() ? "****" : valid ? `${pctText(ratio)}%` : "–";
  const percentSub = hidePrivate() ? t("fundHomeGoalProgressHidden") : valid ? t("fundHomeCurrentCoverage") : t("fundHomeNeedsVerificationccf920");
  const progress = document.getElementById('home-goal-progress');
  if(progress)progress.innerHTML=`<div class="goal-water-percent"><strong>${percentDisplay}</strong><small>${percentSub}</small></div>`;

  const diffLabel = p.surplusMinor > 0 ? t("fundHomeGoalSurplus") : t("fundHomeRemainingGap");
  const diffVal = p.surplusMinor > 0 ? p.surplusMinor : p.gapMinor;

  body.innerHTML = `${waterBg}
    <div class="goal-water-content" role="progressbar" aria-label="${label}" ${level == null ? "" : `aria-valuenow="${level}" aria-valuemin="0" aria-valuemax="100" aria-valuetext="${pctText(ratio)}%"`}>
        <div class="goal-water-metrics">
          <div>
            <small>${t("fundHomeTarget")}</small>
            <strong class="num">${mask(fmtMinor(goal.targetMinor) ?? "–")}</strong>
          </div>
          <div>
            <small>${t("fundHomeCurrentCoverage")}</small>
            <strong class="num">${valid ? mask(fmtMinor(p.scopeValueMinor) ?? "–") : "–"}</strong>
          </div>
          <div>
            <small>${diffLabel}</small>
            <strong class="num">${valid ? mask(fmtMinor(diffVal) ?? "–") : "–"}</strong>
          </div>
        </div>
      <p class="${valid ? "goal-note" : "goal-warn"}">${valid ? t("fundHomeGoalCoverageHint") : t("fundHomeCoverageCannotBeCalculatedYet") + (p.reasonCodes || []).map(reasonText).join("；")}</p>
    </div>`;
}

const CLASS_NAMES = () => ({ equity: t("fundHomeEquity"), gold: t("fundHomeGold"), fixed_income: t("fundHomeFixedIncome"), cash: t("fundHomeCash"), other: t("fundHomeOther") });
function renderAllocation(al) {
  const body = document.getElementById("home-allocation-body");
  if (!body) return;
  if (!al || !al.configured) {
    body.innerHTML = `<div class="home-empty-hint">${t("fundHomeSetTargetWeightsForEquityGoldFixed")}</div>`;
    return;
  }
  if (!al.computable) {
    body.innerHTML = `<div class="home-empty-hint">${t("fundHomeAllocationAssessmentPaused")}${(al.reason_codes || []).map(reasonText).join("；") || t("fundHomeIncompleteData")}${t("fundHomeTargetWeightsRemainVisibleActualWeightsNeed")}</div>`;
  }
  const rows = (al.classes || []).map(c => {
    const name = CLASS_NAMES()[c.risk_class] || c.risk_class;
    const target = (c.target_tenths / 10).toFixed(1);
    const actual = c.actual_tenths != null ? (c.actual_tenths / 10).toFixed(1) : "–";
    const amount = c.amount_minor != null ? mask(fmtMinor(c.amount_minor) ?? "0") : "–";
    const delta = c.delta_tenths != null ? ((c.delta_tenths > 0 ? "+" : "") + (c.delta_tenths / 10).toFixed(1) + "pt") : "–";
    const statusText = c.status === "below" ? t("fundHomeUnderweight") : c.status === "above" ? t("fundHomeOverweight") : c.status === "within" ? t("fundHomeWithinRange") : t("fundHomeNeedsVerificationccf920");
    return `<tr>
      <td>${name}</td><td>${amount} · ${mask(actual)}%</td><td>${mask(target)}%</td><td>${mask(delta)}</td>
      <td><span class="alloc-badge">${statusText}</span></td>
    </tr>`;
  }).join("");
  const totalText = al.total_minor != null ? fmtMinor(al.total_minor) ?? "0" : t("fundHomeNeedsVerificationccf920");
  body.innerHTML = `${!al.computable ? `<div class="home-empty-hint">${(al.reason_codes || []).map(reasonText).join("；")}</div>` : ""}
    <table class="alloc-table">
      <thead><tr><th>${t("fundHomeClass")}</th><th>${t("fundHomeCurrentValueWeight")}</th><th>${t("fundHomeTargetWeight")}</th><th>${t("fundHomeDeviation")}</th><th>${t("fundHomeStatus")}</th></tr></thead>
      <tbody>${rows}</tbody>
    </table>
    <div class="alloc-plan-row">${t("fundHomeTotalAllocatableAssets")}${mask(totalText)}${t("fundHomeCashReservesExcludedDeviationsCallForReview")}</div>`;
}

function renderEvidence(s) {
  const checkTime=s.alerts?.lastCheck?new Date(Number(s.alerts.lastCheck)*1000):null;
  const checked=checkTime&&!Number.isNaN(checkTime.getTime())?checkTime.toLocaleString(state.language,{timeZone:'Asia/Shanghai'}):t('fundHomeSourceTimestampUnknown');
  const body = document.getElementById("home-evidence-body");
  if (!body) return;
  // 方案 01 P06/P07（08 最新）：与我有关的资讯——自动资讯（持仓/自选）+ 手工资料状态。
  // 首页最多 5 条；查看全部在详情面板；刷新由用户显式触发，不自动调 AI。
  body.innerHTML = `
    <details class="home-news-context"><summary>${t("fundHomeEvidenceLayers")}</summary><div class="evidence-hint" id="home-evidence-snapshot">${t("fundHomeCurrentSnapshot")}${escapeHtml((s.snapshotId || "").slice(0, 24))}… · ${s.alerts && s.alerts.openCount > 0 ? `${s.alerts.openCount}${t("fundHomeUnresolvedEvents")}` : t("fundHomeNoUnresolvedEvents")}${s.alerts && s.alerts.lastCheck ? `${t("fundHomeLastChecked")}${escapeHtml(checked)}` : ""}</div>
    <div class="evidence-layer">${t("fundHomeLocalLedgerAssetsTransactionsAndRuleResults")}</div></details>
    <div id="home-info-list" aria-live="polite">${t("fundHomeInformationLoading")}</div>
    <div class="home-evidence-actions" style="margin-top:6px">
      <button class="btn btn-sm" id="home-info-refresh">${t("fundHomeInformationRefresh")}</button>
      <button class="btn btn-sm" id="home-info-next" ${infoNextOffset==null?"hidden":""}>${t("fundHomeInformationNextBatch")}</button>
      <button class="btn btn-sm" id="home-info-all">${t("fundHomeInformationViewAll")}</button>
    </div>`;
  document.getElementById("home-info-refresh")?.addEventListener("click", () => refreshInformation());
  document.getElementById("home-info-next")?.addEventListener("click", () => {if(infoNextOffset!=null)void refreshInformation(infoNextOffset);});
  document.getElementById("home-info-all")?.addEventListener("click", () => toggleDetail("information"));
  informationControls();
  void loadInformationList();
}

// ---------- 自动资讯（方案 04：首页 5 条、scope 切换、来源状态显式） ----------
let infoNextOffset=null,infoCollectionDays=7;
let infoListGeneration=0;
let infoScope = "all",newsScope="all",infoFilters={from:"",to:"",q:"",asset:"",type:"",sort:"relevance",group:""},infoFullGeneration=0;
async function loadInformationList() {
  const body = document.getElementById("home-info-list");
  if (!body) return;
  const generation=++infoListGeneration;
  const scope=infoScope;
  const focusedScope=body.contains(document.activeElement)?document.activeElement?.dataset.infoScope:null;
  try {
    const payload = await api("GET", `/api/information/items?scope=${infoScope}&page=1`);
    if(generation!==infoListGeneration||scope!==infoScope||!body.isConnected)return;
    const items = (payload.items || []).slice(0, 5);
    const failed = (payload.sources || []).filter(src => src.enabled && src.lastErrorCode);
    const scopeTabs = ["all", "market", "holdings", "watchlist"].map(scope =>
      `<button class="hc-range${infoScope === scope ? " active" : ""}" data-info-scope="${scope}" aria-pressed="${infoScope === scope}">${t({all:"fundHomeAll",market:"fundHomeScopeMarket",holdings:"fundHomeScopeHoldings",watchlist:"fundHomeScopeWatchlist"}[scope])}</button>`).join("");
    const list = items.length
      ? items.map(item=>home.renderHomeNewsItem(item)).join("")
      : `<p class="home-empty-hint">${t("fundHomeInformationEmpty")}</p>`;
    const warn = failed.length
      ? `<p class="goal-warn">${t("fundHomeInformationSourceFailed")}${failed.map(f => escapeHtml(home.informationSourceLabel(f.sourceId))).join("、")}</p>`
      : "";
    body.innerHTML = `<div class="hc-range-tabs" role="group" aria-label="${t("fundHomeScopeHoldings")}">${scopeTabs}</div>${warn}${list}
      <div id="home-info-alerts" aria-live="polite"></div>
      <details class="home-news-coverage"><summary>${t("fundHomeSources")}</summary><p class="evidence-hint">${t("fundHomeInformationCoverage")}</p></details>`;
    body.querySelectorAll("[data-act=news-ai]").forEach(btn => btn.addEventListener("click", () => {
      const it = items.find(i => i.id === Number(btn.dataset.id));
      if (it) home.selectNewsForAI?.(it);
    }));
    body.querySelectorAll("[data-info-scope]").forEach(btn => btn.addEventListener("click", () => { cancelInformation();infoNextOffset=null;informationControls();infoScope = btn.dataset.infoScope; void loadInformationList(); }));
    body.querySelectorAll("[data-info-fav]").forEach(btn => btn.addEventListener("click", async () => {
      try { await api("POST", "/api/information/flags", { id: Number(btn.dataset.infoFav), favorite: btn.getAttribute("aria-pressed") !== "true" }); void loadInformationList(); } catch (e) { setStatus(`${t("fundHomeRequestFailed")}${e.message}`); }
    }));
    body.querySelectorAll("[data-info-open]").forEach(btn => btn.addEventListener("click", () => {
      // 详情经 home-evidence 面板；原文外链只由有能力的 Shell 打开。
      home.openInformationDetail?.(Number(btn.dataset.infoOpen));
    }));
    if(focusedScope)body.querySelector(`[data-info-scope="${focusedScope}"]`)?.focus({preventScroll:true});
    void loadInformationAlerts();
  } catch (e) {
    if(generation!==infoListGeneration||scope!==infoScope||!body.isConnected)return;
    body.innerHTML = `<p class="goal-warn">${t("fundHomeInformationCouldNotLoad")}${escapeHtml(e.message)}</p>`;
  }
}

// 资讯复核提醒（R02；方案 05 §8）：查看依据与处理状态。
// 复用既有规则事件状态语义，不另建通知平台、不做闭页常驻监控。
async function loadInformationAlerts() {
  const box = document.getElementById("home-info-alerts");
  if (!box) return;
  try {
    const payload = await api("GET", "/api/information/alerts");
    const alerts = payload.alerts || [];
    if (!alerts.length) { box.innerHTML = ""; return; }
    const rows = alerts.map(a => `<article class="info-item">
      <b>${escapeHtml(a.title)}</b>
      <span class="info-meta">${t("fundHomeReviewStatus")}：${escapeHtml(t({open:"fundHomeAlertOpen",acknowledged:"fundHomeAlertAcknowledged",snoozed:"fundHomeAlertSnoozed"}[a.status]||"fundHomeMarkHandled"))} · ${escapeHtml((a.firstTriggeredAt || "").slice(0, 16).replace("T", " "))}</span>
      <span class="info-actions">
        ${a.itemId ? `<button class="btn btn-sm" data-alert-open="${a.itemId}">${t("fundHomeViewEvidence")}</button>` : ""}
        ${a.status === "open" ? `<button class="btn btn-sm" data-alert-ack="${a.id}">${t("fundHomeAcknowledge")}</button>` : ""}
        <button class="btn btn-sm" data-alert-defer="${a.id}">${t("fundHomeDefer")}</button>
        <button class="btn btn-sm" data-alert-resolve="${a.id}">${t("fundHomeMarkHandled")}</button>
      </span></article>`).join("");
    box.innerHTML = `<p class="evidence-hint">${t("fundHomeReviewReminders")}</p>${rows}`;
    box.querySelectorAll("[data-alert-ack]").forEach(btn => btn.addEventListener("click", async () => {
      try { await api("POST", "/api/information/alerts/action", { id: Number(btn.dataset.alertAck), action: "acknowledge" }); void loadInformationAlerts(); } catch (e) { setStatus(`${t("fundHomeRequestFailed")}${e.message}`); }
    }));
    box.querySelectorAll("[data-alert-defer]").forEach(btn => btn.addEventListener("click", () => {
      const tomorrow=new Date(Date.now()+86400000).toISOString().slice(0,10);
      home.panel(()=>t("fundHomeDefer"),()=>`<form>${home.input("until",t("fundHomeDeferUntilDeferralOnly"),tomorrow,"date",`required min="${tomorrow}"`)}<button type="submit">${t("fundHomeSave")}</button></form>`,root=>home.submit(root,async v=>{
        await api("POST","/api/information/alerts/action",{id:Number(btn.dataset.alertDefer),action:"snooze",snoozedUntil:v.until});home.closePanel();await loadInformationAlerts();
      }),true);
    }));
    box.querySelectorAll("[data-alert-resolve]").forEach(btn => btn.addEventListener("click", async () => {
      try { await api("POST", "/api/information/alerts/action", { id: Number(btn.dataset.alertResolve), action: "resolve" }); void loadInformationAlerts(); } catch (e) { setStatus(`${t("fundHomeRequestFailed")}${e.message}`); }
    }));
    box.querySelectorAll("[data-alert-open]").forEach(btn => btn.addEventListener("click", () => {
      home.openInformationDetail?.(Number(btn.dataset.alertOpen));
    }));
  } catch (e) {
    box.innerHTML = `<p class="goal-warn">${t("fundHomeInformationCouldNotLoad")}${escapeHtml(e.message)}</p>`;
  }
}

let informationGeneration=0,informationRunning=false,informationMessage=null,informationCollectionScope=null;
function informationControls(){for(const id of ['home-info-refresh','news-page-refresh']){const button=document.getElementById(id);if(button){button.disabled=home.displayMode()==='hidden';button.textContent=t(informationRunning?'fundHomeInformationCancel':'fundHomeInformationRefresh');}}for(const id of ['home-info-next','news-page-next']){const button=document.getElementById(id),scope=id==='home-info-next'?infoScope:newsScope;if(button)button.hidden=document.getElementById(id==='home-info-next'?'view-home':'view-news')?.hidden||informationRunning||infoNextOffset==null||home.displayMode()==='hidden'||informationCollectionScope!==(['holdings','watchlist','market'].includes(scope)?scope:'all');}}
function informationStatus(message){informationMessage=message;const value=typeof message==='function'?message():message;setStatus(value);const node=document.getElementById('news-page-status');if(node&&!document.getElementById('view-news').hidden)node.textContent=value;}
function cancelInformation(){++informationGeneration;if(informationRunning)informationStatus(()=>t('fundHomeCancelled'));informationRunning=false;informationControls();home.cancelInformationRequest?.();}
home.cancelInformation=cancelInformation;
async function informationRequest(route,payload){cancelInformation();return home.requestInformation(route,payload);}
window.addEventListener("pagehide",cancelInformation);
async function refreshInformation(identityOffset=0) {
  if(home.displayMode()==='hidden')return;
  if(informationRunning){cancelInformation();return;}
  informationRunning=true;informationControls();informationStatus(()=>t('fundHomeInformationLoading'));
  const gen=++informationGeneration;
  try {
    const view=document.getElementById('view-news'),scope=view&&!view.hidden?newsScope:infoScope;
    informationCollectionScope=['holdings','watchlist','market'].includes(scope)?scope:'all';
    const resp=await home.requestInformation('/api/information/refresh',{scope:informationCollectionScope,days:infoCollectionDays,identityOffset});
    if(!resp||gen!==informationGeneration)return;
    const outcome=(resp.result||resp).outcome||{};
    infoNextOffset=Number.isInteger(outcome.next_identity_offset)?outcome.next_identity_offset:null;
    informationControls();
    const failed=(outcome.sources||[]).filter(s=>s.status==="error"||s.status==="partial").map(s=>`${s.source_id||s.sourceId}(${s.error_code||s.errorCode||""})`);
    const days=infoCollectionDays;
    informationStatus(()=>`${failed.length?`${t("fundHomeInformationRefreshPartial")}${failed.join(", ")}`:t("fundHomeInformationRefreshDone")} · ${t("fundHomeInformationBatchCoverage")} ${outcome.covered_identities||0}/${outcome.total_identities||0} · ${days} ${t("fundHomeInformationDays")}`);
    await loadInformationList();
    if(!document.getElementById('view-news')?.hidden)await actions.loadNewsPage(false);
    await home.notifyPending?.();
  }catch(e){if(gen===informationGeneration)informationStatus(()=>`${t("fundHomeInformationCouldNotLoad")}${Fund.localizeApiError(e.status||500,e.code||'INFORMATION_UPSTREAM','')}`);}
  finally{if(gen===informationGeneration){informationRunning=false;informationControls();}}
}

// 资讯详情（方案 02 §6）：来源/时间/完整度 → 正文 → 关联与依据；原文经 Shell 外链代理。
home.openInformationDetail = async function (id) {
  try {
    const d = await api("GET", `/api/information/item?id=${id}`);
    const rel = (d.relations || []).map(r => `${escapeHtml(r.instrumentRef)}（${t(r.relation === "direct" ? "fundHomeInformationDirect" : r.relation === "indirect" ? "fundHomeInformationIndirect" : "fundHomeNotConfirmed")}）：${escapeHtml(r.reason)}`).join("<br>");
    let contentBlock = d.contentText
      ? `<details open><summary>${escapeHtml(home.informationContentLabel(d))}</summary><pre style="white-space:pre-wrap">${escapeHtml(d.contentText)}</pre></details>`
      : d.completeness === "link_only" && d.sourceId === "em_stock_ann"
        ? `<p class="goal-warn">${t("fundHomeContentNotRetrieved")}；<button class="btn btn-sm" data-info-fetch="${id}">${t("fundHomeInformationFetchContent")}</button></p>`
        : `<p class="evidence-hint">${t("fundHomeContentNotRetrieved")}</p>`;
    if(d.contentCacheExpired && d.sourceId === "em_stock_ann")contentBlock+=`<p class="goal-warn">${t("fundHomeInformationContentExpired")} <button class="btn btn-sm" data-info-fetch="${id}">${t("fundHomeInformationFetchContent")}</button></p>`;
    home.panel(() => t("fundHomeInformationDetail"), () => `
      <p><b>${escapeHtml(d.title)}</b></p>
      <p class="evidence-hint">${escapeHtml(home.informationSourceLabel(d.sourceId))} · ${escapeHtml(home.informationTimeLabel(d.publishedAt))}</p>
      ${d.summary?`<p>${escapeHtml(d.summary)}</p>`:""}
      ${contentBlock}
      ${d.url ? `<p><a href="#" data-info-url="${escapeHtml(d.url)}">${t("fundHomeOpenSource")}</a></p>` : ""}
      ${rel ? `<p class="evidence-hint">${t("fundHomeInformationRelations")}<br>${rel}</p>` : ""}${d.relatedItems?.length?`<h3>${t("fundHomeInformationPossibleRelated")}</h3><p>${t("fundHomeInformationPossibleRelatedHint")}</p>${d.relatedItems.map(item=>`<p><button data-related-info="${item.id}">${escapeHtml(item.title)}</button> · ${escapeHtml(home.informationSourceLabel(item.sourceId))}</p>`).join("")}`:""}<div class="row"><button type="button" data-detail-ai>${t('fundHomeNewsAiBtn')}</button></div>`,
    root => {
      root.querySelector('[data-detail-ai]').onclick=()=>{home.closePanel();document.getElementById('tab-home').click();home.selectNewsForAI?.(d);};
      root.querySelectorAll("[data-related-info]").forEach(b=>b.onclick=()=>home.openInformationDetail(Number(b.dataset.relatedInfo)));
      const fetchBtn = root.querySelector(`[data-info-fetch="${id}"]`);
      if (fetchBtn) fetchBtn.onclick = async () => {
        home.failure({message:''},root);
        fetchBtn.disabled = true;
        try {
          const result=await informationRequest("/api/information/content", { id,expectedVersion:d.contentVersion });
          if(result&&root.isConnected)home.openInformationDetail(id);
        } catch (e) { const message=`${t("fundHomeInformationCouldNotLoad")}${e.message}`;setStatus(message);home.failure({message},root); }
        finally { if(fetchBtn.isConnected)fetchBtn.disabled=false; }
      };
      const link = root.querySelector("[data-info-url]");
      if (link) link.onclick = e => { e.preventDefault(); home.openExternal?.(link.dataset.infoUrl); };
    },true);
  } catch (e) { setStatus(`${t("fundHomeInformationCouldNotLoad")}${e.message}`); }
};

// ---------- 详情展开 ----------
function toggleDetail(kind) {
  if(kind==='information'){document.getElementById('tab-news').click();return;}
  const panel = document.getElementById("home-detail-panel");
  const body = document.getElementById("home-detail-body");
  if (!panel || !body) return;
  if (H.detailOpen === kind) {
    panel.style.display = "none";
    H.detailOpen = null;
    return;
  }
  H.detailOpen = kind;
  body.innerHTML = buildDetail(kind);
  panel.style.display = "block";
  if (kind === "information") void loadInformationFull();
}

// 全量资讯列表（详情面板内，≤50/页；scope 与首页共享）。
let infoFullController=null;
home.filterNewsAsset=asset=>{infoFilters={...infoFilters,asset};void loadInformationFull(1);};
home.readInformation=loadInformationFull;home.refreshInformation=refreshInformation;
home.renderInformationStatus=()=>{if(informationMessage)informationStatus(informationMessage);};
home.cancelInformationRead=()=>{infoFullGeneration++;infoFullController?.abort();infoFullController=null;};
document.getElementById('news-page-refresh')?.addEventListener('click',()=>void refreshInformation());
document.getElementById('news-page-next')?.addEventListener('click',()=>{if(infoNextOffset!=null)void refreshInformation(infoNextOffset);});
document.addEventListener('fund:privacy-changed',()=>{actions.leaveNewsPage();const body=document.getElementById('news-page-body');if(body)body.replaceChildren();if(!document.getElementById('view-news')?.hidden)void actions.loadNewsPage();});
document.addEventListener('fund:language-changed',()=>{if(document.getElementById('view-news')?.hidden)return;informationControls();if(informationMessage)informationStatus(informationMessage);const form=document.querySelector('#news-page-body [data-info-filter]');if(form){const values=Object.fromEntries(new FormData(form));infoCollectionDays=Number(values.collectionDays);delete values.collectionDays;infoFilters=values;}void actions.loadNewsPage(false);});
async function loadInformationFull(page = 1) {
  const newsView=document.getElementById('view-news');
  const body = newsView&&!newsView.hidden?document.getElementById('news-page-body'):document.getElementById("home-info-full");
  if (!body) return;
  if(home.displayMode()==='hidden'){body.textContent=t('fundHomeSelectShowAmountsBeforeViewingPrivateSources');return;}
  let gen;
  try {
    gen=++infoFullGeneration;infoFullController?.abort();infoFullController=new AbortController();
    const scope=newsView&&!newsView.hidden?newsScope:infoScope;
    const query=new URLSearchParams({scope,page:String(page),...infoFilters});
    const data = await api("GET", `/api/information/items?${query}`,null,{signal:infoFullController.signal});
    if(gen!==infoFullGeneration||!body.isConnected)return;
    const focused=body.contains(document.activeElement)?document.activeElement:null;
    const focusKey=focused?.name?`[name="${CSS.escape(focused.name)}"]`:focused?.hasAttribute('data-info-scope')?`[data-info-scope="${focused.dataset.infoScope}"]`:null;
    const advancedOpen=body.querySelector('.news-advanced')?.open||Boolean(infoFilters.from||infoFilters.to||infoFilters.asset||infoFilters.group||infoFilters.type);
    const items = (data.items || []).map(item => `<article class="info-item">
      <div class="news-item-heading"><span class="news-category">${t(item.sourceId==='em_finance'?'fundHomeFinanceNews':item.importance>=2?'fundHomeNewsImportant':'fundHomeNewsRelated')}</span><time>${escapeHtml(home.informationTimeLabel(item.publishedAt))}</time></div>
      <h3>${escapeHtml(item.title)}</h3>
      <p>${escapeHtml(item.summary||t('fundHomeContentNotRetrieved'))}</p>
      <span class="info-meta">${escapeHtml(home.informationSourceLabel(item.sourceId))} · ${escapeHtml(home.informationContentLabel(item))}${item.archived ? ` · ${t("fundHomeArchived")}` : ""}</span>
      <span class="info-actions">
        <button class="btn btn-sm" data-info-open="${item.id}">${t("fundHomeOpenSource")}</button>
        <button class="btn btn-sm btn-accent" data-info-ai="${item.id}">${t('fundHomeNewsAiBtn')}</button>
        <button class="btn btn-sm" data-info-fav="${item.id}" aria-pressed="${item.favorite}">${item.favorite ? t("fundHomeFavorited") : t("fundHomeFavorite")}</button>
        ${item.archived ? "" : `<button class="btn btn-sm" data-info-arch="${item.id}">${t("fundHomeArchive")}</button>`}
      </span></article>`).join("") || `<p class="home-empty-hint">${t("fundHomeInformationEmpty")}</p>`;
    const more = (data.items || []).length === 50
      ? `<button class="btn btn-sm" id="home-info-more" data-page="${page + 1}">${t("fundHomeInformationLoadMore")}</button>` : "";
    body.innerHTML = `<div class="hc-range-tabs news-scope-tabs" role="group">${["all", "market", "holdings", "watchlist", "favorites"].map(value =>
      `<button class="hc-range${scope === value ? " active" : ""}" data-info-scope="${value}" aria-pressed="${scope===value}">${t(value==='all'?'fundHomeAll':value === "holdings" ? "fundHomeScopeHoldings" : value === "watchlist" ? "fundHomeScopeWatchlist" : value==="market"?"fundHomeScopeMarket":"fundHomeFavorite")}</button>`).join("")}</div><form data-info-filter><div class="news-search-row">${home.input("q",t("fundHomeInformationSearch"),infoFilters.q,"search",'maxlength="128"')}${home.select("sort",t("fundHomeInformationSort"),[["relevance",t("fundHomeInformationRelevant")],["latest",t("fundHomeInformationLatest")]],infoFilters.sort)}<button type="submit" class="btn">${t("fundHomeInformationFilter")}</button></div><details class="news-advanced" ${advancedOpen?'open':''}><summary>${t('fundHomeNewsAdvanced')}</summary><div class="news-filter-fields">${home.input("collectionDays",t("fundHomeInformationCollectionDays"),infoCollectionDays,"number",'required min="1" max="30" step="1"')}${home.input("from",t("fundHomeInformationFrom"),infoFilters.from,"date")}${home.input("to",t("fundHomeInformationTo"),infoFilters.to,"date")}${home.select("asset",t("fundHomeInformationAssetRef"),[["",t("fundHomeAll")],...(data.assetOptions||[]).map(ref=>[ref,ref.startsWith("keyword:")?ref.slice(8):ref])],infoFilters.asset)}${home.select("group",t("fundHomeGroupAllocationPlan"),[["",t("fundHomeAll")],...(data.groupOptions||[]).map(g=>[g.id,g.name])],infoFilters.group)}${home.select("type",t("fundHomeInformationType"),[["",t("fundHomeAll")],...["notice","earnings","dividend","suspension","repurchase","financing","insider","regulatory","restructuring","major"].map(type=>[type,t("fundHomeEvent"+type)])],infoFilters.type)}</div><p class="investment-card-tip">${t("fundHomeInformationUnknownDateFilter")}</p><button type="submit" class="btn">${t("fundHomeInformationFilter")}</button></details></form><div class="news-articles">${items}</div><div class="news-pagination"><button type="button" data-prev-info ${page===1?"disabled":""}>${t("fundHomePrevious")}</button> ${page} ${more}</div>`;
    body.querySelector('[data-info-filter]').onsubmit=e=>{e.preventDefault();const values=Object.fromEntries(new FormData(e.target));const days=Number(values.collectionDays);if(days!==infoCollectionDays){cancelInformation();infoNextOffset=null;informationControls();}infoCollectionDays=days;delete values.collectionDays;infoFilters=values;void loadInformationFull(1);};
    body.querySelector('[data-prev-info]').onclick=()=>void loadInformationFull(page-1);
    body.querySelectorAll("[data-info-scope]").forEach(btn => btn.addEventListener("click", () => { cancelInformation();infoNextOffset=null;informationControls();if(newsView&&!newsView.hidden)newsScope=btn.dataset.infoScope;else infoScope=btn.dataset.infoScope;void loadInformationFull(1); }));
    body.querySelectorAll("[data-info-fav]").forEach(btn => btn.addEventListener("click", async () => {
      try { await api("POST", "/api/information/flags", { id: Number(btn.dataset.infoFav), favorite: btn.getAttribute("aria-pressed") !== "true" }); void loadInformationFull(page); } catch (e) { setStatus(`${t("fundHomeRequestFailed")}${e.message}`); }
    }));
    body.querySelectorAll("[data-info-arch]").forEach(btn => btn.addEventListener("click", async () => {
      try { await api("POST", "/api/information/archive", { id: Number(btn.dataset.infoArch) }); void loadInformationFull(page); } catch (e) { setStatus(`${t("fundHomeRequestFailed")}${e.message}`); }
    }));
    body.querySelectorAll("[data-info-open]").forEach(btn => btn.addEventListener("click", () => home.openInformationDetail?.(Number(btn.dataset.infoOpen))));
    body.querySelectorAll('[data-info-ai]').forEach(btn=>btn.onclick=()=>{const item=data.items.find(i=>i.id===Number(btn.dataset.infoAi));if(item){document.getElementById('tab-home').click();home.selectNewsForAI?.(item);}});
    const moreBtn = body.querySelector("#home-info-more");
    if (moreBtn) moreBtn.onclick = () => void loadInformationFull(Number(moreBtn.dataset.page));
    informationControls();
    if(focusKey)body.querySelector(focusKey)?.focus({preventScroll:true});
  } catch (e) {
    if(gen!==infoFullGeneration||e.name==='AbortError')return;
    body.innerHTML = `<p class="goal-warn">${t("fundHomeInformationCouldNotLoad")}</p><button data-info-retry>${t('fundLedgerRetry')}</button>`;
    body.querySelector('[data-info-retry]').onclick=()=>void loadInformationFull(page);
  }
}

function buildDetail(kind) {
  const s = H.snapshot;
  if (!s) return "";
  // 方案 01 P06：查看全部——资讯列表（分页 ≤50）在现有详情面板内，不建一级入口。
  if (kind === "information") {
    return `<div id="home-info-full"><p class="evidence-hint">${t("fundHomeInformationLoading")}</p></div>`;
  }
  return home.buildOverviewDetail ? home.buildOverviewDetail(kind, s, { hidePrivate, hideAmounts, mask, fmtMinorFull, escapeHtml }) : "";
}

// ---------- 市场（复用现有指数接口，不新增轮询） ----------
let benchmarkGen = 0, benchmarkController = null;
document.addEventListener("fund:financial-display-changed",()=>{if(active&&H.snapshot)loadBenchmark(H.snapshot);});
async function loadBenchmark(snapshot) {
  if (!active || document.hidden) return;
  const symbol=Fund.state.benchmarkSymbol||snapshot.financialDisplay?.benchmarkSymbol||"sh000300";
  const from=snapshot.requestedRange?.from??snapshot.portfolio?.history?.from??"",to=snapshot.requestedRange?.to??snapshot.portfolio?.history?.to??"";
  const identity=[snapshot.dataRevision,symbol,from,to].join('|');
  if(H.benchmark?.identity===identity)return;
  const currentIdentity=()=>[H.snapshot?.dataRevision,Fund.state.benchmarkSymbol||H.snapshot?.financialDisplay?.benchmarkSymbol||"sh000300",H.snapshot?.requestedRange?.from??H.snapshot?.portfolio?.history?.from??"",H.snapshot?.requestedRange?.to??H.snapshot?.portfolio?.history?.to??""].join('|');
  // 浏览器 abort 不终止 Runtime 已发送的行情工作；版本变化只合并到最新快照。
  if (benchmarkController) return;
  const gen = ++benchmarkGen;
  const controller = new AbortController(); benchmarkController = controller;
  H.benchmark = null; home.renderBenchmark?.();
  try {
    const data = await api("GET", `/api/portfolio/benchmark?dataRevision=${encodeURIComponent(snapshot.dataRevision)}&symbol=${encodeURIComponent(symbol)}&from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`, null, {signal:controller.signal,retryConflict:false});
    if (gen !== benchmarkGen || controller.signal.aborted || currentIdentity()!==identity) return;
    if (data.dataRevision !== snapshot.dataRevision) throw Error(t("fundHomeOverviewBenchmarkChanged"));
    H.benchmark = freezeSnapshot({...data,identity});
    home.renderBenchmark?.(data);
  } catch (e) {
    if (gen !== benchmarkGen || controller.signal.aborted || currentIdentity()!==identity) return;
    H.benchmark = freezeSnapshot({status:"failed",errorCode:e.code||null,identity}); home.renderBenchmark?.(H.benchmark);
  } finally {
    if (benchmarkController === controller) {
      benchmarkController = null;
      if (currentIdentity()!==identity) loadBenchmark(H.snapshot);
    }
  }
}
// 请求代际（R06）：快速切换/重进时，迟到的旧响应不得覆盖新页面状态。
let marketGen = 0;
let marketController = null;
document.addEventListener("fund:market-indices",e=>{if(active&&!document.hidden&&e.detail){H.marketFailure=false;renderMarket(e.detail);}});
document.addEventListener("fund:market-indices-failed",()=>{if(active&&!document.hidden){H.marketFailure=true;if(H.market)void renderMarket(H.market);else document.getElementById("home-market-time").textContent=t("fundHomeMarketDataIsUnavailableTheLocalLedger");}});
async function renderMarket(cached) {
  const body = document.getElementById("home-market-body");
  if (!body || (!cached && (!active || document.hidden))) return;
  const gen = ++marketGen;
  marketController?.abort();
  const controller = new AbortController();
  marketController = controller;
  try {
    let data = cached;
    if (!data) {
      const response = await authFetch("/api/market/indices", {signal:controller.signal});
      data = await response.json();
      if (!response.ok || data.error) throw Error(t("fundHomeMarketDataIsUnavailableTheLocalLedger"));
      H.marketFailure = false;
    }
    if (gen !== marketGen) return; // 迟到响应丢弃
    // 后端契约（market/quote_json）：{"quotes":[{code,name,price,changePct,...}]}
    const list = (data && data.quotes) || [];
    H.market = data;
    const top = list.slice(0, 5);
    if (!top.length) {
      body.innerHTML = `<div class="home-empty-hint">${t("fundHomeMarketDataIsUnavailableTheLocalLedger")}</div>`;
      return;
    }
    body.innerHTML = top.map(idx => {
      const labelKeys = {"000001":"fundHomeIndexShanghai","399001":"fundHomeIndexShenzhen","399006":"fundHomeIndexChiNext","000688":"fundHomeIndexStar50","000300":"fundHomeIndexCsi300"};
      const key = labelKeys[String(idx.code).replace(/^(sh|sz)/,"")];
      const name = key ? t(key) : state.language === "en" ? idx.code : idx.name || idx.code;
      const pct = idx.changePct != null ? `${idx.changePct > 0 ? "+" : ""}${Number(idx.changePct).toFixed(2)}%` : "–";
      const cls = idx.changePct > 0 ? "up" : idx.changePct < 0 ? "down" : "flat";
      return `<div class="market-row">
        <span class="market-name">${escapeHtml(name || "")}</span>
        <span class="market-val ${cls}">${idx.price != null ? Number(idx.price).toFixed(2) : "–"}</span>
        <span class="market-chg ${cls}">${pct}</span>
        <span class="market-meta">${escapeHtml(idx.asOf || t("fundHomeSourceTimestampUnknown"))}</span>
      </div>`;
    }).join("");
    const tEl = document.getElementById("home-market-time");
    if (tEl) tEl.textContent = H.marketFailure ? t("fundHomeMarketDataIsUnavailableTheLocalLedger") : `${t("fundHomeTencentMarketData")}${top[0]?.asOf || t("fundHomeSourceTimestampUnknown")}`;
  } catch (e) {
    if (gen !== marketGen || controller.signal.aborted) return;
    H.marketFailure = true;
    if (H.market?.quotes?.length) { renderMarket(H.market); return; }
    body.innerHTML = `<div class="home-empty-hint">${t("fundHomeMarketDataCouldNotLoad")}${escapeHtml(e.message)}${t("fundHomeTheLocalLedgerRemainsAvailable")}</div>`;
  } finally {
    if (marketController === controller) marketController = null;
  }
}

function reasonText(code) {
  const map = {
    assets_empty: t("fundHomeNoAssetsRecorded"),
    incomplete: t("fundHomeSomeValuationsCurrenciesOrClassificationsNeedVerification"),
    stale: t("fundHomeValuationNotVerifiedForMoreThan7"),
    missing_nav: t("fundHomeMissingNav"),
    missing_price: t("fundHomeOverviewMissingPrice"),
    currency_issue: t("fundHomeCurrencyNotConfirmed"),
    currency_unconfirmed: t("fundHomeCurrencyNotConfirmed"),
    currency_unsupported: t("fundHomeUnsupportedCurrency"),
    stale_valuation: t("fundHomeStaleValuation"),
    overlap_review: t("fundHomePossibleOverlappingOwnership"),
    unknown_class: t("fundHomeSomeAssetClassificationsAreUnknown"),
    empty_portfolio: t("fundHomePortfolioIsEmpty"),
    zero_total: t("fundHomePortfolioTotalIsZero"),
    reserve_unconfigured: t("fundHomeCashReservesNotConfirmedAllocatableAssetsB"),
    unbound: t("fundHomeNoCoverageAssetsBound"),
    scope_asset_missing: t("fundHomeBoundAssetScopeIsMissing"),
  };
  return map[code] || code;
}

function escapeHtml(value) {
  return String(value ?? "")
    .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ---------- 三态显示（方案 11 §5）：amount|percent|hidden ----------
// H.privacy 为兼容投影：hidden/percent→true（遮金额），amount→false。
let displayMode = "hidden";
let displaySave = Promise.resolve();
function writeDisplayPref(mode) {
  displaySave = displaySave.catch(() => {}).then(async () => {
    await api("POST", "/api/home/preferences", { displayMode: mode });
    // 偏好写入也递增账本 revision；同步后再查询绑定该版本的基准。
    if (mode === displayMode && active && !document.hidden) await loadSnapshot();
  }).catch(e => setStatus(`${t("fundHomePrivacyPreferenceCouldNotBeSaved")}${e.message}`));
}
function applyDisplayMode(mode, persist = true) {
  // 非法值按 hidden 处理（与后端一致）。
  displayMode = ["amount", "percent", "hidden"].includes(mode) ? mode : "hidden";
  H.privacy = displayMode !== "amount";
  H.displayMode = displayMode;
  const btn = document.getElementById("home-privacy-btn");
  if (btn) btn.textContent = displayMode === "amount" ? t("fundHomeDisplayPercent")
    : displayMode === "percent" ? t("fundHomeDisplayHidden") : t("fundHomeDisplayAmount");
  btn?.setAttribute("aria-pressed", displayMode !== "amount" ? "true" : "false");
  if (persist) writeDisplayPref(displayMode);
  home.closePanel?.();
  document.dispatchEvent(new Event("fund:privacy-changed"));
  // 切换模式重绘全部金额/比例节点（含已展开详情），迟到请求不显露。
  if (H.snapshot) renderSnapshot();
  if (H.detailOpen) {
    const body = document.getElementById("home-detail-body");
    if (body) body.innerHTML = buildDetail(H.detailOpen);
  }
}
function cycleDisplayMode() {
  applyDisplayMode(displayMode === "hidden" ? "amount" : displayMode === "amount" ? "percent" : "hidden");
}

// ---------- 隐私（R05：默认隐藏；受限环境持久化失败仅影响记忆，不影响本次会话） ----------
let privacyLoaded = false;
let privacySave = Promise.resolve();
function writePrivacyPref(hidden) {
  privacySave = privacySave.catch(() => {}).then(() => api("POST", "/api/home/preferences", { privacyHidden: hidden })).catch(e => setStatus(`${t("fundHomePrivacyPreferenceCouldNotBeSaved")}${e.message}`));
}
function applyPrivacy(hidden, persist = true) {
  // 旧布尔入口迁移：显式显示→amount，显式隐藏→hidden（方案 11 §5 兼容投影）。
  applyDisplayMode(hidden ? "hidden" : "amount", persist);
}
// ---------- 风险与行动（R11：真实调用检查/事件接口，非占位） ----------
let checkGen = 0;
let checkController = null;
async function runCheckAndRender() {
  const body = document.getElementById("home-actions-body");
  if (!active || document.hidden) return;
  const gen = ++checkGen;
  checkController?.abort();
  const controller = new AbortController();
  checkController = controller;
  document.getElementById('home-today-body')?.setAttribute('aria-busy','true');
  if(body)body.innerHTML = `<div class="home-empty-hint">${t("fundHomeCheckingRules")}</div>`;
  try {
    const data = await api("POST", "/api/check/run", {}, {signal:controller.signal});
    if (!active || gen !== checkGen) return;
    if (data.error) {
      if(body)body.innerHTML = `<div class="home-empty-hint">${t("fundHomeRuleCheckFaileddc70b4")}${escapeHtml(data.message || data.error || "")}</div>`;
      else setStatus(t('fundHomeRuleCheckFailed'));
      return;
    }
    H.actions = data; renderActions(data);
    await loadSnapshot(false, true);
    if (active && gen === checkGen) home.notifyPending?.();
  } catch (e) {
    if (active && gen === checkGen && !controller.signal.aborted) {
      if(body)body.innerHTML = `<div class="home-empty-hint">${t("fundHomeRuleCheckFailed")}${escapeHtml(e.message)}）。</div>`;
      else setStatus(t('fundHomeRuleCheckFailed'));
    }
  } finally {
    if (checkController === controller) {checkController = null;document.getElementById('home-today-body')?.setAttribute('aria-busy','false');}
  }
}

function renderActions(data) {
  const body = document.getElementById("home-actions-body");
  if (!body) return;
  const count = document.getElementById("home-alert-count");
  if (count) count.textContent = data.eventCount > 0 ? `${data.eventCount}${t("fundHomeItems")}` : "";
  const items = data.actions || [];
  if (!items.length) {
    body.innerHTML = `<div class="home-empty-hint">${t("fundHomeNoEventsOrActionCandidatesWereGenerated")}</div>`;
    return;
  }
  body.innerHTML = items.map((a, i) => `
    <div class="action-item" data-idx="${i}">
      <div class="action-head"><b>${escapeHtml(a.resultType)}</b> · ${hidePrivate() ? t("fundHomeObject") : escapeHtml(a.objectName)}</div>
      <div class="action-reason">${hidePrivate() ? t("fundHomeARuleWasTriggeredShowAmountsTo") : escapeHtml(a.reason)}</div>
      ${a.amountCapMinor != null ? `<div class="action-cap">${t("fundHomeReferenceAmountCap")}${mask(fmtMinorFull(a.amountCapMinor))}</div>` : ""}
      ${(a.missing || []).length ? `<div class="action-missing">${t("fundHomeNeedsVerification")}${a.missing.map(m => escapeHtml(m)).join("；")}</div>` : ""}
      ${a.eventId != null ? `<div class="action-ops">
        <button class="btn btn-sm" data-event="${a.eventId}" data-op="ack">${t("fundHomeAcknowledge")}</button>
        <button class="btn btn-sm" data-event="${a.eventId}" data-op="resolve">${t("fundHomeResolve")}</button>
        <button class="btn btn-sm" data-event="${a.eventId}" data-op="notify">${t("fundHomeNotifyTitle")}</button>
      </div>` : ""}
    </div>`).join("");
  body.querySelectorAll("button[data-event]").forEach(btn => {
    btn.addEventListener("click", async () => {
      const id = Number(btn.dataset.event);
      const op = btn.dataset.op;
      if(op==='notify'){home.notifyBusiness?.('event',id);return;}
      const endpoint = op === "ack" ? "/api/events/ack" : "/api/events/resolve";
      try {
        await api("POST", endpoint, { id, note: t("fundHomeHomeAction") });
        setStatus(t("fundHomeSavedIfTheConditionPersistsTheNext"));
        await loadSnapshot();
      } catch (e) {
        setStatus(`${t("fundHomeEventActionFailed")}${e.message}`);
      }
    });
  });
}

actions.homeOpenSources = () => home.manageSources?.();
actions.homeRunCheck = runCheckAndRender;
// ---------- 入口 ----------
// P02：目标与配置 Tab 已撤销；目标管理收口为首页弹窗。
function openGoalManage() {
  home.manageGoals?.();
}

let active = false;
home.notificationSessionActive = () => active && !suspended && !document.hidden;
// 持仓页等次视图的显示开关复用同一 owner：持久化 + fund:privacy-changed 广播。
home.applyDisplayMode = applyDisplayMode;
let entryGen = 0, preferenceController = null, suspended = false, entryPending = null;
function stopHomeQueries() {
  suspended = true;
  home.stopOpportunityRefresh?.();
  if (checkController && H.actions) renderActions(H.actions);
  snapshotGen++; marketGen++; checkGen++; benchmarkGen++; entryGen++;
  for (const controller of [snapshotController, marketController, checkController, preferenceController, benchmarkController]) controller?.abort();
  snapshotController = marketController = checkController = preferenceController = benchmarkController = null;
  document.getElementById('home-today-body')?.setAttribute('aria-busy','false');
}
actions.leaveHome = function () { active = false; document.documentElement.setAttribute("data-motion-paused", ""); stopHomeQueries(); actions.leaveNewsPage?.();actions.leaveDecisionHistory?.();home.cancelAI?.(); home.closePanel?.(); };
async function enterHome() {
  if (!active || document.hidden) return;
  suspended = false;
  const gen = ++entryGen;
  if (!privacyLoaded) {
    preferenceController?.abort();
    const controller = new AbortController(); preferenceController = controller;
    try {
      const p = await api("GET", "/api/home/preferences", null, {signal:controller.signal});
      if (gen !== entryGen || controller.signal.aborted) return;
      applyDisplayMode(p.displayMode || (p.privacyHidden !== false ? "hidden" : "amount"), false); privacyLoaded = true;
    } catch (e) {
      if (gen !== entryGen || controller.signal.aborted) return;
      applyDisplayMode("hidden", false); setStatus(`${t("fundHomePrivacyPreferenceCouldNotLoad")}${e.message}`);
    } finally { if (preferenceController === controller) preferenceController = null; }
  }
  if (!active || gen !== entryGen) return;
  const snapshot = await loadSnapshot();
  if (!snapshot || !active || gen !== entryGen || document.hidden) return;
  home.resumeOpportunityRefresh?.();
  renderMarket();
  await runCheckAndRender();
  if(active&&gen===entryGen)void actions.refreshHomeValuation(true);
}
actions.loadHome = async function () {
  if (active) return entryPending;
  active = true; document.documentElement.toggleAttribute("data-motion-paused",document.hidden);
  const pending = entryPending = enterHome();
  try { await pending; } finally { if (entryPending === pending) entryPending = null; }
};
actions.refreshHome = async function () {
  if (document.hidden) return;
  const gen = ++entryGen;
  H.benchmark = null;
  const snapshot = await loadSnapshot(true);
  if (snapshot && gen === entryGen && active && !document.hidden) await Promise.all([renderMarket(), runCheckAndRender()]);
};
window.addEventListener("pagehide", stopHomeQueries);
actions.refreshHomeValuation = (refreshEstimates = false) => {
  if (!active || document.hidden || snapshotController || document.querySelector("dialog[open]") || ["INPUT","TEXTAREA","SELECT"].includes(document.activeElement?.tagName)) return;
  return loadSnapshot(false,true,refreshEstimates);
};
function resumeHome() {
  if (!suspended || !active || document.hidden) return;
  if (!privacyLoaded || !H.snapshot) { void enterHome(); return; }
  suspended = false;
  if (Date.now()/1000-Number(H.snapshot.generatedAt)>30*60 || H.snapshot.portfolio?.ledger?.day?.date !== new Date(Date.now()+8*3600000).toISOString().slice(0,10)) void actions.refreshHomeValuation();
  home.resumeOpportunityRefresh?.();
  if (!H.benchmark) void loadBenchmark(H.snapshot);
  if (!H.market) void renderMarket();
  if (!H.actions) void runCheckAndRender();
}
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    document.documentElement.setAttribute("data-motion-paused", "");
    stopHomeQueries();
  } else {
    document.documentElement.toggleAttribute("data-motion-paused",!active);
    resumeHome();
  }
});
window.addEventListener("pagehide", () => {
  document.documentElement.setAttribute("data-motion-paused", "");
  stopHomeQueries();
});
window.addEventListener("pageshow", event => {
  document.documentElement.toggleAttribute("data-motion-paused",!active);
  if (event.persisted) resumeHome();
});
// mode/T05：快照以不可变快照发布——owner 在存储时一次性深冻结（不复制
// 大对象），消费方经 snapshot() 只读访问，无法改写内部对象。
function freezeSnapshot(value) {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const key of Object.keys(value)) freezeSnapshot(value[key]);
  }
  return value;
}
Object.assign(home, {
  isActive: () => active && !document.hidden,
  // 公共只读 selector（mode/T05）：三态隐私与当前快照；写入唯一走
  // applyDisplayMode / loadSnapshot（owner 内部）。
  displayMode: () => displayMode,
  privacy: () => displayMode !== "amount",
  // 读取时冻结（幂等）：无论 owner 存储路径如何，公开返回值都是不可变快照。
  snapshot: () => freezeSnapshot(H.snapshot),
  readSnapshot: async () => { await entryPending; return loadSnapshot(false, true); },
  // 基准序列只读投影（基准调试/验收用；写入唯一走 refreshBenchmark）。
  benchmark: () => freezeSnapshot(H.benchmark),
  // 行情加载失败态只读投影（错误区域验证用；写入唯一在 loadMarket）。
  marketFailure: () => freezeSnapshot(H.marketFailure),
  escape: escapeHtml,
  money: minor => mask(fmtMinorFull(minor)),
  status: setStatus,
  refresh: actions.refreshHome,
  requestId: () => Array.from(crypto.getRandomValues(new Uint8Array(16)), b => b.toString(16).padStart(2, "0")).join(""),
});

actions.setHomeRange = function (range, from = "", to = "") {
  if(!["today","month","year","all","custom"].includes(range))return Promise.resolve();
  if(range==="custom"&&(!/^\d{4}-\d{2}-\d{2}$/.test(from)||!/^\d{4}-\d{2}-\d{2}$/.test(to)||from>to||to>(H.snapshot?.requestedRange?.today||new Date(Date.now()+8*3600000).toISOString().slice(0,10))))return Promise.resolve();
  H.range = range;
  H.from = from; H.to = to;
  H.benchmark=null;benchmarkGen++;benchmarkController?.abort();benchmarkController=null;
  Fund.homeRange?.update({key:range,from,to});
  return loadSnapshot(false,true);

};

actions.toggleHomeDetail = toggleDetail;

function bindEvents() {
  document.getElementById("home-refresh-btn")?.addEventListener("click", () => actions.refreshHome());
  document.getElementById("home-privacy-btn")?.addEventListener("click", () => cycleDisplayMode());
  document.getElementById("home-goal-manage")?.addEventListener("click", openGoalManage);
  document.getElementById("home-allocation-manage")?.addEventListener("click", () => home.manageAllocation?.());
  document.querySelectorAll(".hc-detail").forEach(btn => {
    btn.addEventListener("click", () => actions.toggleHomeDetail(btn.dataset.detail));
  });
  document.getElementById("home-ai-explain")?.addEventListener("click", () => home.openAI?.());
  document.getElementById("home-cash-manage")?.addEventListener("click", () => home.manageCash?.());
  document.getElementById("home-currency-manage")?.addEventListener("click", () => home.manageCurrency?.());
  document.getElementById("home-rules-manage")?.addEventListener("click", () => home.manageRules?.());
  document.getElementById("home-today-rules")?.addEventListener("click", () => home.manageRules?.());
  document.getElementById("home-open-positions")?.addEventListener("click", () => document.getElementById("tab-positions")?.click());
  document.getElementById("home-simulate")?.addEventListener("click", () => home.simulate?.());
  document.getElementById("home-history")?.addEventListener("click", () => home.history?.());
  document.getElementById("home-reports-manage")?.addEventListener("click", () => home.manageReports?.());
  document.getElementById("home-sources-manage")?.addEventListener("click", () => {
    if (actions.homeOpenSources) actions.homeOpenSources();
  });
}

function localizeHome() {
  if(H.snapshotFailure){
    const failure=H.snapshotFailure;
    setStatus(`${t("fundHomeHomeSnapshotCouldNotLoad")}${Fund.localizeApiError(failure.status,failure.code,"")}`);
    H.snapshotFailure=failure;
  }
  const root = document.getElementById("view-home");
  const tab = document.getElementById("tab-home");
  if (tab) tab.textContent = t("fundHomeHome");
  root?.querySelectorAll("[data-i18n]").forEach(node => { node.textContent = t(node.dataset.i18n); });
  for (const attr of ["title", "aria-label"]) root?.querySelectorAll(`[data-i18n-${attr}]`).forEach(node => { node.setAttribute(attr, t(node.getAttribute(`data-i18n-${attr}`))); });
  const privacy = document.getElementById("home-privacy-btn");
  if (privacy) privacy.textContent = t(displayMode === "amount" ? "fundHomeDisplayPercent" : displayMode === "percent" ? "fundHomeDisplayHidden" : "fundHomeDisplayAmount");
  renderSnapshot();
  if (H.market) renderMarket(H.market);
  if (H.snapshot?.generatedAt) document.getElementById("home-asof").textContent = t("fundHomeDataAsOf") + new Date(Number(H.snapshot.generatedAt) * 1000).toLocaleTimeString(state.language, { timeZone: "Asia/Shanghai" });
  if (H.detailOpen) document.getElementById("home-detail-body").innerHTML = buildDetail(H.detailOpen);
}
document.addEventListener("fund:language-changed", localizeHome);
localizeHome();

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", bindEvents);
} else {
  bindEvents();
}

Object.assign(home, { mask });
})();
