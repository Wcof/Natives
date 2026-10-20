// Candidate projection and explicit bounded refresh; idle state has no polling or AI request.
(() => {
const {home,t}=Fund;
home.opportunityError=e=>e.messageKey?t(e.messageKey):Fund.localizeApiError(e.status||500,e.code,'');
let filters={kind:"all",market:"all",pool:"all",holding:"unheld",strategy:"all",risk:"all",minScore:"70",sort:"rank"};
let candidatePage=1;
const CANDIDATE_PAGE_SIZE=20;
let draft={...filters};
let advancedFilters=false;
const labels={
  "Sort":"fundHomeOpportunitySort",
  "Rank":"fundHomeOpportunityRank",
  "RankAsc":"fundHomeOpportunityRankAsc",
  "Name":"fundHomeOpportunityName",
  "Move":"fundHomeOpportunityMove",
  "Refresh":"fundHomeOpportunityRefresh",
  "Scope":"fundHomeOpportunityScope",
  "NextBatch":"fundHomeOpportunityNextBatch",
  "Cancel":"fundHomeOpportunityCancel",
  "idle":"fundHomeOpportunityidle",
  "starting":"fundHomeOpportunitystarting",
  "running":"fundHomeOpportunityrunning",
  "cancelling":"fundHomeOpportunitycancelling",
  "cancelled":"fundHomeOpportunitycancelled",
  "complete":"fundHomeOpportunitycomplete",
  "error":"fundHomeOpportunityerror",
  "Fetched":"fundHomeOpportunityFetched",
  "upstream_error":"fundHomeOpportunityupstream_error",
  "unsupported":"fundHomeOpportunityunsupported",
  "empty_history":"fundHomeOpportunityempty_history",
  "invalid_history":"fundHomeOpportunityinvalid_history",
  "store_error":"fundHomeOpportunitystore_error",
  "generation_conflict":"fundHomeOpportunitygeneration_conflict",
  "Automatic":"fundHomeOpportunityAutomatic",
  "Deadline":"fundHomeOpportunityDeadline",
  "BatchHint":"fundHomeOpportunityBatchHint",
  "Above":"fundHomeOpportunityAbove",
  "All":"fundHomeOpportunityAll",
  "AsOf":"fundHomeOpportunityAsOf",
  "Bear":"fundHomeOpportunityBear",
  "Below":"fundHomeOpportunityBelow",
  "Bull":"fundHomeOpportunityBull",
  "CN":"fundHomeOpportunityCN",
  "Closed":"fundHomeOpportunityClosed",
  "Coverage":"fundHomeOpportunityCoverage",
  "Empty":"fundHomeOpportunityEmpty",
  "Evidence":"fundHomeOpportunityEvidence",
  "Filter":"fundHomeOpportunityFilter",
  "Fund":"fundHomeOpportunityFund",
  "HK":"fundHomeOpportunityHK",
  "Held":"fundHomeOpportunityHeld",
  "Heuristic":"fundHomeOpportunityHeuristic",
  "Hidden":"fundHomeOpportunityHidden",
  "Holding":"fundHomeOpportunityHolding",
  "Holdings":"fundHomeOpportunityHoldings",
  "Kind":"fundHomeOpportunityKind",
  "Limited":"fundHomeOpportunityLimited",
  "Ma":"fundHomeOpportunityMa",
  "Macd":"fundHomeOpportunityMacd",
  "Market":"fundHomeOpportunityMarket",
  "MinScore":"fundHomeOpportunityMinScore",
  "Missing":"fundHomeOpportunityMissing",
  "MissingCount":"fundHomeOpportunityMissingCount",
  "Mixed":"fundHomeOpportunityMixed",
  "MixedMa":"fundHomeOpportunityMixedMa",
  "NoSignal":"fundHomeOpportunityNoSignal",
  "NotApplicable":"fundHomeOpportunityNotApplicable",
  "Pool":"fundHomeOpportunityPool",
  "PublishedNav":"fundHomeOpportunityPublishedNav",
  "Reset":"fundHomeOpportunityReset",
  "Risk":"fundHomeOpportunityRisk",
  "Score":"fundHomeOpportunityScore",
  "Snapshot":"fundHomeOpportunitySnapshot",
  "Stock":"fundHomeOpportunityStock",
  "Strategy":"fundHomeOpportunityStrategy",
  "US":"fundHomeOpportunityUS",
  "Unheld":"fundHomeOpportunityUnheld",
  "Unknown":"fundHomeOpportunityUnknown",
  "VolumeRatio":"fundHomeOpportunityVolumeRatio",
  "Watchlist":"fundHomeOpportunityWatchlist",
  "avoid":"fundHomeOpportunityavoid",
  "build_candidate":"fundHomeOpportunitybuild_candidate",
  "cash_unverified":"fundHomeOpportunitycash_unverified",
  "fund_exposure_unknown":"fundHomeOpportunityfund_exposure_unknown",
  "hold":"fundHomeOpportunityhold",
  "insufficient_history":"fundHomeOpportunityinsufficient_history",
  "liquidity_unverified":"fundHomeOpportunityliquidity_unverified",
  "macd_positive":"fundHomeOpportunitymacd_positive",
  "momentum":"fundHomeOpportunitymomentum",
  "observe":"fundHomeOpportunityobserve",
  "pullback":"fundHomeOpportunitypullback",
  "rebound":"fundHomeOpportunityrebound",
  "review_add":"fundHomeOpportunityreview_add",
  "review_reduce":"fundHomeOpportunityreview_reduce",
  "stale_data":"fundHomeOpportunitystale_data",
  "trend_follow":"fundHomeOpportunitytrend_follow",
  "valuation_unverified":"fundHomeOpportunityvaluation_unverified",
  "volume_breakout":"fundHomeOpportunityvolume_breakout",
  "Previous":"fundHomeOpportunityPrevious",
  "Next":"fundHomeOpportunityNext",
  "ConstraintsCount":"fundHomeUrgentConstraints"
};
const label=code=>t(labels[code]||"fundHomeUnknown");
home.opportunityLabel=label;
let scope="all",job={status:"idle"},jobGen=0,jobTimer=null,jobController=null;
const running=()=>["starting","running","cancelling"].includes(job.status);
function redraw(){home.renderOpportunities(home.snapshot()?.opportunities);}
function refreshHtml(hidden=false){
  const esc=home.escape;
  return `<div class="opportunity-refresh" data-opportunity-task-id="${job.taskId||''}" data-opportunity-status="${job.status}" aria-busy="${running()}"><label>${label("Scope")}<select name="refreshScope" data-opportunity-scope ${running()?"disabled":""}>${[["all","All"],["holdings","Holdings"],["watchlist","Watchlist"]].map(([v,k])=>`<option value="${v}" ${scope===v?"selected":""}>${label(k)}</option>`).join("")}</select></label><button type="button" class="btn" name="opportunityRefresh" data-opportunity-refresh ${running()?"disabled":""}>${label("Refresh")}</button>${!running()&&job.nextOffset!=null&&job.scope===scope?`<button type="button" class="btn" name="opportunityNext" data-opportunity-next>${label("NextBatch")}</button>`:""}${running()?`<button type="button" class="btn" name="opportunityCancel" data-opportunity-cancel ${job.status==="cancelling"?"disabled":""}>${label("Cancel")}</button>`:""}<p role="status">${label(job.status)}${hidden?"":job.total!=null?` · ${job.completed||0} / ${job.total}`:""}</p>${!hidden&&job.total!=null?`<progress max="${Math.max(1,job.total)}" value="${job.completed||0}" aria-label="${label("Refresh")}"></progress>`:""}<p class="evidence-hint">${label("BatchHint")}</p>${job.deadlineHit?`<p>${label("Deadline")}</p>`:""}${job.generationError?`<p role="alert">${label(job.generationError)}</p>`:""}${job.error?`<p role="alert">${esc(home.opportunityError(job.error))}</p>`:""}${hidden?"":(job.items||[]).map(r=>`<p>${esc(r.securityId)} · ${r.status==="fetched"?label("Fetched"):label(labels[r.errorCode]?r.errorCode:"Unknown")}</p>`).join("")}</div>`;
}
function bindRefresh(body){
  body.querySelector("[data-opportunity-scope]").onchange=e=>{scope=e.target.value;redraw();};
  body.querySelector("[data-opportunity-refresh]").onclick=()=>startRefresh(0);
  body.querySelector("[data-opportunity-next]")?.addEventListener("click",()=>startRefresh(job.nextOffset,job.poolId));
  body.querySelector("[data-opportunity-cancel]")?.addEventListener("click",()=>{home.stopOpportunityRefresh();redraw();home.resumeOpportunityRefresh();});
}
async function cancelRemote(target){
  try {await Fund.api("POST","/api/opportunities/cancel",typeof target==="number"?{taskId:target}:{requestId:target});}
  catch(e){if(job.taskId===target||job.requestId===target){job.error=e;if(home.isActive?.())redraw();}}
}
async function startRefresh(offset,poolId){
  if(running()||!home.isActive?.())return;
  const gen=++jobGen,requestId=home.requestId();
  job={status:"starting",scope,requestId};redraw();
  try {
    // Keep the quick start response observable so a leave-before-reply can cancel its exact task.
    const started=await Fund.api("POST","/api/opportunities/refresh",{requestId,scope,offset,...poolId?{poolId}:{}});
    if(job.requestId!==requestId)return;
    job.taskId=started.taskId;
    if(gen!==jobGen||!home.isActive?.()){
      job.status="cancelling";await cancelRemote(started.taskId);
      if(home.isActive?.())home.resumeOpportunityRefresh();
      return;
    }
    job.status="running";await poll(gen);
  }catch(e){if(job.requestId===requestId){job.status="error";job.error=e;await cancelRemote(requestId);if(home.isActive?.())redraw();}}
}
async function poll(gen){
  if(gen!==jobGen||!home.isActive?.()||!job.taskId)return;
  const controller=new AbortController();jobController=controller;
  try {
    const result=await Fund.api("GET",`/api/opportunities/status?taskId=${job.taskId}`,null,{signal:controller.signal});
    if(gen!==jobGen||controller.signal.aborted)return;
    job={...job,...result};redraw();
    if(["complete","cancelled"].includes(job.status)){await home.refresh();return;}
    jobTimer=setTimeout(()=>{jobTimer=null;poll(gen);},500);
  }catch(e){if(gen===jobGen&&!controller.signal.aborted){job.status="error";job.error=e;await cancelRemote(job.taskId);redraw();}}
  finally{if(jobController===controller)jobController=null;}
}
home.stopOpportunityRefresh=()=>{
  ++jobGen;clearTimeout(jobTimer);jobTimer=null;jobController?.abort();jobController=null;
  if(running()||job.status==="error"&&job.taskId){
    job.status="cancelling";
    if(job.taskId)cancelRemote(job.taskId);
  }
};
home.resumeOpportunityRefresh=()=>{if(running()&&job.taskId&&!jobController&&!jobTimer)poll(++jobGen);};
async function strategiesDialog(){
  const hh=home;
  let data;
  try{data=await Fund.api("GET","/api/strategies");}catch(e){hh.closePanel();hh.status(e.message);return;}
  const rows=()=>(data.items||[]).map(s=>`<article data-strategy-row="${home.escape(s.code)}"><h4>${home.escape(s.name)} · <code>${home.escape(s.code)}</code></h4><p>${home.escape(s.description)} · ${home.escape(s.riskLevel)} · ${t("fundLedgerThStatus")}：${s.enabled?t("fundHomeEnabled"):t("fundHomeDisabled")} · ${t("fundHomeStrategyWeight")}：${(s.weightPpm/1000000).toFixed(2)}${s.overridden?"":"（默认）"}</p><label>${t("fundHomeStrategyWeight")}<input name="w-${home.escape(s.code)}" type="number" min="0.1" max="3" step="0.05" value="${(s.weightPpm/1000000).toFixed(2)}"></label><button type="button" class="btn" data-strategy-save="${home.escape(s.code)}">${t("fundHomeSave")}</button><button type="button" class="btn" data-strategy-enable="${home.escape(s.code)}" data-on="${s.enabled?0:1}">${s.enabled?t("fundHomeStrategyDisable"):t("fundHomeStrategyEnable")}</button>${s.overridden?`<button type="button" class="btn" data-strategy-restore="${home.escape(s.code)}">${t("fundHomeStrategyRestoreDefault")}</button>`:""}</article>`).join("");
  hh.panel(
    ()=>t("fundHomeStrategyManage"),
    ()=>rows()+`<p class="home-form-error" role="status" aria-live="polite"></p>`,
    root=>{
      const commit=async payload=>{
        await Fund.api("POST","/api/strategies/update",{...payload,expectedRevision:data.revision,requestId:hh.requestId()});
        await hh.refresh();
        strategiesDialog();
      };
      const fail=root.querySelector(".home-form-error");
      root.querySelectorAll("[data-strategy-save]").forEach(b=>b.onclick=async()=>{
        const code=b.dataset.strategySave;
        const input=root.querySelector(`[name="w-${code}"]`);
        const factor=Number(input.value);
        if(!(factor>=0.1&&factor<=3)){fail.textContent=t("fundHomeStrategyWeight");return;}
        b.disabled=true;
        try{await commit({code,weightPpm:Math.round(factor*1000000)});}catch(e){fail.textContent=e.message;b.disabled=false;}
      });
      root.querySelectorAll("[data-strategy-enable]").forEach(b=>b.onclick=async()=>{
        b.disabled=true;
        try{await commit({code:b.dataset.strategyEnable,enabled:b.dataset.on==="1"});}catch(e){fail.textContent=e.message;b.disabled=false;}
      });
      root.querySelectorAll("[data-strategy-restore]").forEach(b=>b.onclick=async()=>{
        b.disabled=true;
        try{await commit({code:b.dataset.strategyRestore,restore:true});}catch(e){fail.textContent=e.message;b.disabled=false;}
      });
    },
    true,
  );
}
home.manageStrategies=strategiesDialog;
home.opportunityPlanHtml = row => {
  if(home.displayMode()==="hidden")return "";
  const p=row.conditions, inv=row.invalidation;
  if(!p)return "";
  const esc=home.escape;
  const keys={Title:'fundHomeOpportunityPlanTitle',Blocked:'fundHomeOpportunityPlanBlocked',Nav:'fundHomeOpportunityPlanNav',Review:'fundHomeOpportunityPlanReview',Unavailable:'fundHomeOpportunityPlanUnavailable',Entry:'fundHomeOpportunityPlanEntry',Target:'fundHomeOpportunityPlanTarget',Below:'fundHomeOpportunityPlanBelow',Defensive:'fundHomeOpportunityPlanDefensive',Refresh:'fundHomeOpportunityPlanRefresh',Reference:'fundHomeOpportunityPlanReference'};
  const text=key=>t(keys[key]);
  const price=value=>{
    if(!Number.isFinite(value)||!Number.isFinite(p.referencePrice)||p.referencePrice<=0)return text("Unavailable");
    return home.displayMode()==="amount"?new Intl.NumberFormat(Fund.state.language,{maximumFractionDigits:4}).format(value):new Intl.NumberFormat(Fund.state.language,{style:"percent",maximumFractionDigits:2,signDisplay:"always"}).format(value/p.referencePrice-1);
  };
  return `<section data-opportunity-plan><h4>${text("Title")}</h4><p>${text(p.state==="blocked"?"Blocked":p.state==="nav_review"?"Nav":p.state==="review"?"Review":"Unavailable")}</p>${p.entryRange?`<p>${text("Entry")}：${esc(price(p.entryRange[0]))} ~ ${esc(price(p.entryRange[1]))}</p>`:""}${p.target!=null?`<p>${text("Target")}：${esc(price(p.target))}</p>`:""}${inv?.below!=null?`<p>${text("Below")}：${esc(price(inv.below))}</p>`:""}${inv?.reason==="risk_control"?`<p>${text("Defensive")}</p>`:""}<p class="evidence-hint">${text("Refresh")}</p><p class="evidence-hint">${text("Reference")}</p></section>`;
};
const urgentCopy={Title:'fundHomeUrgentTitle',Hint:'fundHomeUrgentHint',event:'fundHomeUrgentEvent',holding:'fundHomeUrgentHolding',opportunity:'fundHomeUrgentOpportunity',Locate:'fundHomeUrgentLocate',NoCandidate:'fundHomeUrgentNoCandidate',Truncated:'fundHomeUrgentTruncated'};
function urgentHtml(urgent){
  const esc=home.escape;
  if(!urgent||!Array.isArray(urgent.items))return "";
  const kindLabel=k=>t(urgentCopy[k]||'fundHomeUrgentOpportunity');
  const detail=d=>{
    const m=/^constraints:(\d+)$/.exec(String(d));
    if(m)return t("fundHomeUrgentConstraints").replace("{n}",m[1]);
    return home.opportunityLabel(String(d))||esc(String(d));
  };
  const rows=urgent.items.map(u=>`<li data-urgent-item="${esc(u.securityId)}"><span class="urgent-kind">${kindLabel(u.kind)}</span> <b>${esc(u.name)}</b>${u.changePct!=null?` <span class="${u.changePct>0?"up":u.changePct<0?"down":""}">${Number(u.changePct).toFixed(2)}%</span>`:""}<small>${esc(detail(u.detail))}</small>${u.candidateRef?`<button type="button" class="btn" data-urgent-locate="${esc(u.candidateRef)}">${t(urgentCopy.Locate)}</button>`:""}</li>`).join("");
  return `<section class="home-urgent" data-urgent-section aria-label="${t(urgentCopy.Title)}"><h3>${t(urgentCopy.Title)}</h3><p class="evidence-hint">${t(urgentCopy.Hint)}</p><ul>${rows||""}</ul>${urgent.truncated?`<p class="evidence-hint">${t(urgentCopy.Truncated)}</p>`:""}<p data-urgent-miss role="status" hidden>${t(urgentCopy.NoCandidate)}</p></section>`;
}
function bindUrgent(body){
  body.querySelectorAll("[data-urgent-locate]").forEach(b=>b.onclick=()=>{
    const ref=b.dataset.urgentLocate;
    const target=[...body.querySelectorAll("[data-opportunity-candidate]")].find(el=>el.dataset.opportunityCandidate===ref);
    const miss=body.querySelector("[data-urgent-miss]");
    if(target){if(miss)miss.hidden=true;target.scrollIntoView({behavior:"smooth",block:"start"});target.querySelector("summary")?.focus();target.classList.add("urgent-located");setTimeout(()=>target.classList.remove("urgent-located"),1200);}
    else if(miss)miss.hidden=false;
  });
}
home.renderOpportunities = data => {
  const body=document.getElementById("home-opportunity-body");
  if(!body)return;
  advancedFilters=body.querySelector(".opportunity-filter-more")?.open ?? advancedFilters;
  const focusedAdvanced=document.activeElement===body.querySelector(".opportunity-filter-more > summary");
  const focused=body.contains(document.activeElement)?document.activeElement.name:null;
  if(home.displayMode()==="hidden"){body.innerHTML=refreshHtml(true)+`<p>${label("Hidden")}</p>`;bindRefresh(body);if(focused)body.querySelector(`[name="${focused}"]`)?.focus();return;}
  if(!data){body.textContent=label("Missing");return;}
  const focusedCandidate=document.activeElement?.matches("summary")?document.activeElement.closest("[data-opportunity-candidate]")?.dataset.opportunityCandidate:null;
  const opened=new Set([...body.querySelectorAll("[data-opportunity-candidate] details[open]")].map(el=>el.closest("article").dataset.opportunityCandidate));
  const esc=home.escape;
  const select=(key,options)=>`<label>${label(key[0].toUpperCase()+key.slice(1))}<select name="${key}">${options.map(([value,copy])=>`<option value="${value}" ${draft[key]===value?"selected":""}>${label(copy)}</option>`).join("")}</select></label>`;
  const all=["all","All"];
  const form=`<form data-opportunity-filters><div class="opportunity-filter-primary">${select("kind",[all,["stock","Stock"],["fund","Fund"]])}${select("holding",[all,["held","Held"],["unheld","Unheld"]])}${select("sort",[["rank","Rank"],["rankAsc","RankAsc"],["name","Name"],["move","Move"]])}<label>${label("MinScore")}<input name="minScore" type="number" min="0" max="100" step="1" required value="${esc(draft.minScore)}"></label><button type="submit" class="btn">${label("Filter")}</button><button type="button" class="btn" data-opportunity-reset>${label("Reset")}</button></div><details class="opportunity-filter-more" ${advancedFilters?"open":""}><summary>${t("fundHomeAdvancedFilters")}</summary><div>${select("market",[all,["CN","CN"],["HK","HK"],["US","US"]])}${select("pool",[all,["holdings","Holdings"],["watchlist","Watchlist"],["mixed","Mixed"],["market","Market"]])}${select("strategy",[all,...["trend_follow","macd_positive","volume_breakout","momentum","rebound","pullback"].map(v=>[v,v])])}${select("risk",[all,["unknown","Unknown"]])}<button type="button" class="btn" data-strategy-manage>${t("fundHomeStrategyManage")}</button></div></details></form>`;
  const missing=(data.items||[]).filter(row=>row.quality!=="ready").length;
  const items=(data.items||[]).filter(row=>
    (filters.kind==="all"||(filters.kind==="fund"?["fund","etf"].includes(row.kind):row.kind==="stock"))&&
    (filters.market==="all"||(filters.market==="CN"?["CN","SSE","SZSE","BSE"].includes(row.market):row.market===filters.market))&&
    (filters.pool==="all"||row.pool===filters.pool)&&
    (filters.holding==="all"||row.isHolding===(filters.holding==="held"))&&
    (filters.strategy==="all"||row.strategyTags.includes(filters.strategy))&&
    (filters.risk==="all"||row.riskLevel===filters.risk)&&
    (Number(filters.minScore)===0||row.rankScore!=null&&row.rankScore>=Number(filters.minScore)));
  const sorters={rank:(a,b)=>(b.rankScore??-1)-(a.rankScore??-1)||String(a.securityId).localeCompare(String(b.securityId)),rankAsc:(a,b)=>(a.rankScore??101)-(b.rankScore??101)||String(a.securityId).localeCompare(String(b.securityId)),name:(a,b)=>String(a.name).localeCompare(String(b.name),Fund.state.language)||String(a.securityId).localeCompare(String(b.securityId)),move:(a,b)=>((b.evidence?.lastChangePct??-999)-(a.evidence?.lastChangePct??-999))||String(a.securityId).localeCompare(String(b.securityId))};
  items.sort(sorters[filters.sort]||sorters.rank);
  const pageCount=Math.max(1,Math.ceil(items.length/CANDIDATE_PAGE_SIZE));
  if(candidatePage>pageCount)candidatePage=pageCount;
  const paged=items.slice((candidatePage-1)*CANDIDATE_PAGE_SIZE,candidatePage*CANDIDATE_PAGE_SIZE);
  const pager=items.length>CANDIDATE_PAGE_SIZE?`<nav class="opportunity-pager" aria-label="${label("Filter")}"><button type="button" class="btn" data-candidate-page="-1" ${candidatePage<=1?"disabled":""}>${label("Previous")}</button> <span>${candidatePage} / ${pageCount}</span><button type="button" class="btn" data-candidate-page="1" ${candidatePage>=pageCount?"disabled":""}>${label("Next")}</button></nav>`:"";
  const rows=paged.map(row=>`<article data-opportunity-candidate="${esc(row.securityId)}">
    <h3>${esc(row.name)} · ${esc(row.code)} <small>${label(row.kind==="stock"?"Stock":"Fund")} · ${esc(row.market)}</small></h3>
    <p>${label("Score")}：${row.rankScore??label("Missing")} · ${label("Risk")}：${label("Unknown")} · ${label(row.action)}</p>
    <p>${label("AsOf")}：${esc(row.asOf||label("Missing"))} · ${esc(row.source)} · ${label(row.period==="published-nav"?"PublishedNav":"Closed")}</p>
    <p>${row.strategyTags.map(tag=>label(tag)).join("、")||label("NoSignal")}</p>
    <p>${row.constraints.map(code=>label(code)).join("、")}</p>
    ${home.opportunityPlanHtml(row)}
    <details ${opened.has(row.securityId)?"open":""}><summary>${label("Evidence")}</summary><p>${label("Snapshot")}：${esc(row.snapshotId)}</p>${row.evidence?`<p>${label("Ma")}：${label(row.evidence.maArrangement>0?"Bull":row.evidence.maArrangement<0?"Bear":"MixedMa")} · ${label("Macd")}：${label(row.evidence.macdState>0?"Above":"Below")}</p><p>${label("VolumeRatio")}：${row.evidence.volumeRatio==null?label("NotApplicable"):esc(Number(row.evidence.volumeRatio).toFixed(2))}</p>`:`<p>${label("insufficient_history")}</p>`}<p>${row.riskCodes.map(code=>label(code)).join("、")}</p></details>
  </article>`).join("");
  body.innerHTML=refreshHtml()+`${data.recordedSnapshot?`<p data-opportunity-recorded>${label("Automatic")}</p>`:""}<p class="evidence-hint">${label("Coverage")} · ${label("MissingCount")}：${missing} / ${data.inputCount}${data.limited?` · ${label("Limited")}`:""}</p>${urgentHtml(Fund.home.snapshot()?.urgent)}${form}<p class="evidence-hint">${label("Heuristic")}</p>${pager}${paged.length||items.length?"":`<p class="home-empty-hint">${label("Empty")}</p>`}${rows}${items.length?`<p class="evidence-hint">${label("Coverage")}：${items.length} / ${data.items.length}</p>`:""}${home.opportunityRecordsHtml?.()||""}`;
  home.bindOpportunityRecords?.(body);
  bindRefresh(body);
  bindUrgent(body);
  body.querySelector("[data-opportunity-filters]").onsubmit=e=>{e.preventDefault();if(!e.target.reportValidity())return;filters=Object.fromEntries(new FormData(e.target));draft={...filters};candidatePage=1;home.renderOpportunities(home.snapshot()?.opportunities);};
  body.querySelector("[data-opportunity-filters]").oninput=e=>{if(e.target.name in draft)draft[e.target.name]=e.target.value;};
  const more=body.querySelector(".opportunity-filter-more");
  more.ontoggle=()=>{if(more.isConnected)advancedFilters=more.open;};
  body.querySelectorAll("[data-candidate-page]").forEach(b=>b.onclick=()=>{candidatePage=Math.max(1,candidatePage+Number(b.dataset.candidatePage));home.renderOpportunities(home.snapshot()?.opportunities);});
  body.querySelector("[data-opportunity-reset]").onclick=()=>{filters={kind:"all",market:"all",pool:"all",holding:"unheld",strategy:"all",risk:"all",minScore:"70",sort:"rank"};draft={...filters};candidatePage=1;home.renderOpportunities(home.snapshot()?.opportunities);};
  body.querySelector("[data-strategy-manage]")?.addEventListener("click",strategiesDialog);
  if(focused)body.querySelector(`[name="${focused}"]`)?.focus();
  else if(focusedAdvanced)body.querySelector(".opportunity-filter-more > summary")?.focus();
  else if(focusedCandidate)[...body.querySelectorAll("[data-opportunity-candidate]")].find(el=>el.dataset.opportunityCandidate===focusedCandidate)?.querySelector("summary")?.focus();
};
})();

