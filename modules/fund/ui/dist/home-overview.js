// R2 overview projection; lifecycle and snapshot ownership stay in home.js.
(() => {
const { state, t } = Fund;
const home = Fund.home = Fund.home || {};
// 集中度列表分页状态（卡内保留，语言切换/快照刷新不重置页码）。
const concentrationState = { page: 0 };
// groupId 为空时显示全部资产，否则只看所选分类；隐私遮金额时强制百分比视图。
// cardsOpen：分类／资产卡行是否已展开（超过两行才折叠），按行分别记住。
const historySelection = { groupId: null, detailsOff: new Set(), view: "amount", cursor: null, cardsOpen: {} };
// 资产走势分组配色（demo 复刻）：与 pill 圆点、主图分类线、各分类走势卡共用；
// --series-1 与 accent 同色保留给总资产，分组从第 2 色起循环。
const SERIES_PALETTE = ["--series-2", "--fund-allocation-2", "--series-5", "--fund-allocation-5", "--fund-allocation-3", "--series-3", "--series-6", "--fund-allocation-4"];
const percent = (bps) => bps == null ? "–" : (Number(bps) / 100).toLocaleString(state.language, { maximumFractionDigits: 2 }) + "%";
const dateLabel = date => new Date(date + "T00:00:00+08:00").toLocaleDateString(state.language, { timeZone: "Asia/Shanghai" });
const reasons = {
  missing_baseline: "fundHomeOverviewMissingBaseline", missing_end: "fundHomeOverviewMissingEnd",
  missing_daily_history: "fundHomeOverviewMissingDays", unverified_history: "fundHomeOverviewUnverified",
  nonpositive_baseline: "fundHomeOverviewZeroBaseline",
};
function signedPct(value) {
  if (!Number.isFinite(value)) return "";
  const rounded = Math.abs(value) < 0.005 ? 0 : value;
  return `${rounded > 0 ? "+" : ""}${rounded.toFixed(2)}%`;
}
// 分类选择只有一层：顶部分类卡（含「全部资产」）是唯一入口；
// 底部资产卡跟随所选分类显示它的全部资产，总资产只在“全部资产”视图绘制。
// 口径切换（资产金额/变化对比）是 partials 静态按钮；此处只维护状态并绑定一次。
function updateViewToggle(view) {
  const box = document.getElementById("home-view-toggle");
  if (!box) return;
  const amountEnabled = home.displayMode() === "amount";
  for (const button of box.querySelectorAll("[data-history-view]")) {
    const on = button.dataset.historyView === view;
    button.classList.toggle("active", on);
    button.setAttribute("aria-pressed", String(on));
    if (button.dataset.historyView === "amount") {
      button.disabled = !amountEnabled;
      button.title = amountEnabled ? "" : t("fundHomeHistoryViewAmountLocked");
    }
  }
}
document.getElementById("home-view-toggle")?.querySelectorAll("[data-history-view]").forEach(button => {
  button.addEventListener("click", () => {
    if (button.disabled) return;
    historySelection.view = button.dataset.historyView;
    historySelection.cursor = null;
    home.renderOverview(home.snapshot());
    button.focus();
  });
});
home.renderOverview = snapshot => {
  const p = snapshot.portfolio;
  const body = document.getElementById("home-health-body");
  if (!body) return;
  if (!p) {
    body.textContent = t("fundHomeMissingData");
    return;
  }
  const hidden = home.displayMode() === "hidden";
  const amount = home.displayMode() === "amount";
  const s = p.securities;
  const d = p.diagnostics;
  const h = p.history;
  let intraday=h.timeResolution==='minute';
  const pointTime=(point,minute=intraday)=>minute&&point.observedAt?Number(point.observedAt)*1000:Date.parse(point.date+"T00:00:00+08:00");
  const pointLabel=(point,minute=intraday)=>minute&&point.observedAt?new Date(pointTime(point,minute)).toLocaleTimeString(state.language,{timeZone:'Asia/Shanghai',hour:'2-digit',minute:'2-digit'}):dateLabel(point.date);
  const text = (id, value) => { const el = document.getElementById(id); if (el) el.textContent = value; };
  const money = value => value == null ? "–" : home.money(value);
  const day=p.ledger?.day,estimate=p.ledger?.estimatedDay,dailyDate=snapshot.requestedRange?.today;
  const useEstimate=!day?.complete&&estimate?.estimatedCount>0&&estimate.date===day?.date;
  const daily=useEstimate?estimate:day;
  const dailyAmount=(!dailyDate||daily?.date===dailyDate)?daily?.amountMinor:null;
  text("home-pending-summary", money(dailyAmount));
  const dailyNotes=[daily?.date||dailyDate||"",t('fundHomeDailySummaryScope'),useEstimate?t('fundHomeIncomeEstimateNote',{n:estimate.estimatedCount}):"",daily?.missingCount?t('fundLedgerIncomePartial',{n:daily.missingCount}):""].filter(Boolean);
  text('home-daily-summary-note',dailyNotes.join(' · '));
  const dailyElement=document.getElementById('home-pending-summary');if(dailyElement)dailyElement.style.color=amount&&dailyAmount!=null?(BigInt(dailyAmount)>0n?'var(--color-up)':BigInt(dailyAmount)<0n?'var(--color-down)':''):'';
  text("home-review-summary", hidden ? "****" : snapshot.alerts?.openCount ?? t("fundHomeMissingData"));
  text("home-recorded-basics", hidden ? "****" : s.holdingCount > 0
    ? t("fundHomeRecordedBasics").replace("{n}", s.holdingCount).replace("{cost}", money(s.knownCostMinor)) : "");
  const ledger=p.ledger;
  const metric = (key, value, id) => `<div><dt>${home.escape(t(key))}</dt><dd class="num" ${id?`id="${id}"`:""}>${home.escape(money(value))}</dd></div>`;
  const rate = (profit, cost) => hidden ? "****" : profit == null || cost == null || BigInt(cost)<=0n ? "–" : percent(BigInt(profit)*10000n/BigInt(cost));
  for(const kind of ["stock","fund"]){
    const data=ledger?.byType?.[kind],card=document.querySelector(`#home-${kind}-income [data-income-body]`);
    const notes=[kind==="fund"?t("fundHomeFundIncomeScope"):"",data?.day?.date||"",data?.day?.missingCount?t("fundLedgerIncomePartial",{n:data.day.missingCount}):"",data?.unknownCostCount||data?.missingValueCount?t("fundHomeIncomeGaps",{n:data.unknownCostCount+data.missingValueCount}):""].filter(Boolean);
    if(!data){card.textContent=t("fundHomeIncomeMissing");continue;}
    const profit=data.holdingProfitMinor;
    const estimated=!data.day?.complete&&data.estimatedDay?.estimatedCount>0;
    if(estimated)notes.push(t('fundHomeIncomeEstimateNote',{n:data.estimatedDay.estimatedCount}));
    card.innerHTML=`<span class="hc-label">${t("fundHomeHoldingIncome")}</span><div class="hc-value num" id="home-${kind}-profit">${home.escape(money(profit))}</div><div class="home-income-rate">${rate(profit,data.costMinor)}</div><dl class="home-income-metrics">${metric(estimated?'fundHomeEstimatedDailyIncome':"fundHomeDailyIncome",data.holdingCount===0?null:estimated?data.estimatedDay.amountMinor:data.day?.amountMinor,`home-${kind}-daily`)}${metric("fundHomeRealizedIncome",data.realizedMinor)}${metric("fundHomeIncomeValue",data.marketValueMinor,`home-${kind}-value`)}</dl><p class="hc-sub" id="home-${kind}-income-note">${home.escape(data.holdingCount===0?t("fundHomeIncomeEmpty"):notes.join(" · "))}</p>`;
    if(amount&&profit!=null)card.querySelector('.hc-value').style.color=BigInt(profit)>0n?'var(--color-up)':BigInt(profit)<0n?'var(--color-down)':'';
  }
  const offline=p.offlineIncome,offlineBody=document.querySelector('#home-offline-income [data-income-body]');
  offlineBody.innerHTML=offline?`<span class="hc-label">${t("fundHomeOfflineBookIncome")}</span><div class="hc-value num" id="home-offline-profit">${home.escape(money(offline.bookProfitMinor))}</div><div class="home-income-rate">${rate(offline.bookProfitMinor,offline.costMinor)}</div><dl class="home-income-metrics">${metric("fundHomeOfflineValue",offline.valueMinor)}${metric("fundHomeOfflineRecovered",offline.netRecoveredMinor)}${metric("fundHomeOfflineReceivable",offline.receivableMinor)}</dl><p class="hc-sub">${home.escape(offline.count===0?t("fundHomeIncomeEmpty"):[offline.unknownCostCount?t("fundHomeIncomeGaps",{n:offline.unknownCostCount}):"",offline.missingHistoryCount?t("fundHomeOfflineHistoryGap"):"",offline.staleCount?t("fundHomeOfflineStale",{n:offline.staleCount}):"",t("fundHomeOfflineBookNote")].filter(Boolean).join(" · "))}</p>`:home.escape(t("fundHomeIncomeMissing"));
  text("home-all-daily-value",money(p.dailyReturn.netReturnMinor));
  text("home-all-daily-note",(snapshot.requestedRange?.today||ledger?.day?.date||"")+" · "+(["ready","partial","reference"].includes(p.dailyReturn.status)?t(p.dailyReturn.status==="reference"?"fundHomeRangeReference":"fundHomeOverviewFlowExcluded"):t("fundHomeOverviewDailyMissing")));
  text("home-all-period-value",money(snapshot.wealth.returns?.netReturnMinor));
  text("home-twr-value", hidden ? "****" : h.twrPpm == null ? "–" : percent(BigInt(h.twrPpm) / 100n));
  const total = snapshot.wealth.totalA;
  const invested = snapshot.wealth.investedI;
  text("home-position-value", hidden ? "****" : total != null && invested != null && BigInt(total) > 0n
    ? percent(BigInt(invested) * 10000n / BigInt(total)) : "–");
  text("home-returns-scope", t("fundHomeLedgerScope"));
  const row = (name, value) => `<div class="goal-kv"><span class="k">${name}</span><span class="v">${value}</span></div>`;
  body.innerHTML = row(t("fundHomeOverviewConcentration"), hidden ? "****" : d.hhiBps == null ? "–" : (d.hhiBps / 10000).toLocaleString(state.language, {maximumFractionDigits:4}))
    + row(t("fundHomeOverviewCoverage"), t(d.coverage === "complete" ? "fundHomeOverviewComplete" : "fundHomeIncompleteData"))
    + row(t("fundHomeOverviewMissingPrice"), hidden ? "****" : s.missingValueCount)
    + row(t("fundHomeOverviewUnknownCost"), hidden ? "****" : s.unknownCostCount)
    + row(t("fundHomeOverviewStale"), hidden ? "****" : s.staleCount)
    + `<p class="evidence-hint">${t("fundHomeOverviewNoLookThrough")}</p>`;
  const concentration = document.getElementById("home-concentration-list");
  if (concentration) {
    // 集中度条目：名称＋代码＋权重，每页 5 条分页（名称来自后端诊断投影，
    // 缺名称回退 key 尾段代码）。分页状态保持在卡内，重渲染保留当前页。
    const items = hidden ? [] : d.securities.slice().sort((a,b)=> (b.weightBps ?? -1)-(a.weightBps ?? -1));
    const PAGE = 5;
    const pages = Math.max(1, Math.ceil(items.length / PAGE));
    concentrationState.page = Math.min(concentrationState.page, pages - 1);
    const slice = items.slice(concentrationState.page * PAGE, concentrationState.page * PAGE + PAGE);
    concentration.innerHTML = slice.map(item => {
      const code = item.key.split(":").slice(-1)[0];
      const label = item.name ? `${home.escape(item.name)} <small class="home-conc-code">${home.escape(code)}</small>` : home.escape(code);
      return `<div class="goal-kv"><span class="home-conc-name">${label}</span><span>${percent(item.weightBps)}</span></div>`;
    }).join("") + (pages > 1 ? `<nav class="home-conc-pager" aria-label="${t("fundHomeOverviewConcentration")}">
        <button type="button" data-conc-prev ${concentrationState.page === 0 ? "disabled" : ""} aria-label="${t("fundHomePrevious")}">‹</button>
        <span>${concentrationState.page + 1}/${pages}</span>
        <button type="button" data-conc-next ${concentrationState.page >= pages - 1 ? "disabled" : ""} aria-label="${t("fundHomeNext")}">›</button>
      </nav>` : "");
    concentration.querySelectorAll("[data-conc-prev]").forEach(b => b.onclick = () => { concentrationState.page = Math.max(0, concentrationState.page - 1); home.renderOverview(snapshot); });
    concentration.querySelectorAll("[data-conc-next]").forEach(b => b.onclick = () => { concentrationState.page = Math.min(pages - 1, concentrationState.page + 1); home.renderOverview(snapshot); });
  }
  const today = document.getElementById("home-today-body");
  if (today) today.innerHTML = row(t("fundHomeOverviewOpenAlerts"), hidden ? "****" : snapshot.alerts.openCount)
    + `<p class="evidence-hint">${t(s.missingValueCount || s.unknownCostCount || s.staleCount ? "fundHomeOverviewCheckData" : "fundHomeOverviewCheckRules")}</p>`;
  const chart = document.getElementById("home-history-chart");
  const groupsById = new Map((snapshot.groupAllocation?.groups || []).map(item => [String(item.groupId), item]));
  const liveValue = point => !point.observedAt && point.source !== "holding-nav-replay" && point.date === snapshot.requestedRange?.today && point.quality === "reference";
  const groupValue = (point, id) => {
    const recorded = (point.groups || []).find(item => String(item.groupId) === id);
    if (recorded) return recorded.valueMinor ?? null;
    if (liveValue(point)) return groupsById.get(String(id))?.currentMinor ?? null;
    return null;
  };
  const seriesPointsFor = (points, id) => id === "total" ? points : points.map(point => ({ ...point, valueMinor: groupValue(point, id) }));
  const usableOf = points => points.filter(point => point.valueMinor != null && ["ready", "reference"].includes(point.quality));
  // 区间变化 =（末点 − 首点）／首点，只用可绘制点。它与 Demo 的分类卡同口径：
  // 描述该序列自身的估值变化，不是已核对收益（现金流仍在其中）。
  const changeOf = points => {
    const usable = usableOf(points);
    if (usable.length < 2) return null;
    const first = BigInt(usable[0].valueMinor), last = BigInt(usable.at(-1).valueMinor);
    if (first <= 0n) return null;
    return Number((last - first) * 10000n / first) / 100;
  };
  // 分类还没有历史点时，主图会改用当天的真实观察；标签也必须用同一批点，
  // 否则会出现“有曲线但没有区间变化”的不一致。
  const hasCategoryHistory = id => h.points.some(point => point.date !== snapshot.requestedRange?.today && ["ready", "reference"].includes(point.quality) && (point.groups || []).some(g => String(g.groupId) === id && g.valueMinor != null));
  // 分类还没有历史点时改用当天真实观察；pill、主图与各分类走势卡共用同一切换。
  const seriesFor = id => id === "total" || hasCategoryHistory(id) || !h.todayPoints?.length ? h.points : h.todayPoints;
  const changeFor = id => changeOf(seriesPointsFor(seriesFor(id), id));
  const changes = { total: changeFor("total") };
  for (const item of groupsById.values()) changes[String(item.groupId)] = changeFor(String(item.groupId));
  const groupIds = [...groupsById.keys()];
  const seriesColor = id => id === "total" || groupIds.indexOf(id) < 0
    ? "var(--series-1)"
    : `var(${SERIES_PALETTE[groupIds.indexOf(id) % SERIES_PALETTE.length]})`;
  // 两级选择：类型全选＝按类型汇总；未全选＝按所选类型下的明细。
  const view = home.displayMode() === "amount" ? (historySelection.view === "percent" ? "percent" : "amount") : "percent";
  // 每条线独立取序列：分类无历史时回退当天真实观察（pill/主图/分类卡同口径）。
  const seriesOf = id => seriesPointsFor(seriesFor(id), id);
  const assetsOf = id => (groupsById.get(id)?.assets || []);
  // 可绘制＝至少两个可用估值点，且不是全零。没绑资产的分组当前值为 0，
  // 画出来是一条贴底的 0 线：既压扁纵轴，也让人误以为“选了没反应”。
  // 这类类型只保留按钮位置（置灰+原因），不参与选择、不参与绘制。
  const isAllZero = points => points.length > 0 && points.every(point => BigInt(point.valueMinor) === 0n);
  const plottableTypes = new Set(groupIds.filter(id => {
    const usable = usableOf(seriesOf(id));
    return usable.length >= 2 && !isAllZero(usable);
  }));
  if (!groupsById.has(historySelection.groupId)) historySelection.groupId = null;
  const detailMode = historySelection.groupId != null;
  const selectedTypes = detailMode ? [historySelection.groupId] : groupIds;
  const allTypesOn = !detailMode;
  // 顶部卡行＝全部资产 + 各分类；底部卡行＝所选分类下的资产（选“全部”时列出所有资产）。
  const listedGroups = detailMode ? [selectedTypes[0]] : groupIds;
  const listedAssets = listedGroups.flatMap(id => assetsOf(id).map(asset => ({ ...asset, groupId: id, groupName: groupsById.get(id)?.name || id })));
  const validDetails = new Set(listedAssets.map(asset => asset.historyId));
  historySelection.detailsOff = new Set([...historySelection.detailsOff].filter(id => validDetails.has(id)));
  const liveAssetValue = id => [...groupsById.values()].flatMap(group => group.assets || []).find(asset => asset.historyId === id)?.valueMinor ?? null;
  const holdingPoints = (points, id) => points.map(point => {
    const recorded = (point.groups || []).flatMap(group => group.assets || []).find(asset => asset.historyId === id);
    // A saved observation (including an explicit unknown) wins over daily NAV replay;
    // the live reference point carries no groups, so reuse the current asset value.
    let valueMinor = recorded ? recorded.valueMinor : point.holdingValues?.[id] ?? null;
    if (!recorded && !Object.hasOwn(point.holdingValues || {}, id) && liveValue(point)) valueMinor = liveAssetValue(id);
    return { ...point, valueMinor, quality: valueMinor == null ? "incomplete" : "reference" };
  });
  const detailAccount = asset => snapshot.wealth.cash?.accounts?.find(a => String(a.accountId) === String(asset.accountId))?.accountName || asset.accountId;
  const detailEntries = listedAssets.map(asset => {
    const daily = holdingPoints(h.points, asset.historyId);
    const hasHistory = usableOf(daily).some(point => point.date !== snapshot.requestedRange?.today);
    const points = hasHistory || !h.todayPoints?.length ? daily : holdingPoints(h.todayPoints, asset.historyId);
    // 选“全部”时底部混列各分类资产，必须带上分类名；只看单个分类时分类名从标题就能看出来。
    const detail = [detailMode ? "" : asset.groupName, asset.code, asset.accountId ? detailAccount(asset) : ""].filter(Boolean).join(" · ");
    return { id: asset.historyId, name: asset.name, groupId: asset.groupId, detail, points };
  });
  const shownDetails = detailEntries.filter(entry => !historySelection.detailsOff.has(entry.id));
  // 没有明细（只绑了现金/线下等不可回放资产）的类型退回按类型汇总，避免选中后消失。
  const bareTypes = selectedTypes.filter(id => !assetsOf(id).length && plottableTypes.has(id));
  const detailColor = id => `var(${SERIES_PALETTE[Math.max(0, detailEntries.findIndex(entry => entry.id === id)) % SERIES_PALETTE.length]})`;
  const plotted = [
    ...(!detailMode ? [{ id: "total", name: t("fundHomeHistoryGroupTotal"), points: seriesOf("total") }] : []),
    ...(detailMode
      ? [...shownDetails.map(entry => ({ id: entry.id, name: entry.name, points: entry.points })), ...bareTypes.map(id => ({ id, name: groupsById.get(id)?.name, points: seriesOf(id) }))]
      : selectedTypes.filter(id => plottableTypes.has(id)).map(id => ({ id, name: groupsById.get(id)?.name, points: seriesOf(id) }))),
  ];
  const entryColor = id => id === "total"
    ? "var(--series-1)"
    : detailMode && detailEntries.some(entry => entry.id === id) ? detailColor(id) : seriesColor(id);
  const plot = plotted.map(item => ({ ...item, color: entryColor(item.id), usable: usableOf(item.points), intraday: item.points.length > 0 && item.points.every(p => p.observedAt && p.date === snapshot.requestedRange?.today) }));
  // 任一序列使用日内观察（分类当日回退）即按分钟断线；纯日点序列不受影响。
  intraday = plot.some(item => item.intraday);
  const reconstructed = plot.some(item => item.points.some(point => point.source === "holding-nav-replay" || (detailMode && point.holdingValues?.[item.id] != null)));
  // 参考序列（日内/回放/观察）统一虚线标识，来源态在状态行说明。
  let historyStatus = hidden ? t("fundHomeOverviewHistoryHidden") : intraday ? t('fundHomeHistoryIntraday') : reconstructed ? t('fundHomeHistoryReconstructed') : h.reason ? t(reasons[h.reason] || "fundHomeOverviewUnverified") : t("fundHomeOverviewFlowTiming");
  if (!hidden && h.replayReason) historyStatus += ' · ' + t('fundHomeHistoryReplayLimit');
  // 选中的类型里有画不出来的：分「没有可回放资产」与「有效估值不足两天」两种，
  // 否则用户只会看到图上少了几条线却不知道为什么。
  // 收窄到具体类型后才提示“这一类画不出来”；全选时置灰按钮自己说明了原因。
  const missingTypes = detailMode ? selectedTypes.filter(id => !plottableTypes.has(id)) : [];
  if (!hidden && missingTypes.length) {
    const emptyOnly = missingTypes.every(id => !assetsOf(id).length);
    historyStatus += " · " + t(emptyOnly ? "fundHomeHistoryTypeEmpty" : "fundHomeHistoryGroupUnavailable");
  }
  text("home-history-status", historyStatus);
  if (h.truncated) text("home-history-status", document.getElementById("home-history-status").textContent + " · " + t("fundHomeRangeTruncated"));
  if (h.status === "reference") text("home-history-status", document.getElementById("home-history-status").textContent + " · " + t("fundHomeRangeReference"));
  const selection = document.getElementById("home-history-selection");
  // 选中说明带区间前缀：全部资产 / 单个分类名。
  const rangeNameKey = { today: "fundHomePeriodToday", week: "fundHomePeriodWeek", month: "fundHomePeriodMonth", half: "fundHomePeriodHalf", year1: "fundHomePeriodYear", year: "fundHomeThisYear", all: "fundHomePeriodAll", custom: "fundHomeCustom" }[snapshot.requestedRange?.key || "month"] || "fundHomeRangeMonth";
  let selectionText;
  if (!selectedTypes.length) selectionText = t("fundHomeHistoryPickType");
  else if (allTypesOn) selectionText = t("fundHomeHistoryAllAssets");
  else selectionText = groupsById.get(selectedTypes[0])?.name || selectedTypes[0];
  if (selection) selection.textContent = hidden ? "****" : `${t(rangeNameKey)} · ${selectionText}`;
  // 口径按钮状态 + 资产计数（顶部选分类、底部列资产，计数跟随底部列表）。
  updateViewToggle(view);
  const countBox = document.getElementById("home-series-count");
  if (countBox) {
    countBox.textContent = detailMode
      ? t("fundHomeHistoryDetailsCount", { n: detailEntries.length - historySelection.detailsOff.size, m: detailEntries.length })
      : t("fundHomeHistoryAssetsTotal", { n: detailEntries.length });
  }
  text("home-benchmark-status", t("fundHomeOverviewNoBenchmark"));
  if (chart) {
    chart.replaceChildren();
    // All series share one scale; missing values remain gaps.
    const amountView = view === "amount";
    const drawable = plot.map(item => {
      const base = item.usable.length ? BigInt(item.usable[0].valueMinor) : 0n;
      // 总资产保留单点（用于“只有一个估值”的兜底展示）；其余分类至少要两个
      // 点才成线，且全零序列（没绑资产的分组）不画，免得贴底 0 线压扁纵轴。
      const enough = item.id === "total" ? item.usable.length >= 1 : item.usable.length >= 2;
      const zeroOnly = item.id !== "total" && item.usable.length > 0 && item.usable.every(point => BigInt(point.valueMinor) === 0n);
      return { ...item, base, area: item.id === "total", drawable: enough && !zeroOnly && (amountView || base > 0n) };
    }).filter(item => item.drawable);
    if (hidden) {
      chart.innerHTML=`<div class="history-empty"><p>${t("fundHomeOverviewHistoryHidden")}</p><button type="button" class="btn" data-history-reveal>${t("fundHomeShowAmounts")}</button></div>`;
      chart.querySelector('[data-history-reveal]').onclick=()=>home.applyDisplayMode('amount');
    } else if (!plot.length) {
      chart.innerHTML=`<div class="history-empty"><p>${home.escape(t(selectedTypes.length ? "fundHomeHistoryChoose" : "fundHomeHistoryPickType"))}</p></div>`;
    } else if (!drawable.length || drawable.every(item => item.usable.length < 2)) {
      const usable = plot.find(item => item.usable.length)?.usable || [];
      const point=usable[0],w=snapshot.wealth;
      const current=snapshot.requestedRange?.to===snapshot.requestedRange?.today;
      const value=point?.valueMinor??(!detailMode&&current?(w.quality!=="empty"?(w.totalA??w.knownA):null):null);
      if(value!=null){
        const partial=(point?point.quality!=="ready"&&point.quality!=="reference":w.totalA==null);
        const key=partial?"fundHomeValuedAssets":point?"fundHomeHistoryOneValue":"fundHomeHistoryCurrentValue";
        const date=point?.date||w.latestValuation;
        chart.innerHTML=`<section class="history-single"><div><span>${t(key)}</span><strong>${home.escape(money(value))}</strong><small>${date?home.escape(point?pointLabel(point):dateLabel(date)):t("fundHomeSourceTimestampUnknown")}${!point||point.quality==="reference"?" · "+t("fundHomeRangeReference"):""}</small></div><svg viewBox="0 0 600 120" role="img" aria-label="${t("fundHomeOverviewHistoryLabel")}"><circle cx="300" cy="60" r="6" fill="currentColor"/></svg><p>${t(intraday?"fundHomeHistoryIntradayOnePoint":"fundHomeHistoryOnePoint")}</p></section>`;
        if(!point&&w.quality!=="ready")chart.insertAdjacentHTML('beforeend',`<p class="evidence-hint">${t("fundHomeHistoryValuationGap")}</p>`);
      }else chart.innerHTML=`<div class="history-empty"><p>${t(current?"fundHomeOverviewNoHistory":"fundHomeHistoryRangeEmpty")}</p>${!current?`<button class="btn" type="button" data-history-today>${t("fundHomeToday")}</button>`:""}</div>`;
      if(amount){
        const button=document.createElement('button');button.type='button';button.className='btn';button.id='home-history-save';button.dataset.historySave='';button.textContent=t('fundHomeHistoryRecord');button.onclick=()=>home.manageCash('snapshot');chart.append(button);
      }
      const todayButton=chart.querySelector('[data-history-today]');if(todayButton)todayButton.onclick=()=>Fund.actions.setHomeRange('today');
    } else {
      const TOP = 14, BOTTOM = 104, LEFT = 2, RIGHT = 598;
      const timeline = drawable.flatMap(item => item.usable.map(point => ({ point, intraday: item.intraday, time: pointTime(point, item.intraday) }))).filter(entry => Number.isFinite(entry.time)).sort((a, b) => a.time - b.time);
      const times = timeline.map(entry => entry.time);
      const t0 = times.length ? Math.min(...times) : 0;
      const t1 = times.length ? Math.max(...times) : t0;
      const pctOf = (base, value) => base > 0n ? Number((BigInt(value) - base) * 10000n / base) / 100 : null;
      // 绘制值：金额视图＝绝对金额（元）；百分比视图＝按各序列首点归一化。
      const valueOf = (item, point) => amountView ? Number(point.valueMinor) / 100 : pctOf(item.base, point.valueMinor);
      const xOf = time => t1 === t0 ? (LEFT + RIGHT) / 2 : LEFT + (time - t0) / (t1 - t0) * (RIGHT - LEFT);
      let lo = Infinity, hi = -Infinity;
      for (const item of drawable) for (const point of item.usable) {
        const value = valueOf(item, point);
        if (value == null || !Number.isFinite(value)) continue;
        lo = Math.min(lo, value); hi = Math.max(hi, value);
      }
      if (!Number.isFinite(lo)) { lo = 0; hi = 1; }
      const minSpan = amountView ? 1 : 0.4;
      if (hi - lo < minSpan) { const mid = (hi + lo) / 2; lo = mid - minSpan / 2; hi = mid + minSpan / 2; }
      const pad = (hi - lo) * 0.1;
      lo -= pad; hi += pad;
      const yOf = value => TOP + (hi - value) / (hi - lo) * (BOTTOM - TOP);
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.setAttribute("viewBox", "0 0 600 120");
      svg.setAttribute("preserveAspectRatio", "none");
      svg.setAttribute("role", "img");
      svg.setAttribute("aria-label", t("fundHomeOverviewHistoryLabel"));
      // 刻度按“好看步长”生成：百分比以 0% 为锚点；金额在域内取步长（不锚 0）。
      const niceStep = raw => {
        const magnitude = Math.pow(10, Math.floor(Math.log10(Math.max(raw, 1e-9))));
        for (const multiple of [1, 2, 2.5, 5, 10]) if (multiple * magnitude >= raw) return multiple * magnitude;
        return 10 * magnitude;
      };
      let step = niceStep((hi - lo) / 4.6);
      const buildTicks = () => {
        const out = amountView ? [] : [0];
        if (!amountView) {
          for (let value = step; value <= hi; value += step) out.push(Number(value.toFixed(4)));
          for (let value = -step; value >= lo; value -= step) out.push(Number(value.toFixed(4)));
        } else {
          for (let value = Math.ceil(lo / step) * step; value <= hi + 1e-9; value += step) out.push(Number(value.toFixed(6)));
        }
        return out;
      };
      let ticks = buildTicks();
      while (ticks.length > 7) { step *= 2; ticks = buildTicks(); }
      ticks.sort((a, b) => b - a);
      // 刻度用 HTML 定位在固定 42px 侧栏里：SVG 在 320px 宽时会被
      // 等比缩小，写在 viewBox 里的文字会小到不可读。
      const scale = document.createElement("div");
      scale.className = "history-y";
      scale.setAttribute("aria-hidden", "true");
      for (const value of ticks) {
        const y = yOf(value);
        const grid = document.createElementNS(svg.namespaceURI, "line");
        grid.setAttribute("x1", String(LEFT)); grid.setAttribute("x2", String(RIGHT));
        grid.setAttribute("y1", String(y)); grid.setAttribute("y2", String(y));
        grid.setAttribute("stroke", !amountView && value === 0 ? "var(--natives-edge-strong, var(--natives-edge))" : "var(--natives-edge)");
        grid.setAttribute("stroke-width", "1");
        svg.append(grid);
        const label = document.createElement("span");
        label.style.top = `${(y / 120) * 100}%`;
        if (amountView) label.textContent = Fund.homeTrend.fmtAxisAmount(value, state.language);
        else {
          const digits = step < 0.1 ? 2 : 1;
          const rounded = Math.abs(value) < step / 20 ? 0 : Number(value.toFixed(digits));
          label.textContent = `${rounded > 0 ? "+" : ""}${rounded.toFixed(digits)}%`;
        }
        scale.append(label);
      }
      // 连续线段只标线尾；缺口两侧的孤立有效点仍需可见。
      const defs=document.createElementNS(svg.namespaceURI,"defs");
      const gradient=document.createElementNS(svg.namespaceURI,"linearGradient");
      gradient.setAttribute("id","trend-area-fill");
      gradient.setAttribute("x1","0");gradient.setAttribute("y1","0");gradient.setAttribute("x2","0");gradient.setAttribute("y2","1");
      for(const [offset,opacity] of [["0",".22"],["1","0"]]){
        const stop=document.createElementNS(svg.namespaceURI,"stop");
        stop.setAttribute("offset",offset);stop.setAttribute("stop-color", drawable[0]?.color || "var(--natives-accent)");stop.setAttribute("stop-opacity",opacity);
        gradient.append(stop);
      }
      defs.append(gradient);svg.prepend(defs);
      for (const item of drawable) {
        const node = document.createElementNS(svg.namespaceURI, "g");
        node.setAttribute("data-series", item.id);
        let segment = [], lastTime = null;
        let lastDrawn = null;
        const marked = new Set();
        const mark = point => {
          if (!point || marked.has(point.join(","))) return;
          marked.add(point.join(","));
          const [ex, ey] = point;
          const dot = document.createElementNS(svg.namespaceURI, "circle");
          dot.setAttribute("cx", String(ex)); dot.setAttribute("cy", String(ey));
          dot.setAttribute("r", item.id === "total" ? "2.8" : "2.3");
          dot.setAttribute("fill", item.color); dot.setAttribute("stroke", "var(--natives-surface)"); dot.setAttribute("stroke-width", "1.2");
          node.append(dot);
        };
        const flush = () => {
          if (segment.length === 1) mark(segment[0]);
          if (segment.length > 1) {
            const path = Fund.homeTrend.smoothPath(segment);
            if (item.area) {
              const area = document.createElementNS(svg.namespaceURI, "path");
              // 金额视图面积从图表底部起；百分比视图以 0% 基线起；上边沿共用平滑曲线。
              const zero = amountView ? BOTTOM : yOf(0);
              area.setAttribute("d", `${path}L${segment.at(-1)[0]},${zero}L${segment[0][0]},${zero}Z`);
              area.setAttribute("fill", "url(#trend-area-fill)");
              node.append(area);
            }
            const line = document.createElementNS(svg.namespaceURI, "path");
            line.setAttribute("d", path);
            line.setAttribute("fill", "none"); line.setAttribute("stroke", item.color); line.setAttribute("stroke-width", item.id === "total" ? "2.2" : "1.8");
            line.setAttribute("stroke-linecap", "round"); line.setAttribute("stroke-linejoin", "round");
            node.append(line);
          }
          segment = [];
        };
        for (const point of item.points) {
          if (point.valueMinor == null || !["ready", "reference"].includes(point.quality)) { flush(); lastTime = null; continue; }
          const value = valueOf(item, point);
          if (value == null) { flush(); lastTime = null; continue; }
          const time = pointTime(point, item.intraday);
          if (lastTime != null && (point.breakBefore || (item.intraday ? time - lastTime > 300000 : time - lastTime !== 86400000))) flush();
          const x = xOf(time), y = yOf(value);
          lastDrawn = [x, y];
          if (point.quality === "reference" && !item.intraday && !["valuation-observation", "holding-nav-replay"].includes(point.source)) {
            if (segment.length) {
              const stub = document.createElementNS(svg.namespaceURI, "path");
              stub.setAttribute("d", `M${segment.at(-1)[0]},${segment.at(-1)[1]}L${x},${y}`);
              stub.setAttribute("fill", "none"); stub.setAttribute("stroke", item.color);
              node.append(stub);
            }
            flush();
            mark(lastDrawn);
            lastTime = null;
            continue;
          }
          segment.push([x, y]);
          lastTime = time;
        }
        flush();
        mark(lastDrawn);
        svg.append(node);
      }
      const plotBox = document.createElement("div");
      plotBox.className = "history-plot";
      plotBox.append(scale, svg);
      chart.append(plotBox);
      // 小幅波动需要更长的精确刻度；给侧栏留出实际宽度，避免窄窗截断。
      const axisWidth = Math.ceil(Math.max(scale.clientWidth, ...[...scale.children].map(label => label.getBoundingClientRect().width + 6)));
      plotBox.style.marginRight = `${axisWidth + 2}px`;
      scale.style.width = `${axisWidth}px`; scale.style.right = `-${axisWidth + 2}px`;
      const dates=document.createElement('div');dates.className='history-axis';
      // 只标实际首尾，避免稀疏采样的中间日期被等距排版放错位置。
      const axisLabels=[...new Set([timeline[0],timeline.at(-1)].map(entry=>pointLabel(entry.point,entry.intraday)))];
      dates.innerHTML=axisLabels.map(label=>`<span>${home.escape(label)}</span>`).join('');chart.append(dates);
      // ── 查看点：竖直虚线 + 各线圆点 + 跟随指针的浮层（不再占下方一条）。
      // 轴点取所有可绘制序列的 usable 时间并集；悬停、聚焦后方向键、重渲染恢复共用同一索引。
      const axisTimes=[...new Set(drawable.flatMap(item=>item.usable.map(point=>pointTime(point,item.intraday))))].filter(Number.isFinite).sort((a,b)=>a-b);
      if(axisTimes.length){
        const cursorLayer=document.createElementNS(svg.namespaceURI,"g");
        cursorLayer.setAttribute("data-cursor","");
        cursorLayer.setAttribute("aria-hidden","true");
        svg.append(cursorLayer);
        const pointByTime=item=>{const m=new Map(item.usable.map(p=>[pointTime(p,item.intraday),p]));return m;};
        const lookup=drawable.map(item=>({item,map:pointByTime(item)}));
        const tip=document.createElement("div");
        tip.className="history-tip";
        tip.hidden=true;
        plotBox.append(tip);
        let cursorIndex=Math.min(historySelection.cursor==null?axisTimes.length-1:historySelection.cursor,axisTimes.length-1);
        if(cursorIndex<0)cursorIndex=0;
        let shown=historySelection.cursor!=null;
        const renderCursor=()=>{
          const time=axisTimes[cursorIndex];
          const x=xOf(time);
          cursorLayer.replaceChildren();
          const guide=document.createElementNS(svg.namespaceURI,"line");
          guide.setAttribute("x1",String(x));guide.setAttribute("x2",String(x));
          guide.setAttribute("y1","8");guide.setAttribute("y2","112");
          guide.setAttribute("stroke","var(--natives-control)");guide.setAttribute("stroke-dasharray","4 5");
          cursorLayer.append(guide);
          const rows=[];
          for(const {item,map} of lookup){
            const point=map.get(time);
            const cy=point==null?null:yOf(valueOf(item,point));
            if(cy!=null&&Number.isFinite(cy)){
              const dot=document.createElementNS(svg.namespaceURI,"circle");
              dot.setAttribute("cx",String(x));dot.setAttribute("cy",String(cy));dot.setAttribute("r","3.4");
              dot.setAttribute("fill",item.color);dot.setAttribute("stroke","var(--natives-surface)");dot.setAttribute("stroke-width","1.4");
              cursorLayer.append(dot);
            }
            const labelName=hidden?"****":item.name;
            const valueText=point==null?t("fundHomeHistoryMissingDay"):amountView?`¥ ${home.money(point.valueMinor)}`:signedPct(valueOf(item,point));
            rows.push(`<span class="history-tip-row"><i style="background:${item.color}" aria-hidden="true"></i><span class="history-tip-name">${home.escape(labelName)}</span><b>${home.escape(valueText)}</b></span>`);
          }
          const anchor=lookup.find(({map})=>map.has(time));
          const when=anchor?pointLabel(anchor.map.get(time),anchor.item.intraday):t("fundHomeHistoryNoRecord");
          tip.innerHTML=`<span class="history-tip-date">${home.escape(when)}</span>${rows.join("")}`;
          tip.hidden=!shown;
          if(shown){
            // x 是 viewBox 坐标（0..600），按实际像素宽度换算并夹在绘图区内。
            const width=plotBox.clientWidth||0;
            const left=(x/600)*width-tip.offsetWidth/2;
            tip.style.left=`${Math.max(0,Math.min(width-tip.offsetWidth,left))}px`;
          }
        };
        const hideCursor=()=>{
          shown=false;
          historySelection.cursor=null;
          tip.hidden=true;
          cursorLayer.replaceChildren();
        };
        const moveTo=clientX=>{
          const box=svg.getBoundingClientRect();
          if(!box.width)return;
          const ratio=Math.max(0,Math.min(1,((clientX-box.left)/box.width*600-LEFT)/(RIGHT-LEFT)));
          const time=t0+ratio*(t1-t0);
          const next=axisTimes.reduce((best,value,index)=>Math.abs(value-time)<Math.abs(axisTimes[best]-time)?index:best,0);
          shown=true;
          cursorIndex=next;
          historySelection.cursor=next;
          renderCursor();
        };
        plotBox.addEventListener("pointermove",event=>moveTo(event.clientX));
        plotBox.addEventListener("pointerleave",hideCursor);
        // 键盘：图表本身可聚焦，左右键移动查看点，Esc 收起浮层。
        plotBox.tabIndex=0;
        plotBox.setAttribute("role","img");
        plotBox.setAttribute("aria-label",t("fundHomeOverviewHistoryLabel"));
        plotBox.addEventListener("focus",()=>{shown=true;renderCursor();});
        plotBox.addEventListener("blur",()=>{tip.hidden=true;});
        plotBox.addEventListener("keydown",event=>{
          if(event.key!=="ArrowLeft"&&event.key!=="ArrowRight"&&event.key!=="Home"&&event.key!=="End"&&event.key!=="Escape")return;
          event.preventDefault();
          if(event.key==="Escape"){hideCursor();return;}
          const next=event.key==="Home"?0:event.key==="End"?axisTimes.length-1:cursorIndex+(event.key==="ArrowLeft"?-1:1);
          cursorIndex=Math.max(0,Math.min(axisTimes.length-1,next));
          historySelection.cursor=cursorIndex;
          shown=true;
          renderCursor();
        });
        if(shown)renderCursor();
      }
      const scopeNode=document.getElementById("home-trend-scope");
      if(scopeNode)scopeNode.textContent=detailMode?t("fundHomeHistoryScopeChild"):amountView?t("fundHomeHistoryScopeAmount"):t("fundHomeHistoryScopePercent");
    }
  }
  // ── 一套分类入口（2026-10-25 重构）：顶部卡行＝“全部资产 + 各分类”，单选；
  // 底部卡行＝所选分类下的全部资产，点卡片可单独隐藏某条线。卡＝名称 + 当前值·区间变化 + sparkline。
  const cardOf = item => {
    const usable = usableOf(item.points);
    const change = changeOf(item.points);
    const current = usable.at(-1)?.valueMinor ?? item.current ?? null;
    const valueText = current == null ? "–" : `¥ ${home.money(current)}`;
    const pct = !hidden && Number.isFinite(change)
      ? `<span class="history-cat-return${change > 0 ? " up" : change < 0 ? " down" : ""}">${signedPct(change)}</span>`
      : `<span class="history-cat-return">–</span>`;
    const title = item.title ? ` title="${home.escape(item.title)}"` : "";
    return `<button type="button" class="history-cat-card${item.on ? " selected" : ""}" data-history-cat="${home.escape(item.id)}" aria-pressed="${item.on}"${title}><span class="history-cat-name"><i style="background:${item.color}" aria-hidden="true"></i><span class="history-cat-label">${item.on ? "✓ " : ""}${home.escape(hidden ? "****" : item.name)}</span></span><span class="history-cat-value">${home.escape(valueText)} · ${pct}</span></button>`;
  };
  // 卡行超过一行就折叠：只留第一行，并给“点击展开（还有 N 项）”。展开状态按行保存，
  // 切换区间／刷新快照后不会自己弹回去。
  const applyCardFold = (grid, toggle, cards, key, minVisible = 0) => {
    const els = [...grid.querySelectorAll(".history-cat-card")];
    const tops = els.map(el => Math.round(el.offsetTop));
    const firstRow = tops.filter(top => top === tops[0]).length;
    // 分类行至少留几张：窄屏一行只放得下一两张卡时，折叠会把分类选择器也藏起来。
    const limit = Math.max(firstRow, minVisible);
    const extra = cards.length - limit;
    const collapsible = extra > 0 && grid.offsetWidth > 0;
    if (toggle) {
      toggle.hidden = !collapsible;
      if (collapsible) {
        const open = historySelection.cardsOpen[key] === true;
        toggle.textContent = open ? t("fundHomeHistoryCollapseCards") : t("fundHomeHistoryExpandCards", { n: extra });
        toggle.setAttribute("aria-expanded", String(open));
        toggle.onclick = () => {
          historySelection.cardsOpen[key] = !open;
          applyCardFold(grid, toggle, cards, key, minVisible);
          toggle.focus();
        };
      }
    }
    const open = historySelection.cardsOpen[key] === true;
    els.forEach((el, index) => { el.hidden = collapsible && !open && index >= limit; });
  };
  const paintCards = (box, { key, title, hint, cards, minVisible = 0 }) => {
    if (!box) return;
    // 隐私模式照样占位：卡片只显示 ****，切回金额时布局不跳动。
    if (!cards.length) {
      box.hidden = true;
      box.replaceChildren();
      return;
    }
    box.hidden = false;
    box.innerHTML = `<div class="home-history-categories-head"><strong>${home.escape(title)}</strong><span>${home.escape(hint)}</span></div><div class="home-history-cat-scroll"><div class="home-history-cat-grid">${cards.map(cardOf).join("")}</div></div><button type="button" class="home-history-cat-fold" hidden></button>`;
    applyCardFold(box.querySelector(".home-history-cat-grid"), box.querySelector(".home-history-cat-fold"), cards, key, minVisible);
  };
  // 顶行：全部资产 + 有走势的分类（没数据的不做成可点卡片）。
  paintCards(document.getElementById("home-history-categories"), {
    key: "categories",
    minVisible: 3,
    title: t("fundHomeHistoryCategoryTitle"),
    hint: t("fundHomeHistoryCategoryHint"),
    cards: [
      { id: "total", name: t("fundHomeHistoryAllAssets"), current: snapshot.wealth.totalA, points: seriesOf("total"), color: "var(--series-1)", on: allTypesOn, title: t("fundHomeHistoryCategoryHint") },
      ...groupIds.filter(id => plottableTypes.has(id)).map(id => ({
        id, name: groupsById.get(id)?.name, current: groupsById.get(id)?.currentMinor,
        points: seriesOf(id), color: seriesColor(id), on: detailMode && selectedTypes[0] === id,
      })),
    ],
  });
  // 底行：所选分类的资产；选“全部资产”时列出所有分类的资产（分类名进副标题）。
  paintCards(document.getElementById("home-holding-cards"), {
    key: "holdings",
    title: detailMode ? `${groupsById.get(selectedTypes[0])?.name || selectedTypes[0]} · ${t("fundHomeHistoryHoldingTitle")}` : t("fundHomeHistoryAllHoldingsTitle"),
    hint: detailMode ? t("fundHomeHistoryHoldingHint") : t("fundHomeHistoryAllHoldingsHint"),
    cards: detailEntries.map(entry => ({
      id: entry.id, name: entry.name, points: entry.points,
      color: detailMode ? detailColor(entry.id) : seriesColor(entry.groupId),
      on: !historySelection.detailsOff.has(entry.id),
      title: hidden ? "" : [entry.name, entry.detail].filter(Boolean).join(" · "),
    })),
  });
  document.querySelectorAll("#home-history-categories [data-history-cat], #home-holding-cards [data-history-cat]").forEach(button => {
    button.onclick = () => {
      const id = button.dataset.historyCat;
      const inTop = button.closest("#home-history-categories") != null;
      if (inTop) {
        // 顶部单选：点“全部资产”或再次点已选分类＝回到全部；点其它分类＝只看它。
        const reset = id === "total" || (!allTypesOn && selectedTypes[0] === id);
        historySelection.groupId = reset ? null : id;
        historySelection.detailsOff = new Set();
      } else if (detailMode) {
        // 底部：该分类内单独显示／隐藏这条资产线。
        if (historySelection.detailsOff.has(id)) historySelection.detailsOff.delete(id);
        else historySelection.detailsOff.add(id);
      } else {
        // 全部视图里点资产＝进入它所属分类的明细。
        const entry = detailEntries.find(item => item.id === id);
        if (entry) historySelection.groupId = entry.groupId;
        historySelection.detailsOff = new Set();
      }
      home.renderOverview(home.snapshot());
      const scope = inTop ? "#home-history-categories" : "#home-holding-cards";
      requestAnimationFrame(() => document.querySelector(`${scope} [data-history-cat="${CSS.escape(id)}"]`)?.focus());
    };
  });
  home.renderAttribution(p.attribution);
};
home.renderAttribution = data => {
  const body=document.getElementById("home-attribution-body");
  if(!body)return;
  if(home.displayMode()==="hidden"){body.textContent=t("fundHomeOverviewHistoryHidden");return;}
  const open=body.querySelector("details")?.open, focused=body.contains(document.activeElement);
  if(!["ready","reference"].includes(data?.status)) {
    body.textContent=t(data?.reason==="quantity_mismatch"?"fundHomeOverviewAttributionQuantityGap":"fundHomeOverviewAttributionMissing");return;
  }
  const row=(key,value)=>`<div class="goal-kv"><span>${t(key)}</span><span>${home.escape(home.money(value))}</span></div>`;
  body.innerHTML=`<details ${open?"open":""}><summary>${t("fundHomeOverviewAttribution")}</summary>
    <p class="evidence-hint">${home.escape(dateLabel(data.from))} -- ${home.escape(dateLabel(data.to))} · ${t("fundHomeOverviewAttributionScope")} · ${t(data.status==="reference"?"fundHomeRangeReference":"fundHomeRangeChecked")}</p>
    ${row("fundHomeOverviewAttributionTotal",data.totalNetReturnMinor)}
    ${row("fundHomeOverviewAttributionSecurities",data.securitiesNetContributionMinor)}
    ${row("fundHomeOverviewAttributionIncome",data.unallocatedIncomeMinor)}
    ${row("fundHomeOverviewAttributionCashFees",data.cashFeesMinor)}
    ${row("fundHomeOverviewAttributionOther",data.otherContributionMinor)}
    <p class="evidence-hint">${t("fundHomeOverviewAttributionTop")} ${data.items.length} / ${data.itemCount}</p>
    <table class="alloc-table"><thead><tr><th>${t("fundHomeObject")}</th><th>${t("fundHomeOverviewAttributionPrice")}</th><th>${t("fundHomeOverviewAttributionFees")}</th><th>${t("fundHomeOverviewAttributionNet")}</th></tr></thead><tbody>
    ${data.items.map(item=>`<tr><td>${home.escape(item.code)}</td><td>${home.escape(home.money(item.priceContributionMinor))}</td><td>${home.escape(home.money(item.feesMinor))}</td><td>${home.escape(home.money(item.netContributionMinor))}</td></tr>`).join("")}</tbody></table></details>`;
  if(focused)body.querySelector("summary")?.focus();
};
home.renderBenchmark = data => {
  const status = document.getElementById("home-benchmark-status"), chart = document.getElementById("home-benchmark-chart");
  if (!status || !chart) return;
  chart.replaceChildren();
  if (home.displayMode() === "hidden") { status.textContent = t("fundHomeOverviewHistoryHidden"); return; }
  if (!data || data.status !== "ready") {
    status.textContent = t(data?.status === "failed" ? "fundHomeOverviewBenchmarkFailed" : data ? "fundHomeOverviewNoBenchmark" : "fundHomeOverviewBenchmarkLoading");
    return;
  }
  const pct = ppm => percent(BigInt(ppm) / 100n);
  status.textContent = `${t(({sh000300:"fundHomeIndexCsi300",sh000016:"fundHomeIndexSse50",sh000905:"fundHomeIndexCsi500",sh000001:"fundHomeIndexShanghai",sz399006:"fundHomeIndexChiNext"})[data.symbol]||"fundHomeSettingsUnknown")} · ${dateLabel(data.from)} -- ${dateLabel(data.to)} · ${t("fundHomeOverviewPortfolioReturn")} ${pct(data.portfolioPpm)} · ${t("fundHomeOverviewBenchmarkReturn")} ${pct(data.benchmarkPpm)} · ${t("fundHomeOverviewExcessReturn")} ${pct(data.excessPpm)} · ${t("fundHomeOverviewPriceIndex")} · ${data.source}`;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox","0 0 600 120"); svg.setAttribute("role","img");
  svg.setAttribute("aria-label",t("fundHomeOverviewBenchmarkChart"));
  const values = data.points.flatMap(point=>[BigInt(point.portfolioPpm),BigInt(point.benchmarkPpm)]);
  const min = values.reduce((a,b)=>a<b?a:b), max = values.reduce((a,b)=>a>b?a:b), span=max-min||1n;
  for (const key of ["portfolioPpm","benchmarkPpm"]) {
    const line = document.createElementNS(svg.namespaceURI,"polyline");
    line.setAttribute("points",data.points.map(point=>`${(Date.parse(point.date)-Date.parse(data.from))*600/(Date.parse(data.to)-Date.parse(data.from))},${110-Number((BigInt(point[key])-min)*100n/span)}`).join(" "));
    line.setAttribute("fill","none"); line.setAttribute("stroke","currentColor"); line.setAttribute("stroke-width","2"); line.setAttribute("stroke-linecap","round"); line.setAttribute("stroke-linejoin","round");
    svg.append(line);
  }
  chart.append(svg);
};
// ================= DES-D04 五维体检抽屉（只读快照投影） =================
// 数据来自 home.snapshot().portfolio.health（Rust overview 同批次投影）；
// 不发起新读取、不写库。hidden 模式遮蔽金额/权重/身份。抽屉为原生
// <dialog>，Esc/关闭返回焦点；Tab 为原生 roving，切换维度不改分母。
const drawer = document.getElementById("home-health-drawer");
const drawerBody = document.getElementById("home-health-body-drawer");
const drawerScope = document.getElementById("home-health-scope");
const drawerTabs = [...(drawer?.querySelectorAll('[role="tab"]') ?? [])];
let drawerReturnFocus = null;
let drawerDim = "types";

const KIND_KEY = { stock: "fundHomeHealthKindStock", etf: "fundHomeHealthKindEtf", fund: "fundHomeHealthKindOtc" };
const KIND_NAV = { stock: "positions:stock", etf: "positions:etf", fund: "positions:otc" };
const gapRow = (label, value, hidden) =>
  `<div class="goal-kv"><span class="k">${label}</span><span class="v">${hidden ? "****" : value}</span></div>`;
const pct = bps => bps == null ? "–" : (Number(bps) / 100).toLocaleString(state.language, { maximumFractionDigits: 2 }) + "%";
const kindName = kind => t(KIND_KEY[kind] || "fundHomeOverviewUnknown");

function healthSnapshot() {
  return home.snapshot?.()?.portfolio?.health || null;
}

function renderHealthTypes(h, hidden) {
  const rows = (h.types || []).map(type => {
    const name = kindName(type.instrumentKind);
    const drill = hidden ? "" : ` <button class="btn mini" data-health-jump="${KIND_NAV[type.instrumentKind] || ""}">${t("fundHomeHealthJump")}</button>`;
    return `<article class="home-health-row"><header><b>${home.escape(name)}</b><span>${hidden ? "****" : pct(type.weightBps)}</span></header>`
      + gapRow(t("fundHomeHealthHoldings").replace("{n}", ""), type.holdingCount, false)
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthKnownValue")}</span><span class="v num">${hidden ? "****" : home.money(type.knownValueMinor)}</span></div>`
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthMissingValue").replace("{n}", "")}</span><span class="v">${type.missingValueCount}</span></div>`
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthUnknownCost").replace("{n}", "")}</span><span class="v">${type.unknownCostCount}</span></div>`
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthStale").replace("{n}", "")}</span><span class="v">${type.staleCount}</span></div>${drill}</article>`;
  }).join("");
  return rows || `<p class="evidence-hint">${t("fundHomeMissingData")}</p>`;
}

function renderHealthAccounts(h, hidden) {
  const rows = (h.accounts || []).map(a => {
    // R07：hidden 连身份一起遮蔽（账户名/ID 与可反推的现值一并隐藏）。
    const title = hidden ? "****" : `#${a.accountId} ${home.escape(a.name)}${a.archived ? " " + home.escape(t("fundHomeHealthArchived")) : ""}`;
    return `<article class="home-health-row"><header><b>${title}</b><span>${hidden ? "****" : home.money(a.knownValueMinor)}</span></header>`
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthHoldings").replace("{n}", "")}</span><span class="v">${a.holdingCount}</span></div>`
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthMissingValue").replace("{n}", "")}</span><span class="v">${a.missingValueCount}</span></div>`
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthUnknownCost").replace("{n}", "")}</span><span class="v">${a.unknownCostCount}</span></div>`
      + `<div class="goal-kv"><span class="k">${t("fundHomeHealthStale").replace("{n}", "")}</span><span class="v">${a.staleCount}</span></div>`
      + (hidden ? "" : ` <button class="btn mini" data-health-jump="positions">${t("fundHomeHealthJump")}</button>`)
      + `</article>`;
  }).join("");
  return (h.accountsTruncated ? `<p class="evidence-hint">${home.escape(t("fundHomeHealthAccountsTruncated"))}</p>` : "")
    + (rows || `<p class="evidence-hint">${t("fundHomeMissingData")}</p>`);
}

function renderHealthConcentration(h, hidden) {
  const d = home.snapshot?.()?.portfolio?.diagnostics || {};
  const list = (d.securities || []).slice().sort((a, b) => (b.weightBps ?? -1) - (a.weightBps ?? -1)).slice(0, 10);
  return `<p class="evidence-hint">${home.escape(t("fundHomeHealthNoLookThrough"))}</p>`
    + gapRow(t("fundHomeOverviewConcentration"), d.hhiBps == null ? "–" : (d.hhiBps / 10000).toLocaleString(state.language, { maximumFractionDigits: 4 }), hidden)
    + (hidden ? "" : list.map(item => `<div class="goal-kv"><span>${home.escape(item.key.split(":").slice(-1)[0])}</span><span>${pct(item.weightBps)}</span></div>`).join(""));
}

function renderHealthQuality(h, hidden) {
  const s = home.snapshot?.()?.portfolio?.securities || {};
  const coverage = home.snapshot?.()?.portfolio?.diagnostics?.coverage;
  return gapRow(t("fundHomeOverviewMissingPrice"), hidden ? "****" : s.missingValueCount, hidden)
    + gapRow(t("fundHomeOverviewUnknownCost"), hidden ? "****" : s.unknownCostCount, hidden)
    + gapRow(t("fundHomeOverviewStale"), hidden ? "****" : s.staleCount, hidden)
    + gapRow(t("fundHomeOverviewCoverage"), t(coverage === "complete" ? "fundHomeOverviewComplete" : "fundHomeIncompleteData"), false)
    + `<p class="evidence-hint">${home.escape(t("fundHomeOverviewCheckData"))}</p>`;
}

function renderHealthLiquidity(h, hidden) {
  const liq = h.liquidity || {};
  return gapRow(t("fundHomeHealthOtcPending").replace("{n}", ""), liq.otcPendingCount ?? 0, false)
    + gapRow(t("fundHomeHealthOffline").replace("{n}", ""), liq.offlineCount ?? 0, false)
    + gapRow(t("fundHomeHealthOfflineUnsettled").replace("{n}", ""), liq.offlineUnsettledExits ?? 0, false)
    + gapRow(t("fundHomeHealthOfflineReceivable").replace("{amount}", ""), hidden ? "****" : home.money(liq.offlineReceivableMinor), hidden)
    + `<p class="evidence-hint">${home.escape(t("fundHomeHealthLiquidityNote"))}</p>`;
}

const HEALTH_RENDERERS = {
  types: renderHealthTypes, accounts: renderHealthAccounts, concentration: renderHealthConcentration,
  quality: renderHealthQuality, liquidity: renderHealthLiquidity,
};

function renderHealthDrawer() {
  if (!drawer || drawer.open !== true) return;
  const h = healthSnapshot();
  const hidden = home.displayMode() === "hidden";
  if (!h) {
    drawerScope.textContent = "";
    drawerBody.innerHTML = `<p class="evidence-hint">${home.escape(t("fundHomeMissingData"))}</p>`;
    return;
  }
  const asOf = h.asOf || home.snapshot?.()?.portfolio?.asOf;
  drawerScope.textContent = t("fundHomeHealthScope").replace("{scope}", t("fundHomeOverviewSecuritiesScope")).replace("{asOf}", asOf ? dateLabel(asOf) : t("fundHomeMissingData"));
  drawerBody.innerHTML = (HEALTH_RENDERERS[drawerDim] || renderHealthTypes)(h, hidden);
}

function openHealthDrawer() {
  if (!drawer) return;
  drawerReturnFocus = document.activeElement;
  drawer.showModal();
  renderHealthDrawer();
  drawer.querySelector('[role="tab"][aria-selected="true"]')?.focus();
}
function closeHealthDrawer() {
  if (!drawer?.open) return;
  drawer.close();
}

if (drawer) {
  drawer.addEventListener("close", () => { drawerReturnFocus?.focus?.(); drawerReturnFocus = null; });
  drawer.addEventListener("click", e => { if (e.target === drawer) drawer.close(); });
  drawer.addEventListener("keydown", e => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const idx = drawerTabs.indexOf(document.activeElement);
    if (idx < 0) return;
    e.preventDefault();
    const next = drawerTabs[(idx + (e.key === "ArrowRight" ? 1 : drawerTabs.length - 1)) % drawerTabs.length];
    next.click(); next.focus();
  });
  drawerTabs.forEach(tab => tab.addEventListener("click", () => {
    drawerDim = tab.dataset.dim || "types";
    drawerTabs.forEach(other => { other.setAttribute("aria-selected", other === tab ? "true" : "false"); other.tabIndex = other === tab ? 0 : -1; });
    drawerBody.setAttribute('aria-labelledby', tab.id);
    renderHealthDrawer();
  }));
  drawerBody?.addEventListener("click", e => {
    const jump = e.target.closest("[data-health-jump]")?.dataset.healthJump;
    if (!jump) return;
    // 下钻为应用内导航意图（DES-D04）：跳转持仓/资产/现金核对对应维度。
    closeHealthDrawer();
    const [view, scope] = jump.split(":");
    document.getElementById("tab-" + view)?.click();
    if (view === "positions" && scope && Fund.positions?.showCategory) Fund.positions.showCategory(scope);
    if (view === "assets" && Fund.assets?.open) Fund.assets.open();
  });
  document.getElementById("home-health-open")?.addEventListener("click", openHealthDrawer);
  drawerTabs.forEach(tab => { tab.tabIndex = tab.getAttribute('aria-selected') === 'true' ? 0 : -1; });
  document.getElementById("home-health-drawer")?.querySelectorAll("[data-close]").forEach(b => b.addEventListener("click", closeHealthDrawer));
  // 隐私切换与快照更新时同步已开抽屉（同一快照、立即重绘）。
  document.addEventListener("fund:display-mode-changed", renderHealthDrawer);
  document.addEventListener("fund:home-snapshot-updated", renderHealthDrawer);
}

// ---------- 自定义资金分组（方案 11 §3.1/§4 / P03 折叠树与 Tips） ----------
home.renderGroupAllocation=ga=>{
 const left=document.getElementById('home-distribution-body'),right=document.getElementById('home-combination-body');
 if(!left||!right)return;
 const scrollPositions=[left.scrollTop,right.scrollTop];
 const hidden=home.displayMode()==='hidden',amounts=home.displayMode()==='amount';
 const opened=new Set([...left.querySelectorAll('details[open][data-group-id]')].map(node=>node.dataset.groupId));
 const focus=document.activeElement,focusGroup=left.contains(focus)&&focus.tagName==='SUMMARY'?focus.closest('[data-group-id]')?.dataset.groupId:null;
 const name=value=>hidden?'****':home.escape(value),money=value=>amounts&&value!=null?home.escape(home.money(value)):'****';
 const percent=value=>hidden?'****':value==null?'–':(Number(value)/10).toFixed(1)+'%';
 const legacy=document.getElementById('home-group-body');if(legacy)legacy.replaceChildren();
 const setSummary=(id,value)=>{const node=document.getElementById(id);if(node)node.textContent=value;};
 const groups=ga?.configured?(ga.groups||[]):[];
 const assetCount=groups.reduce((count,g)=>count+(g.assets||[]).length,0);
 const totalKnown=groups.length>0&&groups.every(g=>g.currentMinor!=null);
 const totalMinor=groups.reduce((sum,g)=>sum+(g.currentMinor==null?0n:BigInt(g.currentMinor)),0n);
 const targetTenths=groups.reduce((sum,g)=>sum+Number(g.ratioTenths||0),0);
 if(!ga?.configured){
  setSummary('home-allocation-total','–');setSummary('home-allocation-total-note',t('fundHomeGroupNotConfigured'));
  setSummary('home-allocation-target','–');setSummary('home-allocation-target-note','');
  setSummary('home-allocation-unassigned','–');setSummary('home-allocation-unassigned-note','');
  left.innerHTML=`<p>${home.escape(t('fundHomeGroupNotConfigured'))}</p>`;right.innerHTML=`<p>${home.escape(t('fundHomeGroupSetup'))}</p>`;
 } else {
  setSummary('home-allocation-total',hidden?'****':totalKnown?money(totalMinor):'–');
  setSummary('home-allocation-total-note',totalKnown?t('fundHomeAllocationSummaryCount',{groups:groups.length,assets:assetCount}):t('fundHomeAllocationPartial'));
  setSummary('home-allocation-target',percent(targetTenths));
  setSummary('home-allocation-target-note',t('fundHomeAllocationTargetNote',{groups:groups.length}));
  const unassigned=ga.unassignedMinor==null?null:BigInt(ga.unassignedMinor);
  setSummary('home-allocation-unassigned',hidden?'****':unassigned==null?'–':money(unassigned));
  setSummary('home-allocation-unassigned-note',hidden?'':unassigned==null?t('fundDetailMissing'):unassigned===0n?t('fundHomeAllocationAllAssigned'):t('fundHomeAllocationUnassignedNote'));
  const color=index=>`var(${index===0?'--natives-accent':index<5?'--fund-allocation-'+(index+1):'--natives-control'})`;
  const bar=()=>{
   if(hidden||groups.some(g=>g.currentTenths==null))return '';
   const parts=groups.slice(0,5).map(g=>({name:g.name,value:Number(g.currentTenths)}));
   if(groups.length>5)parts.push({name:t('fundHomeOther'),value:groups.slice(5).reduce((sum,g)=>sum+Number(g.currentTenths),0)});
   return `<div class="home-allocation-bar" aria-hidden="true">${parts.map((part,index)=>`<span style="width:${Math.max(0,Math.min(100,part.value/10))}%;background:${color(index)}"></span>`).join('')}</div><ul class="home-allocation-legend">${parts.map((part,index)=>`<li><i class="home-allocation-dot" style="background:${color(index)}" aria-hidden="true"></i>${name(part.name)} ${percent(part.value)}</li>`).join('')}</ul>`;
  };

  left.innerHTML=bar()+`<div class="distribution-tree">${groups.map(g=>`<details data-group-id="${home.escape(g.groupId)}" ${opened.has(String(g.groupId))?'open':''}><summary><strong>${name(g.name)}</strong><span class="num">${g.currentMinor==null?home.escape(t('fundDetailMissing')):money(g.currentMinor)} · ${percent(g.currentTenths)}</span></summary><p class="allocation-tip">${hidden?'':home.escape(g.description||t('fundHomeGroupDescriptionEmptyHint'))}</p><div class="distribution-children">${(g.assets||[]).map(asset=>{
   let weight='–';if(!hidden&&asset.valueMinor!=null&&g.currentMinor!=null&&BigInt(g.currentMinor)>0n)weight=percent(BigInt(asset.valueMinor)*1000n/BigInt(g.currentMinor));
   return `<div class="distribution-child"><div><strong>${name(asset.name||asset.code||'–')}</strong><small>${hidden?'':home.escape(asset.code||'')} · ${hidden?'':home.escape(t(asset.instrumentKind==='stock'?'fundDetailStockType':asset.venue==='exchange'?'fundLedgerTypeEtf':asset.instrumentKind==='fund'?'fundLedgerTypeOtcFund':'fundHomeGroup'))}</small></div><div class="num">${asset.valueMinor==null?home.escape(t('fundDetailMissing')):money(asset.valueMinor)}<small>${weight}</small></div></div>`;
  }).join('')||`<p>${home.escape(t('fundHomeGroupNoAssets'))}</p>`}</div></details>`).join('')}</div>${ga.unassignedMinor!=null?`<p>${home.escape(t('fundHomeGroupUnassigned'))} ${money(ga.unassignedMinor)}</p>`:''}${ga.invalidRefs?.length?`<p>${home.escape(t('fundHomeGroupInvalidRefs'))} ${ga.invalidRefs.length}</p>`:''}`;
  right.innerHTML=`<table class="combination-table" aria-labelledby="home-combination-title"><thead><tr>${['fundHomeGroup','fundHomeGroupTarget','fundHomeGroupActual','fundHomeAllocationDeviation'].map(key=>`<th scope="col">${home.escape(t(key))}</th>`).join('')}</tr></thead><tbody>${groups.map(g=>{
   const difference=g.currentTenths==null?null:Number(g.currentTenths)-Number(g.ratioTenths);
   const deviation=hidden?'':difference==null?t('fundDetailMissing'):difference===0?t('fundHomeGroupDeviationExact'):t(difference>0?'fundHomeGroupDeviationOver':'fundHomeGroupDeviationUnder')+' '+(Math.abs(difference)/10).toFixed(1)+' '+t('fundDetailPercentagePoints');
   const actual=g.currentTenths==null?null:Math.max(0,Math.min(100,Number(g.currentTenths)/10));
   const track=hidden||actual==null?'':`<div class="combination-track" aria-hidden="true"><span style="width:${actual}%"></span></div>`;
   const delta=hidden?'****':difference==null?'–':`${difference>0?'+':''}${(difference/10).toFixed(1)}pp`;
   return `<tr class="combination-row"><th scope="row">${name(g.name)}<small class="allocation-tip">${hidden?'':home.escape(g.description||'')}</small>${track}</th><td class="num">${percent(g.ratioTenths)}</td><td class="num">${percent(g.currentTenths)}</td><td><span class="combination-deviation" title="${home.escape(deviation)}" aria-label="${home.escape(deviation||'****')}">${delta}</span></td></tr>`;
  }).join('')}</tbody></table><p class="allocation-tip">${home.escape(t('fundHomeGroupTargetTotal'))} ${percent(targetTenths)} · ${home.escape(t('fundHomeGroupRemaining'))} ${percent(ga.unallocatedTenths)}</p>`;
 }
 left.scrollTop=scrollPositions[0];right.scrollTop=scrollPositions[1];
 if(focusGroup&&!focus.isConnected)left.querySelector(`[data-group-id="${CSS.escape(focusGroup)}"] > summary`)?.focus({preventScroll:true});
 for(const id of ['home-group-manage','home-combination-manage','home-allocation-detail']){const button=document.getElementById(id);if(button)button.onclick=()=>home.manageGroupPlan();}
};

home.buildOverviewDetail = function(kind, s, opts) {
  if (!s) return "";
  const { hidePrivate, mask, fmtMinorFull, escapeHtml } = opts;
  if (kind === "cash") {
    const c = s.wealth.cash || {};
    const rows = (c.accounts || []).map(a => `<section class="home-cash-account">
      <h3>${hidePrivate() ? t("fundHomeAccount72acfe") : escapeHtml(a.accountName)}</h3><dl>
      <div><dt>${t("fundHomeSettledCash")}</dt><dd>${mask(fmtMinorFull(a.cashMinor))}</dd></div>
      <div><dt>${t("fundHomeReserve")}</dt><dd>${a.reserveState === "configured" ? mask(fmtMinorFull(a.reservedMinor)) : t("fundHomeReserveUnset")}</dd></div>
      <div><dt>${t("fundHomeCommitment")}</dt><dd>${mask(fmtMinorFull(a.reservedByOrdersMinor))}</dd></div>
      <div><dt>${t("fundHomeAvailable")}</dt><dd>${a.availableMinor != null ? mask(fmtMinorFull(a.availableMinor)) : "–"}</dd></div>
    </dl></section>`).join("");
    return `${rows || t("fundHomeNoCashAssetsRecorded")}
    <div style="margin-top:8px;font-size:12px;color:var(--natives-text-muted)">
      ${t(c.availableMinor == null ? "fundHomeCashBalanceNoReserve" : "fundHomeManageReservesAndCommitmentsInCashReconciliation")}
    </div>`;
  }
  if (kind === "return") {
    const r = s.wealth.returns || {};
    if (!["ready","partial","reference"].includes(r.status)) {
      return `<div>${r.reason === "missing_baseline" ? t("fundHomeMissingOpeningBaselineSaveAValuationSnapshot") : t("fundHomeOpeningOrClosingValuationIsIncompleteReturns")}</div>`;
    }
    return `<table>
      <tr><td>${t("fundHomeOpeningAssets")}${escapeHtml(r.from || "")}）</td><td>${mask(fmtMinorFull(r.beginMinor))}</td></tr>
      <tr><td>${t("fundHomeClosingAssets")}${escapeHtml(r.to || "")}）</td><td>${mask(fmtMinorFull(r.endMinor))}</td></tr>
      <tr><td>${t("fundHomePeriodNetExternalContributions")}</td><td>${mask(fmtMinorFull(r.netExternalMinor))}</td></tr>
      <tr><td><b>${t("fundHomePeriodNetReturnClosingOpeningNetContributions")}</b></td><td><b>${mask(fmtMinorFull(r.netReturnMinor))}</b></td></tr>
    </table>
    <div style="margin-top:8px;font-size:12px;color:var(--natives-text-muted)">
      ${t(r.status==="reference"?"fundHomeRangeReference":"fundHomeRangeChecked")} · ${t("fundHomeDividendsInterestAndFeesAreAlreadyIncluded")}
    </div>`;
  }
  if (kind === "total" || kind === "invested") {
    const w = s.wealth;
    // Detail and the summary card share one snapshot quality vocabulary:
    // backend "incomplete" must never fall through to a generic "needs review"
    // while the issue list claims there is nothing to review.
    const qualityKey = {ready:"fundHomeQualityReady",incomplete:"fundHomeQualityPartial",partial:"fundHomeQualityPartial",empty:"fundHomeQualityEmpty",stale:"fundHomeQualityStale"}[w.quality];
    const issueKey = {
      missing_price: "fundHomeOverviewMissingPrice",
      missing_quote: "fundHomeValuationMissingQuotes",
      currency_unconfirmed: "fundHomeCurrencyNotConfirmed",
      currency_unsupported: "fundHomeUnsupportedCurrency",
      overlap_review: "fundHomePossibleOverlappingOwnership",
      unknown_class: "fundHomeSomeAssetClassificationsAreUnknown",
      stale_valuation: "fundHomeStaleValuation",
      incomplete: "fundHomeSomeValuationsCurrenciesOrClassificationsNeedVerification",
    };
    const issueLabel = i => {
      const key = issueKey[i.code];
      const label = key ? t(key, {n:i.count}) : t("fundHomeSomeValuationsCurrenciesOrClassificationsNeedVerification");
      return Number(i.count) > 1 ? `${label} × ${i.count}` : label;
    };
    const issues = (w.issues || []).map(issueLabel).join("；") || t("fundHomeNone");
    const groups = s.groupAllocation?.groups || [];
    const amount = value => value == null ? "–" : mask(fmtMinorFull(value));
    const name = value => hidePrivate() ? "****" : escapeHtml(value);
    const breakdown = groups.map(g => `<tbody><tr><th scope="row">${name(g.name)}</th><td>${amount(g.currentMinor)}</td></tr>${(g.assets || []).map(a => `<tr><td>${name(a.name)}</td><td>${amount(a.valueMinor)}</td></tr>`).join("")}</tbody>`).join("");
    const unassigned = s.groupAllocation?.unassignedMinor;
    return `<table>
      <tr><td>${t("fundHomeManagedAssetsA")}</td><td>${w.totalA != null ? mask(fmtMinorFull(w.totalA)) : t("fundHomeNeedsVerificationccf920")}</td></tr>
      <tr><td>${t("fundHomeNoncashMarketValueI")}</td><td>${w.investedI != null ? mask(fmtMinorFull(w.investedI)) : t("fundHomeNeedsVerificationccf920")}</td></tr>
      <tr><td>${t("fundHomeLastValuationDate")}</td><td>${escapeHtml(w.latestValuation || "–")}</td></tr>
      <tr><td>${t("fundHomeDataQuality")}</td><td>${escapeHtml(t(qualityKey || "fundHomeNeedsVerificationccf920"))}</td></tr>
      <tr><td>${t("fundHomeItemsToVerify")}</td><td>${escapeHtml(issues)}</td></tr>
    </table>
    <h3 class="home-card-title">${t("fundHomeAssetBreakdown")}</h3>
    <table>${breakdown}${unassigned != null && BigInt(unassigned) !== 0n ? `<tbody><tr><td>${t("fundHomeUngrouped")}</td><td>${amount(unassigned)}</td></tr></tbody>` : ""}</table>
    <div class="home-detail-actions"><button type="button" class="btn" data-wealth-view="positions">${t("fundHomeOverviewHoldings")}</button><button type="button" class="btn" data-wealth-view="assets">${t("fundLedgerNavAssets")}</button></div>
    <div style="margin-top:8px;font-size:12px;color:var(--natives-text-muted)">
      ${t("fundHomeManagedAssetsIncludeOnlyAssetsExplicitlyTracked")}
    </div>`;
  }
  return "";
};
home.renderHomeNewsItem=item=>{
        const rel = home.informationContentLabel(item);
        const time = home.escape(home.informationTimeLabel(item.publishedAt,item.timePrecision));
        return `<article class="info-item"><div class="home-news-meta"><span class="news-category">${home.escape(home.informationSourceLabel(item.sourceId))}</span><time>${time}</time></div><h3>${home.escape(item.title)}</h3>
          ${item.summary?`<p class="home-news-summary">${home.escape(item.summary)}</p>`:''}
          <div class="home-news-item-footer"><small>${home.escape(rel)}</small><span class="info-actions">
            <button class="btn btn-sm" data-act="news-ai" data-id="${item.id}" ${home.isNewsAISelected?.(item.id) ? 'aria-pressed="true"' : 'aria-pressed="false"'}>${t("fundHomeNewsAiBtn")}</button>
            <button class="btn btn-sm" data-info-open="${item.id}">${t("fundHomeOpenSource")}</button>
            <button class="btn btn-sm" data-info-fav="${item.id}" aria-pressed="${item.favorite}">${item.favorite ? t("fundHomeFavorited") : t("fundHomeFavorite")}</button>
          </span></div></article>`;

};
})();
