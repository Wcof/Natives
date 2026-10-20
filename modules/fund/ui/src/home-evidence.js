// F06/F08：资料管理与用户主动授权的解释。
(() => {
const {api,home,t}=globalThis.Fund;
const {escape:esc,panel,input,text,select,check,submit,failure}=home;
let context={};const pending=new Map();let aiActive=false,aiGeneration=0;
home.renderDecisionResult=(report,hidden=false)=>{
 if(hidden)return `<p class="investment-card-tip">${esc(t('fundHomeAnalysisHiddenText'))}</p>`;
 const result=report.validation?.status==='structure_verified'?report.validation.result:null;
 if(!result)return `<p class="investment-card-tip">${esc(t('fundHomeAnalysisUnverified'))}</p><div class="decision-original">${esc(report.resultText||'')}</div>`;
 const list=items=>`<ul>${items.map(item=>`<li>${esc(typeof item==='string'?item:item.text)}${item.evidenceRefs?.length?`<small>${esc(t('fundHomeDecisionReferences'))}：${item.evidenceRefs.map(esc).join(' · ')}</small>`:''}</li>`).join('')}</ul>`;
 const blocks=[['fundHomeDecisionFacts',result.facts],['fundHomeDecisionImpacts',result.impacts],['fundHomeDecisionCounter',result.counterEvidence],['fundHomeDecisionGaps',result.gaps]];
 return `<div class="decision-sections">${blocks.map(([key,items])=>`<section><h3>${esc(t(key))}</h3>${items?.length?list(items):`<p class="muted">${esc(t('fundHomeDecisionNotProvided'))}</p>`}</section>`).join('')}${(result.options||[]).map(option=>`<section class="decision-option"><h3>${esc(t({watch:'fundHomeDecisionActionwatch',hold:'fundHomeDecisionActionhold',build:'fundHomeDecisionActionbuild',add:'fundHomeDecisionActionadd',reduce:'fundHomeDecisionActionreduce',exit:'fundHomeDecisionActionexit',review:'fundHomeDecisionActionreview'}[option.action]||'fundHomeSettingsUnknown'))}</h3><h4>${esc(t('fundHomeDecisionConditions'))}</h4>${list(option.conditions||[])}<h4>${esc(t('fundHomeDecisionInvalidConditions'))}</h4>${list(option.invalidConditions||[])}${option.gaps?.length?`<h4>${esc(t('fundHomeDecisionGaps'))}</h4>${list(option.gaps)}`:''}</section>`).join('')}</div>`;
};
home.renderEvidencePreview=packet=>`<div class="evidence-preview"><h3>${esc(t('fundHomePreviewOutboundContent'))}</h3><p>${esc(home.reportTaskLabel(packet.task))} · ${esc(packet.snapshotId)}</p>${(packet.evidence||[]).map(item=>`<article><h4>${esc(item.title||item.name||item.id)}</h4><p>${esc(item.excerpt||item.summary||t('fundHomeDecisionStructuredMaterial'))}</p></article>`).join('')}<details><summary>${esc(t('fundHomeDecisionTechnicalPayload'))}</summary><pre>${esc(JSON.stringify(packet,null,2))}</pre></details></div>`;
window.addEventListener("message",e=>{
 if(e.source!==window.parent || !e.data)return;
 const m=e.data;
 if(m.type==="init" || m.type==="welcome")context={generation:m.generation,challenge:m.challenge};
 if(m.type!=="natives:reply" || m.generation!==context.generation || m.challenge!==context.challenge)return;
 const p=pending.get(m.requestId);if(!p)return;pending.delete(m.requestId);clearTimeout(p.timer);m.error?p.reject(Error(m.error)):p.resolve(m.result);
});
function broker(op,payload={},type="natives:ai"){
 return new Promise((resolve,reject)=>{const requestId=home.requestId();const timer=setTimeout(()=>{pending.delete(requestId);reject(Error(t("fundHomeBrokerRequestTimedOut")));},type==="natives:secret"?120000:10000);pending.set(requestId,{resolve,reject,timer});window.parent.postMessage({type,op,requestId,...context,...payload},"*");});
}
const inferenceFailure=code=>t({INFERENCE_TIMEOUT:'fundHomeAiTimeout',INFERENCE_CANCELLED:'fundHomeCancelled',INFERENCE_CONNECTION:'fundHomeAiConnection',INFERENCE_AUTH:'fundHomeAiAuth',INFERENCE_QUOTA:'fundHomeAiQuota',INFERENCE_RATE_LIMIT:'fundHomeAiRateLimit',INFERENCE_MODEL:'fundHomeAiModel',INFERENCE_UPSTREAM:'fundHomeAiUpstream',INFERENCE_LENGTH:'fundHomeAiLength',INFERENCE_TRUNCATED:'fundHomeAiTruncated',INFERENCE_STREAM:'fundHomeAiStream',INFERENCE_EMPTY:'fundHomeAiEmpty'}[code]||'fundHomeAnalysisFailed');
const inferenceProgress=phase=>t({waiting:'fundHomeAiWaiting',reasoning:'fundHomeAiReasoning',writing:'fundHomeAiWriting'}[phase]||'fundHomeAnalyzing');
home.requestCapability=broker;
const readTokenBudget=async()=>(await api('GET','/api/settings')).aiMaxTokens??4096;
const tokenBudgetPreview=count=>`<p class="evidence-hint" data-ai-token-budget="${count}">${esc(t('fundHomeAiTokenPreview',{count}))}</p>`;
home.cancelAI=()=>{
  aiGeneration++;
  if(typeof homeAIState!=="undefined"){homeAIState.generation++;homeAIState.preparing=false;if(homeAIState.phase==='running')homeAIState.phase='preview';}
  const wasActive=aiActive;aiActive=false;
  if(wasActive)broker("cancel").catch(error=>home.status(error.message));
};
// 外链只由有能力的 Shell 打开（ADR-0037 用户主动点击代理；方案 02 §6）。
home.openExternal=url=>broker("open",{url},"natives:open-link");
home.openAssetLink=asset=>{
 const symbol=String(asset.quoteSymbol||"");let url;
 if(asset.venue==="otc"&&/^\d{6}$/.test(asset.fundCode))url=`https://fund.eastmoney.com/${asset.fundCode}.html`;
 else if(/^(sh|sz|bj)\d{6}$/.test(symbol))url=Fund.state.linkPlatform==="xueqiu"?`https://xueqiu.com/S/${symbol.toUpperCase()}`:`https://quote.eastmoney.com/${symbol}.html`;
 else if(/^hk\d{5}$/.test(symbol))url=Fund.state.linkPlatform==="xueqiu"?`https://xueqiu.com/S/${symbol.slice(2)}`:`https://quote.eastmoney.com/hk/${symbol.slice(2)}.html`;
 else if(/^us[A-Za-z][A-Za-z0-9.]{0,14}$/.test(symbol))url=Fund.state.linkPlatform==="xueqiu"?`https://xueqiu.com/S/${symbol.slice(2).toUpperCase()}`:`https://quote.eastmoney.com/us/${symbol.slice(2).toUpperCase()}.html`;
 if(!url)return Promise.reject(Error(t("fundHomeSettingsUnknown")));
 return home.openExternal(url);
};
function clearBrokerRequests(){
 for(const request of pending.values()){clearTimeout(request.timer);request.reject(Error(t("fundHomeCancelled")));}pending.clear();
}
window.addEventListener("pagehide",()=>{home.cancelAI();clearBrokerRequests();});
document.addEventListener("visibilitychange",()=>{if(document.hidden){home.cancelAI();clearBrokerRequests();}});
home.notifyPending=async()=>{
 if(!home.notificationSessionActive())return;
 try{const {eventIds,informationIds=[]}=await api("POST","/api/home/notifications/claim",{});for(const [kind,id] of [...eventIds.map(id=>["event",id]),...informationIds.map(id=>["information",id])]){let result="failed";if(home.notificationSessionActive())try{await broker("notify",{payload:{title:t("fundHomeInvestmentRiskReview"),body:t("fundHomeNewUnhandledEventsAreAvailableOpenThe"),tag:`fund-${kind}-${id}`,badge:{text:"!",tone:"warning"}}},"natives:notification");result="delivered";}catch{}await api("POST","/api/home/notifications/result",{id,kind,result});}}catch(e){home.status(`${t("fundHomeNotificationProcessingFailed")}${e.message}`);}
};
home.manageSources=async()=>{
 // 三态（方案11 §5）：仅 hidden 禁止私密查看；percent 不含金额可查看来源。
 if(home.displayMode()==="hidden"){home.status(t("fundHomeSelectShowAmountsBeforeViewingPrivateSources"));return;}
 try{
 const [{sources},{theses},automatic]=await Promise.all([api("GET","/api/sources"),api("GET","/api/theses?archived=1"),api("GET","/api/information/sources")]);
 panel(()=>(t("fundHomeSourcesAndInvestmentTheses")),()=>(`<p>${t("fundHomeStoresOnlyLinksAndExcerptsYouProvide")}</p><button data-add>${t("fundHomeAddSource")}</button><button data-thesis>${t("fundHomeAddInvestmentThesis")}</button><h3>${t("fundHomeSources")}</h3><h3>${t("fundHomeAutomaticSources")}</h3><p>${t("fundHomeInformationCoverage")}</p>${automatic.sources.map(s=>`<article>${check(`auto-${s.sourceId}`,t({em_stock_news:"fundHomeStockNews",em_stock_ann:"fundHomeStockAnnouncements",em_fund_ann:"fundHomeFundAnnouncements",em_finance:"fundHomeFinanceNews"}[s.sourceId]),s.enabled)}<p>${t("fundHomeLastSuccess")}: ${esc(s.lastSuccessAt||t("fundHomeContentNotRetrieved"))} · ${t("fundHomeStatus")}: ${esc(s.lastErrorCode||t(s.enabled?"fundHomeEnabled":"fundHomeDisabled"))}</p></article>`).join("")}<h3>${t("fundHomeSources")}</h3>${sources.map(s=>`<article><b>${esc(s.name)}</b> · ${s.kind==="excerpt"?t("fundHomeUserExcerpt"):t("fundHomeSavedLink")} · ${s.favorite?t("fundHomeFavorited"):""} · ${s.read?t("fundHomeRead"):t("fundHomeUnread")}<p>${esc(s.publisher)}${t("fundHomePublished")}${esc(s.publishedAt||t("fundHomeUnknown"))}${t("fundHomeRetrieved")}${s.excerptFetchedAt?esc(new Date(Number(s.excerptFetchedAt)*1000).toLocaleString()):t("fundHomeContentNotRetrieved")}</p><p>${esc(s.relatedSymbol)} ${s.thesisId?`${t("fundHomeThesis")}${s.thesisId}`:""}</p>${s.url?`<button data-link="${s.id}">${t("fundHomeOpenSource")}</button>`:""}${s.excerptText?`<details><summary>${t("fundHomeViewExcerpt")}</summary><p>${esc(s.excerptText)}</p></details>`:""}<button data-edit="${s.id}">${t("fundHomeEdit")}</button><button data-fav="${s.id}">${s.favorite?t("fundHomeRemoveFavorite"):t("fundHomeFavorite")}</button><button data-read="${s.id}">${s.read?t("fundHomeMarkUnread"):t("fundHomeMarkRead")}</button><button data-archive="${s.id}">${t("fundHomeArchive")}</button></article>`).join("")||t("fundHomeNoSourcesYet")}<h3>${t("fundHomeInvestmentThesesThemes")}</h3>${theses.map(item=>`<article><b>${esc(item.name)}</b><p>${esc(item.holdingReason)}</p><p>${t("fundHomeHorizon")}${esc(item.horizon)}${t("fundHomeVerificationMetrics")}${esc(item.verifyMetrics)}${t("fundHomeInvalidationConditions")}${esc(item.invalidCondition)}</p>${item.archived?`<button data-thesis-restore="${item.id}">${t("fundHomeRestore")}</button>`:`<button data-thesis-edit="${item.id}">${t("fundHomeEdit")}</button><button data-thesis-archive="${item.id}">${t("fundHomeArchive")}</button>`}</article>`).join("")}`),root=>{
 root.querySelectorAll('[name^="auto-"]').forEach(box=>box.onchange=async()=>{const enabled=box.checked;box.disabled=true;try{await api("POST","/api/information/sources/enabled",{sourceId:box.name.slice(5),enabled});home.manageSources();}catch(e){box.checked=!enabled;failure(e);}finally{box.disabled=false;}});
 root.querySelector("[data-add]").onclick=()=>editSource({},theses);
 root.querySelector("[data-thesis]").onclick=()=>editThesis();
 root.querySelectorAll("[data-edit]").forEach(b=>b.onclick=()=>editSource(sources.find(s=>s.id===Number(b.dataset.edit)),theses));
 root.querySelectorAll("[data-link]").forEach(b=>b.onclick=()=>broker("open",{url:sources.find(s=>s.id===Number(b.dataset.link)).url},"natives:open-link").catch(failure));
 for(const op of ["fav","read","archive"])root.querySelectorAll(`[data-${op}]`).forEach(b=>b.onclick=async()=>{try{const s=sources.find(s=>s.id===Number(b.dataset[op]));const body=op==="archive"?{id:s.id}:op==="fav"?{id:s.id,favorite:!s.favorite}:{id:s.id,read:!s.read};await api("POST",op==="archive"?"/api/sources/archive":"/api/sources/flags",body);home.manageSources();}catch(e){failure(e);}});
 root.querySelectorAll("[data-thesis-edit]").forEach(b=>b.onclick=()=>editThesis(theses.find(item=>item.id===Number(b.dataset.thesisEdit))));
 for(const op of ["archive","restore"])root.querySelectorAll(`[data-thesis-${op}]`).forEach(b=>b.onclick=async()=>{try{const item=theses.find(item=>item.id===Number(b.dataset[op==="archive"?"thesisArchive":"thesisRestore"]));await api("POST",`/api/theses/${op}`,{id:item.id,expectedVersion:item.version});home.manageSources();}catch(e){failure(e);}});
 },true);
 }catch(e){home.status(e.message);}
};
function editSource(s,theses){
 panel(()=>(s.id?t("fundHomeEditSource"):t("fundHomeAddSource")),()=>(`<form>${input("name",t("fundHomeSourceName"),s.name||"","text",'required maxlength="128"')}${select("kind",t("fundHomeContentStatus"),[["link",t("fundHomeSavedLinkFullTextNotRead")],["excerpt",t("fundHomePasteARetrievedExcerpt")]],s.kind||"link")}${input("url",t("fundHomeSourceUrl"),s.url||"","url",'maxlength="2048"')}${input("publisher",t("fundHomePublisher"),s.publisher||"")}${input("publishedAt",t("fundHomePublicationDateBlankIfUnknown"),s.publishedAt||"","date")}${input("relatedSymbol",t("fundHomeRelatedAssetMarketQualified"),s.relatedSymbol||"")}${select("thesisId",t("fundHomeRelatedInvestmentThesis"),[["",t("fundHomeNonef2d5f8")],...theses.filter(item=>!item.archived).map(item=>[item.id,item.name])],s.thesisId||"")}${text("excerptText",t("fundHomeExcerptTextNoAutomaticRetrieval"),s.excerptText||"")}<button type="submit">${t("fundHomeSave")}</button></form>`),root=>submit(root,async v=>{if(s.id)v.id=s.id;if(v.thesisId)v.thesisId=Number(v.thesisId);else delete v.thesisId;if(!v.publishedAt)delete v.publishedAt;await api("POST",s.id?"/api/sources/update":"/api/sources",v);home.manageSources();}));
}
function editThesis(item={}){
 const symbols=JSON.parse(item.relatedSymbols||"[]");
 panel(()=>(item.id?t("fundHomeEditInvestmentThesis"):t("fundHomeAddInvestmentThesis")),()=>(`<form>${input("name",t("fundHomeThemeName"),item.name||"","text","required")}${text("holdingReason",t("fundHomeWhyIHoldIt"),item.holdingReason||"")}${input("horizon",t("fundHomePlannedHorizon"),item.horizon||"")}${text("verifyMetrics",t("fundHomeMetricsToVerifyContinuously"),item.verifyMetrics||"")}${text("invalidCondition",t("fundHomeConditionsThatInvalidateTheThesis"),item.invalidCondition||"")}${input("symbols",t("fundHomeRelatedAssetsCommaSeparated"),symbols.join(","))}<button type="submit">${t("fundHomeSave")}</button></form>`),root=>submit(root,async v=>{v.relatedSymbols=v.symbols.split(",").map(s=>s.trim()).filter(Boolean);delete v.symbols;if(item.id){v.id=item.id;v.expectedVersion=item.version;}await api("POST","/api/theses",v);home.manageSources();}));
}
home.openAI=async(initial={})=>{
 // 三态（方案11 §5）：percent 不披露金额仍可用 AI（勾选金额披露被禁）；
 // hidden 全遮，提供明确切换入口。
 const hiddenMode=home.displayMode()==="hidden";
 const percentMode=home.displayMode()==="percent";
 if(hiddenMode){home.status(t("fundHomeSelectShowAmountsBeforePreviewingOutboundEvidence"));return;}
 try{
 home.closePanel();
 const gen=++aiGeneration;
 if(!home.snapshot())await home.refresh();
 // 方案 11 §6：默认模型来自 Model Host options.defaultModel；不选 models[0]。
 const [{models,defaultModel},{sources},infoRes,assets,offline]=await Promise.all([broker("options"),api("GET","/api/sources"),api("GET","/api/information/items?scope=all&page=1"),api("GET","/api/assets"),api("GET","/api/offline/assets")]);
 const research=initial.research;
 const researchIdentity=research?.identity();
 const scopeItems=[["",t(research?"fundTrendsAiPublicScope":"fundHomePortfolioScope")],...(research?[["portfolio",t("fundHomePortfolioScope")]]:[]),...(assets.funds||[]).map(a=>[`fund:${a.accountId}:${a.fundId}`,`${a.name||a.code} · ${a.account}`]),...(assets.manual||[]).map(a=>[a.productType==="cash"?`cash:${a.accountId}`:`manual:${a.id}`,`${a.name} · ${a.account}`]),...(offline.assets||[]).map(a=>[`offline:${a.id}`,`${a.name} · ${a.location}`])];
 if(gen!==aiGeneration)return;
 let scopePage=1,infoPage=1;
 let maxTokens=4096;let previewGeneration=0;let evidence=null;let selectedTask="diagnosis";let previewResearchIdentity=null;let lastResult=null;
 const infoItems=(infoRes.items||[]).slice(0,50);
 const taskItems=()=>[["diagnosis",t("fundHomeTaskDiagnosis")],["conditions",t("fundHomeTaskConditions")],["news_impact",t("fundHomeTaskNewsImpact")],["briefing",t("fundHomeTaskBriefing")]].filter(([task])=>!research || task!=="briefing");
 const defaultOk=defaultModel&&!defaultModel.unavailable;
 const modelField=()=>defaultOk
  ?`${check("useDefault",`${t("fundHomeUseDefaultModel")}${defaultModel.name}`,true)}${select("model",t("fundHomeEnabledModel"),models.map(m=>[`${m.providerId}::${m.modelId}`,m.name]),defaultModel.providerId+"::"+defaultModel.modelId)}`
  :select("model",t("fundHomeEnabledModel"),models.map(m=>[`${m.providerId}::${m.modelId}`,m.name]));
 const root=panel(()=>(t("fundHomeAiExplanationOfThisAssessment")),()=>(`<p>${t("fundHomeThisCallsYourConfiguredProviderAndMay")}</p><form>${select("task",t("fundHomeAnalysisTask"),taskItems())}<span data-briefing hidden>${select("briefingMarket",t("fundHomeBriefingMarket"),[["CN",t("fundHomeBriefingMarketCN")],["HK",t("fundHomeBriefingMarketHK")],["US",t("fundHomeBriefingMarketUS")]],"CN")}${select("briefingSession",t("fundHomeBriefingSession"),[["pre",t("fundHomeBriefingPre")],["post",t("fundHomeBriefingPost")]],"pre")}</span>${select("scopeAsset",t("fundHomeAiScopeOptional"),scopeItems,initial.asset||"")}<button type="button" data-more-scope ${(offline.assets||[]).length<50?"hidden":""}>${t("fundHomeInformationLoadMore")}</button>${modelField()}<fieldset><legend>${t("fundHomeOptionalSourcesUpToFive")}</legend>${sources.map(s=>check(`source-${s.id}`,`${s.name} · ${s.kind==="link"?t("fundHomeLinkOnlyFullTextNotRead"):t("fundHomeVerbatimExcerpt")}`)).join("")}</fieldset>${check("discloseFinance",t("fundHomeIncludeFinancialAmountsAndAccountDetailsShow"),false,percentMode?"disabled":"")}<fieldset><legend>${t("fundHomeInformationPickForAi")}</legend>${infoItems.map(i=>check(`info-${i.id}`,`${String(i.title).slice(0,40)} · ${home.informationSourceLabel(i.sourceId)} · ${home.informationContentLabel(i)}`)).join("")||`<p class="evidence-hint">${t("fundHomeInformationEmpty")}</p>`}<button type="button" data-more-info ${infoItems.length<50?"hidden":""}>${t("fundHomeInformationLoadMore")}</button></fieldset><button type="submit">${percentMode?t("fundHomePercentModeAmountsNotIncluded"):t("fundHomePreviewOutboundContent")}</button></form><div data-preview tabindex="0"></div><button data-send disabled>${t("fundHomeConfirmSendingAndAnalyze")}</button><button data-cancel disabled>${t("fundHomeCancelAnalysis")}</button><button data-save-report disabled>${t("fundHomeSaveReport")}</button><div data-result role="status" aria-live="polite"></div>`),root=>{
 for(const kind of ["scope","info"]){
  const button=root.querySelector(`[data-more-${kind}]`);button.onclick=async()=>{button.disabled=true;try{
   const data=await api("GET",kind==="scope"?`/api/offline/assets?page=${scopePage+1}`:`/api/information/items?scope=all&page=${infoPage+1}`);
   if(!root.isConnected)return;
   if(kind==="scope"){scopePage++;for(const a of data.assets||[]){const option=document.createElement("option");option.value=`offline:${a.id}`;option.textContent=`${a.name} · ${a.location}`;root.querySelector('[name=scopeAsset]').append(option);}button.hidden=(data.assets||[]).length<50;}
   else{infoPage++;const rows=(data.items||[]).filter(i=>!infoItems.some(old=>old.id===i.id));infoItems.push(...rows);button.insertAdjacentHTML("beforebegin",rows.map(i=>check(`info-${i.id}`,`${String(i.title).slice(0,40)} · ${i.sourceId}`)).join(""));button.hidden=(data.items||[]).length<50;}
  }catch(e){failure(e);}finally{button.disabled=false;}};
 }
 submit(root,async(v,form)=>{if(!models.length)throw Error(t("fundHomeNoModelIsAvailableEnableAModel"));selectedTask=form.elements.task.value;
  // 方案 05：盘前／盘后简报按市场＋时段生成；缺选择时给可执行错误。
  const briefingFields=root.querySelector("[data-briefing]");
  briefingFields.hidden=selectedTask!=="briefing";
  const sourceIds=sources.filter(s=>form.elements[`source-${s.id}`].checked).map(s=>s.id);const scopeAsset=(form.elements.scopeAsset?.value||"").trim();
  // M08（方案 05 §3）：资讯影响任务必须选择 1~5 条资讯（informationIds）。
  const informationIds=infoItems.filter(i=>form.elements[`info-${i.id}`]?.checked).map(i=>i.id);
  if(selectedTask==="news_impact"&&!informationIds.length)throw Error(t("fundHomeInformationPickForAiRequired"));
  if(informationIds.length>5)throw Error(t("fundHomeInformationPickForAiRequired"));
  const briefingMarket=form.elements.briefingMarket?.value||"",briefingSession=form.elements.briefingSession?.value||"";
  const previewRequest=++previewGeneration;
   const budget=await readTokenBudget();
  if(research)await home.refresh();
  if(previewRequest!==previewGeneration || !root.isConnected)return;
  const chartContext=research?.capture();
  if(research && (researchIdentity!==research.identity() || !chartContext?.rows.length))throw Error(t('fundTrendsAiPreviewChanged'));
  if(research && !scopeAsset && form.elements.discloseFinance.checked)throw Error(t('fundTrendsAiChoosePrivateScope'));
  previewResearchIdentity=research?.identity();
  const packet=await api("POST","/api/home/evidence",{dataRevision:home.snapshot().dataRevision,sourceIds,discloseFinance:percentMode?false:form.elements.discloseFinance.checked,task:selectedTask,...(research && !scopeAsset ? {scope:{asset:'research:'+chartContext.identity.symbol}} : scopeAsset && scopeAsset!=='portfolio' ? {scope:{asset:scopeAsset}} : {}),...(chartContext?{research:chartContext}:{}),...(informationIds.length?{informationIds}:{}),...(selectedTask==="briefing"?{briefingMarket,briefingSession}:{})});if(previewRequest!==previewGeneration || !root.isConnected || (research && previewResearchIdentity!==research.identity()))return;maxTokens=budget;evidence=packet;root.querySelector("[data-preview]").innerHTML=percentMode&&!evidence.scope?.research?`<p>${esc(t("fundHomePercentModeAmountsNotIncluded"))}</p>`:home.renderEvidencePreview(evidence);root.querySelector("[data-preview]").insertAdjacentHTML("afterbegin",tokenBudgetPreview(maxTokens));root.querySelector("[data-send]").disabled=false;});
 root.querySelector("form").addEventListener("change",()=>{previewGeneration++;evidence=null;lastResult=null;home.cancelAI();root.querySelector("[data-save-report]").disabled=true;root.querySelector("[data-preview]").textContent="";root.querySelector("[data-send]").disabled=true;const briefingFields=root.querySelector("[data-briefing]");if(briefingFields)briefingFields.hidden=root.querySelector("[name=task]").value!=="briefing";});
 root.querySelector("[data-send]").onclick=async()=>{
  if(!evidence || aiActive)return;
  if(research && previewResearchIdentity!==research.identity()){evidence=null;root.querySelector('[data-send]').disabled=true;failure(Error(t('fundTrendsAiPreviewChanged')));return;}
  if(evidence.dataRevision!==home.snapshot().dataRevision){failure(Error(t("fundHomeLocalDataChangedPreviewAgain")));return;}
  const m=models.find(m=>`${m.providerId}::${m.modelId}`===root.querySelector("select[name=model]").value);if(!m)return;
  // 方案 11 §6：用户勾选默认时走 selection=default + 预览期 configToken；
  // 默认变更/失效由 Model Host 报 default_changed，前端提示重新预览。
  const useDefault=defaultOk&&root.querySelector("[name=useDefault]")?.checked;
  const startPayload=useDefault
    ?{selection:"default",expectedConfigToken:defaultModel.configToken,text:JSON.stringify(evidence)}
    :{providerId:m.providerId,modelId:m.modelId,text:JSON.stringify(evidence)};
  const runGen=++aiGeneration;
  aiActive=true;root.querySelector("[data-send]").disabled=true;root.querySelector("[data-cancel]").disabled=false;
  const output=root.querySelector("[data-result]");output.textContent=t("fundHomeAnalyzing");
  try {
   if(await readTokenBudget()!==maxTokens)throw Error(t('fundHomeAiTokenChanged'));
   if(!aiActive||runGen!==aiGeneration||!root.isConnected)return;
   const startedInfo=await broker("start",{...startPayload,maxTokens});
   const started=Date.now();
   while(aiActive && runGen===aiGeneration && root.isConnected){
    if(Date.now()-started>95000)throw Error(t("fundHomeAnalysisTimedOut"));
    await new Promise(resolve=>setTimeout(resolve,800));
    if(!aiActive || runGen!==aiGeneration)break;
    const r=await broker("status");if(!r.done)output.textContent=inferenceProgress(r.phase);if(!aiActive || runGen!==aiGeneration || !root.isConnected)break;if(r.done){if(r.failure)throw Error(inferenceFailure(r.failure));
     // 记录真实模型身份（方案 11 §6/§7）：直接用 start 返回的身份字段，不从显示名拆解。
     const modelLabel=startedInfo&&(startedInfo.providerId||startedInfo.modelId)?`${startedInfo.providerId||""}/${startedInfo.modelId||""}`:m.name;
     lastResult={resultText:r.text,task:selectedTask,providerId:startedInfo?.providerId||"",modelId:startedInfo?.modelId||"",modelName:modelLabel,snapshotId:evidence.snapshotId,dataRevision:evidence.dataRevision,scope:evidence.scope,evidence};
     const validation=await api("POST","/api/analysis/validate",{resultText:r.text,evidence});
     if(runGen!==aiGeneration||!root.isConnected)break;
     lastResult.validation=validation;
     output.innerHTML=`<p>${esc(t(validation.status==='structure_verified'?'fundHomeAnalysisVerified':'fundHomeAnalysisUnverified'))} · ${esc(modelLabel)} · ${esc(evidence.snapshotId)}</p>${evidence.dataRevision===home.snapshot().dataRevision?'':`<p>${esc(t('fundHomeDataChangedTheFollowingRefersOnlyTo'))}</p>`}${home.renderDecisionResult(lastResult,percentMode)}`;
     root.querySelector("[data-save-report]").disabled=false;
     aiActive=false;break;}
   }
  }catch(e){if(root.isConnected && runGen===aiGeneration)output.textContent=e.message;}finally{if(runGen===aiGeneration){if(aiActive)broker("cancel").catch(()=>{});aiActive=false;root.querySelector("[data-cancel]").disabled=true;root.querySelector("[data-send]").disabled=!evidence;}}
 };
 if(research){
  const invalidate=()=>{if(!root.isConnected)return;previewGeneration++;evidence=null;lastResult=null;home.cancelAI();root.querySelector('[data-send]').disabled=true;root.querySelector('[data-cancel]').disabled=true;root.querySelector('[data-save-report]').disabled=true;root.querySelector('[data-preview]').textContent='';root.querySelector('[data-result]').textContent=t('fundTrendsAiPreviewChanged');};
  for(const name of ['fund:research-selection-changed','fund:detail-selection-changed','fund:research-viewport-changed'])document.addEventListener(name,invalidate);
  root.addEventListener('close',()=>{for(const name of ['fund:research-selection-changed','fund:detail-selection-changed','fund:research-viewport-changed'])document.removeEventListener(name,invalidate);},{once:true});
 }
 root.querySelector("[data-cancel]").onclick=()=>{home.cancelAI();root.querySelector("[data-send]").disabled=!evidence;root.querySelector("[data-cancel]").disabled=true;root.querySelector("[data-result]").textContent=t("fundHomeCancelled");};
 // 用户主动保存报告（方案 11 §7）：记录模型、范围、证据版本；不自动保存。
 root.querySelector("[data-save-report]").onclick=async()=>{
  if(!lastResult)return;
  try{
   // 契约：resultText + start 返回的真实 providerId/modelId（方案 11 §7）。
   const r=await api("POST","/api/analysis/reports/save",{...lastResult});
   root.querySelector("[data-save-report]").disabled=true;
   root.querySelector("[data-result]").insertAdjacentHTML("beforeend",`<p>${esc(t("fundHomeReportSaved"))}${r.staleSnapshot?esc(t("fundHomeReportStaleSnapshot")):""}</p>`);
  }catch(e){failure(e);}
 };
 },true);
 }catch(e){home.status(e.message);}
};
// 分析报告管理（方案 11 §7）：列表/查看/分页/删除形成真实页面路径；
// 仅用户主动删除；旧快照明确标注，不当作实时建议。
home.manageReports=async(page=1,includeDeleted=false)=>{
 if(home.sensitive())return;
 try{
  const data=await api("GET",`/api/analysis/reports?page=${page}${includeDeleted?"&includeDeleted=1":""}`),reports=data.reports||[];
  panel(()=>t("fundHomeReportsTitle"),()=>`${check("deleted",t("fundHomeShowArchived"),includeDeleted)}${reports.map(r=>`<article><b>${esc(home.reportTaskLabel(r.task))}</b> · ${esc(r.modelName||`${r.providerId}/${r.modelId}`)} · ${esc(new Date(Number(r.savedAt)*1000).toLocaleString())}<p>${t(r.validation?.status==="structure_verified"?"fundHomeAnalysisVerified":"fundHomeAnalysisUnverified")}${r.dataRevision!==home.snapshot()?.dataRevision?" · "+t("fundHomeReportsStaleSnapshot"):""}</p><details><summary>${t("fundHomeReportsView")}</summary>${home.renderDecisionResult(r)}</details>${r.deleted?`<button data-report-op="restore" data-id="${r.id}">${t("fundHomeRestore")}</button>`:`<button data-notify-report="${r.id}">${t("fundHomeNotifyTitle")}</button><button data-compare="${r.id}">${t("fundHomeReportCompare")}</button><button data-report-op="delete" data-id="${r.id}">${t("fundHomeReportsDelete")}</button>`}${home.reportFeedbackMarkup(r)}<pre data-changes="${r.id}" role="status"></pre></article>`).join("")||`<p>${t("fundHomeReportsEmpty")}</p>`}<button data-prev ${page===1?"disabled":""}>${t("fundHomePrevious")}</button><span>${page}</span><button data-next ${reports.length<50?"disabled":""}>${t("fundHomeNext")}</button>`,root=>{
   root.querySelector("[name=deleted]").onchange=e=>home.manageReports(1,e.target.checked);
   root.querySelector("[data-prev]").onclick=()=>home.manageReports(page-1,includeDeleted);root.querySelector("[data-next]").onclick=()=>home.manageReports(page+1,includeDeleted);
   home.bindReportFeedback(root,reports);
   root.querySelectorAll("[data-notify-report]").forEach(b=>b.onclick=()=>home.notifyBusiness("report",Number(b.dataset.notifyReport)));
   root.querySelectorAll("[data-report-op]").forEach(b=>b.onclick=async()=>{if(!await home.confirm(t("fundHomeReportsDeleteConfirm")))return;try{const r=reports.find(r=>r.id===Number(b.dataset.id));await api("POST",`/api/analysis/reports/${b.dataset.reportOp}`,{id:r.id,expectedVersion:r.version});home.manageReports(page,includeDeleted);}catch(e){failure(e);}});
   root.querySelectorAll("[data-compare]").forEach(b=>b.onclick=async()=>{try{const c=await api("GET",`/api/analysis/reports/compare?id=${b.dataset.compare}`);root.querySelector(`[data-changes="${b.dataset.compare}"]`).textContent=c.first?t("fundHomeReportFirst"):[["localFactsChanged","fundHomeLocalFacts"],["financeChanged","fundHomeCashAndFlowReconciliation"],["rulesChanged","fundHomeRules"],["strategiesChanged","fundHomeInvestmentThesesThemes"],["quotesChanged","fundHomeRecordedQuotes"],["briefingOverviewChanged","fundHomeBriefingOverview"]].filter(([key])=>c[key]!=null).map(([key,label])=>`${t(label)}：${t(c[key]?"fundHomeChanged":"fundHomeUnchanged")}`).join("\n")+`\n${t("fundHomeInformationAdded")}: ${(c.informationAdded||[]).length} · ${t("fundHomeInformationUpdated")}: ${(c.informationUpdated||[]).length} · ${t("fundHomeInformationRemoved")}: ${(c.informationRemoved||[]).length}`;}catch(e){failure(e);}});
  });
 }catch(e){home.status(e.message);}
};

// ================= 首页原地 AI 决策（方案 P11/P12 / 5.3 节） =================
let homeAIState = {
  newsId: null,
  newsItem: null,
  phase: 'idle', // 'idle' | 'preview' | 'running' | 'result' | 'error'
  disclose: false,
  evidence: null,
  lastResult: null,
  errorMsg: "",
  progress: "waiting",
  generation: 0,
};
const reportSaves=new WeakMap();
async function ensureSavedReport(result){
 if(result.reportId)return result.reportId;
 if(!reportSaves.has(result))reportSaves.set(result,api('POST','/api/analysis/reports/save',{...result}));
 try{const receipt=await reportSaves.get(result);result.saved=true;result.reportId=receipt.id;return receipt.id;}
 finally{reportSaves.delete(result);}
}

function renderHomeAIContent() {
  const s = homeAIState;
  const n = s.newsItem;
  if(home.displayMode()==='hidden')return `<p class="evidence-hint">${esc(t('fundHomeAnalysisHiddenText'))}</p>`;
  if (s.phase === 'idle' || !n) {
    return `<div class="home-ai-empty">
      <svg class="svg-icon" aria-hidden="true" style="width:32px;height:32px;color:var(--natives-accent);"><use href="#i-news" /></svg>
      <h3 style="font-size:15px;margin:4px 0">${esc(t("fundHomeAiEmptyTitle"))}</h3>
      <p class="muted" style="max-width:320px">${esc(t("fundHomeAiEmptyDesc"))}</p>
    </div>`;
  }
  const chosenHeader = `<div class="home-ai-source">
    <small style="color:var(--natives-accent);font-weight:600">${esc(t("fundHomeNewsPanelTitle"))}</small>
    <h3 style="font-size:14px;margin:4px 0 6px">${esc(n.title)}</h3>
    <div class="news-meta" style="font-size:11px;color:var(--natives-text-muted);display:flex;gap:8px">
      <span>${esc(home.informationSourceLabel(n.sourceId))}</span>
      <span>${esc(home.informationTimeLabel(n.publishedAt))}</span><span>${esc(home.informationContentLabel(n))}</span>
    </div>
  </div>`;

  if (s.phase === 'preview') {
    const materials = Array.isArray(s.evidence?.evidence) ? s.evidence.evidence.filter(piece => piece.kind === 'information') : [];
    const isHidden = home.displayMode() === "hidden";
    const percentMode = home.displayMode() === "percent";
    const isPrivate = isHidden || percentMode;
    return chosenHeader + `<div class="decision-block">
      <h3 style="font-size:14px;margin-bottom:8px">${esc(t("fundHomeAiReviewEvidence"))}</h3>
      <p class="muted home-ai-material">${materials.map(p=>esc(p.excerpt||p.summary||p.title)).join('<br>')||esc(t('fundDetailLoading'))}</p>
      <small>${esc(t('fundHomeAiExcerptScope'))}</small>
      ${s.evidence?tokenBudgetPreview(s.maxTokens):""}
      <dl class="metric-list" style="margin:12px 0;font-size:12px">
        <div><dt style="color:var(--natives-text-muted)">${esc(t('fundHomeAiPublicMaterials'))}</dt><dd>${esc(t('fundHomeAiPublicScope'))}</dd></div>
        <div><dt style="color:var(--natives-text-muted)">${esc(t('fundHomeAiLocalFacts'))}</dt><dd>${esc(t('fundHomeAiFactScope'))}</dd></div>
        <div><dt style="color:var(--natives-text-muted)">${esc(t('fundHomeAiSensitiveData'))}</dt><dd>${esc(t(s.disclose && !isPrivate ? 'fundHomeAiFinanceIncluded' : 'fundHomeAiFinanceExcluded'))}</dd></div>
      </dl>
      <label class="home-check" style="margin:12px 0;display:flex;align-items:center;gap:6px;font-size:12px">
        <input id="home-ai-disclose" type="checkbox" ${s.disclose && !isPrivate ? "checked" : ""} ${isPrivate ? "disabled" : ""}>
        <span>${esc(t("fundHomeIncludeFinancialAmountsAndAccountDetailsShow"))}</span>
      </label>
      <div class="row" style="margin-top:16px;display:flex;gap:10px">
        <button class="btn btn-accent" data-act="home-ai-confirm" ${!s.evidence||s.preparing?'disabled':''}>${esc(t("fundHomeAiConfirmAnalyze"))}</button>
        <button class="btn" data-act="home-ai-cancel">${esc(t("fundHomeAiCancelSelect"))}</button>
      </div>
    </div>`;
  }

  if (s.phase === 'running') {
    return chosenHeader + `<div class="decision-block">
      <div class="home-ai-empty" style="min-height:180px">
        <p style="font-size:13px">${esc(inferenceProgress(s.progress))}</p>
        <button class="btn btn-sm" data-act="home-ai-stop" style="margin-top:12px">${esc(t("fundHomeCancelAnalysis"))}</button>
      </div>
    </div>`;
  }

  if (s.phase === 'error') {
    return chosenHeader + `<div class="home-ai-error" role="alert">
      <h3 style="font-size:14px;margin-bottom:4px">${esc(t("fundHomeAiAnalysisUnavailable"))}</h3>
      <p style="font-size:12px;margin:0 0 10px">${esc(s.errorMsg || t("fundHomeAnalysisFailed"))}</p>
      <button class="btn btn-sm" data-act="home-ai-retry">${esc(t("fundHomeAiRetry"))}</button>
      <button class="btn btn-sm" data-act="home-ai-model-settings">${esc(t('fundHomeAiConfigureModel'))}</button>
    </div>`;
  }

  if (s.phase === 'result' && s.lastResult) {
    const r = s.lastResult;
    const validation = r.validation || {};
    const isHidden = home.displayMode() === "hidden";
    const percentMode = home.displayMode() === "percent";
    const isPrivate = isHidden || percentMode;
    const statusText = t(validation.status === "structure_verified" ? "fundHomeAnalysisVerified" : "fundHomeAnalysisUnverified");
    return chosenHeader + `<div class="decision-block">
      <div style="font-size:12px;color:var(--natives-accent);margin-bottom:8px">${esc(statusText)} · ${esc(r.modelName || "")}${r.dataRevision!==home.snapshot()?.dataRevision?' · '+esc(t('fundHomeReportsStaleSnapshot')):''}</div>
      ${home.renderDecisionResult(r,isPrivate)}
      <div class="home-ai-receipt" role="status">
        ${r.saved ? `<p>✓ ${esc(t("fundHomeReportSaved") || "已保存分析报告。")}</p>` : ""}
        ${r.decision ? `<p>✓ ${esc(t("fundHomeDecisionRecorded"))}：${esc(t({observe:'fundHomeObserve',defer:'fundHomeDefer',done:'fundHomeHandledDoesNotImplyExecution'}[r.decision.action]))}<br><small>${esc(isPrivate?t('fundHomeAnalysisHiddenText'):r.decision.note)}</small></p>` : ""}
      </div>
      <div class="row" style="margin-top:14px;display:flex;gap:10px;flex-wrap:wrap">
        <button class="btn btn-accent" data-act="home-ai-record">${esc(t("fundHomeRecordMyDecision"))}</button>
        <button class="btn" data-act="home-ai-save" ${r.saved ? "disabled" : ""}>${esc(t("fundHomeSaveReport"))}</button>
        <button class="btn quiet" data-act="home-ai-retry">${esc(t("fundHomeAiRetry"))}</button>
      </div>
    </div>`;
  }

  return "";
}

function updateHomeAI(focus = true) {
  const body = document.getElementById("home-ai-body");
  if (!body) return;
  const scroll = body.scrollTop;
  body.innerHTML = renderHomeAIContent();
  body.scrollTop = focus ? 0 : scroll;

  document.querySelectorAll("[data-act=news-ai]").forEach(btn => {
    const isChosen = Number(btn.dataset.id) === homeAIState.newsId && homeAIState.phase !== 'idle';
    btn.setAttribute("aria-pressed", String(isChosen));
  });

  const disclose = body.querySelector("#home-ai-disclose");
  if (disclose) {
    disclose.onchange=e=>{home.cancelAI();homeAIState.disclose=e.target.checked;void prepareHomeEvidence();};
  }
  const confirmBtn = body.querySelector("[data-act=home-ai-confirm]");
  if (confirmBtn) {
    confirmBtn.onclick = () => runHomeAI();
  }
  const cancelBtn = body.querySelector("[data-act=home-ai-cancel]");
  if (cancelBtn) {
    cancelBtn.onclick = () => {
      home.cancelAI();
      homeAIState.phase = 'idle';
      homeAIState.newsId = null;
      homeAIState.newsItem = null;
      updateHomeAI(true);
    };
  }
  const stopBtn = body.querySelector("[data-act=home-ai-stop]");
  if (stopBtn) {
    stopBtn.onclick = () => {
      home.cancelAI();
      homeAIState.phase = 'preview';
      updateHomeAI(false);
    };
  }
  const retryBtn = body.querySelector("[data-act=home-ai-retry]");
  if (retryBtn) {
    retryBtn.onclick = () => {
      home.cancelAI();homeAIState.phase='preview';void prepareHomeEvidence();
    };
  }
  const saveBtn = body.querySelector("[data-act=home-ai-save]");
  if (saveBtn) {
    saveBtn.onclick = async () => {
      if (!homeAIState.lastResult || homeAIState.lastResult.saved) return;
      saveBtn.disabled = true;
      try {
        const result=homeAIState.lastResult;
        await ensureSavedReport(result);
        if(homeAIState.lastResult===result)updateHomeAI(false);
      } catch (err) {
        saveBtn.disabled = false;
        home.status(err.message);
      }
    };
  }
  const recordBtn=body.querySelector('[data-act=home-ai-record]');
  if(recordBtn)recordBtn.onclick=recordHomeDecision;
  const modelBtn=body.querySelector('[data-act=home-ai-model-settings]');
  if(modelBtn)modelBtn.onclick=async()=>{
    const restore=()=>{if(document.getElementById('view-home')?.hidden===false)body.querySelector('[data-act=home-ai-model-settings]')?.focus({preventScroll:true});};
    window.addEventListener('focus',restore,{once:true});
    try{await broker('open',{},'natives:model-settings');}
    catch(error){window.removeEventListener('focus',restore);home.status(error.message);}
  };
}
async function recordHomeDecision(){
 const result=homeAIState.lastResult,item=homeAIState.newsItem;if(!result||!item)return;
 if(home.displayMode()!=='amount'){home.status(t('fundHomeSelectShowAmountsBeforeViewingPrivateSources'));return;}
 const requestId=home.requestId();
 panel(()=>t('fundHomeRecordMyDecision'),()=>`<p>${esc(item.title)}</p><p class="evidence-hint">${esc(t('fundHomeAiRecordSaveHint'))}</p><form>${select('action',t('fundHomeMyAction'),[['observe',t('fundHomeObserve')],['defer',t('fundHomeDefer')],['done',t('fundHomeHandledDoesNotImplyExecution')]])}${text('note',t('fundHomeReasonAndEvidence'))}${home.save()}</form>`,root=>submit(root,async values=>{
  if(!String(values.note||'').trim())throw Error(t('fundHomeAiDecisionReasonRequired'));
  await ensureSavedReport(result);
  await api('POST','/api/information/decisions',{informationId:item.id,reportId:result.reportId,action:values.action,note:values.note,requestId});
  result.decision={action:values.action,note:values.note};home.closePanel();if(homeAIState.lastResult===result)updateHomeAI(false);
 }));
}
async function prepareHomeEvidence(){
 const own=++homeAIState.generation;homeAIState.evidence=null;homeAIState.preparing=true;updateHomeAI(false);
 try{
  for(let attempt=0;attempt<2;attempt++){
   const snapshot=await home.readSnapshot();if(own!==homeAIState.generation)return;if(!snapshot)throw Error(t('fundHomeHomeSnapshotCouldNotLoad'));
   try{
    const maxTokens=await readTokenBudget();
    const packet=await api('POST','/api/home/evidence',{dataRevision:snapshot.dataRevision,sourceIds:[],informationIds:[homeAIState.newsId],discloseFinance:home.displayMode()==='amount'&&homeAIState.disclose,task:'news_impact'});
    if(own===homeAIState.generation){homeAIState.evidence=packet;homeAIState.maxTokens=maxTokens;}break;
   }catch(error){if(error.status!==409||attempt===1)throw error;}
  }
 }catch(error){if(own===homeAIState.generation){homeAIState.errorMsg=error.message;homeAIState.phase='error';}}
 finally{if(own===homeAIState.generation){homeAIState.preparing=false;updateHomeAI(false);}}
}

async function runHomeAI() {
  const packet=homeAIState.evidence;
  if(!packet||packet.dataRevision!==home.snapshot().dataRevision){homeAIState.phase='preview';await prepareHomeEvidence();return;}
  const gen=++homeAIState.generation;let remoteStarted=false,finished=false;
  aiActive = true;
  homeAIState.phase = 'running';
  homeAIState.progress='waiting';
  updateHomeAI(false);
  try {
    if(await readTokenBudget()!==homeAIState.maxTokens)throw Error(t('fundHomeAiTokenChanged'));
    const { models, defaultModel } = await broker("options");
    if (gen !== homeAIState.generation || !aiActive) return;
    if (!models || !models.length) throw Error(t("fundHomeNoModelIsAvailableEnableAModel"));

    if (!defaultModel || defaultModel.unavailable) throw Error(t('fundHomeAiDefaultUnavailable'));
    const startPayload = { maxTokens:homeAIState.maxTokens,selection: "default", expectedConfigToken: defaultModel.configToken, text: JSON.stringify(packet) };

    const startedInfo = await broker("start", startPayload);remoteStarted=true;
    if (gen !== homeAIState.generation || !aiActive) return;

    const started = Date.now();
    while (aiActive && gen === homeAIState.generation) {
      if (Date.now() - started > 95000) throw Error(t("fundHomeAnalysisTimedOut"));
      await new Promise(r => setTimeout(r, 800));
      if (!aiActive || gen !== homeAIState.generation) break;
      const res = await broker("status");
      if (!aiActive || gen !== homeAIState.generation) break;
      if(!res.done&&res.phase&&res.phase!==homeAIState.progress){homeAIState.progress=res.phase;updateHomeAI(false);}
      if (res.done) {
        if (res.failure) throw Error(inferenceFailure(res.failure));
        const modelLabel = startedInfo && (startedInfo.providerId || startedInfo.modelId)
          ? `${startedInfo.providerId || ""}/${startedInfo.modelId || ""}`
          : defaultModel.name;
        const validation = await api("POST", "/api/analysis/validate", {
          resultText: res.text,
          evidence: packet,
        });
        if (gen !== homeAIState.generation || !aiActive) break;
        homeAIState.lastResult = {
          resultText: res.text,
          task: "news_impact",
          providerId: startedInfo?.providerId || "",
          modelId: startedInfo?.modelId || "",
          modelName: modelLabel,
          snapshotId: packet.snapshotId,
          dataRevision: packet.dataRevision,
          scope: packet.scope,
          evidence: packet,
          validation,
          saved: false,
        };
        finished=true;homeAIState.phase='result';
        aiActive = false;
        updateHomeAI(false);
        break;
      }
    }
  } catch (err) {
    if (gen === homeAIState.generation && aiActive) {
      aiActive = false;
      homeAIState.phase = 'error';
      homeAIState.errorMsg = err.message;
      updateHomeAI(false);
    }
  } finally {
    if (gen === homeAIState.generation) {
      if(remoteStarted&&!finished)broker('cancel').catch(error=>home.status(error.message));
      aiActive=false;
    }
  }
}

home.selectNewsForAI = item => {
  home.cancelAI();
  homeAIState.generation++;
  homeAIState.newsId = item.id;
  homeAIState.newsItem = item;
  homeAIState.phase = 'preview';
  homeAIState.disclose = false;
  homeAIState.evidence = null;
  homeAIState.lastResult = null;
  homeAIState.errorMsg = "";
  updateHomeAI(true);void prepareHomeEvidence();
};

home.isNewsAISelected = id => homeAIState.newsId === id && homeAIState.phase !== 'idle';
home.updateHomeAI = updateHomeAI;
home.inlineAIActive=()=>aiActive||Boolean(homeAIState.preparing);

function onPrivacyChanged() {
  if (home.displayMode() === "hidden" || home.displayMode() === "percent") {
    homeAIState.disclose = false;
  }
  home.cancelAI();
  if(homeAIState.phase==='preview'&&home.displayMode()!=='hidden')void prepareHomeEvidence();else updateHomeAI(false);
}
document.addEventListener("fund:privacy-changed", onPrivacyChanged);
document.addEventListener("fund:display-mode-changed", onPrivacyChanged);
document.addEventListener('fund:language-changed',()=>updateHomeAI(false));

document.addEventListener('fund:home-snapshot-updated',()=>{
 const body=document.getElementById('home-ai-body');if(!body)return;
 if(homeAIState.preparing)return;
 const changed=homeAIState.evidence&&homeAIState.evidence.dataRevision!==home.snapshot()?.dataRevision;
 if(changed&&homeAIState.phase==='running'){home.cancelAI();homeAIState.phase='preview';void prepareHomeEvidence();}
 else if(changed&&homeAIState.phase==='preview')void prepareHomeEvidence();
 else updateHomeAI(false);
});
})();
