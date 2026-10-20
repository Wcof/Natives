(() => {
const { state, actions } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const C = trends.C;

const STORAGE_HISTORY = 'qs_history';
const STORAGE_SIGNALS = 'qs_signal_records';
const MAX_HISTORY = 30;
const MAX_SIGNAL_RECORDS = 200;
let _currentTab = 'watch';
let _panelOpen = false;
let overviewRequest=null,overviewGeneration=0,overviewRows=null;

function cancelOverview() { overviewRequest?.abort();overviewRequest=null;++overviewGeneration; }

// --- localStorage 读写 ---
function getWatchlist() { return (state.watchlistItems || []).map(item=>({...item,code:item.symbol})); }
function getHistory() {
  try { return JSON.parse(localStorage.getItem(STORAGE_HISTORY) || '[]'); } catch(e) { return []; }
}
function saveHistory(list) {
  try { localStorage.setItem(STORAGE_HISTORY, JSON.stringify(list)); } catch(e) {}
}

// --- 自选股操作 ---
let membershipBusy=false;
async function setMember(code,name,included) {
  if(membershipBusy)return;membershipBusy=true;
  const button=document.getElementById('star-btn');button.disabled=true;
  try {await actions.setWatchlistMember({symbol:code,name,assetType:Fund.researchSeries.kind(code)},included);}
  catch(error){document.getElementById('quote-bar').textContent=error.message || Fund.t('fundHomeRequestFailed');}
  finally {membershipBusy=false;button.disabled=false;updateStarButton(tState.currentSymbol);renderWatchlist();updateBadges();}
}
function toggleStar() {if(tState.currentSymbol)return setMember(tState.currentSymbol,tState.currentStockName,!getWatchlist().some(item=>item.code===tState.currentSymbol));}
function removeFromWatchlist(code) {return setMember(code,'',false);}
function updateStarButton(symbol) {
  const btn = document.getElementById('star-btn');
  if (!btn || !symbol) return;
  const inWatch = getWatchlist().some(s => s.code === symbol);
  btn.textContent = inWatch ? '★' : '☆';
  btn.classList.toggle('starred', inWatch);
  btn.title = Fund.t(inWatch ? 'fundTrendsWatchRemove' : 'fundLedgerWatchAdd');
  btn.setAttribute('aria-label',btn.title);
}

// --- 历史记录操作 ---
function addHistory(code, name, action, score) {
  let list = getHistory();
  list = list.filter(s => s.code !== code);
  list.unshift({ code, name, action, score, time: Date.now() });
  if (list.length > MAX_HISTORY) list = list.slice(0, MAX_HISTORY);
  saveHistory(list);
  if (_panelOpen && _currentTab === 'history') renderHistory();
  updateBadges();
}

function clearHistory() {
  saveHistory([]);
  renderHistory();
  updateBadges();
}

// --- 渲染 ---
function renderList(container, rows, removable) {
  const el=document.getElementById(container);if(!el)return;el.replaceChildren();
  if(!rows.length){const hint=document.createElement('p');hint.className='wp-empty';hint.textContent=Fund.t('fundLedgerNoResults');el.append(hint);return;}
  for(const item of rows){
    const row=document.createElement('div');row.className='wp-item';
    const open=document.createElement('button');open.type='button';open.className='wp-open';open.textContent=[item.code,item.name].filter(Boolean).join(' · ');
    open.onclick=()=>{trends.coordinator.selectStock(item.code,item.name,'',item.assetType);closePanel();};row.append(open);
    if(removable){const remove=document.createElement('button');remove.type='button';remove.className='remove';remove.textContent='×';remove.setAttribute('aria-label',Fund.t('fundHomeRemoveFavorite'));remove.onclick=()=>removeFromWatchlist(item.code);row.append(remove);}
    else {const time=document.createElement('span');time.className='time';time.textContent=fmtTime(item.time);row.append(time);}
    el.append(row);
  }
}
function renderWatchlist() {renderList('wp-content-watch',getWatchlist(),true);}
function renderHistory() {renderList('wp-content-history',getHistory(),false);}

function sigTag(action, score) {
  if (!action) return '<span class="sig-tag none">--</span>';
  const cls = action === '买入' ? 'buy' : action === '卖出' ? 'sell' : 'watch';
  return `<span class="sig-tag ${cls}">${action}${score ? ' ' + score : ''}</span>`;
}

function fmtTime(ts) {
  if (!ts) return '';
  const d = new Date(ts);
  const now = new Date();
  const diff = now - d;
  const locale=state.language==='en'?'en':'zh-CN',relative=new Intl.RelativeTimeFormat(locale,{numeric:'auto'});
  if (Math.abs(diff) < 60000) return relative.format(0,'second');
  if (Math.abs(diff) < 3600000) return relative.format(-Math.floor(diff / 60000),'minute');
  if (d.toDateString() === now.toDateString()) {
    return Fund.t('fundHomeToday')+' '+d.toLocaleTimeString(locale,{hour:'2-digit',minute:'2-digit'});
  }
  const yest = new Date(now); yest.setDate(yest.getDate() - 1);
  if (d.toDateString() === yest.toDateString()) return relative.format(-1,'day');
  return d.toLocaleDateString(locale,{month:'numeric',day:'numeric'});
}

function escHtml(s) {
  if (!s) return '';
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

// --- 面板控制 ---
function closePanel() {
  cancelOverview();
  const panel = document.getElementById('wp-panel');
  if (panel) panel.classList.remove('show');
  _panelOpen = false;
  const wb = document.getElementById('watch-btn');
  if (wb) {wb.classList.remove('active');wb.setAttribute('aria-expanded','false');}
  const hb = document.getElementById('history-btn');
  if (hb) {hb.classList.remove('active');hb.setAttribute('aria-expanded','false');}
}

function togglePanel(tab) {
  const panel = document.getElementById('wp-panel');
  if (!panel) return;
  if (_panelOpen && panel.classList.contains('show')) {
    if (_currentTab === tab) {
      closePanel();
      return;
    }
  } else {
    panel.classList.add('show');
    _panelOpen = true;
  }
  switchTab(tab);
}

function switchTab(tab) {
  if(tab!=='overview')cancelOverview();
  _currentTab = tab;
  document.querySelectorAll('.wp-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === tab);
  });
  const wcw = document.getElementById('wp-content-watch');
  if (wcw) wcw.style.display = tab === 'watch' ? 'block' : 'none';
  const wch = document.getElementById('wp-content-history');
  if (wch) wch.style.display = tab === 'history' ? 'block' : 'none';
  const wco = document.getElementById('wp-content-overview');
  if (wco) wco.style.display = tab === 'overview' ? 'block' : 'none';
  const wb = document.getElementById('watch-btn');
  if (wb) {wb.classList.toggle('active', tab === 'watch');wb.setAttribute('aria-expanded',String(_panelOpen));}
  const hb = document.getElementById('history-btn');
  if (hb) {hb.classList.toggle('active', tab === 'history');hb.setAttribute('aria-expanded',String(_panelOpen));}
  if (tab === 'watch') renderWatchlist();
  else if (tab === 'history') renderHistory();
  else if (tab === 'overview') loadOverview();
  const footer = document.getElementById('wp-footer');
  const list = tab === 'watch' ? getWatchlist() : tab === 'history' ? getHistory() : [];
  if (footer) {
    if (list.length > 0 && tab !== 'overview') {
      footer.style.display = 'flex';
      const fInfo = document.getElementById('wp-footer-info');
      if (fInfo) fInfo.textContent = tab === 'watch'
        ? Fund.t('fundTrendsWatchCount',{n:list.length})
        : Fund.t('fundTrendsHistoryCount',{n:list.length});
      const cBtn = document.getElementById('wp-clear-btn');
      if (cBtn) cBtn.textContent = Fund.t(tab === 'watch' ? 'fundTrendsClearWatch' : 'fundTrendsClearHistory');
    } else {
      footer.style.display = 'none';
    }
  }
  if (tab === 'overview') clearWatchChangeBadge();
}

async function clearCurrentTab() {
  if (_currentTab === 'watch') {
    if(!await Fund.home.confirm(Fund.t('fundTrendsClearWatchConfirm')))return;
    for(const item of getWatchlist())await setMember(item.code,item.name,false);
    renderWatchlist();
    updateStarButton(tState.currentSymbol);
  } else {
    clearHistory();
  }
  updateBadges();
  switchTab(_currentTab);
}

function updateBadges() {
  const wl = getWatchlist().length;
  const hl = getHistory().length;
  const wb = document.getElementById('watch-count');
  const hb = document.getElementById('history-count');
  const wt = document.getElementById('wt-count');
  const ht = document.getElementById('ht-count');
  const ov = document.getElementById('ov-count');
  if (wb) { wb.textContent = wl; wb.style.display = wl > 0 ? 'inline-block' : 'none'; }
  if (hb) { hb.textContent = hl; hb.style.display = hl > 0 ? 'inline-block' : 'none'; }
  if (wt) wt.textContent = wl > 0 ? `(${wl})` : '';
  if (ht) ht.textContent = hl > 0 ? `(${hl})` : '';
  if (ov) ov.textContent = wl > 0 ? `(${wl})` : '';
}

// ===== 多股一览 =====
async function loadOverview() {
  cancelOverview();overviewRows=null;
  const el=document.getElementById('wp-content-overview'),list=getWatchlist();
  if(!el)return;
  if(!list.length){renderOverview([]);return;}
  const request=new AbortController(),generation=overviewGeneration;
  overviewRequest=request;
  el.textContent=Fund.t('fundTrendsOverviewLoading');
  try {
    const supported=list.filter(row=>/^(sh|sz|bj)\d{6}$|^us[a-z\d]+$/i.test(row.code)),quotes=new Map();
    for(let start=0;start<supported.length;start+=60){
      const result=await Fund.api('GET','/api/market/quotes?codes='+encodeURIComponent(supported.slice(start,start+60).map(row=>row.code).join(',')),null,{signal:request.signal});
      if(request.signal.aborted||generation!==overviewGeneration)return;
      if(!Array.isArray(result.quotes))throw Error('QUOTES_RESPONSE');
      for(const quote of result.quotes)quotes.set(quote.code,quote);
    }
    if(request.signal.aborted||generation!==overviewGeneration)return;
    overviewRows=list.map(row=>({...row,name:quotes.get(row.code)?.name||row.name,pct:quotes.get(row.code)?.changePct??null}));
    renderOverview(overviewRows);
  }catch(error){
    if(request.signal.aborted||generation!==overviewGeneration)return;
    const message=document.createElement('p');message.setAttribute('role','alert');message.textContent=Fund.t('fundHomeRequestFailed');
    const retry=document.createElement('button');retry.type='button';retry.textContent=Fund.t('fundLedgerRetry');retry.onclick=loadOverview;el.replaceChildren(message,retry);
  }finally{if(overviewRequest===request)overviewRequest=null;}
}

function renderOverview(results) {
  const el=document.getElementById('wp-content-overview');if(!el)return;el.replaceChildren();
  if(!results.length){el.textContent=Fund.t('fundTrendsOverviewEmpty');return;}
  const sorted=[...results].sort((a,b)=>(Number.isFinite(b.pct)?b.pct:-Infinity)-(Number.isFinite(a.pct)?a.pct:-Infinity));
  for(const item of sorted){
    const row=document.createElement('button');row.type='button';row.className='wp-ov-item';
    row.onclick=()=>{trends.coordinator.selectStock(item.code,item.name,'',item.assetType);closePanel();document.getElementById('search-input')?.focus({preventScroll:true});};
    for(const [cls,text]of [['code',item.code],['name',item.name],['pct',Number.isFinite(item.pct)?(item.pct>0?'+':'')+item.pct.toFixed(2)+'%':'--'],['score',item.score??'--'],['sig',item.action?actionLabel(item.action):'--']]){
      const cell=document.createElement('span');cell.className='wp-ov-'+cls;cell.textContent=text;row.append(cell);
      if(cls==='pct'){cell.classList.add(item.pct>0?'up':item.pct<0?'down':'flat');if(!Number.isFinite(item.pct))cell.title=Fund.t('fundHomeOpportunityObsMissingPrice');}
      if(cls==='sig'){const tag=document.createElement('span');tag.className='wp-ov-sig-tag '+(!item.action?'none':item.action==='买入'?'buy':item.action==='卖出'?'sell':'watch');tag.textContent=text;cell.replaceChildren(tag);}
      if(cls==='score')cell.style.color=item.score>=60?C.up:item.score>=40?Fund.themeColor('--natives-warning'):item.score>0?C.down:Fund.themeColor('--natives-text-muted');
    }
    el.append(row);
  }
}

// ===== 信号准确率统计 =====
function getSignalRecords() {
  try { return JSON.parse(localStorage.getItem(STORAGE_SIGNALS) || '[]'); } catch(e) { return []; }
}
function saveSignalRecords(list) {
  try { localStorage.setItem(STORAGE_SIGNALS, JSON.stringify(list.slice(0, MAX_SIGNAL_RECORDS))); } catch(e) {}
}
function recordSignal(code, name, action, score, price) {
  const records = getSignalRecords();
  records.unshift({ code, name, action, score, price, time: Date.now() });
  saveSignalRecords(records);
}
function renderSignalAccuracy(code) {
  const card=document.getElementById('accuracy-card');if(!card)return;
  const records=getSignalRecords().filter(row=>row.code===code);card.style.display=records.length?'block':'none';
  const stats=document.getElementById('sa-stats');stats.textContent=Fund.t('fundTrendsSignalObservation',{n:records.length});
  document.getElementById('sa-detail').textContent=Fund.t('fundTrendsSignalNotBacktest');
}

// ===== 信号变更提醒 =====
function checkSignalChange(code, name, action, score, price) {
  const prev = tState.lastSignal[code];
  if (prev && prev.action && prev.action !== action) {
    showToast(name, code, prev.action, action, price);
    const watch = getWatchlist();
    if (watch.some(s => s.code === code)) {
      const badge = document.getElementById('watch-change');
      if (badge) {
        const count = parseInt(badge.textContent || '0') + 1;
        badge.textContent = count;
        badge.style.display = 'inline-block';
      }
    }
  }
  tState.lastSignal[code] = { action, score, price, time: Date.now() };
}

function showToast(name, code, oldAction, newAction, price) {
  const container = document.getElementById('toast-container');
  if (!container) return;
  const cls = newAction === '买入' ? 'buy' : newAction === '卖出' ? 'sell' : '';
  const oldColor = oldAction === '买入' ? C.up : oldAction === '卖出' ? C.down : Fund.themeColor('--natives-warning');
  const newColor = newAction === '买入' ? C.up : newAction === '卖出' ? C.down : Fund.themeColor('--natives-warning');
  const toast = document.createElement('div');
  toast.className = `toast ${cls}`;
  toast.onclick = () => removeToast(toast);
  toast.innerHTML = `
    <div style="font-weight:bold;font-size:14px;margin-bottom:4px">${escHtml(name)} (${code})</div>
    <div>${Fund.t('fundTrendsSignalChange')}: <span style="color:${oldColor}">${escHtml(actionLabel(oldAction))}</span> → <span style="color:${newColor};font-weight:bold">${escHtml(actionLabel(newAction))}</span></div>
    <div style="font-size:var(--natives-fs-secondary);color:var(--natives-text-muted);margin-top:4px">${Fund.t('fundTrendsCurrentPrice')} ${Number.isFinite(price) ? price.toFixed(2) : '--'}</div>
  `;
  container.appendChild(toast);
  setTimeout(() => removeToast(toast), 8000);
}

function actionLabel(action) {
  const key={'买入':'fundLedgerTypeBuy','卖出':'fundLedgerTypeSell','观望':'fundTrendsActionWatch','谨慎买入':'fundTrendsActionCautious','强烈买入':'fundTrendsActionStrongBuy'}[action];
  return key?Fund.t(key):action;
}

function removeToast(el) {
  if (!el || !el.parentNode) return;
  el.classList.add('removing');
  setTimeout(() => { if (el.parentNode) el.remove(); }, 300);
}

function clearWatchChangeBadge() {
  const badge = document.getElementById('watch-change');
  if (badge) {
    badge.textContent = '0';
    badge.style.display = 'none';
  }
}

// 扫描功能已拆分至独立的 trends-scan.js 模块
// 绑定全局与内联事件
window.removeFromWatchlist = removeFromWatchlist;
window.closePanel = closePanel;
window.toggleStar = toggleStar;
window.togglePanel = togglePanel;
window.switchTab = switchTab;
window.clearCurrentTab = clearCurrentTab;

document.addEventListener('fund:watchlist-changed',()=>{updateStarButton(tState.currentSymbol);updateBadges();if(_panelOpen)switchTab(_currentTab);});
document.addEventListener('fund:language-changed',()=>{updateStarButton(tState.currentSymbol);if(_panelOpen){if(_currentTab==='overview'&&overviewRows)renderOverview(overviewRows);else if(_currentTab!=='overview')switchTab(_currentTab);}document.getElementById('toast-container')?.replaceChildren();});
window.addEventListener('pagehide',closePanel);

const watchBtn = document.getElementById('watch-btn');
if (watchBtn) watchBtn.addEventListener('click', () => togglePanel('watch'));
const historyBtn = document.getElementById('history-btn');
if (historyBtn) historyBtn.addEventListener('click', () => togglePanel('history'));
for(const button of [watchBtn,historyBtn])if(button){button.setAttribute('aria-controls','wp-panel');button.setAttribute('aria-expanded','false');}
document.addEventListener('keydown',event=>{
  if(event.key!=='Escape'||!_panelOpen||document.querySelector('dialog[open]'))return;
  event.preventDefault();event.stopPropagation();const button=_currentTab==='history'?historyBtn:watchBtn;closePanel();button?.focus({preventScroll:true});
});
const starBtn = document.getElementById('star-btn');
if (starBtn) starBtn.addEventListener('click', toggleStar);
const clearBtn = document.getElementById('wp-clear-btn');
if (clearBtn) clearBtn.addEventListener('click', clearCurrentTab);
document.querySelectorAll('.wp-tab').forEach(t => {
  t.addEventListener('click', () => switchTab(t.dataset.tab));
});

document.addEventListener('click', e => {
  if (_panelOpen && !e.target.closest('.watch-wrap')) {
    closePanel();
  }
});

const wpPanel = document.getElementById('wp-panel');
if (wpPanel) wpPanel.addEventListener('click', e => e.stopPropagation());

trends.watch = {
  getWatchlist,
  getHistory,
  toggleStar,
  removeFromWatchlist,
  updateStarButton,
  addHistory,
  clearHistory,
  renderWatchlist,
  renderHistory,
  closePanel,
  togglePanel,
  switchTab,
  updateBadges,
  loadOverview,
  recordSignal,
  renderSignalAccuracy,
  checkSignalChange,
  openScan: (...args) => trends.scan?.openScan(...args),
  closeScan: (...args) => trends.scan?.closeScan(...args),
  startScan: (...args) => trends.scan?.startScan(...args),
  renderScanIdle: (...args) => trends.scan?.renderScanIdle(...args),
  analyzeFromScan: (...args) => trends.scan?.analyzeFromScan(...args),
};

})();
