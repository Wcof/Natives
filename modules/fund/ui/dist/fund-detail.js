// Private holding detail owns selection, bounded requests and return focus.
(() => {
const {actions,api,home,t}=Fund,esc=home.escape;
const $=id=>document.getElementById(id);
let holding=null,previousView='positions',previousFocus=null,previousScroll=0;
let tab='holdings',ranges={performance:'1m',profit:'1m'},generation=0,controller=null,minuteGeneration=0,profitGeneration=0;
let refreshBusy=false,lastRefresh=0;
let quoteBusy=false;
let managedTotal=null;
let holdingChoices=[];
let nav=[],quarter=null,quotes=new Map(),selected=null,minute=null,profits=null,profitKey='',tablePages={performance:0,profit:0};
const themeObserver=new MutationObserver(()=>{if(holding&&!document.hidden)redraw();});
const privateHidden=()=>home.displayMode()==='hidden',amountsHidden=()=>home.displayMode()!=='amount';
const text=(id,value)=>{if($(id))$(id).textContent=value;};
const today=()=>new Date(Date.now()+8*3600000).toISOString().slice(0,10);
function rangeStart(end,key){
 const d=new Date(end+'T00:00:00Z');if(key==='1d')return end;
 if(key==='7d'){d.setUTCDate(d.getUTCDate()-6);return d.toISOString().slice(0,10);}
 const months={ '1m':1,'3m':3,'6m':6,'1y':12 }[key]||1;
 const day=d.getUTCDate();d.setUTCDate(1);d.setUTCMonth(d.getUTCMonth()-months);
 d.setUTCDate(Math.min(day,new Date(Date.UTC(d.getUTCFullYear(),d.getUTCMonth()+1,0)).getUTCDate()));return d.toISOString().slice(0,10);
}
function money(value,signed=false){
 if(amountsHidden())return '****';if(value==null)return t('fundDetailMissing');
 try{const minor=Fund.decimalToMinor(value);return (signed&&minor>0n?'+':'')+Fund.formatMinorExact(minor);}catch{return t('fundDetailMissing');}
}
function percent(value){return value==null||!Number.isFinite(Number(value))?t('fundDetailMissing'):(Number(value)>0?'+':'')+Number(value).toFixed(2)+'%';}
function signed(id,value){text(id,money(value,true));const el=$(id);if(el)el.className='money '+(!amountsHidden()&&value!=null?(Number(value)>0?'up':Number(value)<0?'down':''):'');}
function renderBasic(){
 if(!holding)return;const h=holding,income=h.income||{},hidden=privateHidden();
 text('fd-fund-name',hidden?'****':h.fundName||h.fundCode);text('fd-fund-code',hidden?'****':h.fundCode);
 text('fd-fund-kind',t(h.venue==='exchange'?'fundLedgerTypeEtf':'fundLedgerTypeOtcFund'));
 const themeNode=$('fd-fund-themes');if(themeNode){themeNode.dataset.fundClassification=hidden?'':h.fundCode;Fund.fundClassification?.mount($('view-fund-detail'));}
 text('fd-fund-account',amountsHidden()?'****':`${t('fundLedgerThAccount')}：${h.account||h.accountId}`);
 text('fd-basic-asof',h.nav?.date||h.quote?.asOf||income.holding?.date||t('fundDetailDateUnknown'));
 const priceDate=h.venue==='exchange'?h.quote?.asOf?.slice(0,10):h.nav?.date;
 const confirmedDay=income.day?.date===today()&&income.day.amount!=null;
 const estimated=!confirmedDay&&priceDate!==today()&&h.estimate?.status==='estimated'&&h.estimate.date===today();
 const growth=priceDate===today()?h.dayPct:estimated?Number(h.estimate.rateBps)/100:null;
 text('fd-head-day-label',`${t(estimated?'fundDetailEstimatedChange':h.venue==='exchange'?'fundDetailMarketChange':'fundDetailNavChange')} · ${today()}`);
 text('fd-head-day-pct',hidden?'****':percent(growth));
 if($('fd-head-day-pct'))$('fd-head-day-pct').className=hidden?'':growth>0?'up':growth<0?'down':'';
 text('fd-val-amount',money(h.marketValue));text('fd-val-shares',amountsHidden()?'****':h.quantity??t('fundDetailMissing'));
 const total=managedTotal;
 let ratio=t('fundDetailMissing');
 if(!hidden&&h.marketValue!=null&&total!=null){try{const denominator=BigInt(total),value=Fund.decimalToMinor(h.marketValue);if(denominator>0n)ratio=Fund.formatMinorExact(value*10000n/denominator)+'%';}catch{}}
 text('fd-val-ratio',hidden?'****':ratio);signed('fd-val-profit',income.holding?.amount);
 text('fd-val-profit-rate',hidden?'****':percent(income.holding?.rateBps==null?null:Number(income.holding.rateBps)/100));
 text('fd-val-unit-cost',amountsHidden()?'****':h.costUnknown?t('fundDetailMissing'):h.averageCost??t('fundDetailMissing'));
 signed('fd-val-day-income',estimated?income.estimatedDay?.amount:income.day?.date===today()?income.day.amount:null);signed('fd-val-prev-income',income.yesterday?.date===new Date(Date.now()+8*3600000-86400000).toISOString().slice(0,10)?income.yesterday.amount:null);
 text('fd-label-day-income',`${t(estimated?'fundDetailEstimatedIncome':'fundDetailDayIncome')} · ${today()}`);
 $('fd-estimate-note').hidden=!estimated||hidden;$('fd-estimate-value').hidden=!estimated||hidden;
 if(estimated&&!hidden){text('fd-estimate-value',t('fundDetailEstimatedValue',{value:money(income.estimatedDay?.marketValue),nav:h.estimate.unitNav}));text('fd-estimate-note',t('fundDetailEstimateNote',{coverage:(Number(h.estimate.coverageBps)/100).toFixed(2),report:h.estimate.reportDate,time:h.estimate.asOf?new Date(h.estimate.asOf).toLocaleString(Fund.state.language,{timeZone:'Asia/Shanghai'}):t('fundDetailDateUnknown')}));}else{text('fd-estimate-value','');text('fd-estimate-note','');}
 text('fd-label-prev-income',`${t('fundDetailYesterdayIncome')} · ${income.yesterday?.date||t('fundDetailDateUnknown')}`);
 text('fd-val-days',hidden?'****':h.holdingDays==null?t('fundDetailMissing'):`${h.holdingDays} ${t('fundDetailDays')}`);
 renderYear();
 renderChoices();
}
function renderChoices(){const select=$('fd-holding-select');if(!select)return;select.replaceChildren(...holdingChoices.map(h=>{const option=document.createElement('option');option.value=`${h.accountId}:${h.fundId}`;option.textContent=privateHidden()?'****':`${h.fundName||h.fundCode} · ${amountsHidden()?'****':h.account||h.accountId}`;return option;}));select.value=holding?`${holding.accountId}:${holding.fundId}`:'';select.disabled=privateHidden()||!holdingChoices.length;}
async function loadChoices(){const gen=generation;try{const snap=await request('/api/positions/detail',gen);holdingChoices=snap.holdings.map(h=>({...h,account:snap.accounts.find(a=>a.id===h.accountId)?.name,dataRevision:snap.dataRevision}));renderChoices();}catch(error){if(error.name!=='AbortError')status('fd-basic-asof',error);}}
$('fd-holding-select')?.addEventListener('change',e=>{const h=holdingChoices.find(h=>`${h.accountId}:${h.fundId}`===e.target.value);if(!h)return;if(h.instrumentKind==='stock'){close();actions.openStockDetail(h.quoteSymbol,h.fundName,h.instrumentKind,h);}else open(h);});
function renderYear(){
 const last=nav.at(-1),start=last?rangeStart(last.navDate,'1y'):null,baseline=nav.filter(r=>r.navDate<=start).at(-1);
 const mixed=baseline&&nav.filter(row=>row.navDate>=baseline.navDate).some(row=>row.source!==last.source);
 text('fd-head-year-label',t('fundDetailYearNavChange'));
 text('fd-head-year-pct',privateHidden()?'****':mixed?t('fundDetailMixedSources'):baseline&&Number(baseline.unitNav)>0?percent((Number(last.unitNav)/Number(baseline.unitNav)-1)*100):t('fundDetailHistoryShort'));
}
function request(path,gen=generation){return api('GET',path,null,{signal:controller?.signal}).then(value=>{if(gen!==generation||!holding)throw new DOMException('Stale detail','AbortError');return value;}).catch(error=>{if(gen!==generation||!holding)throw new DOMException('Stale detail','AbortError');throw error;});}
function status(id,error){if(error?.name!=='AbortError')text(id,error?.message||t('fundDetailLoadFailed'));}
function reset(){generation++;minuteGeneration++;profitGeneration++;quoteBusy=false;controller?.abort();controller=new AbortController();Fund.holdingDetailCharts?.disposeAll();}
function leave(){themeObserver.disconnect();Fund.fundClassification?.cancel($('view-fund-detail'));reset();controller?.abort();controller=null;holding=null;holdingChoices=[];renderChoices();if($('view-fund-detail'))$('view-fund-detail').hidden=true;document.body.classList.remove('fund-detail-active');}
function close(){
 if(!holding)return;const focus=previousFocus,scroll=previousScroll,view=previousView;leave();
 const button=$('tab-'+view);if(button)button.click();else if($('view-'+view))$('view-'+view).hidden=false;
 const main=document.querySelector('.main-column');if(main)main.scrollTop=scroll;
 const target=focus?.isConnected?focus:focus?.id?$(focus.id):null;target?.focus({preventScroll:true});
}
function open(h){
 if(!h||!h.accountId||!h.fundId)return;
 if(!holding){previousFocus=document.activeElement;previousView=document.querySelector('.main-column > section:not([hidden])')?.id.replace('view-','')||'positions';previousScroll=document.querySelector('.main-column')?.scrollTop||0;}
 actions.leaveHome?.();actions.leavePositions?.();actions.pauseStockDetail?.();reset();refreshBusy=false;Fund.holdingSummary?.clear();holding={...h};themeObserver.observe(document.documentElement,{attributes:true,attributeFilter:['data-theme']});lastRefresh=Date.now();
 managedTotal=home.snapshot()?.dataRevision===h.dataRevision?home.snapshot()?.wealth?.totalA:null;
 tab='holdings';ranges={performance:'1m',profit:'1m'};nav=[];quarter=null;quotes=new Map();selected=null;minute=null;profits=null;profitKey='';tablePages={performance:0,profit:0};
 document.querySelectorAll('.main-column > section').forEach(s=>s.hidden=true);$('view-fund-detail').hidden=false;document.body.classList.add('fund-detail-active');
 Fund.applyI18nDom?.();for(const group of ['perf','profit'])document.querySelectorAll('#fd-'+group+'-ranges button').forEach(b=>{b.classList.toggle('active',b.dataset.range==='1m');b.setAttribute('aria-pressed',String(b.dataset.range==='1m'));});renderBasic();switchTab(tab);$('fd-fund-name')?.focus({preventScroll:true});
 holdingChoices=[holding];renderChoices();void loadChoices();void loadManagedTotal();void loadQuarter();void loadNav();
}
async function loadManagedTotal(){
 const gen=generation;
 try{const snapshot=await request('/api/home/snapshot',gen);managedTotal=snapshot.dataRevision===holding.dataRevision?snapshot.wealth?.totalA:null;renderBasic();}catch(error){if(error.name!=='AbortError')managedTotal=null;}
}
async function loadNav(){
 const gen=generation,code=holding?.fundCode;if(!code)return;text('fd-perf-chart-note',t('fundDetailLoading'));
 try{
  let rows=[],cursor=null;
  for(let page=0;page<4;page++){
   const response=await request(`/api/nav?code=${encodeURIComponent(code)}&limit=100${cursor?'&beforeDate='+encodeURIComponent(cursor):''}`,gen);
   if(!Array.isArray(response.nav))throw Error(t('fundDetailInvalidData'));
   rows.push(...response.nav);cursor=response.nextCursor;if(!cursor)break;
  }
  if(rows.some(r=>!/^\d{4}-\d{2}-\d{2}$/.test(r.navDate)||!Number.isFinite(Number(r.unitNav))||Number(r.unitNav)<0))throw Error(t('fundDetailInvalidData'));
  nav=[...new Map(rows.map(r=>[r.navDate,r])).values()].sort((a,b)=>a.navDate.localeCompare(b.navDate));
  const end=nav.at(-1)?.navDate;if(!end||nav[0].navDate>rangeStart(end,'1y')){try{const live=await request('/api/fund/nav-history?code='+encodeURIComponent(code),gen);if(Array.isArray(live.nav)&&live.nav.length)nav=[...new Map([...live.nav,...nav].map(r=>[r.navDate,r])).values()].sort((a,b)=>a.navDate.localeCompare(b.navDate));}catch(error){status('fd-perf-chart-note',error);}}
  renderYear();if(tab==='performance')await renderPerformance();
 }catch(error){status('fd-perf-chart-note',error);}
}
function symbol(stock){if(stock.assetType==='bond')return null;if(stock.quoteSymbol)return stock.quoteSymbol;const c=stock.code;if(/^\d{5}$/.test(c))return 'hk'+c;if(/^\d{6}$/.test(c))return (/^(?:[48]|92)/.test(c)?'bj':c.startsWith('6')?'sh':'sz')+c;return /^[A-Za-z][A-Za-z0-9.]{0,14}$/.test(c)?'us'+c:null;}
function renderQuarter(){
 const rows=[...(quarter?.stocks||[]),...(quarter?.bonds||[]).map(r=>({...r,assetType:'bond'}))];
 text('fd-quarterly-status',quarter?`${t('fundDetailQuarterDate')} ${quarter.report_date||t('fundDetailDateUnknown')} · ${t('fundDetailPreviousQuarter')} ${quarter.previous_report_date||t('fundDetailPreviousUnknown')} · ${t('fundDetailQuarterNote')}`:t('fundDetailLoading'));
 $('fd-quarterly-tbody').innerHTML=rows.map(s=>{
  const quote=quotes.get(symbol(s)),change=!quarter?.previous_report_date||s.previous_ratio_pct==null||s.change_pct==null||s.change_pct===''||s.change_type==='新进'?null:Number(s.change_pct);
  return `<tr class="${selected?.code===s.code?'selected':''}"><td><button type="button" class="fund-constituent" data-stock="${esc(s.code)}" aria-pressed="${selected?.code===s.code}">${esc(s.name)}</button><small>${esc(s.code)} · ${esc(s.industry||t('fundDetailUnclassified'))}</small></td><td class="num">${esc(percent(quote?.changePct))}<small>${esc(quote?.asOf||t('fundDetailDateUnknown'))}</small></td><td class="num">${s.ratio_pct==null||s.ratio_pct===''?esc(t('fundDetailMissing')):esc(s.ratio_pct)+'%'}</td><td class="num">${change==null||!Number.isFinite(change)?esc(t('fundDetailPreviousUnknown')):(change>0?'+':'')+change.toFixed(2)+' '+esc(t('fundDetailPercentagePoints'))}</td></tr>`;
 }).join('')||`<tr><td colspan="4">${esc(t('fundDetailNoQuarter'))}</td></tr>`;
 $('fd-quarterly-tbody').querySelectorAll('[data-stock]').forEach(b=>b.onclick=()=>{selected=rows.find(s=>s.code===b.dataset.stock);renderQuarter();void loadMinute();});
}
async function loadQuarter(){
 const gen=generation,code=holding?.fundCode;if(!code)return;
 try{
  quarter=await request('/api/fund/quarterly-positions?code='+encodeURIComponent(code),gen);selected=quarter.stocks?.[0]||(quarter.bonds?.[0]?{...quarter.bonds[0],assetType:'bond'}:null);renderQuarter();
  await loadQuarterQuotes();
  if(tab==='holdings'&&selected)await loadMinute();
 }catch(error){if(error.name!=='AbortError'){$('fd-quarterly-tbody').innerHTML=`<tr><td colspan="4">${esc(error.message)}</td></tr>`;status('fd-quarterly-status',error);}}
}
async function loadQuarterQuotes(){
 if(!quarter||quoteBusy||document.hidden)return;const gen=generation;quoteBusy=true;
 const symbols=[...new Set((quarter.stocks||[]).map(symbol).filter(Boolean))];
 try{const loaded=new Map();for(let offset=0;offset<symbols.length;offset+=60){
  const response=await request('/api/market/quotes?codes='+encodeURIComponent(symbols.slice(offset,offset+60).join(',')),gen);
  for(const quote of response.quotes||[])loaded.set(quote.code,quote);
 }if(gen===generation){quotes=loaded;renderQuarter();}}
 catch(error){status('fd-quarterly-status',error);}
 finally{if(gen===generation)quoteBusy=false;}
}
async function loadMinute(){
 const gen=generation,own=++minuteGeneration,stock=selected,target=stock&&symbol(stock);minute=null;Fund.holdingDetailCharts.clearMinute();text('fd-selected-stock-title',stock?.name||'');if(!target){text('fd-selected-stock-asof',t('fundDetailNoMinute'));return;}
 text('fd-selected-stock-title',stock.name);text('fd-selected-stock-asof',t('fundDetailLoading'));
 try{
  const response=await request('/api/market/minute?symbol='+encodeURIComponent(target),gen);if(own!==minuteGeneration)return;
  minute=response;text('fd-selected-stock-asof',response.date||t('fundDetailDateUnknown'));if(tab!=='holdings')return;
  await Fund.holdingDetailCharts.renderMinuteChart($('fd-stock-minute-chart'),response.points||[],response.preClose);
 }catch(error){if(own===minuteGeneration){Fund.holdingDetailCharts.clearMinute();status('fd-selected-stock-asof',error);}}
}
function table(section,rows){
 const page=tablePages[section],shown=rows.slice().reverse().slice(page*100,(page+1)*100);
 const body=$(section==='performance'?'fd-perf-tbody':'fd-profit-tbody');
 body.innerHTML=shown.map(r=>{
  if(section==='profit')return `<tr><td>${esc(r.date)}</td><td class="num">${r.amount==null?esc(t('fundDetailMissing')):esc(money(r.amount,true))}</td></tr>`;
  const i=nav.indexOf(r),previous=nav[i-1],change=r.dailyGrowthPct!=null?Number(r.dailyGrowthPct):previous&&previous.source===r.source&&Number(previous.unitNav)>0?(Number(r.unitNav)/Number(previous.unitNav)-1)*100:null;
  return `<tr><td>${esc(r.navDate)}</td><td class="num">${esc(r.unitNav)}</td><td class="num">${esc(percent(change))}</td></tr>`;
 }).join('')||`<tr><td colspan="${section==='performance'?3:2}">${esc(t('fundDetailNoRecords'))}</td></tr>`;
 const pager=$('fd-'+(section==='performance'?'perf':'profit')+'-pager');if(pager){pager.hidden=rows.length<=100;pager.innerHTML=`<button type="button" data-page="-1" ${page===0?'disabled':''}>${esc(t('fundHomePrevious'))}</button><span>${page+1} / ${Math.max(1,Math.ceil(rows.length/100))}</span><button type="button" data-page="1" ${(page+1)*100>=rows.length?'disabled':''}>${esc(t('fundHomeNext'))}</button>`;pager.querySelectorAll('button').forEach(b=>b.onclick=()=>{tablePages[section]+=Number(b.dataset.page);table(section,rows);});}
}
async function renderPerformance(){
 const end=nav.at(-1)?.navDate,from=end&&rangeStart(end,ranges.performance),rows=from?nav.filter(r=>r.navDate>=from):[];
 table('performance',rows);text('fd-perf-chart-note',end?`${from} -- ${end}${nav[0]?.navDate>from?' · '+t('fundDetailHistoryShort'):''}`:t('fundDetailNoRecords'));
 const last=rows.at(-1),index=nav.indexOf(last),previous=nav[index-1],growth=last?.dailyGrowthPct??(previous&&previous.source===last?.source&&Number(previous.unitNav)>0?(Number(last.unitNav)/Number(previous.unitNav)-1)*100:null);
 $('fd-perf-summary').innerHTML=`<div><small>${esc(t('fundDetailNav'))}</small><strong>${esc(last?.unitNav||t('fundDetailMissing'))}</strong></div><div><small>${esc(t('fundDetailGrowth'))}</small><strong>${esc(percent(growth))}</strong></div><small>${rows.length} · ${esc(t('fundDetailRecordCount'))}</small>`;
 try{await Fund.holdingDetailCharts.renderPerfChart($('fd-perf-chart'),rows);}catch(error){status('fd-perf-chart-note',error);}
}
async function renderProfit(){
 const h=holding;if(!h)return;const gen=generation,own=++profitGeneration;
 if(amountsHidden()){$('fd-profit-summary').textContent=t('fundDetailHidden');$('fd-profit-tbody').innerHTML=`<tr><td colspan="2">${esc(t('fundDetailHidden'))}</td></tr>`;await Fund.holdingDetailCharts.renderProfitChart($('fd-profit-chart'),[],true);return;}
 const to=today(),from=rangeStart(to,ranges.profit),key=[h.accountId,h.fundId,from,to,h.dataRevision].join(':');
 text('fd-profit-chart-note',t('fundDetailLoading'));
 try{
  if(profitKey!==key){
   const response=await request(`/api/holding/income-history?accountId=${h.accountId}&fundId=${h.fundId}&from=${from}&to=${to}&expectedRevision=${encodeURIComponent(h.dataRevision)}`,gen);
   if(own!==profitGeneration||amountsHidden())return;profits=response;profitKey=key;
  }
  if(own!==profitGeneration||amountsHidden())return;
  const last=profits.rows?.at(-1);$('fd-profit-summary').innerHTML=`<div><small>${esc(t('fundDetailDayIncome'))} · ${esc(last?.date||t('fundDetailDateUnknown'))}</small><strong>${esc(money(last?.amount,true))}</strong></div><div><small>${esc(t('fundDetailProfit'))}</small><strong>${esc(money(holding.income?.holding?.amount,true))}</strong></div><small>${profits.rows?.length||0} · ${esc(t('fundDetailRecordCount'))}</small>`;
  table('profit',profits.rows||[]);if(tab!=='profit')return;if(profits.status==='unavailable'){text('fd-profit-chart-note',t('fundDetailCurrencyUnavailable'));await Fund.holdingDetailCharts.renderProfitChart($('fd-profit-chart'),[],false);return;}text('fd-profit-chart-note',`${from} -- ${to}${profits.status==='partial'?' · '+t('fundDetailPartial'):''}`);
  await Fund.holdingDetailCharts.renderProfitChart($('fd-profit-chart'),profits.rows||[],false);
 }catch(error){if(own===profitGeneration){status('fd-profit-chart-note',error);if(error.name!=='AbortError')$('fd-profit-tbody').innerHTML=`<tr><td colspan="2">${esc(error.message)}</td></tr>`;}}
}
function switchTab(next){
 if(!['holdings','performance','profit'].includes(next))return;tab=next;
 document.querySelectorAll('.fund-section-tab').forEach(b=>{const active=b.dataset.tab===tab;b.classList.toggle('active',active);b.setAttribute('aria-selected',String(active));b.tabIndex=active?0:-1;});
 document.querySelectorAll('.fund-tab-panel').forEach(p=>p.hidden=p.id!=='fd-panel-'+tab);
 if(tab==='performance')void renderPerformance();if(tab==='profit')void renderProfit();if(tab==='holdings'&&selected)void loadMinute();
}
async function refresh(manual=true){
 if(!holding||refreshBusy||document.hidden)return;refreshBusy=true;
 const gen=generation,identity=[holding.accountId,holding.fundId],previousNavDate=holding.nav?.date;
 try{
  if(holding.venue==='otc')await Fund.syncHoldingNav?.([{fundCode:holding.fundCode,navDate:holding.nav?.date}],controller?.signal,manual);
  const snap=await request('/api/positions/detail'+(holding.venue==='exchange'?'?refresh=1&kind=etf':''),gen);
  const h=snap.holdings.find(x=>x.accountId===identity[0]&&x.fundId===identity[1]);if(!h)throw Error(t('fundDetailHoldingGone'));
  managedTotal=null;holding={...h,account:snap.accounts.find(a=>a.id===h.accountId)?.name,dataRevision:snap.dataRevision};profitKey='';lastRefresh=Date.now();renderBasic();
  void loadManagedTotal();
  if(manual||previousNavDate!==h.nav?.date)void loadNav();if(manual)void loadQuarter();if(tab==='profit')void renderProfit();
 }catch(error){status('fd-basic-asof',error);}finally{if(gen===generation)refreshBusy=false;}
}
function tick(context){
 if(!holding||document.hidden)return;
 if(context.open&&tab==='holdings')void loadQuarterQuotes();
 if(context.manual||context.open||holding.venue==='exchange'||holding.income?.day?.amount==null||holding.income?.day?.date!==today()||Date.now()-lastRefresh>=15*60*1000)void refresh(Boolean(context.manual)).then(()=>{if(context.open&&holding&&tab==='holdings'&&selected)void loadMinute();});
 else if(context.open&&tab==='holdings'&&selected&&!refreshBusy)void loadMinute();
}

function redraw(){if(!holding||$('view-fund-detail').hidden)return;Fund.applyI18nDom?.();renderBasic();renderQuarter();if(tab==='performance')void renderPerformance();if(tab==='profit')void renderProfit();if(tab==='holdings'&&minute)void Fund.holdingDetailCharts.renderMinuteChart($('fd-stock-minute-chart'),minute.points||[],minute.preClose).catch(e=>status('fd-selected-stock-asof',e));}
document.addEventListener('DOMContentLoaded',()=>{
 $('fd-btn-back')?.addEventListener('click',close);$('fd-btn-refresh')?.addEventListener('click',()=>void refresh());
 document.querySelectorAll('.fund-section-tab').forEach(b=>b.addEventListener('click',()=>switchTab(b.dataset.tab)));
 document.querySelector('.fund-section-tabs')?.addEventListener('keydown',e=>{if(e.metaKey||e.ctrlKey||e.altKey||!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;const buttons=[...document.querySelectorAll('.fund-section-tab')],i=buttons.indexOf(document.activeElement);if(i<0)return;e.preventDefault();const j=e.key==='Home'?0:e.key==='End'?buttons.length-1:(i+(e.key==='ArrowRight'?1:buttons.length-1))%buttons.length;buttons[j].click();buttons[j].focus();});
 for(const section of ['perf','profit'])document.querySelectorAll('#fd-'+section+'-ranges .fund-range-btn').forEach(b=>b.addEventListener('click',()=>{const key=section==='perf'?'performance':'profit';ranges[key]=b.dataset.range;tablePages[key]=0;document.querySelectorAll('#fd-'+section+'-ranges button').forEach(x=>{x.classList.toggle('active',x===b);x.setAttribute('aria-pressed',String(x===b));});if(key==='performance')void renderPerformance();else void renderProfit();}));
 $('fd-btn-edit')?.addEventListener('click',()=>{if(holding){if(holding.venue==='otc')Fund.otcOps.holdingEdit(holding);else actions.openHoldingEditor({...holding,returnFocusId:'fd-btn-edit'});}});
 $('fd-btn-buy')?.addEventListener('click',()=>{if(holding){if(holding.venue==='otc')Fund.otcOps.buy(holding);else actions.openTxDialog(holding,'BUY');}});
 $('fd-btn-sell')?.addEventListener('click',()=>{if(holding){if(holding.venue==='otc')Fund.otcOps.sell(holding);else actions.openTxDialog(holding,'SELL');}});
 $('fd-btn-more')?.addEventListener('click',()=>{if(holding)actions.openHoldingMore(holding,$('fd-btn-more'));});
});
window.addEventListener('resize',()=>Fund.holdingDetailCharts.resizeAll());window.addEventListener('pagehide',leave);
document.addEventListener('visibilitychange',()=>{if(!holding)return;if(document.hidden){reset();controller?.abort();}else{controller=new AbortController();if(holding.income?.day?.date!==today())void refresh(false);redraw();if(!quarter)void loadQuarter();if(!nav.length)void loadNav();}});
for(const name of ['fund:privacy-changed','fund:language-changed','fund:financial-display-changed'])document.addEventListener(name,redraw);
Fund.openFundDetail=open;Fund.closeFundDetail=close;Fund.leaveFundDetail=leave;Fund.tickFundDetail=tick;
Fund.refreshFundDetail=()=>refresh(false);
})();