// History is loaded only on request. Reads have one abort owner; committed writes survive leaving.
(() => {
const {home,t}=Fund;
let history=null,stats=null,page=1,batchIndex=0,busy=false,error='',generation=0,controller=null,retry=null;
const drafts=new Map();
const copy={History:'fundHomeOpportunityHistory',DraftHint:'fundHomeOpportunityDraftHint',Pool:'fundHomeOpportunityPool',Strategy:'fundHomeOpportunityStrategy',holdings:'fundHomeOpportunityHoldings',watchlist:'fundHomeOpportunityWatchlist',mixed:'fundHomeOpportunityMixed',Unknown:'fundHomeOpportunityUnknown',Capture:'fundHomeOpportunityCapture',Records:'fundHomeOpportunityRecords',Hint:'fundHomeOpportunityRecordHint',Empty:'fundHomeOpportunityRecordEmpty',Useful:'fundHomeOpportunityUseful',NotUseful:'fundHomeOpportunityNotUseful',Reason:'fundHomeOpportunityReason',Previous:'fundHomeOpportunityPrevious',Next:'fundHomeOpportunityNext',Count:'fundHomeOpportunityFeedbackCount',Rate:'fundHomeOpportunityUsefulRate',Saved:'fundHomeOpportunitySavedAt',Busy:'fundHomeOpportunityBusy',None:'fundHomeOpportunityNoFeedback',Expired:'fundHomeOpportunityExpired',Observe:'fundHomeOpportunityObserve',mature:'fundHomeOpportunityObsMature',immature:'fundHomeOpportunityObsImmature',missing_price:'fundHomeOpportunityObsMissingPrice',window_lost:'fundHomeOpportunityObsWindowLost',signal_unknown:'fundHomeOpportunityObsSignalUnknown',ObsUp:'fundHomeOpportunityObsUp',ObsDown:'fundHomeOpportunityObsDown',ObsFlat:'fundHomeOpportunityObsFlat',Hit:'fundHomeOpportunityObsHit',Miss:'fundHomeOpportunityObsMiss',ObsChange:'fundHomeOpportunityObsChange',ObsBaseline:'fundHomeOpportunityObsBaseline',ObsEnd:'fundHomeOpportunityObsEnd',ObsObservedAt:'fundHomeOpportunityObsObservedAt',ObsCoverage:'fundHomeOpportunityObsCoverage',ObsMetric:'fundHomeOpportunityObsMetric',ObsHorizon:'fundHomeOpportunityObsHorizon',Price:'fundHomeOpportunityPrice'};
const label=k=>t(copy[k]);
const redraw=()=>home.renderOpportunities(home.snapshot()?.opportunities);
const key=(batch,row)=>`${batch.snapshotId}:${row.candidateId}`;
const observationHtml=row=>{
  const esc=home.escape;
  const percent=value=>new Intl.NumberFormat(Fund.state.language,{style:'percent',maximumFractionDigits:2,signDisplay:'always'}).format(value/1000000);
  const amount=home.displayMode()==='amount';
  const price=raw=>raw==null?'':` · ${label('Price')}：${(raw/10000).toFixed(4)}`;
  const horizons=row.observation?.horizons||{};
  return ['3','5','10'].map(h=>{
    const o=horizons[h];
    if(!o)return '';
    let line=`<p data-observation-horizon="${h}">${label('ObsHorizon')}：${h} · ${label(copy[o.status]?o.status:'Unknown')}`;
    if(o.hit!=null)line+=` · ${o.hit?label('Hit'):label('Miss')}`;
    if(o.returnPpm!=null)line+=` · ${label('ObsChange')}：${percent(o.returnPpm)}`;
    if(amount&&o.baseline)line+=` · ${label('ObsBaseline')}：${esc(o.baseline.date)}${price(o.baseline.priceRaw)}`;
    if(amount&&o.end)line+=` · ${label('ObsEnd')}：${esc(o.end.date)}${price(o.end.priceRaw)}`;
    if(o.observedDay)line+=` · ${label('ObsObservedAt')}：${esc(o.observedDay)}`;
    return line+'</p>';
  }).join('');
};
const observationStatsHtml=()=>{
  if(!stats?.observations)return '';
  const percent=value=>value==null?'—':new Intl.NumberFormat(Fund.state.language,{style:'percent',maximumFractionDigits:1}).format(value/1000000);
  const lines=['3','5','10'].map(h=>{
    const s=stats.observations.horizons[h];
    const dir=key=>{const d=s.directions[key];const dirKey=`Obs${key[0].toUpperCase()}${key.slice(1)}`;return `${label(dirKey)}：${d.hits}/${d.total}${d.hitRatePpm==null?'':`（${percent(d.hitRatePpm)}）`}`;};
    return `<p data-observation-coverage="${h}">${label('ObsCoverage')} ${h}：${label('mature')} ${s.mature}/${s.total} · ${dir('up')} · ${dir('down')} · ${dir('flat')}</p>`;
  }).join('');
  return `${lines}<p class="evidence-hint">${label('ObsMetric')}</p>`;
};
home.opportunityRecordsHtml=()=>{
  const esc=home.escape;
  const rate=stats?.feedback.usefulRatePpm;
  const percent=value=>value==null?'—':new Intl.NumberFormat(Fund.state.language,{style:'percent',maximumFractionDigits:1}).format(value/1000000);
  const groups=(stats?.groups||[]).map(g=>`<p>${label(g.kind==='source'?'Pool':'Strategy')}：${g.kind==='source'?label(copy[g.key]?g.key:'Unknown'):esc(g.key)} · ${label('Count')}：${g.feedback.total} · ${label('Rate')}：${percent(g.feedback.usefulRatePpm)}</p>`).join('');
  const opened=new Set([...document.querySelectorAll("[data-opportunity-record-batch][open]")].map(el=>el.dataset.opportunityRecordBatch));
  const rows=(history?.items||[]).flatMap((batch,bi)=>bi!==batchIndex?[]:[`<details data-opportunity-record-batch="${esc(batch.snapshotId)}" ${opened.has(batch.snapshotId)?"open":""}><summary>${label('Saved')}：${esc(new Date(Number(batch.generatedAt)*1000).toLocaleString(Fund.state.language,{timeZone:'Asia/Shanghai'}))} · ${batch.items.length}</summary><p>${label('Expired')}</p><p><button class="btn" type="button" data-observation-refresh="${esc(batch.snapshotId)}" ${busy||drafts.size?'disabled':''}>${label('Observe')}</button></p>${batch.items.map((row,ri)=>`<div data-opportunity-record="${bi}:${ri}"><h4>${esc(row.name)} · ${esc(row.code)}</h4><p>${home.opportunityLabel(row.kind==='stock'?'Stock':'Fund')} · ${esc(row.market)} · ${home.opportunityLabel(row.action)} · ${home.opportunityLabel('Score')}：${row.rankScore??home.opportunityLabel('Missing')}</p><p>${home.opportunityLabel('AsOf')}：${esc(row.asOf||home.opportunityLabel('Missing'))} · ${esc(row.source)}</p>${home.opportunityPlanHtml(row)}${observationHtml(row)}<p>${row.feedback.useful==null?label('None'):label(row.feedback.useful?'Useful':'NotUseful')} · v${row.feedback.version}</p><label>${label('Reason')}<textarea name="feedback-${bi}-${ri}" maxlength="200" data-feedback-reason>${esc(drafts.get(key(batch,row))??row.feedback.reason)}</textarea></label><button class="btn" type="button" data-feedback-useful="true" ${busy?'disabled':''}>${label('Useful')}</button><button class="btn" type="button" data-feedback-useful="false" ${busy?'disabled':''}>${label('NotUseful')}</button><button class="btn" type="button" data-candidate-notify>${esc(t("fundHomeNotifyTitle"))}</button><button class="btn" type="button" name="feedback-history-${bi}-${ri}" data-feedback-history>${label('History')}</button></div>`).join('')}</details>`]).join('');
  return `<section class="opportunity-records"><h3>${label('Records')}</h3><p>${label('Hint')}</p><button class="btn" name="opportunityCapture" type="button" data-opportunity-capture ${busy||drafts.size?'disabled':''}>${label('Capture')}</button><button class="btn" name="opportunityRecords" type="button" data-opportunity-records ${busy?'disabled':''}>${label('Records')}</button>${busy?`<p role="status">${label('Busy')}</p>`:''}${error?`<p role="alert">${esc(home.opportunityError(error))}</p>`:''}${stats?`<p>${label('Count')}：${stats.feedback.total} · ${label('Rate')}：${percent(rate)}</p>`:''}${groups}${observationStatsHtml()}<p data-feedback-draft-hint ${drafts.size?'':'hidden'}>${label('DraftHint')}</p>${history?`<select name="opportunityBatch" data-record-batch-select aria-label="${label('Records')}" ${busy||drafts.size?'disabled':''}>${history.items.map((b,i)=>`<option value="${i}" ${i===batchIndex?'selected':''}>${esc(b.generatedDay||b.snapshotId)} · ${i+1}</option>`).join('')}</select>${rows||`<p>${label('Empty')}</p>`}<p>${page} / ${Math.max(1,Math.ceil(history.total/10))}</p><button class="btn" type="button" name="opportunityPrevious" data-record-page="-1" ${busy||drafts.size||page<=1?'disabled':''}>${label('Previous')}</button><button class="btn" type="button" name="opportunityNextPage" data-record-page="1" ${busy||drafts.size||page*10>=history.total?'disabled':''}>${label('Next')}</button>`:''}</section>`;
};
async function load(target=page){
  if(target!==page&&drafts.size)return;
  const gen=++generation;controller?.abort();controller=new AbortController();const owned=controller;
  busy=true;error='';redraw();
  try{
    const result=await Fund.api('GET',`/api/opportunities/snapshots?page=${target}`,null,{signal:owned.signal});
    const summary=await Fund.api('GET','/api/opportunities/stats',null,{signal:owned.signal});
    if(gen!==generation||owned.signal.aborted)return;
    const selected=history?.items[batchIndex]?.snapshotId;
    if(drafts.size&&selected&&!result.items.some(b=>b.snapshotId===selected)){error={messageKey:copy.DraftHint};return;}
    batchIndex=target===page?Math.max(0,result.items.findIndex(b=>b.snapshotId===selected)):0;
    history=result;stats=summary;page=target;
  }catch(e){if(gen===generation&&!owned.signal.aborted)error=e;}
  finally{if(gen===generation){busy=false;controller=null;if(home.isActive?.())redraw();}}
}
async function write(path,payload){
  if(busy)return;
  const gen=++generation;busy=true;error='';redraw();
  try{
    const signature=JSON.stringify([path,payload]);
    if(retry?.signature!==signature)retry={signature,requestId:home.requestId()};
    const requestId=retry.requestId;
    await Fund.api('POST',path,{...payload,requestId});
    if(retry?.requestId===requestId)retry=null;
    if(path.endsWith('feedback')&&drafts.get(`${payload.snapshotId}:${payload.candidateId}`)===payload.reason)drafts.delete(`${payload.snapshotId}:${payload.candidateId}`);
    if(gen!==generation||!home.isActive?.())return;
    await home.refresh();await load(path.endsWith('capture')?1:page);
  }catch(e){if(gen===generation)error=e;}
  finally{if(gen===generation){busy=false;if(home.isActive?.())redraw();}}
}
home.bindOpportunityRecords=body=>{
  body.querySelector('[data-opportunity-capture]')?.addEventListener('click',()=>{const p=home.snapshot()?.opportunities;if(p)write('/api/opportunities/capture',{snapshotId:p.snapshotId,dataRevision:p.dataRevision});});
  body.querySelector('[data-opportunity-records]')?.addEventListener('click',()=>load());
  const batchSelector=body.querySelector('[data-record-batch-select]');
  if(batchSelector)batchSelector.onchange=()=>{if(!drafts.size){batchIndex=Number(batchSelector.value);redraw();}};
  body.querySelectorAll('[data-record-page]').forEach(el=>el.onclick=()=>load(page+Number(el.dataset.recordPage)));
  body.querySelectorAll('[data-observation-refresh]').forEach(el=>el.onclick=()=>write('/api/opportunities/observations/refresh',{snapshotId:el.dataset.observationRefresh}));
  body.querySelectorAll('[data-opportunity-record]').forEach(el=>{
    const [bi,ri]=el.dataset.opportunityRecord.split(':').map(Number),batch=history.items[bi],row=batch.items[ri];
    const input=el.querySelector('textarea');input.oninput=()=>{if(input.value===row.feedback.reason)drafts.delete(key(batch,row));else drafts.set(key(batch,row),input.value);const region=body.querySelector('.opportunity-records');region.querySelector('[data-feedback-draft-hint]').hidden=!drafts.size;region.querySelectorAll('[data-record-page],[data-record-batch-select],[data-opportunity-capture]').forEach(button=>{button.disabled=busy||drafts.size>0||(button.dataset.recordPage==='-1'&&page<=1)||(button.dataset.recordPage==='1'&&page*10>=history.total);});};
    el.querySelector('[data-feedback-history]').onclick=()=>showHistory(batch,row);
    el.querySelector('[data-candidate-notify]').onclick=()=>home.notifyBusiness('candidate',row.candidateId,batch.snapshotId);
    el.querySelectorAll('[data-feedback-useful]').forEach(button=>button.onclick=()=>write('/api/opportunities/feedback',{snapshotId:batch.snapshotId,candidateId:row.candidateId,dataRevision:batch.dataRevision,useful:button.dataset.feedbackUseful==='true',reason:input.value,expectedVersion:row.feedback.version}));
  });
};
let historyGeneration=0,historyController=null,historyRoot=null;
home.cancelOpportunityHistory=()=>{++historyGeneration;historyController?.abort();historyController=null;historyRoot=null;};
function showHistory(batch,row){
  if(!home.isActive?.()||home.displayMode()==='hidden')return;
  const model={page:1,total:0,items:[],loading:true,error:''};
  const esc=home.escape;
  const html=()=>`<div data-feedback-history-content><p>${esc(row.name)} · ${esc(row.code)}</p>${model.loading?`<p role="status">${label('Busy')}</p>`:''}${model.error?`<p role="alert">${esc(home.opportunityError(model.error))}</p>`:''}${model.items.map(f=>`<article><h3>${t('fundLedgerHistoryVersion')}：${f.version} · ${label(f.useful?'Useful':'NotUseful')}</h3><p>${esc(f.reason)}</p><p>${t('fundLedgerHistoryRecordedAt')}：${esc(new Date(Number(f.createdAt)*1000).toLocaleString(Fund.state.language,{timeZone:'Asia/Shanghai'}))}</p></article>`).join('')}${!model.loading&&!model.items.length?`<p>${label('None')}</p>`:''}<p>${model.page} / ${Math.max(1,Math.ceil(model.total/20))}</p><button class="btn" type="button" name="feedbackHistoryPrevious" data-feedback-page="-1" ${model.loading||model.page<=1?'disabled':''}>${label('Previous')}</button><button class="btn" type="button" name="feedbackHistoryNext" data-feedback-page="1" ${model.loading||model.page*20>=model.total?'disabled':''}>${label('Next')}</button></div>`;
  const bind=root=>root.querySelectorAll('[data-feedback-page]').forEach(button=>button.onclick=()=>read(model.page+Number(button.dataset.feedbackPage)));
  const root=home.panel(()=>label('History'),html,bind,true);
  if(!root)return;
  historyRoot=root;
  function render(){if(historyRoot===root&&root.isConnected){root.querySelector('[data-feedback-history-content]').outerHTML=html();bind(root);}}
  async function read(page){
    const gen=++historyGeneration;historyController?.abort();const owned=new AbortController();historyController=owned;
    const focusName=root.contains(document.activeElement)?document.activeElement.name:null;
    model.loading=true;model.error='';render();
    try {
      const query=new URLSearchParams({snapshotId:batch.snapshotId,candidateId:row.candidateId,page:String(page)});
      const result=await Fund.api('GET',`/api/opportunities/feedback/history?${query}`,null,{signal:owned.signal});
      if(gen!==historyGeneration||owned.signal.aborted||historyRoot!==root)return;
      Object.assign(model,result);
    }catch(e){if(gen===historyGeneration&&!owned.signal.aborted)model.error=e;}
    finally{if(gen===historyGeneration){model.loading=false;if(historyController===owned)historyController=null;render();if(focusName){const button=root.querySelector(`[name="${CSS.escape(focusName)}"]`);(button&&!button.disabled?button:root.querySelector('[data-close]'))?.focus();}}}
  }
  read(1);
}
const stop=home.stopOpportunityRefresh;
home.stopOpportunityRefresh=()=>{stop();++generation;controller?.abort();controller=null;busy=false;if(historyRoot?.isConnected)home.closePanel?.();else home.cancelOpportunityHistory();};
})();
