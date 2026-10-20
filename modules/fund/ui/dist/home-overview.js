// R2 overview projection; lifecycle and snapshot ownership stay in home.js.
(() => {
const { state, t } = Fund;
const home = Fund.home = Fund.home || {};
// 集中度列表分页状态（卡内保留，语言切换/快照刷新不重置页码）。
const concentrationState = { page: 0 };
const historyState = { page:0, identity:"" };
const percent = (bps) => bps == null ? "–" : (Number(bps) / 100).toLocaleString(state.language, { maximumFractionDigits: 2 }) + "%";
const dateLabel = date => new Date(date + "T00:00:00+08:00").toLocaleDateString(state.language, { timeZone: "Asia/Shanghai" });
const reasons = {
  missing_baseline: "fundHomeOverviewMissingBaseline", missing_end: "fundHomeOverviewMissingEnd",
  missing_daily_history: "fundHomeOverviewMissingDays", unverified_history: "fundHomeOverviewUnverified",
  nonpositive_baseline: "fundHomeOverviewZeroBaseline",
};
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
    : (BigInt(cum.totalMinor) > 0n ? "+" : "") + home.money(cum.totalMinor);
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
  const details = document.getElementById("home-history-points");
  text("home-history-status", h.reason ? t(reasons[h.reason] || "fundHomeOverviewUnverified") : t("fundHomeOverviewFlowTiming"));
  if(h.truncated)text("home-history-status",document.getElementById("home-history-status").textContent+" · "+t("fundHomeRangeTruncated"));
  if(h.status==="reference")text("home-history-status",document.getElementById("home-history-status").textContent+" · "+t("fundHomeRangeReference"));
  text("home-benchmark-status", t("fundHomeOverviewNoBenchmark"));
  if (chart) {
    const usable=h.points.filter(p=>["ready","reference"].includes(p.quality));
    chart.replaceChildren();
    if(hidden){
      chart.innerHTML=`<div class="history-empty"><p>${t("fundHomeOverviewHistoryHidden")}</p><button type="button" class="btn" data-history-reveal>${t("fundHomeShowAmounts")}</button></div>`;
      chart.querySelector('[data-history-reveal]').onclick=()=>home.applyDisplayMode('amount');
    } else if(usable.length<=1){
      const point=usable[0],w=snapshot.wealth;
      const current=snapshot.requestedRange?.to===snapshot.requestedRange?.today;
      const value=point?.valueMinor??(current&&w.quality!=="empty"?(w.totalA??w.knownA):null);
      if(value!=null){
        const partial=point?point.quality!=="ready"&&point.quality!=="reference":w.totalA==null;
        const key=partial?"fundHomeValuedAssets":point?"fundHomeHistoryOneValue":"fundHomeHistoryCurrentValue";
        const date=point?.date||w.latestValuation;
        chart.innerHTML=`<section class="history-single"><div><span>${t(key)}</span><strong>${home.escape(money(value))}</strong><small>${date?home.escape(dateLabel(date)):t("fundHomeSourceTimestampUnknown")}${!point||point.quality==="reference"?" · "+t("fundHomeRangeReference"):""}</small></div><svg viewBox="0 0 600 120" role="img" aria-label="${t("fundHomeOverviewHistoryLabel")}"><circle cx="300" cy="60" r="6" fill="currentColor"/></svg><p>${t("fundHomeHistoryOnePoint")}</p></section>`;
        if(!point&&w.quality!=="ready")chart.insertAdjacentHTML('beforeend',`<p class="evidence-hint">${t("fundHomeHistoryValuationGap")}</p>`);
      }else chart.innerHTML=`<div class="history-empty"><p>${t(current?"fundHomeOverviewNoHistory":"fundHomeHistoryRangeEmpty")}</p>${!current?`<button class="btn" type="button" data-history-today>${t("fundHomeToday")}</button>`:""}</div>`;
      if(amount){
        const button=document.createElement('button');button.type='button';button.className='btn';button.id='home-history-save';button.dataset.historySave='';button.textContent=t('fundHomeHistoryRecord');button.onclick=()=>home.manageCash('snapshot');chart.append(button);
      }
      const todayButton=chart.querySelector('[data-history-today]');if(todayButton)todayButton.onclick=()=>Fund.actions.setHomeRange('today');
    } else if (h.points.length >= 2) {
      // Amounts never enter SVG attributes/tooltips; only normalized coordinates.
      const values = h.points.map(point => BigInt(point.valueMinor));
      const plotted=usable.map(point=>BigInt(point.valueMinor));
      const min = plotted.reduce((a,b)=>a<b?a:b), max = plotted.reduce((a,b)=>a>b?a:b);
      const span = max - min || 1n;
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.setAttribute("viewBox", "0 0 600 120");
      svg.setAttribute("role", "img");
      svg.setAttribute("aria-label", t("fundHomeOverviewHistoryLabel"));
      let segment = [];
      const flush = () => {
        if (segment.length > 1) {
          const line = document.createElementNS(svg.namespaceURI,"polyline");
          line.setAttribute("points",segment.join(" "));
          line.setAttribute("fill","none"); line.setAttribute("stroke","currentColor"); line.setAttribute("stroke-width","2");
          svg.append(line);
        }
        segment = [];
      };
      h.points.forEach((point,i)=> {
        if (!["ready","reference"].includes(point.quality)) { flush(); return; }
        if (i && Date.parse(point.date)-Date.parse(h.points[i-1].date) !== 86400000) flush();
        const x = 16+(Date.parse(point.date)-Date.parse(h.points[0].date))*568
          /Math.max(86400000,Date.parse(h.points.at(-1).date)-Date.parse(h.points[0].date));
        const y=max===min?60:110-Number((values[i]-min)*100n/span);
        const dot=document.createElementNS(svg.namespaceURI,"circle");
        dot.setAttribute("cx",String(x));dot.setAttribute("cy",String(y));dot.setAttribute("r","3");dot.setAttribute("fill","currentColor");svg.append(dot);
        if(point.quality==="reference"){
          if(segment.length){const line=document.createElementNS(svg.namespaceURI,"polyline");line.setAttribute("points",[segment.at(-1),`${x},${y}`].join(" "));line.setAttribute("fill","none");line.setAttribute("stroke","currentColor");line.setAttribute("stroke-dasharray","4 4");svg.append(line);}flush();return;
        }
        segment.push(`${x},${y}`);
      });
      flush(); chart.append(svg);
      const dates=document.createElement('div');dates.className='history-axis';
      dates.innerHTML=`<span>${home.escape(dateLabel(h.points[0].date))}</span><span>${home.escape(dateLabel(h.points.at(-1).date))}</span>`;chart.append(dates);
    } else chart.textContent = t(hidden ? "fundHomeOverviewHistoryHidden" : "fundHomeOverviewNoHistory");
  }
  if (details) {
    const open = details.querySelector("details")?.open;
    const focused = details.contains(document.activeElement);
    const identity=h.from+":"+h.to;
    if(historyState.identity!==identity){historyState.page=0;historyState.identity=identity;}
    const pages=Math.max(1,Math.ceil(h.points.length/20));historyState.page=Math.min(historyState.page,pages-1);
    const pagePoints=h.points.slice(historyState.page*20,historyState.page*20+20);
    details.innerHTML = hidden ? "" : `<details ${open ? "open" : ""}><summary>${t("fundHomeOverviewSavedPoints")}</summary><table class="alloc-table"><thead><tr><th>${t("fundHomeValuationDate")}</th><th>${t("fundHomeOverviewRecordedValue")}</th></tr></thead><tbody>${pagePoints.map(point=>`<tr><td>${home.escape(dateLabel(point.date))}</td><td>${amount ? home.escape(money(point.valueMinor)) : "****"} · ${t(point.quality === "reference"?"fundHomeRangeReference":point.quality === "ready" ? "fundHomeOverviewComplete" : "fundHomeIncompleteData")}</td></tr>`).join("")}</tbody></table>${pages>1?`<nav class="home-conc-pager"><button type="button" data-history-prev ${historyState.page===0?"disabled":""} aria-label="${t("fundHomePrevious")}">‹</button><span>${historyState.page+1}/${pages}</span><button type="button" data-history-next ${historyState.page>=pages-1?"disabled":""} aria-label="${t("fundHomeNext")}">›</button></nav>`:""}</details>`;
    for(const [selector,delta] of [["[data-history-prev]",-1],["[data-history-next]",1]]){
      const button=details.querySelector(selector);if(button)button.onclick=()=>{historyState.page+=delta;home.renderOverview(snapshot);details.querySelector(selector)?.focus();};
    }
    if (focused) details.querySelector("summary")?.focus();
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
    line.setAttribute("fill","none"); line.setAttribute("stroke","currentColor"); line.setAttribute("stroke-width","2");
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
 if(!ga?.configured){left.innerHTML=`<p>${home.escape(t('fundHomeGroupNotConfigured'))}</p>`;right.innerHTML=`<p>${home.escape(t('fundHomeGroupSetup'))}</p>`;}
 else{
  const groups=ga.groups||[];
  const bar=field=>{if(hidden||groups.some(g=>g[field]==null))return '';const values=groups.slice(0,5).map(g=>Number(g[field]));if(groups.length>5)values.push(groups.slice(5).reduce((sum,g)=>sum+Number(g[field]),0));return `<div class="home-allocation-bar" aria-hidden="true">${values.map((value,index)=>`<span style="width:${Math.max(0,Math.min(100,value/10))}%;background:var(${index===0?'--natives-accent':index<5?'--fund-allocation-'+(index+1):'--natives-control'})"></span>`).join('')}</div>`;};

  left.innerHTML=bar('currentTenths')+`<div class="distribution-tree">${groups.map(g=>`<details data-group-id="${home.escape(g.groupId)}" ${opened.has(String(g.groupId))?'open':''}><summary><strong>${name(g.name)}</strong><span class="num">${g.currentMinor==null?home.escape(t('fundDetailMissing')):money(g.currentMinor)} · ${percent(g.currentTenths)}</span></summary><p class="allocation-tip">${hidden?'':home.escape(g.description||t('fundHomeGroupDescriptionEmptyHint'))}</p><div class="distribution-children">${(g.assets||[]).map(asset=>{
   let weight='–';if(!hidden&&asset.valueMinor!=null&&g.currentMinor!=null&&BigInt(g.currentMinor)>0n)weight=percent(BigInt(asset.valueMinor)*1000n/BigInt(g.currentMinor));
   return `<div class="distribution-child"><div><strong>${name(asset.name||asset.code||'–')}</strong><small>${hidden?'':home.escape(asset.code||'')} · ${hidden?'':home.escape(t(asset.instrumentKind==='stock'?'fundDetailStockType':asset.venue==='exchange'?'fundLedgerTypeEtf':asset.instrumentKind==='fund'?'fundLedgerTypeOtcFund':'fundHomeGroup'))}</small></div><div class="num">${asset.valueMinor==null?home.escape(t('fundDetailMissing')):money(asset.valueMinor)}<small>${weight}</small></div></div>`;
  }).join('')||`<p>${home.escape(t('fundHomeGroupNoAssets'))}</p>`}</div></details>`).join('')}</div>${ga.unassignedMinor!=null?`<p>${home.escape(t('fundHomeGroupUnassigned'))} ${money(ga.unassignedMinor)}</p>`:''}${ga.invalidRefs?.length?`<p>${home.escape(t('fundHomeGroupInvalidRefs'))} ${ga.invalidRefs.length}</p>`:''}`;
  right.innerHTML=bar('ratioTenths')+`<div class="combination-columns"><span>${home.escape(t('fundHomeGroup'))}</span><span>${home.escape(t('fundHomeGroupTarget'))}</span><span>${home.escape(t('fundHomeGroupActual'))}</span></div>${groups.map(g=>{
   const difference=g.currentTenths==null?null:Number(g.currentTenths)-Number(g.ratioTenths);
   const deviation=hidden?'':difference==null?t('fundDetailMissing'):difference===0?t('fundHomeGroupDeviationExact'):t(difference>0?'fundHomeGroupDeviationOver':'fundHomeGroupDeviationUnder')+' '+(Math.abs(difference)/10).toFixed(1)+' '+t('fundDetailPercentagePoints');
   return `<div class="combination-row"><div class="combination-values"><strong>${name(g.name)}</strong><span>${percent(g.ratioTenths)}</span><span>${percent(g.currentTenths)}</span></div><p class="allocation-tip">${hidden?'':home.escape(g.description||t('fundHomeGroupDescriptionEmptyHint'))}</p><p class="allocation-tip">${home.escape(deviation)}</p></div>`;
  }).join('')}<p>${home.escape(t('fundHomeGroupTargetTotal'))} ${percent(groups.reduce((n,g)=>n+Number(g.ratioTenths),0))} · ${home.escape(t('fundHomeGroupRemaining'))} ${percent(ga.unallocatedTenths)}</p>`;
 }
 left.scrollTop=scrollPositions[0];right.scrollTop=scrollPositions[1];
 if(focusGroup&&!focus.isConnected)left.querySelector(`[data-group-id="${CSS.escape(focusGroup)}"] > summary`)?.focus({preventScroll:true});
 for(const id of ['home-group-manage','home-combination-manage']){const button=document.getElementById(id);if(button)button.onclick=()=>home.manageGroupPlan();}
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
    const issues = (w.issues || []).map(i => `${i.code} × ${i.count}`).join("；") || t("fundHomeNone");
    return `<table>
      <tr><td>${t("fundHomeManagedAssetsA")}</td><td>${w.totalA != null ? mask(fmtMinorFull(w.totalA)) : t("fundHomeNeedsVerificationccf920")}</td></tr>
      <tr><td>${t("fundHomeNoncashMarketValueI")}</td><td>${w.investedI != null ? mask(fmtMinorFull(w.investedI)) : t("fundHomeNeedsVerificationccf920")}</td></tr>
      <tr><td>${t("fundHomeLastValuationDate")}</td><td>${escapeHtml(w.latestValuation || "–")}</td></tr>
      <tr><td>${t("fundHomeDataQuality")}</td><td>${escapeHtml(w.quality)}</td></tr>
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
        const time = home.escape(home.informationTimeLabel(item.publishedAt));
        return `<article class="info-item"><div class="home-news-meta"><span class="news-category">${home.escape(home.informationSourceLabel(item.sourceId))}</span><time>${time}</time></div><h3>${home.escape(item.title)}</h3>
          ${item.summary?`<p class="home-news-summary">${home.escape(item.summary)}</p>`:''}
          <div class="home-news-item-footer"><small>${home.escape(rel)}</small><span class="info-actions">
            <button class="btn btn-sm" data-act="news-ai" data-id="${item.id}" ${home.isNewsAISelected?.(item.id) ? 'aria-pressed="true"' : 'aria-pressed="false"'}>${t("fundHomeNewsAiBtn")}</button>
            <button class="btn btn-sm" data-info-open="${item.id}">${t("fundHomeOpenSource")}</button>
            <button class="btn btn-sm" data-info-fav="${item.id}" aria-pressed="${item.favorite}">${item.favorite ? t("fundHomeFavorited") : t("fundHomeFavorite")}</button>
          </span></div></article>`;

};
})();
