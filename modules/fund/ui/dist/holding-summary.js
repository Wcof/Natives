// Account-bound private summary in the public stock research surface.
(() => {
const {home,actions,api,t}=Fund,esc=home.escape;
let selected=null,generation=0,controller=null,currentTab='overview',recordsData=null,managedTotal=null,managedController=null;
const money=value=>home.displayMode()!=='amount'?'****':value==null?t('fundDetailMissing'):Fund.formatMinorExact(Fund.decimalToMinor(value));
function render(){
 const box=document.getElementById('sd-holding-context');if(!box)return;box.hidden=!selected;if(!selected){box.replaceChildren();return;}
 const h=selected;
 const metric=(key,value)=>`<section class="card"><small>${esc(t(key))}</small><strong class="holding-amount">${value}</strong>${key==='fundDetailProfit'?`<small>${home.displayMode()==='hidden'?'****':h.income?.holding?.rateBps==null?esc(t('fundDetailMissing')):(Number(h.income.holding.rateBps)>0?'+':'')+(Number(h.income.holding.rateBps)/100).toFixed(2)+'%'}</small>`:''}</section>`;
 const total=managedTotal??(home.snapshot()?.dataRevision===h.dataRevision?home.snapshot()?.wealth?.totalA:null);
 let ratio=t('fundDetailMissing');if(home.displayMode()!=='hidden'&&total!=null&&h.marketValue!=null&&BigInt(total)>0n)ratio=Fund.formatMinorExact(Fund.decimalToMinor(h.marketValue)*10000n/BigInt(total))+'%';
 box.innerHTML=`<p class="muted">${home.displayMode()!=='amount'?'****':esc(h.account||h.accountId)}</p><div class="stock-holding-summary">${metric('fundDetailAmount',esc(money(h.marketValue)))}${metric('fundLedgerThCost',esc(money(h.costUnknown?null:h.cost)))}${metric('fundDetailProfit',esc(money(h.income?.holding?.amount)))}${metric('fundDetailManagedWeight',home.displayMode()==='hidden'?'****':esc(ratio))}</div><div class="stock-holding-tabs" role="tablist">${[['overview','fundDetailStockHolding'],['records','fundDetailHoldingRecords'],['information','fundHomeNews'],['research','fundLedgerNavTrends']].map(([key,label])=>`<button type="button" id="sd-holding-tab-${key}" role="tab" ${key==='records'?'data-holding-records':''} data-holding-tab="${key}" aria-controls="${key==='research'?'sd-holding-research':'sd-holding-panel'}" aria-selected="${key===currentTab}" tabindex="${key===currentTab?0:-1}">${esc(t(label))}</button>`).join('')}<button type="button" data-holding-more>${esc(t('fundDetailMore'))}</button><button type="button" data-holding-refresh>${esc(t('fundDetailRefresh'))}</button></div><div class="card" id="sd-holding-panel" role="tabpanel" aria-labelledby="sd-holding-tab-${currentTab}"><dl class="metric-list">${[[h.instrumentKind==='stock'?'fundDetailStockShares':'fundDetailShares',home.displayMode()!=='amount'?'****':h.quantity],[h.instrumentKind==='stock'?'fundDetailStockUnitCost':'fundDetailUnitCost',home.displayMode()!=='amount'?'****':h.averageCost],['fundDetailDayIncome',money(h.income?.day?.amount)],['fundDetailYesterdayIncome',money(h.income?.yesterday?.amount)]].map(([key,value])=>`<div><dt>${esc(t(key))}${key==='fundDetailDayIncome'?`<small>${esc(h.income?.day?.date||t('fundDetailDateUnknown'))}</small>`:key==='fundDetailYesterdayIncome'?`<small>${esc(h.income?.yesterday?.date||t('fundDetailDateUnknown'))}</small>`:''}</dt><dd>${esc(value??t('fundDetailMissing'))}</dd></div>`).join('')}</dl><p>${esc(h.income?.day?.date||t('fundDetailDateUnknown'))}</p></div>`;
 box.querySelector('[data-holding-more]').onclick=e=>actions.openHoldingMore(h,e.currentTarget);
 box.querySelector('[data-holding-refresh]').onclick=()=>void refresh();
 const overview=box.querySelector('#sd-holding-panel').innerHTML;
 const tabs=[...box.querySelectorAll('[data-holding-tab]')];
 tabs.forEach(button=>button.onclick=()=>{
  tabs.forEach(tab=>{tab.setAttribute('aria-selected',String(tab===button));tab.tabIndex=tab===button?0:-1;});
  controller?.abort();generation++;currentTab=button.dataset.holdingTab;document.getElementById('view-stock-detail').dataset.holdingTab=currentTab;
  const panel=box.querySelector('#sd-holding-panel');panel.hidden=button.dataset.holdingTab==='research';panel.setAttribute('aria-labelledby','sd-holding-tab-'+currentTab);
  if(button.dataset.holdingTab==='overview'){panel.innerHTML=overview;return;}
  if(button.dataset.holdingTab==='records')void records(true);
  if(button.dataset.holdingTab==='information')void information();
  if(button.dataset.holdingTab==='research')actions.resizeStockDetailCharts?.();
 });
 box.querySelector('.stock-holding-tabs').onkeydown=e=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;const i=tabs.indexOf(document.activeElement);if(i<0)return;e.preventDefault();const j=e.key==='Home'?0:e.key==='End'?tabs.length-1:(i+(e.key==='ArrowLeft'?tabs.length-1:1))%tabs.length;tabs[j].click();tabs[j].focus();};
 box.querySelector('#sd-holding-panel').hidden=currentTab==='research';
 if(currentTab==='records')void records(true);if(currentTab==='information')void information();

}
function set(holding){controller?.abort();managedController?.abort();generation++;recordsData=null;managedTotal=null;selected=holding?.accountId&&holding?.fundId?{...holding}:null;const view=document.getElementById('view-stock-detail');view.classList.toggle('stock-holding-active',Boolean(selected));currentTab='overview';view.dataset.holdingTab=currentTab;render();if(selected)void loadManagedTotal();}
async function loadManagedTotal(){
 const h=selected,own=generation;if(!h?.dataRevision)return;managedController?.abort();const owned=managedController=new AbortController();
 try{const snapshot=await api('GET','/api/home/snapshot',null,{signal:owned.signal});if(owned.signal.aborted||selected?.accountId!==h.accountId||selected?.fundId!==h.fundId||selected?.dataRevision!==h.dataRevision)return;managedTotal=snapshot.dataRevision===h.dataRevision?snapshot.wealth?.totalA:null;const weight=document.querySelector('#sd-holding-context .stock-holding-summary > section:last-child strong');if(weight)weight.textContent=home.displayMode()==='hidden'?'****':managedTotal!=null&&h.marketValue!=null&&BigInt(managedTotal)>0n?Fund.formatMinorExact(Fund.decimalToMinor(h.marketValue)*10000n/BigInt(managedTotal))+'%':t('fundDetailMissing');}catch(e){if(own===generation)managedTotal=null;}finally{if(managedController===owned)managedController=null;}
}
async function refresh(){
 if(!selected)return;const own=++generation,identity=[selected.accountId,selected.fundId];controller?.abort();controller=new AbortController();
 try{const snapshot=await api('GET','/api/positions/detail?refresh=1&kind=stock',null,{signal:controller.signal});if(own!==generation)return;const h=snapshot.holdings.find(h=>h.accountId===identity[0]&&h.fundId===identity[1]);if(!h)throw Error(t('fundDetailHoldingGone'));selected={...h,account:snapshot.accounts.find(a=>a.id===h.accountId)?.name,dataRevision:snapshot.dataRevision};recordsData=null;managedTotal=null;render();void loadManagedTotal();}catch(error){if(error.name!=='AbortError'&&own===generation)home.status(error.message);}
}
async function information(){
 const root=document.getElementById('sd-holding-panel');if(!selected||home.privacy()){root.textContent=t('fundHomeAnalysisHiddenText');return;}
 const h=selected,own=generation;controller?.abort();controller=new AbortController();root.textContent=t('fundHomeLoading');
 try{const data=await api('GET','/api/information/items?scope=holdings&asset='+encodeURIComponent('security:CN:stock:'+h.fundCode),null,{signal:controller.signal});if(own!==generation||currentTab!=='information'||home.privacy())return;
 root.innerHTML=(data.items||[]).map(item=>`<article class="info-item"><h3>${esc(item.title)}</h3><small>${esc(item.sourceId)} · ${esc(item.publishedAt||t('fundHomeSourceTimestampUnknown'))}</small><p>${esc(item.summary||t('fundHomeContentNotRetrieved'))}</p><button type="button" data-info="${item.id}">${esc(t('fundHomeOpenSource'))}</button><button type="button" data-ai="${item.id}">${esc(t('fundHomeNewsAiBtn'))}</button></article>`).join('')||`<p>${esc(t('fundHomeInformationEmpty'))}</p>`;
 root.querySelectorAll('[data-info]').forEach(b=>b.onclick=()=>home.openInformationDetail(Number(b.dataset.info)));
 root.querySelectorAll('[data-ai]').forEach(b=>b.onclick=()=>{const item=data.items.find(i=>i.id===Number(b.dataset.ai));document.getElementById('tab-home').click();home.selectNewsForAI(item);});
 }catch(e){if(own===generation&&e.name!=='AbortError'){root.innerHTML=`<p>${esc(t('fundHomeInformationCouldNotLoad'))}</p><button data-retry>${esc(t('fundLedgerRetry'))}</button>`;root.querySelector('[data-retry]').onclick=information;}}
}
async function records(inline=false){
 if(!selected||home.sensitive()){if(inline)document.getElementById('sd-holding-panel').textContent=t('fundHomeAnalysisHiddenText');return;}const h=selected,own=generation;controller?.abort();controller=new AbortController();
 if(inline)document.getElementById('sd-holding-panel').textContent=t('fundHomeLoading');
 try{let data=inline&&recordsData?recordsData:await api('GET',`/api/holding/records?accountId=${h.accountId}&fundId=${h.fundId}&page=1`,null,{signal:controller.signal});if(own!==generation||inline&&currentTab!=='records')return;if(inline)recordsData=data;
 const markup=()=>`<p>${esc(h.account||h.accountId)} · ${esc(h.fundName)}</p>${data.opening?`<p>${esc(t('fundHomeOpeningHistory'))}: ${esc(data.opening.asOf||'')} · ${esc(data.opening.quantity)} · ${esc(money(data.opening.totalCost))}</p>`:''}<div class="table-wrap"><table class="data-table"><thead><tr>${['fundDetailDate','fundHomeMyAction','fundDetailShares','fundDetailTradePrice','fundLedgerFeeAmount','fundDetailRecordState'].map(key=>`<th>${esc(t(key))}</th>`).join('')}</tr></thead><tbody>${(data.transactions||[]).map(tx=>`<tr><td>${esc(tx.tradeDate)}</td><td>${esc(t(tx.type==='BUY'?'fundLedgerBuy':tx.type==='SELL'?'fundLedgerSell':'fundHomeSettingsUnknown'))}</td><td>${esc(tx.quantity)}</td><td>${esc(tx.price)}</td><td>${esc(money(tx.fee))}</td><td>${esc(t(tx.state==='confirmed'?'fundDetailTradeConfirmed':'fundDetailTradePending'))}</td></tr>`).join('')||`<tr><td colspan="6">${esc(t('fundDetailNoRecords'))}</td></tr>`}</tbody></table></div><nav><button type="button" data-record-dir="prev" ${data.page===1?'disabled':''}>${esc(t('fundHomePrevious'))}</button><span>${data.page}</span><button type="button" data-record-dir="next" ${data.hasMore?'':'disabled'}>${esc(t('fundHomeNext'))}</button></nav><p role="status" data-record-error></p>`;
 const bind=root=>{
  const requests=new Set();root.addEventListener('close',()=>requests.forEach(request=>request.abort()),{once:true});
  const bind=()=>root.querySelectorAll('[data-record-dir]').forEach(button=>button.onclick=async()=>{
   const direction=button.dataset.recordDir,page=data.page+(direction==='next'?1:-1),focus=document.activeElement;controller?.abort();const requestController=controller=new AbortController();requests.add(requestController);
   root.querySelectorAll('[data-record-dir]').forEach(node=>node.disabled=true);
   try{const next=await api('GET',`/api/holding/records?accountId=${h.accountId}&fundId=${h.fundId}&page=${page}`,null,{signal:requestController.signal});
    if((inline&&own!==generation)||requestController.signal.aborted||selected?.accountId!==h.accountId||selected?.fundId!==h.fundId||!root.isConnected||home.privacy())return;
    if(next.dataRevision!==data.dataRevision)throw Error(t('fundDetailRecordsChanged'));
    data=next;if(inline)recordsData=data;root.querySelector('[data-record-page]').innerHTML=markup();bind();if(!focus.isConnected){const preferred=root.querySelector(`[data-record-dir="${direction}"]`);(preferred&&!preferred.disabled?preferred:root.querySelector("[data-record-dir]:not([disabled])"))?.focus({preventScroll:true});}
   }catch(error){if(error.name!=='AbortError'&&root.isConnected)root.querySelector('[data-record-error]').textContent=error.message;}
   finally{requests.delete(requestController);if(root.isConnected){root.querySelector('[data-record-dir=prev]').disabled=data.page===1;root.querySelector('[data-record-dir=next]').disabled=!data.hasMore;}}
  });bind();
 };
 if(inline){const root=document.getElementById('sd-holding-panel');root.innerHTML=`<div data-record-page>${markup()}</div>`;bind(root);}else home.panel(()=>t('fundDetailHoldingRecords'),()=>`<div data-record-page>${markup()}</div>`,bind);
 }catch(error){if(error.name!=='AbortError'&&own===generation){if(inline){const root=document.getElementById('sd-holding-panel');root.innerHTML=`<p role="alert">${esc(t('fundDetailLoadFailed'))}</p><button type="button" data-record-retry>${esc(t('fundLedgerRetry'))}</button>`;root.querySelector('[data-record-retry]').onclick=()=>void records(true);}else home.status(error.message);}}
}
for(const event of ['fund:privacy-changed','fund:language-changed'])document.addEventListener(event,()=>{const focus=document.activeElement?.dataset;const target=focus?.recordDir?`[data-record-dir="${focus.recordDir}"]`:focus?.holdingTab?`[data-holding-tab="${focus.holdingTab}"]`:null;controller?.abort();managedController?.abort();generation++;render();if(target)document.querySelector('#sd-holding-context '+target)?.focus({preventScroll:true});});
window.addEventListener('pagehide',()=>set(null));
Fund.holdingSummary={set,clear:()=>set(null),refresh};
})();
