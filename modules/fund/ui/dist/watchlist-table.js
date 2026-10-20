(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：自选分组与自选行情表。职责一句话——维护自选分组页签、
// 自选表（表头排序/涨速/闪烁/sparkline）与添加自选流程。
// 更新策略：行复用 + 属性级更新——轮询只改单元格 textContent/class，
// 不重建 DOM，页面零闪烁；仅成员增删时创建/移除对应行。
// import 方向：core（状态/api/主题取色）；跨域经 actions（app.selectSymbol 等）。

// ---------- 自选分组：顶部页签切换 ----------
const groupTabs = document.getElementById('watchlist-groups');
let watchlistProjection=null;
const selectedSymbols=new Set();
document.addEventListener('fund:language-changed',()=>{if(watchlistProjection)renderGroups(watchlistProjection.groups||[]);updateSelection();});
let groupRequest=null, quoteRequest=null, groupGeneration=0, quoteGeneration=0;
function watchlistStatus(key,retry=false) {
  const el=document.getElementById('watchlist-status');el.replaceChildren();
  if(key)el.append(document.createTextNode(Fund.t(key)));
  if(retry){const button=document.createElement('button');button.type='button';button.className='btn';button.textContent=Fund.t('fundLedgerRetry');button.onclick=refreshWatchlist;el.append(button);}
}
function renderGroups(groups) {
  const actual=[{id:'',name:Fund.t('fundHomeAll')},...(groups.length?groups:[{id:'default',name:Fund.t('fundWatchlistDefault')}])];
  if(!actual.some(group=>group.id===state.activeGroup))state.activeGroup='';
  const focused=groupTabs.contains(document.activeElement)?document.activeElement.dataset.id:null;
  const existing=new Map([...groupTabs.children].map(button=>[button.dataset.id,button]));
  for(const [index,group] of actual.entries()){
    const button=existing.get(group.id)||document.createElement('button');button.type='button';button.className='group-tab';button.dataset.id=group.id;button.textContent=group.name;button.setAttribute('role','tab');button.setAttribute('aria-selected',String(group.id===state.activeGroup));button.tabIndex=group.id===state.activeGroup?0:-1;button.classList.toggle('active',group.id===state.activeGroup);
    if(groupTabs.children[index]!==button)groupTabs.insertBefore(button,groupTabs.children[index]||null);
    existing.delete(group.id);
  }
  for(const button of existing.values())button.remove();
  if(focused!=null)groupTabs.querySelector(`[data-id="${CSS.escape(focused)}"]`)?.focus({preventScroll:true});
}
groupTabs.addEventListener('click',event=>{
  const button=event.target.closest('[data-id]');if(!button||button.dataset.id===state.activeGroup)return;
  state.activeGroup=button.dataset.id;resetRows();_lastQuoteSig=null;refreshWatchlist();
});
groupTabs.addEventListener('keydown',event=>{
 if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
 const buttons=[...groupTabs.querySelectorAll('button')],index=buttons.indexOf(document.activeElement);if(index<0)return;
 event.preventDefault();const next=event.key==='Home'?0:event.key==='End'?buttons.length-1:(index+(event.key==='ArrowLeft'?buttons.length-1:1))%buttons.length;
 buttons[next].click();buttons[next].focus();
});
document.getElementById('watchlist-create-group').onclick=()=>{
 const home=Fund.home;
 home.panel(()=>Fund.t('fundHomeAddGroup'),()=>`<form>${home.input('name',Fund.t('fundHomeName'),'','text','required maxlength="64"')}<button class="btn" type="submit">${Fund.t('fundHomeSave')}</button><p role="alert"></p></form>`,root=>{
  const form=root.querySelector('form');form.onsubmit=async event=>{
   event.preventDefault();const button=form.querySelector('button');if(button.disabled)return;button.disabled=true;
   try{const id=home.requestId();await api('POST','/api/watchlists',{id,name:form.elements.name.value.trim()});state.activeGroup=id;home.closePanel();refreshWatchlist();}
   catch(error){root.querySelector('[role=alert]').textContent=error.message;button.disabled=false;}
  };
 });
};
document.getElementById('watchlist-manage-group').onclick=()=>{
 const home=Fund.home, initial=watchlistProjection;if(!initial?.digest){watchlistStatus('fundHomeRequestFailed',true);return;}
 const managedGroup=state.activeGroup || 'default';
 let reviewed=initial,needsReview=false;
 const groupOptions=()=>reviewed.groups.map(group=>[group.id,group.name]);
 home.panel(()=>Fund.t('fundWatchManage'),()=>`<form>${home.select('op',Fund.t('fundWatchOperation'),['rename','delete','move','orderGroup','orderItem'].map(op=>[op,Fund.t('fundWatchManage'+op)]),'rename')}${home.select('id',Fund.t('fundWatchlistGroups'),groupOptions(),managedGroup)}${home.input('name',Fund.t('fundHomeName'),reviewed.groups.find(group=>group.id===managedGroup)?.name||'','text','maxlength="64"')}${home.select('target',Fund.t('fundWatchTarget'),groupOptions(),'')}${home.select('symbol',Fund.t('fundWatchInstrument'),reviewed.items.filter(item=>item.watchlistId===managedGroup).map(item=>[item.symbol,item.name+' · '+item.symbol]),'')}${home.input('sortOrder',Fund.t('fundWatchSortOrder'),0,'number','step="1"')}<p data-impact></p><button class="btn" type="submit">${Fund.t('fundHomeSave')}</button><button class="btn" type="button" data-review hidden>${Fund.t('fundWatchReview')}</button><p role="alert"></p></form>`,root=>{
  const form=root.querySelector('form'),review=root.querySelector('[data-review]');
  const update=()=>{
   const op=form.elements.op.value,id=form.elements.id.value;
   for(const [name,show] of [['name',op==='rename'],['target',op==='move'],['symbol',['move','orderItem'].includes(op)],['sortOrder',op.startsWith('order')]]){const field=form.elements[name];field.closest('label').hidden=!show;field.disabled=!show;}
   root.querySelector('[data-impact]').textContent=Fund.t(op==='delete'?'fundWatchDeleteImpact':'fundWatchMetadataOnly',{n:reviewed.items.filter(item=>item.watchlistId===id).length});
   review.hidden=!needsReview;
   form.querySelector('[type=submit]').disabled=needsReview || !reviewed.groups.some(group=>group.id===id) || (op==='delete'&&id==='default');
  };
  form.elements.op.onchange=update;
  form.elements.id.onchange=()=>{const id=form.elements.id.value;form.elements.name.value=reviewed.groups.find(group=>group.id===id)?.name||'';form.elements.symbol.replaceChildren(...reviewed.items.filter(item=>item.watchlistId===id).map(item=>{const option=document.createElement('option');option.value=item.symbol;option.textContent=item.name+' · '+item.symbol;return option;}));update();};
  review.onclick=async()=>{review.disabled=true;try{const latest=await api('GET','/api/watchlists');if(!root.isConnected)return;if(!latest.digest)throw Error(Fund.t('fundHomeRequestFailed'));reviewed=latest;for(const name of ['id','target']){const select=form.elements[name],chosen=select.value;select.replaceChildren(...latest.groups.map(group=>{const option=document.createElement('option');option.value=group.id;option.textContent=group.name;return option;}));select.value=chosen;}needsReview=false;review.hidden=true;root.querySelector('[role=alert]').textContent=Fund.t('fundWatchReviewed');update();}catch(error){root.querySelector('[role=alert]').textContent=error.message;}finally{review.disabled=false;}};
  form.onsubmit=async event=>{
   event.preventDefault();const submit=form.querySelector('[type=submit]');if(submit.disabled)return;const op=form.elements.op.value,id=form.elements.id.value;
   if(op==='delete'&&!await home.confirm(Fund.t('fundWatchDeleteImpact',{n:reviewed.items.filter(item=>item.watchlistId===id).length})))return;
   submit.disabled=true;
   try{await api('POST','/api/watchlists/manage',{op,id,expectedDigest:reviewed.digest,...(op==='rename'?{name:form.elements.name.value.trim()}:{}),...(op==='move'?{target:form.elements.target.value}:{}),...(['move','orderItem'].includes(op)?{symbol:form.elements.symbol.value}:{}),...(op.startsWith('order')?{sortOrder:Number(form.elements.sortOrder.value)}:{})});home.closePanel();await refreshWatchlist();}
   catch(error){if(!root.isConnected)return;root.querySelector('[role=alert]').textContent=error.message || Fund.t('fundHomeRequestFailed');if(error.status===409){needsReview=true;review.hidden=false;}update();}
  };update();
 });
};
document.getElementById('watchlist-detail-columns').addEventListener('change',event=>document.getElementById('watchlist-table').classList.toggle('show-detail-columns',event.target.checked));
actions.leaveWatchlist=()=>{groupRequest?.abort();quoteRequest?.abort();++groupGeneration;++quoteGeneration;};
window.addEventListener('pagehide',actions.leaveWatchlist);

function refreshWatchlist() {
  groupRequest?.abort();groupRequest=new AbortController();const generation=++groupGeneration;
  return api("GET", "/api/watchlists",null,{signal:groupRequest.signal}).then(function(res) {
    if(generation!==groupGeneration)return;
    watchlistProjection=res;renderGroups(res.groups||[]);watchlistStatus('');
    const allItems = res.items || [];
    // 空列表就是空：不填充演示标的，不引导强制输入（用户明确要求）。
    const selected=state.activeGroup?allItems.filter(it=>it.watchlistId===state.activeGroup):allItems;
    state.watchlistItems = [...new Map(selected.map(it=>[it.symbol,it])).values()];
    for(const symbol of selectedSymbols)if(!state.watchlistItems.some(it=>it.symbol===symbol))selectedSymbols.delete(symbol);
    rowIndex.forEach((tr,symbol)=>{const item=state.watchlistItems.find(item=>item.symbol===symbol);if(item)updateRowIdentity(tr,item);else{tr.remove();rowIndex.delete(symbol);}});
    updateSelection();
    renderWatchlistTable();
    actions.updateFavButtonState();
    actions.updateKPI();
    document.dispatchEvent(new Event("fund:watchlist-changed"));
    return true;
  }).catch(function(err) {
    if(generation!==groupGeneration||err.name==='AbortError')return;
    watchlistStatus('fundHomeRequestFailed',true);
  });
}
actions.setWatchlistMember = async function(item, included, group=state.activeGroup) {
  try {
    if(included)await api('POST','/api/watchlists/items',{watchlistId:group || 'default',symbol:item.symbol,name:item.name,assetType:item.assetType || Fund.researchSeries.kind(item.symbol)});
    else {
      const groups=group?[group]:[...new Set((watchlistProjection?.items||[]).filter(it=>it.symbol===item.symbol).map(it=>it.watchlistId))];
      for(const id of groups)await api('DELETE','/api/watchlists/items?watchlistId='+encodeURIComponent(id)+'&symbol='+encodeURIComponent(item.symbol));
    }
  } finally {await refreshWatchlist();}
};
actions.refreshWatchlist = refreshWatchlist;
actions.renderWatchlistTable = renderWatchlistTable;

function updateSelection(){
 const total=state.watchlistItems.length,count=selectedSymbols.size,all=document.getElementById('watchlist-select-all');
 all.disabled=!total;all.checked=!!total&&count===total;all.indeterminate=count>0&&count<total;
 document.getElementById('watchlist-selection-count').textContent=Fund.t('fundWatchSelectedCount',{n:count});
 for(const id of ['watchlist-move-selected','watchlist-clear-selected'])document.getElementById(id).disabled=!count;
 document.querySelectorAll('#watchlist-table [data-watch-select]').forEach(input=>{input.checked=selectedSymbols.has(input.dataset.watchSelect);const item=state.watchlistItems.find(it=>it.symbol===input.dataset.watchSelect);input.setAttribute('aria-label',Fund.t('fundWatchSelectInstrument',{name:(item?.name||input.dataset.watchSelect)+' · '+input.dataset.watchSelect}));});
}
document.getElementById('watchlist-select-all').onchange=event=>{selectedSymbols.clear();if(event.target.checked)state.watchlistItems.forEach(it=>selectedSymbols.add(it.symbol));updateSelection();};
document.getElementById('watchlist-clear-selected').onclick=()=>{selectedSymbols.clear();updateSelection();};
document.getElementById('watchlist-move-selected').onclick=()=>moveDialog([...selectedSymbols]);
function moveDialog(symbols){
 const home=Fund.home,esc=home.escape,source=state.activeGroup;
 let reviewed=watchlistProjection,needsReview=false,moving=false,loading=true,failed=false,target='',view,request,generation=0;
 const members=symbol=>(reviewed?.items||[]).filter(it=>it.symbol===symbol);
 const groupName=id=>reviewed?.groups.find(g=>g.id===id)?.name||id;
 const targets=()=>[['',Fund.t('fundWatchChooseTarget')],...(reviewed?.groups||[]).filter(g=>g.id!==source).map(g=>[g.id,g.name])];
 async function readLatest(){
  request?.abort();request=new AbortController();const seq=++generation;loading=true;failed=false;view.update();
  try{
   const latest=await api('GET','/api/watchlists',null,{signal:request.signal});
   if(!view.root.isConnected||seq!==generation)return;
   if(!latest.digest||!Array.isArray(latest.groups)||!Array.isArray(latest.items))throw Error(Fund.t('fundHomeRequestFailed'));
   reviewed=latest;needsReview=false;view.form.elements.target.replaceChildren(...targets().map(([id,name])=>{const option=document.createElement('option');option.value=id;option.textContent=name;return option;}));view.form.elements.target.value=target;view.renderItems();view.root.querySelector('[role=alert]').textContent='';
  }catch(error){if(seq===generation&&view.root.isConnected&&error.name!=='AbortError'){failed=true;view.root.querySelector('[role=alert]').textContent=error.message;}}
  finally{if(seq===generation&&view.root.isConnected){loading=false;view.update();}}
 }
 const panel=home.panel(()=>Fund.t('fundWatchMoveTitle'),()=>`<form data-watch-move><p>${esc(Fund.t(source?'fundWatchMoveGroupHint':'fundWatchMoveAllHint',{group:groupName(source)}))}</p><div data-move-items></div>${home.select('target',Fund.t('fundWatchTarget'),targets(),target)}<p data-move-impact role="status"></p><button class="btn" type="submit">${esc(Fund.t('fundWatchConfirmMove'))}</button><button class="btn" type="button" data-move-review hidden>${esc(Fund.t('fundWatchReview'))}</button><p role="alert"></p></form>`,root=>{
  const form=root.querySelector('form'),list=root.querySelector('[data-move-items]'),submit=form.querySelector('[type=submit]'),review=form.querySelector('[data-move-review]');
  function renderItems(){
   list.replaceChildren();
   for(const symbol of symbols){const row=document.createElement('div');row.className='watchlist-move-item';row.textContent=(members(symbol)[0]?.name||symbol)+' · '+symbol;list.append(row);}
  }
  function update(){
   let reason='';
   if(loading)reason='fundWatchGroupsLoading';
   else if(failed)reason='fundHomeRequestFailed';
   else if(symbols.length>200)reason='fundWatchMoveLimit';
   else if(symbols.some(symbol=>!members(symbol).some(it=>!source||it.watchlistId===source)))reason='fundWatchMoveMissing';
   else if(target&&target===source)reason='fundWatchMoveSame';
   else if(target&&!reviewed.groups.some(g=>g.id===target))reason='fundWatchMoveMissing';
   else if(target&&!source&&symbols.every(symbol=>members(symbol).every(it=>it.watchlistId===target)))reason='fundWatchAlreadyTarget';
   else if(targets().length===1)reason='fundWatchNoTarget';
   root.querySelector('[data-move-impact]').textContent=reason?Fund.t(reason):Fund.t('fundWatchMoveSelectionCount',{n:symbols.length,merged:target?symbols.filter(symbol=>members(symbol).some(it=>it.watchlistId===target)).length:0});
   submit.disabled=moving||loading||failed||needsReview||!target||!!reason;form.elements.target.disabled=moving||loading||failed;review.hidden=!(needsReview||failed);review.disabled=moving||loading;review.textContent=Fund.t(failed?'fundLedgerRetry':'fundWatchReview');
  }
  form.elements.target.onchange=()=>{target=form.elements.target.value;update();};
  review.onclick=readLatest;
  form.onsubmit=async event=>{
   event.preventDefault();if(moving||submit.disabled)return;moving=true;update();
   try{await api('POST','/api/watchlists/manage',{op:'moveSelected',source,target,symbols,expectedDigest:reviewed.digest});if(root.isConnected)home.closePanel();symbols.forEach(symbol=>selectedSymbols.delete(symbol));if(await refreshWatchlist())watchlistStatus('fundWatchMoved');if(!document.querySelector('dialog[open]')&&!document.activeElement?.closest('button,input'))groupTabs.querySelector('[aria-selected=true]')?.focus();}
   catch(error){if(!root.isConnected)return;root.querySelector('[role=alert]').textContent=error.message;moving=false;needsReview=error.status===409;view.update();}
  };
  view={root,form,renderItems,update};renderItems();update();
 },true);
 if(panel){panel.addEventListener('close',()=>{request?.abort();++generation;},{once:true});readLatest();}
}

// ---------- 表头排序（同花顺风格：首次点击降序，再点切换升序；字符串列升序优先） ----------
let sortState = { key: null, asc: false };
// 最近一次行情快照缓存：排序点击只重排内存数据，不重复请求（R-P4 页面级去重）
let lastQuoteRows = null;

document.querySelectorAll("#watchlist-table th.sortable").forEach(function(th) {
  const key=th.querySelector('[data-i18n]')?.dataset.i18n;const button=document.createElement('button');button.type='button';button.className='table-sort';if(key)button.dataset.i18n=key;button.textContent=key?Fund.t(key):th.textContent;th.replaceChildren(button);
  button.addEventListener("click", function() {
    const key = th.dataset.key;
    if (sortState.key === key) { sortState.asc = !sortState.asc; }
    else { sortState = { key: key, asc: ["symbol", "name"].indexOf(key) >= 0 }; }
    document.querySelectorAll("#watchlist-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    document.querySelectorAll('#watchlist-table th.sortable').forEach(cell=>cell.removeAttribute('aria-sort'));
    th.setAttribute('aria-sort',sortState.asc?'ascending':'descending');
    th.classList.add(sortState.asc ? "sort-asc" : "sort-desc");
    // 只重排内存快照，不重新请求行情（R-P4 页面级去重）
    if (lastQuoteRows) renderSortedRows(lastQuoteRows);
    else renderWatchlistTable();
  });
});

function sortKeyOf(item) {
  const q = item._q || {};
  switch (sortState.key) {
    case "symbol": return item.symbol;
    case "name": return item.name;
    case "price": return q.price;
    case "changePct": return q.changePct;
    case "change": return q.change;
    case "speed": return item._speed;
    case "volume": return q.volume;
    case "turnover": return q.turnover;
    case "amplitude": return q.amplitude;
    case "volumeRatio": return q.volumeRatio;
    default: return null;
  }
}

function sortRows(rows) {
  const sorted = rows.slice();
  if (sortState.key) {
    const dir = sortState.asc ? 1 : -1;
    sorted.sort(function(a, b) {
      const ka = sortKeyOf(a), kb = sortKeyOf(b);
      if (ka == null && kb == null) return 0;
      if (ka == null) return 1;
      if (kb == null) return -1;
      return ka < kb ? -dir : ka > kb ? dir : 0;
    });
  }
  return sorted;
}

// ---------- 行复用索引：symbol → <tr>（属性级更新的核心状态，唯一归属本模块） ----------
const rowIndex = new Map();

function resetRows() {
  selectedSymbols.clear();updateSelection();
  rowIndex.forEach(function(tr) { tr.remove(); });
  rowIndex.clear();
}

// ---------- 行情取数：拉报价 → 计算涨速 → 增量落表 ----------
function renderWatchlistTable() {
  const tbody = document.querySelector("#watchlist-table tbody");
  if (!tbody) return;
  quoteRequest?.abort();++quoteGeneration;
  const symbols = state.watchlistItems.map(function(i) { return i.symbol; }).join(",");
  if (!symbols) {
    resetRows(); lastQuoteRows = null;
    const row=document.createElement('tr'),cell=document.createElement('td');cell.colSpan=14;cell.textContent=Fund.t('fundWatchlistEmpty');row.append(cell);tbody.replaceChildren(row);
    return;
  }

  quoteRequest?.abort();quoteRequest=new AbortController();const generation=++quoteGeneration,group=state.activeGroup;
  const fundItems=state.watchlistItems.filter(item=>Fund.researchSeries.kind(item.symbol,'',item.assetType)==='fund');
  const quoted=state.watchlistItems.filter(item=>!fundItems.includes(item)).map(item=>item.symbol).join(',');
  const signal=quoteRequest.signal;
  Promise.all([quoted?api('GET','/api/market/quotes?codes='+encodeURIComponent(quoted),null,{signal}):Promise.resolve({quotes:[]}),Promise.allSettled(fundItems.map(item=>api('GET','/api/nav?code='+encodeURIComponent(item.symbol.replace(/^jj/,''))+'&limit=1',null,{signal})))]).then(function([res,navResults]) {
    let partial=false;
    navResults.forEach((result,index)=>{const item=fundItems[index];if(result.status==='fulfilled'){const row=result.value.nav?.[0];const valid=row && /^\d+(\.\d+)?$/.test(row.unitNav);res.quotes.push({code:item.symbol,price:valid?Number(row.unitNav):null,navExact:valid?row.unitNav:null,asOf:row?.navDate,source:row?.source});}else{partial=true;res.quotes.push({...item._q,code:item.symbol,stale:true});}});
    if(generation!==quoteGeneration||group!==state.activeGroup)return;
    watchlistStatus(partial?'fundHomeRequestFailed':'');
    const quotes = res.quotes || [];
    const quoteMap = {};
    quotes.forEach(function(q) { quoteMap[q.code] = q; });

    // 行情无变化时跳过更新：轮询期间数据没变就不触碰 DOM（二级守卫，
    // 即使数据变了也只做属性级更新，不会闪烁）。
    // 指纹包含自选成员列表：增删自选/切分组后签名必然变化，不会漏渲染。
    const sig = symbols + "#" + quotes.map(function(q) {
      return [q.code, q.price, q.changePct, q.volume, q.turnover, q.amplitude, q.volumeRatio, q.asOf, q.navExact, q.stale].join("|");
    }).join(";");
    if (sig === _lastQuoteSig) return;
    _lastQuoteSig = sig;

    // 涨速：与上次快照的涨跌幅差（近似 1 分钟涨速，同花顺口径）
    const rows = state.watchlistItems.map(function(item) {
      const q = quoteMap[item.symbol] || {};
      const prevPct = state.lastQuotesMap[item.symbol + ":pct"];
      if (q.changePct != null) state.lastQuotesMap[item.symbol + ":pct"] = q.changePct;
      item._q = q;
      item._speed = (q.changePct != null && prevPct != null) ? (q.changePct - prevPct) : null;
      return item;
    });
    // 缓存快照供排序点击复用：只重排内存数据，不重复请求（R-P4 页面级去重）
    lastQuoteRows = rows.slice();
    renderSortedRows(rows);
  }).catch(function(err) {
    if(generation!==quoteGeneration||err.name==='AbortError')return;
    _lastQuoteSig=null;watchlistStatus('fundHomeRequestFailed',true);
  });
}
// 上次行情指纹：与上次完全一致则跳过 DOM 更新
let _lastQuoteSig = null;

// 按当前 sortState 排序并增量落表（供取数路径与表头点击共用）
function renderSortedRows(rows) {
  const tbody = document.querySelector("#watchlist-table tbody");
  if (!tbody) return;
  const sorted = sortRows(rows);
  const seen = new Set();
  sorted.forEach(function(item) {
    let tr = rowIndex.get(item.symbol);
    if (!tr) { tr = buildRow(item); rowIndex.set(item.symbol, tr); }
    // 先入 DOM 再更新：sparkline 需按 id 找到已挂载的 canvas，
    // 闪烁动画的 reflow 重启也要求节点在文档中
    tbody.appendChild(tr);
    updateRow(tr, item);
    seen.add(item.symbol);
  });
  // 移除已删成员的行与历史错误/加载占位行
  rowIndex.forEach(function(tr, sym) {
    if (!seen.has(sym)) { tr.remove(); rowIndex.delete(sym); }
  });
  Array.from(tbody.children).forEach(function(tr) {
    if (!rowIndex.has(tr.dataset.symbol)) tr.remove();
  });
  actions.updateKPI();
}

// ---------- 行构建（每 symbol 仅一次；点击/删除事件绑定一次） ----------
function buildRow(item) {
  const tr = document.createElement("tr");
  tr.className = "clickable";
  tr.dataset.symbol = item.symbol;
  tr.dataset.name = item.name;
  tr.dataset.assetType = item.assetType || "";

  const tdSym = document.createElement("td");
  const b = document.createElement("b");
  b.textContent = item.symbol;
  tdSym.appendChild(b);
  const tdName = document.createElement("td");
  const checkbox=document.createElement('input');checkbox.type='checkbox';checkbox.dataset.watchSelect=item.symbol;checkbox.setAttribute('aria-label',Fund.t('fundWatchSelectInstrument',{name:item.name+' · '+item.symbol}));checkbox.onclick=event=>event.stopPropagation();checkbox.ondblclick=event=>event.stopPropagation();checkbox.onchange=()=>{if(checkbox.checked)selectedSymbols.add(item.symbol);else selectedSymbols.delete(item.symbol);updateSelection();};checkbox.checked=selectedSymbols.has(item.symbol);const selectLabel=document.createElement('label');selectLabel.className='watchlist-row-select';selectLabel.onclick=event=>event.stopPropagation();selectLabel.ondblclick=event=>event.stopPropagation();selectLabel.append(checkbox);tdName.append(selectLabel);
  const nameButton=document.createElement('button');nameButton.type='button';nameButton.className='table-sort';nameButton.dataset.watchName='';nameButton.textContent=item.name;nameButton.onclick=event=>{event.stopPropagation();actions.openStockDetail(tr.dataset.symbol,tr.dataset.name,tr.dataset.assetType);};tdName.append(nameButton);
  tr.append(tdSym, tdName);
  // 13 列：0代码 1名称 2价 3幅 4额 5速 6spark 7量 8换手 9振幅 10量比 11预警 12删除
  for (let i = 0; i < 11; i++) {
    const td = document.createElement("td");
    if (i === 4) {
      td.className = "spark-cell";
      const canvas = document.createElement("canvas");
      canvas.className = "sparkline";
      canvas.id = "spark-" + item.symbol;
      td.appendChild(canvas);
    } else if (i === 9) {
      // 预警列：颜色固定 accent，更新时只改 textContent
      td.style.color = "var(--natives-accent)";
    } else if (i === 10) {
      const move=document.createElement('button');move.type='button';move.className='btn';move.dataset.watchMove=item.symbol;move.dataset.i18n='fundWatchMove';move.textContent=Fund.t('fundWatchMove');move.onclick=event=>{event.stopPropagation();moveDialog([item.symbol]);};move.ondblclick=event=>event.stopPropagation();td.append(move);
      const del = document.createElement("button");
      del.className = "btn-del";
      const removeKey=state.activeGroup?'fundLedgerWatchRemove':'fundWatchRemoveAll';
      del.title = Fund.t(removeKey);
      del.dataset.i18nTitle = removeKey;
      del.dataset.symbol = item.symbol;
      del.textContent = "×";
      td.appendChild(del);
    } else {
      td.className = "num";
    }
    tr.appendChild(td);
  }

  const groupCell=document.createElement('td');groupCell.dataset.watchGroups='';tr.insertBefore(groupCell,tr.lastElementChild);
  const trend=document.createElement('button');trend.type='button';trend.className='btn';trend.dataset.i18n='fundLedgerNavTrends';trend.textContent=Fund.t('fundLedgerNavTrends');trend.onclick=e=>{e.stopPropagation();document.getElementById('tab-trends').click();actions.loadTrendsStock(tr.dataset.symbol,tr.dataset.name,tr.dataset.assetType);};tr.lastElementChild.prepend(trend);
  const news=document.createElement('button');news.type='button';news.className='btn';news.dataset.i18n='fundHomeNews';news.textContent=Fund.t('fundHomeNews');news.onclick=e=>{e.stopPropagation();const kind=Fund.researchSeries.kind(tr.dataset.symbol,'',tr.dataset.assetType);document.getElementById('tab-news').click();Fund.home.filterNewsAsset('security:CN:'+ (kind==='fund'?'fund:':'stock:') + (kind==='fund'?tr.dataset.symbol.replace(/^jj/,''):tr.dataset.symbol));};tr.lastElementChild.prepend(news);
  tr.addEventListener("click", function(e) {
    if (e.target.classList.contains("btn-del")) {
      e.stopPropagation();
      const button=e.target;if(button.disabled)return;button.disabled=true;
      actions.setWatchlistMember(item,false).catch(()=>{watchlistStatus('fundHomeRequestFailed');button.disabled=false;});
      return;
    }
    actions.selectSymbol(tr.dataset.symbol, tr.dataset.name, true);
  });
  tr.addEventListener("dblclick", function() {
    actions.openStockDetail(tr.dataset.symbol, tr.dataset.name, tr.dataset.assetType);
  });
  return tr;
}

// ---------- 行更新：只改变化的 textContent / class，不触碰 DOM 结构 ----------
function setNum(td, text, cls) {
  if (td.textContent !== text) td.textContent = text;
  const want = "num" + (cls ? " " + cls : "");
  if (td.className !== want) td.className = want;
}

function updateRowIdentity(tr,item){
 tr.dataset.name=item.name;tr.dataset.assetType=item.assetType||'';tr.querySelector('[data-watch-name]').textContent=item.name;const cell=tr.querySelector('[data-watch-groups]');if(cell)cell.textContent=[...new Set((watchlistProjection?.items||[]).filter(row=>row.symbol===item.symbol).map(row=>watchlistProjection.groups.find(g=>g.id===row.watchlistId)?.name||Fund.t('fundWatchlistDefault')))].join(' · ');
}
function updateRow(tr, item) {
  updateRowIdentity(tr,item);
  const q = item._q || {};
  const tds = tr.children;
  const cls = q.changePct > 0 ? "up" : q.changePct < 0 ? "down" : "";

  // 闪烁：价格相对上次快照变化时加 flash 类（CSS 动画，800ms 后移除）。
  // 行复用下需先移除类再强制 reflow 才能重启动画。
  const lastPrice = state.lastQuotesMap[item.symbol];
  if (lastPrice != null && q.price != null && q.price !== lastPrice) {
    tr.classList.remove("flash-up", "flash-down");
    void tr.offsetWidth;
    tr.classList.add(q.price > lastPrice ? "flash-up" : "flash-down");
    setTimeout(function() { tr.classList.remove("flash-up", "flash-down"); }, 800);
  }
  if (q.price != null) {
    state.lastQuotesMap[item.symbol] = q.price;
    // 报价流统一入口：预警引擎（dashboard.checkAlerts）随轮询驱动（审计 C1/C2）
    if (typeof actions.ingestQuoteTick === "function") {
      actions.ingestQuoteTick({ symbol: item.symbol, name: item.name, price: q.price, changePercent: q.changePct });
    }
  }
  if (q.changePct != null) item._lastChangePct = q.changePct;

  setNum(tds[2], q.navExact || (q.price != null ? q.price.toFixed(2) : '–'), cls);
  const date=document.createElement('small');date.className='watchlist-quote-date';date.textContent=[q.asOf || Fund.t('fundTrendsSourceUnknown'),q.stale?Fund.t('fundHomeRequestFailed'):''].filter(Boolean).join(' · ');tds[2].append(date);
  setNum(tds[3], q.changePct != null ? (q.changePct > 0 ? "+" : "") + q.changePct.toFixed(2) + "%" : "–", cls);
  setNum(tds[4], q.change != null ? (q.change > 0 ? "+" : "") + q.change.toFixed(2) : "–", cls);
  setNum(tds[5], item._speed != null ? (item._speed > 0 ? "+" : "") + item._speed.toFixed(2) : "–",
         item._speed > 0 ? "up" : item._speed < 0 ? "down" : "");
  if(Fund.researchSeries.kind(item.symbol,'',item.assetType)!=='fund')drawSparkline(item.symbol, (q.changePct || 0) >= 0);
  setNum(tds[7], q.volume ?? "–", null);
  setNum(tds[8], q.turnover != null ? q.turnover.toFixed(2) : "–", null);
  setNum(tds[9], q.amplitude != null ? q.amplitude.toFixed(2) : "–", null);
  setNum(tds[10], q.volumeRatio != null ? q.volumeRatio.toFixed(2) : "–", null);
  const hasAlert = state.alertRules[item.symbol] ? state.alertRules[item.symbol] + "%" : "–";
  if (tds[11].textContent !== hasAlert) tds[11].textContent = hasAlert;
  tr.classList.toggle("selected", item.symbol === state.currentSymbol);
}

// ---------- Sparkline（60px 原生 Canvas 微缩走势） ----------
// sparkline 数据缓存：同一标的 60s 内复用，避免每行每刷各拉一次 minute 打满上游。
const sparkCache = {};
function drawSparkline(symbol, isUp) {
  const canvas = document.getElementById("spark-" + symbol) || document.getElementById("m-spark-" + symbol);
  if (!canvas) return;
  canvas.width = 60;
  canvas.height = 20;
  const ctx = canvas.getContext("2d");
  const draw = function(pts) {
    if (!pts || pts.length < 2) return;
    const prices = pts.map(function(p) { return p.price; });
    const min = Math.min.apply(null, prices);
    const max = Math.max.apply(null, prices);
    const range = (max - min) || 1;

    ctx.clearRect(0, 0, 60, 20);
    ctx.beginPath();
    ctx.strokeStyle = isUp ? themeColor("--color-up") : themeColor("--color-down");
    ctx.lineWidth = 1.2;
    for (let i = 0; i < pts.length; i++) {
      const x = (i / (pts.length - 1)) * 58 + 1;
      const y = 19 - ((prices[i] - min) / range) * 16;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  };
  const cached = sparkCache[symbol];
  if (cached && Date.now() - cached.ts < 60000) { draw(cached.pts); return; }
  api("GET", "/api/market/minute?symbol=" + symbol).then(function(res) {
    const pts = res.points || [];
    sparkCache[symbol] = { ts: Date.now(), pts: pts };
    draw(pts);
  }).catch(function() {});
}

// ---------- 添加自选：经 /api/market/suggest 搜索确认（代码/名称/拼音均可） ----------
// 输入代码或名称后先查联想接口拿真实 symbol+name，再入自选；多命中时取第一个。
// 失败给出明确提示（此前静默失败导致"无法搜索"的表象）。
let suggestRequest=null;
async function addSymbolFromInput() {
  const input=document.getElementById('add-symbol-input'),msg=document.getElementById('add-symbol-msg');
  const code=input.value.trim();if(!code)return;
  suggestRequest?.abort();suggestRequest=new AbortController();const signal=suggestRequest.signal;
  msg.textContent=Fund.t('fundHomeLoading');
  try {
    const res=await api('GET','/api/market/suggest?input='+encodeURIComponent(code),null,{signal});
    if(signal.aborted||input.value.trim()!==code)return;
    const hits=(res.suggestions||[]).slice(0,50);
    if(!hits.length){msg.textContent=Fund.t('fundLedgerNoResults');return;}
    msg.textContent='';
    const home=Fund.home,group=state.activeGroup || 'default';
    home.panel(()=>Fund.t('fundLedgerWatchAdd'),()=>'<div data-watch-candidates></div><p role="alert"></p>',root=>{
      const list=root.querySelector('[data-watch-candidates]');
      for(const hit of hits){
        const button=document.createElement('button');button.type='button';button.className='btn';button.dataset.watchCandidate=hit.symbol;button.textContent=[hit.name,hit.symbol,hit.market,hit.type].filter(Boolean).join(' · ');list.append(button);
        button.onclick=async()=>{
          list.querySelectorAll('button').forEach(el=>el.disabled=true);
          try {await api('POST','/api/watchlists/items',{watchlistId:group,symbol:hit.symbol,name:hit.name,assetType:Fund.researchSeries.kind(hit.symbol,hit.market,hit.assetType)});if(input.value.trim()===code)input.value='';home.closePanel();refreshWatchlist();actions.selectSymbol(hit.symbol,hit.name);}
          catch(error){root.querySelector('[role=alert]').textContent=error.message;list.querySelectorAll('button').forEach(el=>el.disabled=false);}
        };
      }
    });
  } catch(error){if(!signal.aborted)msg.textContent=Fund.t('fundHomeRequestFailed');}
}
document.getElementById('add-symbol-input').addEventListener('input',()=>suggestRequest?.abort());

document.getElementById("add-symbol-btn").addEventListener("click", addSymbolFromInput);
document.getElementById("add-symbol-input").addEventListener("keydown", function(e) {
  if (e.key === "Enter") { e.preventDefault(); addSymbolFromInput(); }
});

})();
