// 首页管理入口。原生表单与 dialog；写入完成后重取快照。
(() => {
const {api,home,actions,t}=globalThis.Fund;
const esc=home.escape;
let informationGeneration=0,informationActive=null,informationPending=null,informationRetired=null,informationWait=null;
function informationPause(){return new Promise(resolve=>{informationWait={resolve,timer:setTimeout(()=>{informationWait=null;resolve();},500)};});}
function retireInformation(id){
 informationRetired=api('POST','/api/information/cancel',{taskId:id}).then(()=>id,error=>{if(error.status!==404)throw error;return null;});
 // Cancellation is best effort on unload; a subsequent request awaits its result.
 informationRetired.catch(()=>{});
}
home.cancelInformationRequest=()=>{
 ++informationGeneration;
 if(informationWait){clearTimeout(informationWait.timer);informationWait.resolve();informationWait=null;}
 if(informationActive){retireInformation(informationActive);informationActive=null;}
};
async function drainInformation(gen){
 if(informationPending)await informationPending.catch(()=>{});
 const retired=informationRetired,id=await retired;
 const deadline=Date.now()+25000;
 while(id&&gen===informationGeneration){
  let status;
  try{status=await api('GET',`/api/information/status?taskId=${id}`);}catch(error){if(error.status===404)break;throw error;}
  if(status.result)break;
  if(Date.now()>=deadline){const error=Error(t('fundHomeInformationBusy'));error.status=409;error.code='INFORMATION_REFRESH_IN_PROGRESS';throw error;}
  await informationPause();
 }
 if(informationRetired===retired&&gen===informationGeneration)informationRetired=null;
}
home.requestInformation=async(route,payload)=>{
 home.cancelInformationRequest();const gen=informationGeneration;
 await drainInformation(gen);if(gen!==informationGeneration)return null;
 const pending=api('POST',route,{...payload,requestId:home.requestId()});informationPending=pending;
 let response;
 try{response=await pending;}finally{if(informationPending===pending)informationPending=null;}
 if(gen!==informationGeneration){if(response.taskId)retireInformation(response.taskId);return null;}
 informationActive=response.taskId;
 try{
  while(response.taskId&&!response.result){
   await informationPause();if(gen!==informationGeneration)return null;
   response=await api('GET',`/api/information/status?taskId=${informationActive}`);
   if(gen!==informationGeneration||response.status==='cancelled')return null;
  }
  if(response.result?.error){const error=Error(response.result.error);error.code=response.result.error;error.status=response.result.status||500;throw error;}
  return response.result||response;
 }finally{if(gen===informationGeneration){if(response.status==='cancelled'&&informationActive)retireInformation(informationActive);informationActive=null;}}
};
home.informationSourceLabel=id=>t({em_stock_news:'fundHomeStockNews',em_stock_ann:'fundHomeStockAnnouncements',em_fund_ann:'fundHomeFundAnnouncements',em_finance:'fundHomeFinanceNews'}[id]||'fundHomeSources');
home.informationContentLabel=item=>t(item.completeness==='truncated'?'fundHomeInformationTruncated':item.completeness==='full'?'fundHomeVerbatimExcerpt':item.summary?'fundHomeNewsSummary':'fundHomeLinkOnlyFullTextNotRead');
home.informationTimeLabel=value=>{const date=value?new Date(value):null;return date&&Number.isFinite(date.getTime())?date.toLocaleString(Fund.state.language,{timeZone:'Asia/Shanghai'}):t('fundHomeSourceTimestampUnknown');};
let newsGeneration=0,newsController=null,newsCollectionAt=0,newsCollecting=false;
actions.leaveNewsPage=()=>{newsGeneration++;newsController?.abort();newsController=null;home.cancelInformationRead();home.cancelInformation();newsCollecting=false;document.getElementById('view-news')?.setAttribute('aria-busy','false');};
actions.loadNewsPage=async(collect=true)=>{
 const view=document.getElementById('view-news');if(!view||view.hidden||document.hidden)return;
 const gen=++newsGeneration;newsController?.abort();const controller=new AbortController();newsController=controller;
 view.setAttribute('aria-busy','true');
 const coverage=document.getElementById('news-page-coverage');coverage.textContent=t('fundHomeLoading');
 const trigger=document.getElementById('news-page-refresh');trigger.disabled=home.displayMode()==='hidden';
 if(trigger.disabled)document.getElementById('news-page-status').textContent='';else home.renderInformationStatus();
 try{
  await home.readInformation();
  const data=await api('GET','/api/information/sources',null,{signal:controller.signal});if(gen!==newsGeneration||controller.signal.aborted||view.hidden)return;
  coverage.innerHTML=data.sources.map(s=>`<article><strong>${esc(home.informationSourceLabel(s.sourceId))}</strong><small>${esc(s.lastSuccessAt?home.informationTimeLabel(s.lastSuccessAt):t('fundHomeContentNotRetrieved'))} · ${esc(s.lastErrorCode||t(s.enabled?'fundHomeEnabled':'fundHomeDisabled'))}</small></article>`).join('');
  const now=Date.now(),stale=data.sources.some(s=>s.enabled&&(!s.lastAttemptAt||!Number.isFinite(Date.parse(s.lastAttemptAt))||now-Date.parse(s.lastAttemptAt)>=15*60*1000));
  if(collect&&home.displayMode()!=='hidden'&&stale&&!newsCollecting&&now-newsCollectionAt>=15*60*1000){newsCollecting=true;newsCollectionAt=now;try{await home.refreshInformation();}finally{newsCollecting=false;}}
 }catch(e){if(gen===newsGeneration&&!controller.signal.aborted)coverage.textContent=t('fundHomeInformationCouldNotLoad');}
 finally{if(gen===newsGeneration){view.setAttribute('aria-busy','false');if(newsController===controller)newsController=null;}}
};
document.getElementById('news-page-sources')?.addEventListener('click',()=>home.manageSources?.());
document.addEventListener('visibilitychange',()=>{const view=document.getElementById('view-news');if(document.hidden&&view&&!view.hidden)actions.leaveNewsPage();});
let dialog,returnFocus,renderDialog,currencyGeneration=0;
const input=(name,label,value="",type="text",extra="")=>`<label>${esc(label)}<input name="${name}" type="${type}" value="${esc(value)}" ${extra}></label>`;
const text=(name,label,value="")=>`<label>${esc(label)}<textarea name="${name}" maxlength="${name==="excerptText"?12000:4096}" rows="3">${esc(value)}</textarea></label>`;
const select=(name,label,items,value="")=>`<label>${esc(label)}<select name="${name}">${items.map(([v,t])=>`<option value="${esc(v)}" ${String(value)===String(v)?"selected":""}>${esc(t)}</option>`).join("")}</select></label>`;
const check=(name,label,value=false,extra="")=>`<label class="home-check"><input type="checkbox" name="${name}" ${value?"checked":""} ${extra}>${esc(label)}</label>`;
function close() { currencyGeneration++;if(dialog){home.cancelBusinessNotification?.();home.cancelAI?.();home.cancelInformation?.();home.cancelOpportunityHistory?.();dialog.close();dialog.remove();dialog=null;renderDialog=null;(returnFocus?.isConnected?returnFocus:returnFocus?.id?document.getElementById(returnFocus.id):returnFocus?.name?document.querySelector(`[name="${CSS.escape(returnFocus.name)}"]`):null)?.focus();} }
document.addEventListener('keydown',event=>{
  if(event.key!=='Tab'||event.defaultPrevented||event.ctrlKey||event.metaKey||event.altKey)return;
  const node=Array.from(document.querySelectorAll('dialog[open]')).at(-1);if(!node)return;
  const controls=Array.from(node.querySelectorAll('button,input,select,textarea,a[href],summary,[tabindex]'))
   .filter(control=>!control.disabled&&control.tabIndex>=0&&control.getClientRects().length);
  const first=controls[0],last=controls.at(-1),active=document.activeElement;
  const target=event.shiftKey&&(active===first||!node.contains(active))?last:
   !event.shiftKey&&(active===last||!node.contains(active))?first:null;
  if(target){event.preventDefault();target.focus();}
});
function panel(title,html,bind,percentOk=false) {
  // 三态（方案 11 §5/02 §7）：hidden 全拦；percent 仅放行不含金额的 AI/资料面板
  // （percentOk 由调用方显式声明，金额管理表单不放行）。
  if(home.privacy()&&!(percentOk&&home.displayMode()==="percent")){sensitive();return;}
  close();returnFocus=document.activeElement;
  dialog=document.createElement("dialog");dialog.className="home-manager";
  renderDialog=()=>{
    dialog.innerHTML=`<header><h2 id="home-dialog-title">${esc(title())}</h2><button type="button" name="homeDialogClose" data-close aria-label="${t("fundHomeClose")}">${t("fundHomeClose")}</button></header><div class="home-manager-body">${html()}</div><p class="home-form-error" role="status" aria-live="polite"></p>`;
    dialog.querySelector("[data-close]").onclick=close;
    bind?.(dialog);
  };
  dialog.setAttribute("aria-labelledby","home-dialog-title");document.body.appendChild(dialog);
  dialog.addEventListener("cancel",e=>{e.preventDefault();close();});
  renderDialog();dialog.showModal();
  return dialog;
}
function failure(e,root=dialog){const target=root?.querySelector(".home-form-error");if(target)target.textContent=e.message;}
function submit(root,callback){root.querySelectorAll("form").forEach(form=>form.onsubmit=async e=>{e.preventDefault();const button=form.querySelector("[type=submit]");button.disabled=true;try {await callback(Object.fromEntries(new FormData(form)),form); }catch(error){failure(error,root);}finally {if(button.isConnected)button.disabled=false;}});}
const save=()=>`<button type="submit" class="btn">${t("fundHomeSave")}</button>`;
const moneyExtra='min="0" step="0.01" inputmode="decimal"';
const cashAccounts=()=>home.snapshot()?.wealth.cash.accounts||[];
function sensitive(){
 if(!home.privacy())return false;
 const message=t("fundHomeSelectShowAmountsBeforeOpeningFinancialManagement");home.status(message);
 const view=document.querySelector(".main-column > section:not([hidden])");
 if(view&&view.id!=="view-home"){
  let node=view.querySelector("[data-sensitive-feedback]");
  if(!node){node=document.createElement("p");node.dataset.sensitiveFeedback="";node.className="investment-sensitive-feedback";node.setAttribute("role","status");node.setAttribute("aria-live","polite");view.prepend(node);}
  node.textContent=message;
 }
 return true;
}
document.addEventListener("fund:language-changed",()=>document.querySelectorAll("[data-sensitive-feedback]").forEach(node=>node.textContent=t("fundHomeSelectShowAmountsBeforeOpeningFinancialManagement")));
document.addEventListener("fund:privacy-changed",()=>document.querySelectorAll("[data-sensitive-feedback]").forEach(node=>node.remove()));
const accounts=()=>cashAccounts().map(a=>[a.accountId,a.accountName]);
const classItems=()=>[["equity",t("fundHomeEquity")],["gold",t("fundHomeGold")],["fixed_income",t("fundHomeFixedIncome")],["cash",t("fundHomeCash")],["other",t("fundHomeOther")]];
home.closePanel=close;
Object.assign(home,{panel,input,text,select,check,submit,failure,sensitive,save});
home.confirm=message=>new Promise(resolve=>{
 const focus=document.activeElement,owner=dialog,node=document.createElement("dialog");node.className="home-manager";
 node.setAttribute("aria-labelledby","home-confirm-title");
 node.innerHTML=`<p id="home-confirm-title">${esc(message)}</p><button type="button" data-no autofocus>${t("fundHomeCancelled")}</button><button type="button" data-yes>${t("fundHomeConfirm")}</button>`;
 let finished=false;
 const finish=answer=>{if(finished)return;finished=true;node.close();node.remove();window.removeEventListener("pagehide",cancel);resolve(answer&&owner===dialog);focus?.focus();};
 const cancel=()=>finish(false);
 node.querySelector("[data-no]").onclick=cancel;node.querySelector("[data-yes]").onclick=()=>finish(true);
 node.addEventListener("cancel",e=>{e.preventDefault();cancel();});window.addEventListener("pagehide",cancel,{once:true});document.body.appendChild(node);node.showModal();
});

home.manageCurrency=async()=>{
 if(sensitive())return;
 const generation=++currencyGeneration;
 try {
  const assets=await api("GET","/api/assets");
  if(generation!==currencyGeneration||home.displayMode()!=="amount")return;
  const funds=[...new Map((assets.funds||[]).filter(f=>f.navCurrency==="unknown").map(f=>[f.fundId,f])).values()];
  if(!funds.length){await home.refresh();return;}
  panel(()=>t("fundHomeConfirmCurrency"),()=>`<form><p>${t("fundHomeConfirmCurrencyHint")}</p>${select("fundId",t("fundHomeName"),funds.map(f=>[f.fundId,`${f.name} (${f.code})`]))}${select("navCurrency",t("fundHomeCurrency"),[["unknown",t("fundHomeCurrencyNotConfirmed")],["CNY",t("fundHomeCurrencyCny")],["unsupported",t("fundHomeUnsupportedCurrency")]])}${save()}</form>`,root=>{
   submit(root,async v=>{
    if(v.navCurrency==="unknown")throw Error(t("fundHomeChooseCurrency"));
    const fund=funds.find(f=>String(f.fundId)===v.fundId);
    await api("POST","/api/fund-asset-metadata",{fundId:fund.fundId,navCurrency:v.navCurrency,riskClass:fund.riskClass,dataRevision:assets.dataRevision});
    if(!root.isConnected)return;
    close();await home.refresh();
   });
  });
 }catch(e){if(generation===currencyGeneration)home.status(e.message);}
};

// 自定义资金分组（方案 11 §4）：draft→保存；expectedRevision 冲突保留草稿；
// 旧五类配置仅作参考草稿预览，用户保存才激活并暂停旧偏离规则。
home.manageGroupPlan=async()=>{
 if(sensitive())return;
 try {
  const [plan,assets,offline]=await Promise.all([api("GET","/api/home/group-plan"),api("GET","/api/assets"),api("GET","/api/offline/assets")]);
  const fields=["fundBindings","manualBindings","offlineBindings","cashBindings"];
  const key=(field,ref)=>`${field}:${ref.accountId??ref.assetId}:${ref.fundId??""}`;
  let draft=(plan.groups||[]).map(g=>({...g,ratioText:String(g.ratioTenths/10),...Object.fromEntries(fields.map(f=>[f,(g[f]||[]).map(r=>({...r}))]))}));
  for(const g of draft){g.manualBindings=g.manualBindings.filter(r=>{const asset=(assets.manual||[]).find(a=>a.id===r.assetId);if(asset?.productType!=="cash")return true;if(!g.cashBindings.some(r=>r.accountId===asset.accountId))g.cashBindings.push({accountId:asset.accountId});return false;});}
  let picked=-1,offlinePage=1,hasMoreOffline=(offline.assets||[]).length===50;
  const candidates=(assets.funds||[]).map(f=>({field:"fundBindings",ref:{accountId:f.accountId,fundId:f.fundId},label:`${f.name} (${f.code}) · ${f.account}`}))
   .concat((assets.manual||[]).map(a=>({field:a.productType==="cash"?"cashBindings":"manualBindings",ref:a.productType==="cash"?{accountId:a.accountId}:{assetId:a.id},label:`${a.name} · ${a.account}`})))
   .concat((offline.assets||[]).filter(a=>!a.deleted).map(a=>({field:"offlineBindings",ref:{assetId:a.id},label:`${a.name} · ${a.location}`})));
  const owns=(g,c)=>(g[c.field]||[]).some(r=>key(c.field,r)===key(c.field,c.ref));
  const rows=()=>draft.map((g,i)=>`<fieldset><legend>${t("fundHomeGroup")} ${i+1}</legend>${input(`name-${i}`,t("fundHomeName"),g.name,"text",'required maxlength="40"')}${input(`desc-${i}`,t("fundHomeGroupDescription"),g.description||"","text",g.groupId?'maxlength="200"':'required maxlength="200"')}${input(`ratio-${i}`,t("fundHomeTargeta798ac"),g.ratioText,"number",'min="0" max="100" step="0.1" required')}<p>${fields.reduce((n,f)=>n+g[f].length,0)} ${t("fundHomeGroupPickAssets")}</p><button type="button" data-pick="${i}">${t("fundHomeGroupPickAssets")}</button><button type="button" data-up="${i}" ${i===0?"disabled":""}>${t("fundHomeMoveUp")}</button><button type="button" data-down="${i}" ${i===draft.length-1?"disabled":""}>${t("fundHomeMoveDown")}</button><button type="button" data-remove="${i}">${t("fundHomeRemove")}</button>${picked===i?`<section><input type="search" data-search aria-label="${t("fundHomeGroupPickAssetsSearch")}" placeholder="${t("fundHomeGroupPickAssetsSearch")}"><p>${t("fundHomeGroupPickAssetsHint")}</p><div data-candidates>${candidates.map((c,n)=>`<label class="home-check" data-choice="${n}"><input type="checkbox" data-candidate="${n}" ${owns(g,c)?"checked":""}>${esc(c.label)}${draft.some((other,j)=>j!==i&&owns(other,c))?` · ${t("fundHomeGroupAssetInOtherGroup")}`:""}</label>`).join("")||esc(t("fundHomeNoSourcesYet"))}</div>${hasMoreOffline?`<button type="button" data-more-assets>${t("fundHomeNext")}</button>`:""}</section>`:""}</fieldset>`).join("");
  const total=()=>draft.reduce((n,g)=>n+(Number(g.ratioText)||0),0);
  const render=()=>`<form><p>${t("fundHomeGroupSimpleHint")}</p><div data-groups>${rows()}</div><p data-total role="status">${t("fundHomeGroupTotal")} ${total().toFixed(1)}% · ${t("fundHomeGroupUnallocated")} ${(100-total()).toFixed(1)}%</p><button type="button" data-add ${draft.length>=50?"disabled":""}>${t("fundHomeAddGroup")}</button>${save()}</form>`;
  panel(()=>t("fundHomeGroupPlanTitle"),render,root=>{
   const updateTotal=()=>{root.querySelector("[data-total]").textContent=`${t("fundHomeGroupTotal")} ${total().toFixed(1)}% · ${total()>100?t("fundHomeGroupOverLimit"):t("fundHomeGroupUnallocated")+" "+(100-total()).toFixed(1)+"%"}`;};
   draft.forEach((g,i)=>{for(const [field,name] of [["name",`name-${i}`],["description",`desc-${i}`],["ratioText",`ratio-${i}`]])root.querySelector(`[name="${name}"]`).oninput=e=>{g[field]=e.target.value;updateTotal();};});
   const redraw=()=>renderDialog();
   root.querySelector("[data-add]").onclick=()=>{draft.push({groupId:"",name:"",description:"",ratioText:"",...Object.fromEntries(fields.map(f=>[f,[]]))});picked=-1;redraw();};
   root.querySelectorAll("[data-remove]").forEach(b=>b.onclick=async()=>{if(!await home.confirm(t("fundHomeGroupRemoveConfirm")))return;draft.splice(Number(b.dataset.remove),1);picked=-1;redraw();});
   root.querySelectorAll("[data-up],[data-down]").forEach(b=>b.onclick=()=>{const i=Number(b.dataset.up??b.dataset.down),j=i+(b.dataset.up!=null?-1:1);[draft[i],draft[j]]=[draft[j],draft[i]];picked=-1;redraw();});
   root.querySelectorAll("[data-pick]").forEach(b=>b.onclick=()=>{picked=picked===Number(b.dataset.pick)?-1:Number(b.dataset.pick);redraw();});
   const more=root.querySelector("[data-more-assets]");if(more)more.onclick=async()=>{more.disabled=true;try{const data=await api("GET",`/api/offline/assets?page=${offlinePage+1}`);if(!root.isConnected)return;offlinePage++;hasMoreOffline=(data.assets||[]).length===50;candidates.push(...(data.assets||[]).map(a=>({field:"offlineBindings",ref:{assetId:a.id},label:`${a.name} · ${a.location}`})));redraw();}catch(e){if(root.isConnected)failure(e);more.disabled=false;}};
   const search=root.querySelector("[data-search]");if(search)search.oninput=e=>root.querySelectorAll("[data-choice]").forEach(row=>{row.hidden=!candidates[Number(row.dataset.choice)].label.toLowerCase().includes(e.target.value.trim().toLowerCase());});
   root.querySelectorAll("[data-candidate]").forEach(box=>box.onchange=async()=>{
    const c=candidates[Number(box.dataset.candidate)],g=draft[picked];
    if(box.checked&&draft.some((other,i)=>i!==picked&&owns(other,c))&&!await home.confirm(t("fundHomeGroupMoveAsset"))){box.checked=false;return;}
    draft.forEach(other=>{other[c.field]=other[c.field].filter(r=>key(c.field,r)!==key(c.field,c.ref));});
    if(box.checked)g[c.field].push({...c.ref});
   });
   submit(root,async()=>{
    const groups=draft.map((g,i)=>{
      if(!/^\d+(?:\.\d)?$/.test(g.ratioText))throw Error(t("fundHomeGroupRatioInvalid"));
      if(!g.groupId&&!(g.description||"").trim())throw Error(t("fundHomeGroupDescriptionRequired"));
      if((g.description||"").trim().length>200)throw Error(t("fundHomeGroupDescriptionRequired"));
      return {...g,description:(g.description||"").trim(),ratioTenths:Number(g.ratioText.replace(".",""))*(g.ratioText.includes(".")?1:10),sortOrder:i};
    });
    if(groups.reduce((n,g)=>n+g.ratioTenths,0)>1000)throw Error(t("fundHomeGroupOverLimit"));
    await api("PUT","/api/home/group-plan",{groups,expectedRevision:plan.revision||0});await home.refresh();close();
   });
  });
 }catch(e){home.status(e.message);}
};

document.addEventListener("fund:language-changed",()=>{
 if(!dialog)return;
 // Native form values and focus survive projection changes; no writes or new AI request.
 home.cancelAI?.();
 const fields=Array.from(dialog.querySelectorAll("input,select,textarea"),node=>({name:node.name,value:node.value,checked:node.checked}));
 const focus=document.activeElement?.name;
 const controls=()=>Array.from(dialog.querySelectorAll('button,input,select,textarea,summary,[tabindex]'));
 const focusIndex=controls().indexOf(document.activeElement);
 const disclosures=Array.from(dialog.querySelectorAll('details'),node=>node.open);
 const scroll=dialog.scrollTop;
 renderDialog();
 dialog.querySelectorAll('details').forEach((node,index)=>{node.open=disclosures[index]??node.open;});
 const nodes=()=>Array.from(dialog.querySelectorAll("input,select,textarea"));
 fields.forEach((field,index)=>{const node=nodes()[index];if(node?.name===field.name){node.value=field.value;node.checked=field.checked;if(node.tagName==="SELECT")node.dispatchEvent(new Event("change",{bubbles:true}));}});
 (controls()[focusIndex]||dialog.querySelector(`[name="${CSS.escape(focus||"")}"]`))?.focus();dialog.scrollTop=scroll;
});

home.manageGoals=async()=>{
 if(sensitive())return;
 try {
  const {goals}=await api("GET","/api/goals?archived=1");
  const current=goals.find(g=>!g.archived),history=goals.filter(g=>g.archived);
  const list=()=>history.map(g=>`<article>${home.money(g.targetMinor)} · ${esc(g.name)} · ${esc(g.scopeKind)} · ${esc(g.targetDate||"")}<button type="button" data-restore="${g.id}" ${current?"disabled":""}>${t("fundHomeRestore")}</button></article>`).join("");
  panel(()=>t("fundHomeGoalSettings"),()=>`<p>${t("fundHomeGoalCalculatedOnRegisteredAssets")}</p>${current?`<p>${home.money(current.targetMinor)}</p><button type="button" data-edit>${t("fundHomeEdit")}</button><button type="button" data-archive>${t("fundHomeArchive")}</button>`:`<button type="button" data-create>${t("fundHomeAddGoal")}</button>`}${history.length?`<details><summary>${t("fundHomeArchived")}</summary>${list()}</details>`:""}`,root=>{
   const edit=root.querySelector("[data-edit],[data-create]");if(edit)edit.onclick=()=>editGoal(current||{});
   const archive=root.querySelector("[data-archive]");if(archive)archive.onclick=async()=>{try{if(!await home.confirm(t("fundHomeGoalArchiveConfirm")))return;await api("POST","/api/goals/archive",{id:current.id,expectedVersion:current.version});await home.refresh();home.manageGoals();}catch(e){failure(e);}};
   root.querySelectorAll("[data-restore]").forEach(b=>b.onclick=async()=>{try{const g=history.find(g=>g.id===Number(b.dataset.restore));await api("POST","/api/goals/restore",{id:g.id,expectedVersion:g.version});await home.refresh();home.manageGoals();}catch(e){failure(e);}});
  });
 }catch(e){home.status(e.message);}
};
function editGoal(g={}){
 // 方案 01 P02（08 最新）：唯一目标只填金额——无名称、期限、范围、月供输入。
 // 旧目标的名称/期限/范围仅作为历史保留显示，不提供编辑入口；提交固定 A 范围与默认名称。
 const legacy = g.id
   ? `${g.name&&g.name!==t("fundHomeMyInvestmentGoal")?`<p class="alloc-plan-row">${esc(t("fundHomeGoalName"))}：${esc(g.name)}（${t("fundHomeReportsStaleSnapshot")}）</p>`:""}${g.targetDate?`<p class="alloc-plan-row">${esc(t("fundHomeTargetDate"))}：${esc(g.targetDate)}（${t("fundHomeNoDeadlineSet")}）</p>`:""}`
   : "";
 panel(()=>(g.id?t("fundHomeEditFinancialGoal"):t("fundHomeGoalSettings")),()=>(`<form>${legacy}<p>${t("fundHomeGoalCalculatedOnRegisteredAssets")}</p>${input("target",t("fundHomeTargetAmountCny"),g.targetMinor==null?"":(BigInt(g.targetMinor)/100n).toString()+"."+(BigInt(g.targetMinor)%100n).toString().padStart(2,"0"),"number",`${moneyExtra} required`)}${save()}</form>`),root=>submit(root,async(v)=>{
  if(g.id&&g.scopeKind!=="A"&&!await home.confirm(t("fundHomeGoalScopeConvertConfirm")))return;
  const payload={name:g.name||t("fundHomeMyInvestmentGoal"),target:v.target,scopeKind:"A",accountIds:[],note:g.note||""};
  if(g.id){payload.id=g.id;payload.expectedVersion=g.version;await api("POST","/api/goals/update",payload);}
  else await api("POST","/api/goals",payload);
  await home.refresh();home.manageGoals();
 }));
}
home.manageAllocation=async()=>{
 if(sensitive())return;
 const rows=home.snapshot()?.allocation.classes||[];
 panel(()=>(t("fundHomeAllocationTargetsAndReviewRanges")),()=>(`<p>${t("fundHomeTheFiveWeightsMustTotal1000")}</p><form>${classItems().map(([key,label])=>{const r=rows.find(r=>r.risk_class===key)||{};return `<fieldset><legend>${label}</legend>${input(`${key}-target`,t("fundHomeTargeta798ac"),r.target_tenths==null?"":r.target_tenths/10,"number",'min="0" max="100" step="0.1" required')}${input(`${key}-lower`,t("fundHomeLowerBound"),r.lower_tenths==null?"":r.lower_tenths/10,"number",'min="0" max="100" step="0.1" required')}${input(`${key}-upper`,t("fundHomeUpperBound"),r.upper_tenths==null?"":r.upper_tenths/10,"number",'min="0" max="100" step="0.1" required')}</fieldset>`;}).join("")}${save()}</form>`),root=>submit(root,async v=>{
  const targets=classItems().map(([riskClass])=>({riskClass,targetTenths:Math.round(Number(v[`${riskClass}-target`])*10),lowerTenths:Math.round(Number(v[`${riskClass}-lower`])*10),upperTenths:Math.round(Number(v[`${riskClass}-upper`])*10)}));
  await api("POST","/api/allocation",{targets});close();await home.refresh();
 }));
};
home.manageCash=async(section)=>{
 if(sensitive())return;
 try {
 const [cash,history,orders,trades]=await Promise.all([api("GET","/api/finance/cash"),api("GET","/api/finance/events?limit=100"),api("GET","/api/finance/reservations"),api("GET","/api/finance/trades")]);
 const options=cash.accounts.map(a=>[a.accountId,a.accountName]);
 const reserveForms=()=>cash.accounts.map(a=>`<form data-kind="reserve"><h3>${esc(a.accountName)}</h3><p>${t("fundHomeSettled")}${home.money(a.cashMinor)}${t("fundHomeAvailable6df265")}${a.availableMinor==null?t("fundHomeNeedsVerificationccf920"):home.money(a.availableMinor)}</p><input type="hidden" name="accountId" value="${a.accountId}">${input("amount",t("fundHomeReserveBlankUnconfirmed0ExplicitlyNone"),a.reservedMinor==null?"":`${BigInt(a.reservedMinor)/100n}.${String(BigInt(a.reservedMinor)%100n).padStart(2,"0")}`,"number",moneyExtra)}${input("purpose",t("fundHomeReservePurpose"))}${save()}</form>`).join("");
 const events=()=>[["external_in",t("fundHomeExternalContribution")],["external_out",t("fundHomeExternalWithdrawal")],["internal_transfer",t("fundHomeInternalTransfer")],["dividend_cash",t("fundHomeCashDividendInterest")],["dividend_reinvest",t("fundHomeManualAssetDividendReinvestment")],["fee",t("fundHomeFee")],["correction",t("fundHomeVerifiedCorrectionMayBeNegative")],["reconciliation",t("fundHomeUnclassifiedDifferenceReturnsPaused")],["opening",t("fundHomeOpeningCashBalance")]];
 const cashPanel=panel(()=>(t("fundHomeCashAndFlowReconciliation")),()=>(`${reserveForms()||t("fundHomeRecordAccountsAndCashInAssetsFirst")}<details><summary>${t("fundHomeCreateCashAccount")}</summary><form data-kind="cash">${input("account",t("fundHomeAccount"),"","text",'required maxlength="64"')}${input("marketValue",t("fundHomeOpeningCashBalance"),"","number",`${moneyExtra} required`)}${input("valuedAt",t("fundHomeActualEventDate"),new Date(Date.now()+8*3600000).toISOString().slice(0,10),"date","required")}${check("confirmed",t("fundHomeConfirmActualCash"),false,"required")}${save()}</form></details><h3>${t("fundHomeRecordCashFlow")}</h3><form data-kind="event">${select("accountId",t("fundHomeAccount"),options)}${select("eventType",t("fundHomeType"),events())}${input("amount",t("fundHomeAmountSignedChangeForCorrectionsDifferences"),"","number",'step="0.01" required')}${input("occurredAt",t("fundHomeActualEventDate"),new Date(Date.now()+8*3600000).toISOString().slice(0,10),"date","required")}${select("counterpartyAccountId",t("fundHomeInternalTransferDestination"),[["",t("fundHomeSelectOnlyForInternalTransfers")],...options])}${input("assetId",t("fundHomeManualAssetIdReinvestmentOnly"),"","number","min=1")}${text("note",t("fundHomeReconciliationNotes"))}${save()}</form><h3>${t("fundHomeTradeCashSettlement")}</h3>${trades.trades.map(item=>`<form data-kind="trade"><p>${esc(item.account)} · ${esc(item.symbol)} · ${esc(item.type)} · ${home.money(item.deltaMinor)} · ${esc(item.status)}</p><input type="hidden" name="id" value="${item.id}">${select("mode",t("fundHomeReconciliationMode"),item.status==="receivable"?[["post",t("fundHomeActuallySettledPostToCash")]]:[["included",t("fundHomeAlreadyIncludedInCurrentCashAvoidDouble")],["post",t("fundHomeNotIncludedInCurrentCashPostThe")]])}${input("date",t("fundHomeActualReconciliationDate"),new Date(Date.now()+8*3600000).toISOString().slice(0,10),"date","required")}${text("note",t("fundHomeReconciliationEvidence"))}${save()}</form>`).join("")}<h3>${t("fundHomeBuyCommitments")}</h3><form data-kind="reservation">${select("accountId",t("fundHomePaymentAccount"),options)}${input("symbol",t("fundHomeAssetIdentityEGSecuritySse600519"),"","text","required")}${select("riskClass",t("fundHomePrimaryRiskClass"),classItems().filter(([k])=>k!=="cash"))}${input("amount",t("fundHomeCommittedPrincipal"),"","number",`${moneyExtra} required`)}${input("expectedDate",t("fundHomeExpectedReviewDate"),"","date","required")}${text("note",t("fundHomeNotes"))}${save()}</form>${orders.reservations.map(o=>`<article>${esc(o.symbol)} · ${home.money(o.amountMinor)} · ${esc(o.status)}${o.status==="held"?`<button data-release="${o.id}">${t("fundHomeCancelAndRelease")}</button>`:""}</article>`).join("")}<p>${t("fundHomeRecordActualExecutionsInTransactionsOrManual")}</p><h3>${t("fundHomeTodaySValuationAndReturnBaseline")}</h3><form data-kind="snapshot">${check("flowsChecked",t("fundHomeAllPeriodContributionsWithdrawalsDividendsFeesAnd"))}${text("note",t("fundHomeValuationVerificationNotes"))}${save()}</form><h3>${t("fundHomeLatest100CashEvents")}</h3>${history.events.map(e=>`<p>${esc(e.occurredAt)} · ${esc(e.eventType)} · ${home.money(e.amountMinor)} · ${esc(e.note)}</p>`).join("")}`),root=>{
  root.querySelectorAll("[data-release]").forEach(b=>b.onclick=async()=>{try{await api("POST","/api/finance/reservations/settle",{id:Number(b.dataset.release),action:"release"});await home.refresh();home.manageCash();}catch(e){failure(e);}});
  submit(root,async(v,form)=>{v.accountId=Number(v.accountId);if(v.counterpartyAccountId)v.counterpartyAccountId=Number(v.counterpartyAccountId);else delete v.counterpartyAccountId;
   const kind=form.dataset.kind;
   if(kind==="cash"){await api("POST","/api/assets",{account:v.account,name:v.account,productType:"cash",riskClass:"cash",marketValue:v.marketValue,valuedAt:v.valuedAt});await home.refresh();home.manageCash();return;}
   if(kind==="event"){v.idempotencyKey=form.dataset.requestKey||(form.dataset.requestKey=home.requestId());if(v.assetId)v.assetId=Number(v.assetId);else delete v.assetId;}
   if(kind==="trade")v.id=Number(v.id);
   if(kind==="snapshot")v.flowsChecked=form.elements.flowsChecked.checked;
   await api("POST",{trade:"/api/finance/trades/settle",reserve:"/api/finance/reserve",event:"/api/finance/events",reservation:"/api/finance/reservations",snapshot:"/api/finance/snapshot/save"}[kind],v);await home.refresh();home.manageCash();
  });
 });
 if(section==="snapshot"&&cashPanel){const form=cashPanel.querySelector('[data-kind="snapshot"]');form.scrollIntoView({block:"center"});form.elements.flowsChecked.focus();}
 }catch(e){home.status(e.message);}
};
const templates=()=>({cash_short:[t("fundHomeCashShortfall9e000e"),{}],class_unknown:[t("fundHomeUnknownClassification"),{}],stale_valuation:[t("fundHomeStaleValuation"),{}],allocation_drift:[t("fundHomeAllocationDrift"),{}],concentration:[t("fundHomeAssetConcentration"),{threshold:t("fundHomeYourPercentage")}],price_threshold:[t("fundHomePriceThreshold"),{threshold:t("fundHomeYourPrice"),direction:"below"}],daily_change:[t("fundHomeDailyPriceChange"),{threshold:t("fundHomeYourPercentage"),direction:"absolute"}],ma_cross:[t("fundHomeClosedDailyMa2060"),{direction:"down"}],drawdown:[t("fundHomeRollingDrawdown"),{threshold:t("fundHomeYourPercentage"),window:60}],strategy_exit:[t("fundHomeStrategyExit"),{condition:"expiry",expiryDate:"YYYY-MM-DD"}],investment_lag:[t("fundHomeContributionLag"),{monthlyPlan:t("fundHomePlannedAmount"),dueDate:"YYYY-MM-DD"}]});
home.manageRules=async()=>{
 if(sensitive())return;
 try {
 const [{rules},prefs]=await Promise.all([api("GET","/api/rules"),api("GET","/api/home/preferences")]);
 panel(()=>(t("fundHomeRulesAndAlerts")),()=>(`<p>${t("fundHomeSaveAndConfirmAllInvestmentThresholdsYourself")}</p>${check("notifications",t("fundHomeAllowSystemNotificationsForUnhandledEvents"),prefs.notificationsEnabled)}<button data-create>${t("fundHomeAddRule")}</button>${rules.map(r=>`<article><b>${esc(r.name)}</b> · ${esc(r.ruleType)} · v${r.version} · ${r.enabled?t("fundHomeEnabled"):t("fundHomeDisabled")}<div><button data-edit="${r.id}">${t("fundHomeEdit")}</button><button data-toggle="${r.id}">${r.enabled?t("fundHomeDisabled"):t("fundHomeEnabled")}</button><button data-archive="${r.id}">${t("fundHomeArchive")}</button></div></article>`).join("")}`),root=>{
 root.querySelector("[name=notifications]").onchange=async e=>{try{await api("POST","/api/home/preferences",{notificationsEnabled:e.target.checked});await home.refresh();}catch(error){failure(error);}};
 root.querySelector("[data-create]").onclick=()=>editRule();
 root.querySelectorAll("[data-edit]").forEach(b=>b.onclick=()=>editRule(rules.find(r=>r.id===Number(b.dataset.edit))));
 for(const op of ["toggle","archive"])root.querySelectorAll(`[data-${op}]`).forEach(b=>b.onclick=async()=>{try{const r=rules.find(r=>r.id===Number(b.dataset[op]));await api("POST",`/api/rules/${op==="toggle"?"enabled":"archive"}`,{id:r.id,enabled:!r.enabled,expectedVersion:r.version});await home.refresh();home.manageRules();}catch(e){failure(e);}});
 });
 }catch(e){home.status(e.message);}
};
function editRule(r={}){
 const existing=typeof r.params==="string"?JSON.parse(r.params):r.params||{};
 const directions=(values,current)=>select("direction",t("fundHomeTriggerDirection"),values,current);
 const fields=(type,p)=>{
  if(type==="concentration")return input("threshold",t("fundHomeMaximumAssetWeight"),p.threshold??"","number",'min="0.1" max="100" step="0.1" required');
  if(type==="price_threshold"||type==="daily_change")return input("threshold",type==="price_threshold"?t("fundHomePriceThreshold"):t("fundHomeDailyChangeThreshold"),p.threshold??"","number",'min="0.01" step="0.01" required')+directions(type==="price_threshold"?[["below",t("fundHomeAtOrBelow")],["above",t("fundHomeAtOrAbove")]]:[["absolute",t("fundHomeAbsoluteChange")],["above",t("fundHomeRiseReaches")],["below",t("fundHomeFallReaches")]],p.direction||"below");
  if(type==="ma_cross")return directions([["down",t("fundHomeCrossDown")],["up",t("fundHomeCrossUp")]],p.direction||"down")+`<p>${t("fundHomeUsesMa20AndMa60OnClosedDaily")}</p>`;
  if(type==="drawdown")return input("threshold",t("fundHomeRollingDrawdownReaches"),p.threshold??"","number",'min="0.1" max="100" step="0.1" required')+input("window",t("fundHomeClosedDailyCandleWindow"),p.window??60,"number",'min="2" max="800" step="1" required');
  if(type==="investment_lag")return input("monthlyPlan",t("fundHomeThisMonthSPlannedContribution"),p.monthlyPlan??"","number",'min="0.01" step="0.01" required')+input("dueDate",t("fundHomeThisMonthSReviewDate"),p.dueDate||"","date","required");
  if(type==="strategy_exit")return select("condition",t("fundHomeExitCondition"),[["expiry",t("fundHomeExpiryReview")],["price",t("fundHomePriceCondition")],["thesis",t("fundHomeInvestmentThesisInvalidated")]],p.condition||"expiry")+`<div data-condition-fields>${conditionFields(p.condition||"expiry",p)}</div>`;
  return `<p>${t("fundHomeUsesVerifiedLocalDataInsufficientDataIs")}</p>`;
 };
 const conditionFields=(condition,p)=>condition==="expiry"?input("expiryDate",t("fundHomeExpiryReviewDate"),p.expiryDate||"","date","required"):condition==="price"?input("threshold",t("fundHomePriceAtOrBelow"),p.threshold??"","number",'min="0.01" step="0.01" required'):check("invalidated",t("fundHomeIHaveVerifiedThatTheInvestmentThesis"),p.invalidated);
 panel(()=>(t("fundHomeConfigureRule")),()=>(`<form>${input("name",t("fundHomeName"),r.name||"","text",'required maxlength="64"')}${select("ruleType",t("fundHomeRuleType"),Object.entries(templates()).map(([k,[name]])=>[k,name]),r.ruleType||"cash_short")}${input("objectKey",t("fundHomeAssetIdentityRequiredForAnIndividualAsset"),r.objectKey||"")}${select("period",t("fundHomeDataPeriod"),[["day",t("fundHomeDaily")],["quote",t("fundHomeQuote")],["month",t("fundHomeMonthly")]],r.period||"day")}<div data-rule-fields>${fields(r.ruleType||"cash_short",existing)}</div>${check("enabled",t("fundHomeConfirmTheseInvestmentConditionsAndEnable"),r.enabled)}${save()}</form>`),root=>{
 const form=root.querySelector("form");
 form.elements.ruleType.onchange=()=>{root.querySelector("[data-rule-fields]").innerHTML=fields(form.elements.ruleType.value,{});};
 form.addEventListener("change",e=>{if(e.target.name==="condition")root.querySelector("[data-condition-fields]").innerHTML=conditionFields(e.target.value,{});});
 submit(root,async v=>{
  const type=v.ruleType,params={};
  if(["price_threshold","daily_change","ma_cross","drawdown","strategy_exit"].includes(type)&&!v.objectKey.trim())throw Error(t("fundHomeThisRuleRequiresAnAssetWithA"));
  if(["concentration","price_threshold","daily_change","drawdown"].includes(type)||type==="strategy_exit"&&v.condition==="price")params.threshold=Number(v.threshold);
  if(["price_threshold","daily_change","ma_cross"].includes(type))params.direction=v.direction;
  if(type==="drawdown")params.window=Number(v.window);
  if(type==="investment_lag"){params.monthlyPlan=Number(v.monthlyPlan);params.dueDate=v.dueDate;}
  if(type==="strategy_exit"){params.condition=v.condition;if(v.condition==="expiry")params.expiryDate=v.expiryDate;if(v.condition==="thesis")params.invalidated=form.elements.invalidated.checked;}
  const payload={name:v.name,ruleType:type,objectKey:v.objectKey,period:v.period,params,enabled:form.elements.enabled.checked};
  if(r.id){payload.id=r.id;payload.expectedVersion=r.version;}
  await api("POST",r.id?"/api/rules/update":"/api/rules",payload);await home.refresh();home.manageRules();
 });
 });
}
home.simulate=()=>{
 if(sensitive())return;
 const snap=home.snapshot();
 if(!snap){home.status(t("fundHomeLoadTheHomeSnapshotFirst"));return;}
 const candidates=[];let selectedAccount=null;
 panel(()=>(t("fundHomeAmountSimulation")),()=>(`<p>${t("fundHomeShowsPrincipalCapsOnlyWithoutExecutionOr")}</p><form>${select("accountId",t("fundHomePaymentAccount"),accounts())}${input("symbol",t("fundHomeAssetIdentityMarketQualifiedOrFund"),"","text","required")}${select("riskClass",t("fundHomePrimaryRiskClass"),classItems().filter(([k])=>k!=="cash"))}${input("singleLimit",t("fundHomeSingleOrderPrincipalLimit"),"","number",`${moneyExtra} required`)}${input("fee",t("fundHomeConfirmedFee0ForExplicitlyNoFee"),"","number",`${moneyExtra} required`)}<button type="submit">${t("fundHomeAddToSimulation")}</button></form><div data-results role="status"></div>`),root=>{
 submit(root,async v=>{const accountId=Number(v.accountId);if(selectedAccount!==null&&selectedAccount!==accountId)throw new Error(t("fundHomeEachBatchUsesOnePaymentAccountReopen"));selectedAccount=accountId;candidates.push(v);try{const results=await api("POST","/api/simulate/buy",{accountId,dataRevision:home.snapshot().dataRevision,candidates});root.querySelector("[data-results]").innerHTML=results.results.map((r,i)=>`<article><b>${esc(candidates[i].symbol)}</b>：${r.computable?`${t("fundHomePrincipalCap")}${home.money(r.capMinor)}${r.capMinor===0?t("fundHomeNoAdditionalContributionNeededOrInsufficientBudget"):""}`:esc(r.reason)}<ul>${(r.breakdown||[]).map(([k,v])=>`<li>${esc(k)}：${home.money(v)}</li>`).join("")}</ul></article>`).join("");}catch(e){candidates.pop();throw e;}});
 });
};
let historyGeneration=0,historyController=null;
actions.leaveDecisionHistory=()=>{historyGeneration++;historyController?.abort();historyController=null;const body=document.getElementById('history-page-body');body?.dispatchEvent(new Event('close'));};
home.history=()=>document.getElementById('tab-history').click();
actions.loadDecisionHistory=async()=>{
 const body=document.getElementById('history-page-body'),view=document.getElementById('view-history');if(!body||view.hidden)return;
 actions.leaveDecisionHistory();const own=historyGeneration;historyController=new AbortController();
 if(home.privacy()){body.textContent=t('fundHomeSelectShowAmountsBeforeOpeningFinancialManagement');return;}
 body.innerHTML=`<p role="status">${esc(t('fundHomeLoading'))}</p>`;
 try{
 const options={signal:historyController.signal};
 const [events,history,information]=await Promise.all([api('GET','/api/events',null,options),api('GET','/api/decisions',null,options),api('GET','/api/information/decisions',null,options)]);
 if(own!==historyGeneration||view.hidden||home.privacy())return;
 const timeline=(information.records||[]).map(r=>`<article class="card decision-timeline"><h3>${esc(r.title)}</h3><div class="timeline-item"><small>${esc(t('fundHomeInformationDetail'))}</small><button type="button" data-history-info="${r.informationId}">${esc(t('fundHomeOpenSource'))}</button></div><div class="timeline-item"><small>${esc(t('fundHomeReportsTitle'))}</small><p>${esc(r.modelName)} · ${esc(r.snapshotId)}</p><details data-decision-report="${r.reportId}"><summary>${esc(t('fundHomeSavedReportView'))}</summary><div data-saved-report></div></details></div><div class="timeline-item"><small>${esc(t('fundHomeMyAction'))} · ${esc(new Date(Number(r.createdAt)*1000).toLocaleString(Fund.state.language))}</small><h4>${esc(t({observe:'fundHomeObserve',defer:'fundHomeDefer',done:'fundHomeHandledDoesNotImplyExecution'}[r.action]||'fundHomeSettingsUnknown'))}</h4><p>${esc(r.note)}</p></div></article>`).join('');
 body.innerHTML=`<div class="decision-history-layout"><div class="decision-timelines">${timeline||`<section class="card investment-row-empty">${esc(t('fundHomeReportsEmpty'))}</section>`}</div><aside class="card"><h3>${esc(t('fundHomeDecisionReviewPoints'))}</h3><p class="investment-card-tip">${esc(t('fundHomeDecisionHistoryScope'))}</p><h4>${esc(t('fundHomeRecentDecisions'))}</h4>${(history.records||[]).map(r=>`<div class="timeline-item"><small>${esc(r.createdAt)}</small><p>${esc(t({observe:'fundHomeObserve',defer:'fundHomeDefer',done:'fundHomeHandledDoesNotImplyExecution'}[r.action]||'fundHomeSettingsUnknown'))} · ${esc(r.note)}</p></div>`).join('')||`<p>${esc(t('fundHomeReportsEmpty'))}</p>`}</aside></div><details class="card decision-event-list"><summary>${esc(t('fundHomeRecentEvents'))} · ${(events.events||[]).length}</summary>${(events.events||[]).map(e=>`<article><h3>${esc(e.objectKey)}</h3><p>${esc(e.ruleType)} · ${esc(e.status)}</p><form data-event="${e.id}">${select('action',t('fundHomeMyAction'),[['observe',t('fundHomeObserve')],['defer',t('fundHomeDefer')],['done',t('fundHomeHandledDoesNotImplyExecution')]])}${text('note',t('fundHomeReasonAndEvidence'))}${input('until',t('fundHomeDeferUntilDeferralOnly'),'','date')}${save()}</form></article>`).join('')}</details><p class="home-form-error" role="status"></p>`;
 home.bindDecisionReports(body);
 body.querySelectorAll('[data-history-info]').forEach(button=>button.onclick=()=>home.openInformationDetail(Number(button.dataset.historyInfo)));
 submit(body,async(v,form)=>{
  const eventId=Number(form.dataset.event);if(v.action==='defer'&&!v.until)throw Error(t('fundHomeSelectTheDeferralEndDate'));
  await api('POST','/api/decisions',{eventId,action:v.action,note:v.note,snapshotId:home.snapshot().snapshotId});
  await api('POST',v.action==='defer'?'/api/events/snooze':'/api/events/ack',v.action==='defer'?{id:eventId,until:v.until}:{id:eventId});
  await home.refresh();await actions.loadDecisionHistory();
 });
 document.getElementById('history-page-title')?.focus({preventScroll:true});
 }catch(e){if(own===historyGeneration&&e.name!=='AbortError'){body.innerHTML=`<p role="alert">${esc(t('fundLedgerLoadFailed'))}</p><button data-history-retry>${esc(t('fundLedgerRetry'))}</button>`;body.querySelector('[data-history-retry]').onclick=actions.loadDecisionHistory;}}
};
document.getElementById('history-page-reports')?.addEventListener('click',()=>home.manageReports());
for(const event of ['fund:privacy-changed','fund:language-changed'])document.addEventListener(event,()=>{actions.leaveDecisionHistory();const body=document.getElementById('history-page-body');body?.replaceChildren();if(!document.getElementById('view-history')?.hidden)void actions.loadDecisionHistory();});
window.addEventListener('pagehide',actions.leaveDecisionHistory);

})();
