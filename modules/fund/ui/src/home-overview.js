// R2 overview projection; lifecycle and snapshot ownership stay in home.js.
(() => {
const { state, t } = Fund;
const home = Fund.home = Fund.home || {};
// 集中度列表分页状态（卡内保留，语言切换/快照刷新不重置页码）。
const concentrationState = { page: 0 };
// 资产走势多选状态：off＝隐藏的序列 id（空＝默认全部线同图），view＝用户
// 选择的金额/百分比视图；隐私遮金额（percent/hidden）时强制百分比视图。
const historySelection = { off: new Set(), view: "amount" };
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
// 金额轴紧凑格式：万/亿单位带一位小数去尾零，小值千分位整数（元）。
function fmtAxisAmount(value) {
  const abs = Math.abs(value);
  if (abs >= 1e8) return `${(value / 1e8).toFixed(abs >= 1e9 ? 0 : 1).replace(/\.0$/, "")}亿`;
  if (abs >= 1e4) return `${(value / 1e4).toFixed(abs >= 1e5 ? 0 : 1).replace(/\.0$/, "")}万`;
  return Math.round(value).toLocaleString("en-US");
}
// pill 行：行首「全部」重置 ＋ 总资产/分组多选 pill ＋ 尾部金额/百分比视图切换。
// 交互规则：默认全选；全选状态下点任一分组＝进入「总资产＋该分组」单看，
// 非全选状态点击＝切换该线显隐；点「全部」恢复全部线同图。
function renderHistorySeriesControls(snapshot, changes = {}, colorOf = () => "var(--series-1)", view = "amount") {
  const container = document.getElementById("home-history-series");
  if (!container) return;
  const groups = snapshot.groupAllocation?.configured ? (snapshot.groupAllocation.groups || []) : [];
  if (!groups.length) {
    historySelection.off.clear();
    container.hidden = true;
    container.replaceChildren();
    return;
  }
  const valid = new Set(["total", ...groups.map(group => String(group.groupId))]);
  historySelection.off = new Set([...historySelection.off].filter(id => valid.has(id)));
  container.hidden = false;
  const hidden = home.displayMode() === "hidden";
  const amountView = home.displayMode() === "amount";
  const allOn = historySelection.off.size === 0;
  const buttons = [{ id: "total", name: t("fundHomeHistoryGroupTotal") }, ...groups.map(group => ({ id: String(group.groupId), name: group.name }))];
  const reset = `<button type="button" class="history-series-tab history-series-all" data-history-reset aria-pressed="${allOn}">${home.escape(t("fundHomeHistoryAllSeries"))}</button>`;
  const tabs = buttons.map(button => {
    // 区间变化与 Demo 的分类卡一致：百分比只描述该分类自身走势，
    // 隐私模式下连名称一起遮蔽（金额与比例都不外露）。彩点颜色与主图
    // 分类线、各分类走势卡共用同一 palette，保证三处视觉一致。
    const change = !hidden && Number.isFinite(changes[button.id])
      ? `<span class="history-series-change${changes[button.id] > 0 ? " up" : changes[button.id] < 0 ? " down" : ""}" title="${home.escape(t("fundHomeHistoryChange"))}">${signedPct(changes[button.id])}</span>`
      : "";
    return `<button type="button" class="history-series-tab" data-history-group="${home.escape(button.id)}" aria-pressed="${!historySelection.off.has(button.id)}"><i class="history-series-dot" style="background:${colorOf(button.id)}" aria-hidden="true"></i>${home.escape(hidden ? "****" : button.name)}${change}</button>`;
  }).join("");
  // 视图切换：金额轴只在显示金额的隐私模式下可用；遮金额时固定百分比并禁用。
  const viewToggle = `<span class="history-view-toggle" role="group" aria-label="${home.escape(t("fundHomeHistoryViewLabel"))}">
    <button type="button" data-history-view="amount" aria-pressed="${view === "amount"}" ${amountView ? "" : "disabled title=" + home.escape(t("fundHomeHistoryViewAmountLocked"))}>${home.escape(t("fundHomeHistoryViewAmount"))}</button>
    <button type="button" data-history-view="percent" aria-pressed="${view === "percent"}">${home.escape(t("fundHomeHistoryViewPercent"))}</button>
  </span>`;
  container.innerHTML = reset + tabs + viewToggle;
  container.querySelectorAll("[data-history-reset]").forEach(button => {
    button.onclick = () => {
      historySelection.off.clear();
      home.renderOverview(home.snapshot());
      requestAnimationFrame(() => document.querySelector("#home-history-series [data-history-reset]")?.focus());
    };
  });
  container.querySelectorAll("[data-history-group]").forEach(button => {
    button.onclick = () => {
      const id = button.dataset.historyGroup;
      if (allOn && id !== "total") {
        // 全选 → 单看：仅保留总资产基准线与该分组。
        historySelection.off = new Set(buttons.map(item => item.id).filter(other => other !== id && other !== "total"));
      } else if (historySelection.off.has(id)) {
        historySelection.off.delete(id);
      } else {
        historySelection.off.add(id);
      }
      home.renderOverview(home.snapshot());
      requestAnimationFrame(() => document.querySelector(`#home-history-series [data-history-group="${id}"]`)?.focus());
    };
  });
  container.querySelectorAll("[data-history-view]").forEach(button => {
    button.onclick = () => {
      historySelection.view = button.dataset.historyView;
      home.renderOverview(home.snapshot());
      requestAnimationFrame(() => document.querySelector(`#home-history-series [data-history-view="${historySelection.view}"]`)?.focus());
    };
  });
}
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
  const pointTime=point=>intraday?Number(point.observedAt)*1000:Date.parse(point.date);
  const pointLabel=point=>intraday&&point.observedAt?new Date(pointTime(point)).toLocaleTimeString(state.language,{timeZone:'Asia/Shanghai',hour:'2-digit',minute:'2-digit'}):dateLabel(point.date);
  const text = (id, value) => { const el = document.getElementById(id); if (el) el.textContent = value; };
  const money = value => value == null ? t("fundHomeNeedsVerificationccf920") : home.money(value);
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
  // 资产走势大数字区（demo chart-total）：净收益大数字旁同步时间加权收益。
  const twrHeadline = document.getElementById("home-chart-twr");
  if (twrHeadline) {
    const twrValue = hidden || h.twrPpm == null ? null : Number(BigInt(h.twrPpm)) / 10000;
    twrHeadline.textContent = hidden ? "****" : h.twrPpm == null ? "–" : signedPct(twrValue);
    twrHeadline.classList.toggle("up", twrValue != null && twrValue > 0);
    twrHeadline.classList.toggle("down", twrValue != null && twrValue < 0);
  }
  const total = snapshot.wealth.totalA;
  const invested = snapshot.wealth.investedI;
  text("home-position-value", hidden ? "****" : total != null && invested != null && BigInt(total) > 0n
    ? percent(BigInt(invested) * 10000n / BigInt(total)) : "–");
  text("home-returns-scope", t("fundHomeLedgerScope"));
  const row = (name, value) => `<div class="goal-kv"><span class="k">${name}</span><span class="v">${value}</span></div>`;
  // N04 累计投资收益（证券账本范围）：浮动＋已实现＋分红/再投资−独立费用；
  // 未知成本缺口时只显示已知部分并标注，不冒充完整。
  const cum = snapshot.wealth.cumulative || {};
  const cumValue = hidden ? "****" : cum.totalMinor == null
    ? "–"
    : home.money(cum.totalMinor);
  const cumNote = cum.coverageFrom
    ? t("fundHomeCumulativeSince").replace("{date}", dateLabel(cum.coverageFrom))
      + (cum.complete ? "" : " · " + t("fundHomeCumulativeGaps"))
    : t("fundHomeMissingData");
  text("home-cumulative-value", cumValue);
  text("home-cumulative-note", cumNote);
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
    if (recorded?.valueMinor != null) return recorded.valueMinor;
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
  // 多选派生：off 为空＝全部线同图；遮金额隐私（percent/hidden）强制百分比视图。
  const view = home.displayMode() === "amount" ? (historySelection.view === "percent" ? "percent" : "amount") : "percent";
  renderHistorySeriesControls(snapshot, changes, seriesColor, view);
  let selectedIds = ["total", ...groupIds].filter(id => !historySelection.off.has(id));
  if (!selectedIds.length) selectedIds = ["total", ...groupIds];
  const totalOn = selectedIds.includes("total");
  const selectedGroups = groupIds.filter(id => selectedIds.includes(id));
  // 每条线独立取序列：分类无历史时回退当天真实观察（pill/主图/分类卡同口径）。
  const seriesOf = id => seriesPointsFor(seriesFor(id), id);
  if (selectedGroups.some(id => seriesFor(id) !== h.points)) intraday = true;
  const plot = selectedIds.map(id => {
    const points = seriesOf(id);
    return { id, color: seriesColor(id), area: id === "total", points, usable: usableOf(points) };
  });
  const totalEntry = plot.find(item => item.id === "total");
  const anyEntry = plot.find(item => item.usable.length);
  const totalPoints = totalEntry?.points ?? anyEntry?.points ?? h.points;
  const reconstructed = plot.some(item => item.points.some(point => point.source === "holding-nav-replay"));
  const observedSeries = intraday || reconstructed || plot.some(item => item.points.some(point => point.source === "valuation-observation"));
  let historyStatus = hidden ? t("fundHomeOverviewHistoryHidden") : intraday ? t('fundHomeHistoryIntraday') : reconstructed ? t('fundHomeHistoryReconstructed') : h.reason ? t(reasons[h.reason] || "fundHomeOverviewUnverified") : t("fundHomeOverviewFlowTiming");
  if (!hidden && h.replayReason) historyStatus += ' · ' + t('fundHomeHistoryReplayLimit');
  if (!hidden && historySelection.off.size && !intraday && selectedGroups.some(id => usableOf(seriesOf(id)).length < 2)) historyStatus += " · " + t("fundHomeHistoryGroupUnavailable");
  text("home-history-status", historyStatus);
  if (h.truncated) text("home-history-status", document.getElementById("home-history-status").textContent + " · " + t("fundHomeRangeTruncated"));
  if (h.status === "reference") text("home-history-status", document.getElementById("home-history-status").textContent + " · " + t("fundHomeRangeReference"));
  const selection = document.getElementById("home-history-selection");
  // 选中说明带区间前缀：全部＝「本月 · 全部」；单组沿用「总资产 + 名称」。
  const rangeNameKey = { today: "fundHomeToday", month: "fundHomeThisMonth", year: "fundHomeThisYear", all: "fundHomeSinceTrackingBegan", custom: "fundHomeCustom" }[snapshot.requestedRange?.key || "month"] || "fundHomeThisMonth";
  let selectionText;
  if (!historySelection.off.size) selectionText = t("fundHomeHistoryAllSeries");
  else if (!selectedGroups.length) selectionText = t("fundHomeHistoryGroupTotal");
  else if (selectedGroups.length === 1) {
    const name = groupsById.get(selectedGroups[0])?.name || selectedGroups[0];
    selectionText = totalOn ? `${t("fundHomeHistoryGroupTotal")} + ${name}` : name;
  } else {
    const countText = t("fundHomeHistoryGroupsCount", { n: selectedGroups.length });
    selectionText = totalOn ? `${t("fundHomeHistoryGroupTotal")} + ${countText}` : countText;
  }
  if (selection) selection.textContent = hidden ? "****" : `${t(rangeNameKey)} · ${selectionText}`;
  text("home-benchmark-status", t("fundHomeOverviewNoBenchmark"));
  if (chart) {
    chart.replaceChildren();
    // 金额视图＝每条线画自身金额（看钱）；百分比视图＝各序列以区间首个可绘制
    // 点归一化为 0% 对比。总资产线带面积填充，各分组线按所选叠加。
    const amountView = view === "amount";
    const drawable = plot.map(item => {
      const base = item.usable.length ? BigInt(item.usable[0].valueMinor) : 0n;
      return { ...item, base, drawable: item.usable.length >= 2 && (amountView || base > 0n) };
    }).filter(item => item.drawable);
    if (hidden) {
      chart.innerHTML=`<div class="history-empty"><p>${t("fundHomeOverviewHistoryHidden")}</p><button type="button" class="btn" data-history-reveal>${t("fundHomeShowAmounts")}</button></div>`;
      chart.querySelector('[data-history-reveal]').onclick=()=>home.applyDisplayMode('amount');
    } else if (!drawable.length) {
      const usable = plot.find(item => item.usable.length)?.usable || [];
      const point=usable[0],w=snapshot.wealth;
      const current=snapshot.requestedRange?.to===snapshot.requestedRange?.today;
      const value=point?.valueMinor??(current?(w.quality!=="empty"?(w.totalA??w.knownA):null):null);
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
      const times = drawable.flatMap(item => item.points.map(pointTime)).filter(Number.isFinite);
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
        grid.setAttribute("stroke", !amountView && value === 0 ? "var(--natives-border-strong, var(--natives-border))" : "var(--natives-border)");
        grid.setAttribute("stroke-width", "1");
        svg.append(grid);
        const label = document.createElement("span");
        label.style.top = `${(y / 120) * 100}%`;
        if (amountView) label.textContent = fmtAxisAmount(value);
        else {
          const digits = step < 0.1 ? 2 : 1;
          const rounded = Math.abs(value) < step / 20 ? 0 : Number(value.toFixed(digits));
          label.textContent = `${rounded > 0 ? "+" : ""}${rounded.toFixed(digits)}%`;
        }
        scale.append(label);
      }
      // 总资产先画（面积＋线），各所选分组再叠加，共用同一坐标轴。
      for (const item of drawable) {
        const node = document.createElementNS(svg.namespaceURI, "g");
        node.setAttribute("data-series", item.id);
        let segment = [], lastTime = null;
        const flush = () => {
          if (segment.length > 1) {
            if (item.area) {
              const area = document.createElementNS(svg.namespaceURI, "polygon");
              // 金额视图面积从图表底部起；百分比视图以 0% 基线起。
              const zero = amountView ? BOTTOM : yOf(0);
              area.setAttribute("points", `${segment[0].split(",")[0]},${zero} ${segment.join(" ")} ${segment.at(-1).split(",")[0]},${zero}`);
              area.setAttribute("fill", "var(--draw-fill)");
              node.append(area);
            }
            const line = document.createElementNS(svg.namespaceURI, "polyline");
            line.setAttribute("points", segment.join(" "));
            line.setAttribute("fill", "none"); line.setAttribute("stroke", item.color); line.setAttribute("stroke-width", item.id === "total" ? "2" : "1.6");
            line.setAttribute("stroke-linecap", "round"); line.setAttribute("stroke-linejoin", "round");
            if (observedSeries) line.setAttribute("stroke-dasharray", "4 4");
            node.append(line);
          }
          segment = [];
        };
        for (const point of item.points) {
          if (point.valueMinor == null || !["ready", "reference"].includes(point.quality)) { flush(); lastTime = null; continue; }
          const value = valueOf(item, point);
          if (value == null) { flush(); lastTime = null; continue; }
          const time = pointTime(point);
          if (lastTime != null && (point.breakBefore || (intraday ? time - lastTime > 300000 : time - lastTime !== 86400000))) flush();
          const x = xOf(time), y = yOf(value);
          const dot = document.createElementNS(svg.namespaceURI, "circle");
          dot.setAttribute("cx", String(x)); dot.setAttribute("cy", String(y)); dot.setAttribute("r", item.id === "total" ? "2.8" : "2.4");
          dot.setAttribute("fill", item.color);
          node.append(dot);
          if (point.quality === "reference" && !intraday && !["valuation-observation", "holding-nav-replay"].includes(point.source)) {
            if (segment.length) {
              const stub = document.createElementNS(svg.namespaceURI, "polyline");
              stub.setAttribute("points", [segment.at(-1), `${x},${y}`].join(" "));
              stub.setAttribute("fill", "none"); stub.setAttribute("stroke", item.color); stub.setAttribute("stroke-dasharray", "4 4");
              node.append(stub);
            }
            flush();
            lastTime = null;
            continue;
          }
          segment.push(`${x},${y}`);
          lastTime = time;
        }
        flush();
        svg.append(node);
      }
      const plotBox = document.createElement("div");
      plotBox.className = "history-plot";
      plotBox.append(scale, svg);
      chart.append(plotBox);
      const dates=document.createElement('div');dates.className='history-axis';
      // demo 口径的 5 段日期轴：标签只取真实点日期等距采样，缺日期不补造。
      const axisCount=Math.min(5,totalPoints.length);
      const axisLabels=[];
      for(let k=0;k<axisCount;k++){
        const idx=Math.round(k*(totalPoints.length-1)/Math.max(axisCount-1,1));
        const label=pointLabel(totalPoints[idx]);
        if(!axisLabels.length||axisLabels.at(-1)!==label)axisLabels.push(label);
      }
      dates.innerHTML=axisLabels.map(label=>`<span>${home.escape(label)}</span>`).join('');chart.append(dates);
      const summarySeries = drawable.find(item => item.id === "total") || drawable[0];
      const first=summarySeries?.usable[0],last=summarySeries?.usable.at(-1);
      if(first&&last){
        const summary=document.createElement('p');summary.className='evidence-hint';
        summary.textContent=`${pointLabel(first)} ${money(first.valueMinor)} -- ${pointLabel(last)} ${money(last.valueMinor)}${Number.isFinite(changes.total)?` · ${signedPct(changes.total)}`:""}`;
        chart.append(summary);
      }
    }
  }
  // 各分类走势（demo category-trends）：横向滚动小卡＝彩点＋名称＋区间变化＋迷你走势，
  // 点击切换主图分类；sparkline 与主图同源（同一批真实估值点），不足两点不画线。
  const categories=document.getElementById("home-history-categories");
  if(categories){
    const cards=[...groupsById.values()].map(group=>{
      const id=String(group.groupId);
      const usable=usableOf(seriesPointsFor(seriesFor(id),id));
      const change=changes[id];
      const color=seriesColor(id);
      const pct=!hidden&&Number.isFinite(change)
        ?`<span class="history-cat-return${change>0?" up":change<0?" down":""}">${signedPct(change)}</span>`
        :`<span class="history-cat-return">–</span>`;
      let spark="";
      if(!hidden&&usable.length>=2){
        const values=usable.map(point=>Number(BigInt(point.valueMinor)));
        const min=Math.min(...values),max=Math.max(...values);
        const step=usable.length>1?116/(usable.length-1):0;
        const path=usable.map((point,index)=>`${(2+index*step).toFixed(1)},${(2+(max===min?0.5:1-(values[index]-min)/(max-min))*20).toFixed(1)}`).join(" ");
        spark=`<svg class="history-cat-spark" viewBox="0 0 120 24" preserveAspectRatio="none" aria-hidden="true"><polyline points="${path}" fill="none" stroke="${color}" stroke-width="1.6"/></svg>`;
      }
      const selected=!historySelection.off.has(id);
      return `<button type="button" class="history-cat-card${selected?" selected":""}" data-history-cat="${home.escape(id)}" aria-pressed="${selected}"><span class="history-cat-name"><i style="background:${color}" aria-hidden="true"></i>${home.escape(hidden?"****":group.name)}</span>${pct}${spark}</button>`;
    });
    if(cards.length){
      categories.hidden=false;
      categories.innerHTML=`<div class="home-history-categories-head"><strong>${home.escape(t("fundHomeHistoryCategories"))}</strong><span>${home.escape(t("fundHomeHistoryCategoriesHint"))}</span></div><div class="home-history-cat-scroll"><div class="home-history-cat-grid">${cards.join("")}</div></div>`;
      categories.querySelectorAll("[data-history-cat]").forEach(button=>{
        button.onclick=()=>{
          const id=button.dataset.historyCat;
          // 与 pill 同规则：全选状态下点卡＝单看该分组（总资产基准保留）。
          if(historySelection.off.size===0)historySelection.off=new Set([...groupIds].filter(other=>other!==id));
          else if(historySelection.off.has(id))historySelection.off.delete(id);
          else historySelection.off.add(id);
          home.renderOverview(home.snapshot());
          requestAnimationFrame(()=>document.querySelector(`#home-history-categories [data-history-cat="${id}"]`)?.focus());
        };
      });
    }else{categories.hidden=true;categories.replaceChildren();}
  }
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
    if (key === "benchmarkPpm") line.setAttribute("stroke-dasharray","6 4");
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
    const rows = (c.accounts || []).map(a => `<tr>
      <td>${hidePrivate() ? t("fundHomeAccount72acfe") : escapeHtml(a.accountName)}</td>
      <td>${mask(fmtMinorFull(a.cashMinor))}</td>
      <td>${a.reserveState === "configured" ? mask(fmtMinorFull(a.reservedMinor)) : t("fundHomeNotConfirmed")}</td>
      <td>${mask(fmtMinorFull(a.reservedByOrdersMinor))}</td>
      <td>${a.availableMinor != null ? mask(fmtMinorFull(a.availableMinor)) : t("fundHomeNotConfirmed")}</td>
    </tr>`).join("");
    return `<table>
      <thead><tr><td>${t("fundHomeAccount")}</td><td>${t("fundHomeSettledCash")}</td><td>${t("fundHomeReserve")}</td><td>${t("fundHomeCommitment")}</td><td>${t("fundHomeAvailable")}</td></tr></thead>
      <tbody>${rows || `<tr><td colspan="5">${t("fundHomeNoCashAssetsRecorded")}</td></tr>`}</tbody>
    </table>
    <div style="margin-top:8px;font-size:12px;color:var(--natives-text-muted)">
      ${t("fundHomeManageReservesAndCommitmentsInCashReconciliation")}
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
    return `<table>
      <tr><td>${t("fundHomeManagedAssetsA")}</td><td>${w.totalA != null ? mask(fmtMinorFull(w.totalA)) : t("fundHomeNeedsVerificationccf920")}</td></tr>
      <tr><td>${t("fundHomeNoncashMarketValueI")}</td><td>${w.investedI != null ? mask(fmtMinorFull(w.investedI)) : t("fundHomeNeedsVerificationccf920")}</td></tr>
      <tr><td>${t("fundHomeLastValuationDate")}</td><td>${escapeHtml(w.latestValuation || "–")}</td></tr>
      <tr><td>${t("fundHomeDataQuality")}</td><td>${escapeHtml(t(qualityKey || "fundHomeNeedsVerificationccf920"))}</td></tr>
      <tr><td>${t("fundHomeItemsToVerify")}</td><td>${escapeHtml(issues)}</td></tr>
    </table>
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
