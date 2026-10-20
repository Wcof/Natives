// 线下投资资产：估值、退出与到账事实复用 Fund 账本，不把现金或证券重复建档。
(() => {
const {api,actions,t}=globalThis.Fund;
const home=()=>globalThis.Fund.home;
let rows=[],legacy=[],page=1,includeDeleted=false,type="",generation=0,controller=null,loadedKey=null;
let loading=false,loadError=null,historyGeneration=0,historyController=null,historyRoot=null;
const active=()=>!document.getElementById("view-assets").hidden&&!document.hidden;
function cancelHistory(){historyGeneration++;historyController?.abort();historyController=null;historyRoot=null;}
actions.leaveAssets=()=>{generation++;controller?.abort();controller=null;loading=false;cancelHistory();};
window.addEventListener("pagehide",actions.leaveAssets);
document.addEventListener("visibilitychange",()=>{if(document.hidden)actions.leaveAssets();});
const today=()=>new Date(Date.now()+8*3600000).toISOString().slice(0,10);
const minor=value=>{
 if(!/^\d+(?:\.\d{1,2})?$/.test(value))throw Error(t("fundHomeAmountInvalid"));
 const [whole,part=""]=value.split(".");const n=BigInt(whole)*100n+BigInt(part.padEnd(2,"0"));
 if(n>9223372036854775807n)throw Error(t("fundHomeAmountInvalid"));return n.toString();
};
const decimal=value=>{const n=BigInt(value||"0");return `${n/100n}.${String(n%100n).padStart(2,"0")}`;};
const types=()=>[["gold",t("fundHomePhysicalGold")],["deposit",t("fundHomeTermDeposit")],["wealth",t("fundHomeBankWealth")],["other",t("fundHomeOther")]];
const label=value=>types().find(([key])=>key===value)?.[1]||value;
const status=message=>{document.getElementById("asset-msg").textContent=message;};
function render(){
 const h=home(),esc=h.escape,hidden=h.displayMode()==="hidden",amount=h.displayMode()==="amount";
 const current=loadedKey===JSON.stringify([page,type,includeDeleted]),list=current?rows:[];
 const metric=(key,value)=>`<div><dt>${esc(t(key))}</dt><dd>${value}</dd></div>`;
 document.getElementById("offline-cards").innerHTML=list.map(a=>`<article class="card offline-asset-card">
 <div class="between"><h3>${hidden?"****":esc(a.name)}</h3><span class="tag">${esc(label(a.productType))}</span></div>
 <div><small>${esc(t("fundHomeValuationDate"))} · ${esc(a.valuedAt)}</small><div class="holding-amount num">${amount?esc(Fund.formatMinorExact(a.valueMinor)):"****"}</div></div>
 <dl class="metric-list">${metric("fundHomeOfflineLocation",hidden?"****":esc(a.location||t("fundHomeUnknown")))}${metric("fundHomeStatus",esc(a.deleted?t("fundLedgerArchivedStatus"):a.exitDate?t(BigInt(a.valueMinor)>0n?"fundHomeOfflinePartialExit":"fundHomeOfflineExited"):t("fundHomeOfflineActive")))}${a.productType==='gold'?metric("fundHomeGoldWeightOptional",hidden?'****':esc(a.attrs?.weight||t('fundHomeUnknown'))):metric("fundHomeMaturityOptional",hidden?'****':esc(a.attrs?.maturityDate||t('fundHomeUnknown')))}</dl>
 <p class="investment-card-tip">${!amount?'****':esc(a.note||t("fundHomeOfflineValuationHint"))}</p>
 <div class="holding-foot">${a.deleted?`<button data-offline="restore" data-id="${a.id}">${t("fundHomeRestore")}</button>`:`<button data-offline="edit" data-id="${a.id}">${t("fundHomeEdit")}</button><button data-offline="history" data-id="${a.id}">${t("fundHomeHistory")}</button><button data-offline="exit" data-id="${a.id}">${t("fundHomeOfflineExit")}</button><button class="quiet" data-offline="delete" data-id="${a.id}">${t("fundLedgerArchiveRecord")}</button>`}</div></article>`).join("")||`<div class="card investment-row-empty"><p>${t(loading?"fundHomeLoading":loadError?"fundLedgerLoadFailed":"fundHomeOfflineEmpty")}</p></div>`;
 const valid=list.filter(a=>!a.deleted),value=valid.reduce((sum,a)=>sum+BigInt(a.valueMinor),0n);
 const summary=(key,v)=>`<div><span>${t(key)}</span><b>${v}</b></div>`;
 document.getElementById("offline-summary").innerHTML=current?summary("fundHomeOfflinePageValue",amount?esc(Fund.formatMinorExact(value)):"****")+summary("fundHomeOfflinePageRecords",list.length)+summary("fundHomeOfflinePageActive",valid.length)+`<p>${t("fundHomeOfflineSummaryScope")}</p>`:"";
 status(loading?t("fundHomeLoading"):loadError?t("fundLedgerLoadFailed")+" · "+loadError.message:"");
 document.getElementById("offline-retry").hidden=!loadError;
 document.getElementById("offline-cards").setAttribute("aria-busy",String(loading));
 document.getElementById("offline-page").textContent=String(page);document.getElementById("offline-prev").disabled=loading||page===1;document.getElementById("offline-next").disabled=loading||!!loadError||!current||(rows.length<50&&legacy.length<50);
 const old=current&&(!type||type==="gold")?legacy.filter(a=>a.productType==="gold"&&!a.convertedRef&&(includeDeleted||!a.deleted)):[];
 document.getElementById("offline-legacy").innerHTML=old.length?`<h3>${t("fundHomeLegacyAssets")}</h3><p>${t("fundHomeLegacyAssetsHint")}</p>${old.map(a=>`<article>${hidden?"****":esc(a.name)} · ${amount?esc(a.marketValue):"****"}${a.deleted?`<button data-legacy-op="restore" data-id="${a.id}">${t("fundHomeRestore")}</button>`:`<button data-legacy-op="edit" data-id="${a.id}">${t("fundHomeEdit")}</button><button data-legacy-op="delete" data-id="${a.id}">${t("fundLedgerArchiveRecord")}</button>`}</article>`).join("")}`:"";
}
async function load(){
 if(!active())return;
 const gen=++generation,key=JSON.stringify([page,type,includeDeleted]);controller?.abort();const owned=controller=new AbortController();
 loading=true;loadError=null;render();
 try{
  const [data,old]=await Promise.all([api("GET",`/api/offline/assets?page=${page}&productType=${encodeURIComponent(type)}${includeDeleted?"&includeDeleted=1":""}`,null,{signal:owned.signal}),!type||type==="gold"?api("GET",`/api/legacy-assets?kind=gold&page=${page}`,null,{signal:owned.signal}):Promise.resolve({assets:[]})]);
  if(gen!==generation||!active())return;rows=data.assets||[];legacy=old.assets||[];loadedKey=key;
 }catch(e){if(gen===generation&&!owned.signal.aborted)loadError=e;}
 finally{if(gen===generation){loading=false;controller=null;render();}}
}
function edit(a={}){
 const h=home(),{input,text,select,panel,submit}=h;
 const extra='min="0" step="0.01" required inputmode="decimal"';
 panel(()=>t(a.id?"fundHomeEditOffline":"fundHomeAddOffline"),()=>`<form>${select("productType",t("fundHomeType"),types(),a.productType||"gold")}${input("name",t("fundHomeName"),a.name||"","text",'required maxlength="64"')}${input("location",t("fundHomeOfflineLocation"),a.location||"","text",'maxlength="128"')}${input("value",t("fundHomeCurrentValuation"),a.valueMinor==null?"":decimal(a.valueMinor),"number",extra)}${input("valuedAt",t("fundHomeValuationDate"),a.valuedAt||today(),"date",`required max="${today()}"`)}${input("cost",t("fundHomeCostOptional"),a.costMinor==null?"":decimal(a.costMinor),"number",'min="0" step="0.01"')}${select("riskClass",t("fundHomeRiskClass"),[["unknown",t("fundHomeUnknown")],["gold",t("fundHomeGold")],["fixed_income",t("fundHomeFixedIncome")],["other",t("fundHomeOther")]],a.riskClass||"unknown")}${input("maturityDate",t("fundHomeMaturityOptional"),a.attrs?.maturityDate||"","date")}${input("weight",t("fundHomeGoldWeightOptional"),a.attrs?.weight||"","number",'min="0" step="0.0001"')}${text("note",t("fundHomeNote"),a.note||"")}<p>${t("fundHomeOfflineValuationHint")}</p><button type="submit">${t("fundHomeSave")}</button></form>`,root=>submit(root,async v=>{
  const payload={...v,valueMinor:minor(v.value),costMinor:v.cost===""?null:minor(v.cost),attrs:{...(a.attrs||{}),maturityDate:v.maturityDate,weight:v.weight}};
  if(a.id){payload.id=a.id;payload.expectedVersion=a.version;}
  await api("POST",a.id?"/api/offline/assets/update":"/api/offline/assets",payload);h.closePanel();await load();await h.refresh();
 }));
}
function exit(a){
 const h=home(),{input,select,panel,submit}=h,key=h.requestId();
 const accounts=h.snapshot()?.wealth.cash.accounts||[];
 panel(()=>t("fundHomeOfflineExit"),()=>`<form><p>${t("fundHomeOfflineExitHint")}</p>${input("exitDate",t("fundHomeActualEventDate"),today(),"date",`required max="${today()}"`)}${input("amount",t("fundHomeRecoveryAmount"),"","number",'min="0" step="0.01" required')}${input("fee",t("fundHomeConfirmFee"),"","number",'min="0" step="0.01" required')}${input("remaining",t("fundHomeRemainingValuation"),"","number",'min="0" step="0.01" required')}${select("account",t("fundHomeSettlementStatus"),[["",t("fundHomeNotSettled")],...accounts.map(a=>[a.accountId,a.accountName])])}<button type="submit">${t("fundHomeSave")}</button></form>`,root=>submit(root,async v=>{
  await api("POST","/api/offline/assets/exit",{id:a.id,expectedVersion:a.version,requestId:key,exitDate:v.exitDate,exitAmountMinor:minor(v.amount),exitFeeMinor:minor(v.fee),remainingValueMinor:minor(v.remaining),settleAccountId:v.account?Number(v.account):null});h.closePanel();await load();await h.refresh();
 }));
}
async function history(a,page=1){
 const h=home();if(h.sensitive())return;
 cancelHistory();const gen=historyGeneration,owned=historyController=new AbortController();
 let values=[],exits=[],busy=true,error=null;
 const content=()=>`<div data-asset-history>${busy?`<p role="status">${t("fundHomeLoading")}</p>`:""}${error?`<p role="alert">${h.escape(error.message)}</p><button data-history-retry>${t("fundLedgerRetry")}</button>`:""}<h3>${t("fundHomeValuationHistory")}</h3>${values.map(v=>`<p>${h.escape(v.valuedAt)} · ${h.escape(decimal(v.valueMinor))}</p>`).join("")}<h3>${t("fundHomeOfflineExit")}</h3>${exits.map(e=>`<article>${h.escape(e.exitDate)} · ${h.escape(decimal(e.amountMinor))} · ${e.settled?t("fundHomeSettled"):t("fundHomeNotSettled")}${e.settled?"":`<button data-settle="${e.id}">${t("fundHomeConfirmSettlement")}</button>`}</article>`).join("")}<button data-history-prev ${busy||page===1?"disabled":""}>${t("fundHomePrevious")}</button> ${page} <button data-history-next ${busy||error||values.length<50&&exits.length<50?"disabled":""}>${t("fundHomeNext")}</button></div>`;
 const bind=root=>{root.querySelector("[data-history-prev]").onclick=()=>history(a,page-1);root.querySelector("[data-history-next]").onclick=()=>history(a,page+1);root.querySelector("[data-history-retry]")?.addEventListener("click",()=>history(a,page));root.querySelectorAll("[data-settle]").forEach(b=>b.onclick=()=>settle(exits.find(e=>e.id===Number(b.dataset.settle))));};
 const root=h.panel(()=>t("fundHomeHistory")+" · "+a.name,content,bind);if(!root)return;historyRoot=root;
 root.addEventListener("close",()=>{if(historyRoot===root)cancelHistory();},{once:true});
 try{const [v,data]=await Promise.all([api("GET",`/api/offline/assets/history?id=${a.id}&page=${page}`,null,{signal:owned.signal}),api("GET",`/api/offline/assets/exits?id=${a.id}&page=${page}`,null,{signal:owned.signal})]);values=v;exits=data.exits;}
 catch(e){if(!owned.signal.aborted)error=e;}
 if(gen!==historyGeneration||!root.isConnected||!active()||h.displayMode()!=="amount")return;
 busy=false;root.querySelector("[data-asset-history]").outerHTML=content();bind(root);
}
function settle(e){
 const h=home(),accounts=h.snapshot()?.wealth.cash.accounts||[];
 h.panel(()=>t("fundHomeConfirmSettlement"),()=>`<form>${h.select("account",t("fundHomeAccount"),accounts.map(a=>[a.accountId,a.accountName]))}${h.input("settledAt",t("fundHomeActualEventDate"),today(),"date",`required min="${e.exitDate}" max="${today()}"`)}<button type="submit">${t("fundHomeSave")}</button></form>`,root=>h.submit(root,async v=>{await api("POST","/api/offline/assets/settle",{id:e.id,expectedVersion:e.version,accountId:Number(v.account),settledAt:v.settledAt});h.closePanel();await load();await h.refresh();}));
}
document.addEventListener("click",async e=>{
 const b=e.target.closest("[data-legacy-op]");if(!b)return;const a=legacy.find(a=>a.id===Number(b.dataset.id)),h=home();if(!a||h.sensitive())return;
 if(b.dataset.legacyOp==="edit")h.panel(()=>t("fundHomeEditOffline"),()=>`<form>${h.input("name",t("fundHomeName"),a.name,"text",'required maxlength="64"')}${h.input("marketValue",t("fundHomeCurrentValuation"),a.marketValue,"number",'required min="0" step="0.01"')}${h.input("valuedAt",t("fundHomeValuationDate"),a.valuedAt,"date",`required max="${today()}"`)}<button type="submit">${t("fundHomeSave")}</button></form>`,root=>h.submit(root,async v=>{await api("POST","/api/legacy-assets/change",{...v,id:a.id,expectedVersion:a.version,op:"update",riskClass:a.riskClass});h.closePanel();await load();await h.refresh();}));
 else try{if(!await h.confirm(t(b.dataset.legacyOp==="restore"?"fundHomeOfflineRestoreHint":"fundHomeOfflineDeleteHint")+" · "+a.name))return;await api("POST","/api/legacy-assets/change",{id:a.id,expectedVersion:a.version,op:b.dataset.legacyOp});await load();await h.refresh();}catch(error){status(error.message);}
});
document.addEventListener("click",async e=>{
 if(e.target.closest("#offline-add")){edit();return;}
 const button=e.target.closest("[data-offline]");if(!button)return;const a=rows.find(a=>a.id===Number(button.dataset.id));if(!a)return;
 const h=home();if(h.sensitive())return;
 try{switch(button.dataset.offline){case "edit":edit(a);break;case "exit":exit(a);break;case "history":await history(a);break;default:if(!await h.confirm(t(button.dataset.offline==="restore"?"fundHomeOfflineRestoreHint":"fundHomeOfflineDeleteHint")+" · "+a.name))return;await api("POST",`/api/offline/assets/${button.dataset.offline}`,{id:a.id,expectedVersion:a.version,reason:"user"});await load();await h.refresh();}}
 catch(error){status(error.message);}
});
document.getElementById("offline-filter").onchange=e=>{type=e.target.value;page=1;load();};
document.getElementById("offline-deleted").onchange=e=>{includeDeleted=e.target.checked;page=1;load();};
document.getElementById("offline-prev").onclick=()=>{page--;load();};document.getElementById("offline-next").onclick=()=>{page++;load();};
document.getElementById("offline-retry").onclick=load;
document.addEventListener("fund:language-changed",render);document.addEventListener("fund:privacy-changed",()=>{cancelHistory();render();});
actions.loadAssets=load;
})();
