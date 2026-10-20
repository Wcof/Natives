(() => {
const { state, actions, api, t } = globalThis.Fund;
// Fund 领域模块：热门板块与成分股联动表。职责一句话——维护板块分类页签、
// 板块行情表（排序/选中）与板块成分股表（排序/选中联动标的切换）。
// 自选分组与自选行情表已按职责拆分至 watchlist-table.js。
// import 方向：core（状态/api）；跨域经 actions（app.selectSymbol 等）。

// ---------- 热门板块与成分股联动模块（去除分钟K线，上下双表联动） ----------
let currentSectorType = "all";
let currentSelectedSector = null;
let lastSectorsData = [];
let lastStocksData = [];
let sectorSort = { key: "changePct", asc: false };
let stockSort = { key: "changePct", asc: false };
let sectorGeneration = 0, stocksGeneration = 0, sectorRequest = null, stocksRequest = null;
const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
function compare(a,b,key,asc) {
  const av=a[key],bv=b[key];
  if(av==null)return bv==null?0:1;
  if(bv==null)return -1;
  return (typeof av==='string'?av.localeCompare(String(bv)):av-bv)*(asc?1:-1);
}
function rowStatus(tbody,key,columns) {
 const tr=document.createElement('tr'),td=document.createElement('td');
 td.colSpan=columns;td.className='investment-row-empty';td.textContent=t(key);tr.append(td);tbody.replaceChildren(tr);
}
function leaveMarket(){sectorGeneration++;stocksGeneration++;sectorRequest?.abort();stocksRequest?.abort();}
actions.leaveMarket=leaveMarket;
window.addEventListener('pagehide',leaveMarket);

const sTabs = document.querySelectorAll("#sector-category-tabs .group-tab");
sTabs.forEach(function(tab) {
  tab.addEventListener("click", function() {
    sTabs.forEach(function(t) { t.classList.remove("active"); });
    tab.classList.add("active");
    currentSectorType = tab.dataset.type;
    refreshSectors();
  });
});

function refreshSectors() {
  const tbody = document.querySelector("#sectors-table tbody");
  if (!tbody) return;
  sectorRequest?.abort();const request=new AbortController(),generation=++sectorGeneration;
  sectorRequest=request;
  if(!lastSectorsData.length)rowStatus(tbody,'fundHomeLoading',9);
  if(!lastSectorsData.length)document.getElementById('market-candidates').textContent=t('fundHomeLoading');

  api("GET", "/api/market/sectors?type=" + currentSectorType,null,{signal:request.signal}).then(function(res) {
    if(generation!==sectorGeneration)return;
    lastSectorsData = res.sectors || res.quotes || [];
    document.getElementById('market-status').textContent='';
    renderSectorsTable();
  }).catch(function(err) {
    if(generation!==sectorGeneration||request.signal.aborted)return;
    if(!lastSectorsData.length)rowStatus(tbody,'fundMarketReadFailed',9);
    document.getElementById('market-status').textContent=t('fundMarketReadFailed');
    if(!lastSectorsData.length)document.getElementById('market-candidates').textContent=t('fundMarketReadFailed');
  });
}
actions.refreshSectors = refreshSectors;
actions.refreshMarket = refreshSectors;
document.addEventListener('fund:language-changed',()=>{renderSectorsTable();renderSectorStocksTable();if(currentSelectedSector)document.getElementById('sector-stocks-title').textContent=currentSelectedSector.name+' · '+t('fundMarketConstituents',{n:lastStocksData.length});});

function renderSectorsTable() {
  const tbody = document.querySelector("#sectors-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "";

  const items = lastSectorsData.slice();
  if (sectorSort.key) {
    items.sort((a,b)=>compare(a,b,sectorSort.key,sectorSort.asc));
  }
  const cards=document.getElementById('market-candidates');
  cards.innerHTML=items.slice(0,6).map(s=>`<article class="card market-candidate"><div class="between"><h3>${escape(s.name)}</h3><span class="tag">${escape(t('fundMarketResearchObject'))}</span></div><small>${escape(s.asOf||t('fundHomeSourceTimestampUnknown'))}</small><div class="holding-amount ${s.changePct>0?'up':s.changePct<0?'down':''}">${s.changePct==null?'–':(s.changePct>0?'+':'')+s.changePct.toFixed(2)+'%'}</div><dl class="metric-list"><div><dt>${escape(t('fundMarketUiTopGainer'))}</dt><dd>${escape(s.topStockName||'–')}</dd></div><div><dt>${escape(t('fundMarketFiveDay'))}</dt><dd>${s.changePct5==null?'–':s.changePct5.toFixed(2)+'%'}</dd></div><div><dt>${escape(t('fundMarketTwentyDay'))}</dt><dd>${s.changePct20==null?'–':s.changePct20.toFixed(2)+'%'}</dd></div></dl><p class="investment-card-tip">${escape(t('fundMarketResearchScope'))}</p><button type="button" data-sector-code="${escape(s.code)}" aria-pressed="${currentSelectedSector?.code===s.code}">${escape(t('fundMarketConstituentAction'))}</button></article>`).join('')||`<p>${escape(t('fundMarketNoSectors'))}</p>`;
  cards.querySelectorAll('[data-sector-code]').forEach(button=>button.onclick=()=>{const sector=items.find(s=>s.code===button.dataset.sectorCode);if(sector){selectSector(sector);document.getElementById('sector-stocks-title').scrollIntoView({block:'nearest'});}});

  if (items.length === 0) {
    rowStatus(tbody,'fundMarketNoSectors',9);
    return;
  }

  items.forEach(function(s, idx) {
    const tr = document.createElement("tr");
    tr.className = "clickable";
    if (currentSelectedSector && currentSelectedSector.code === s.code) {
      tr.classList.add("selected-sector");
    }

    const pct = s.changePct != null ? (s.changePct > 0 ? "+" : "") + s.changePct.toFixed(2) + "%" : "–";
    const pctCls = s.changePct > 0 ? "up" : s.changePct < 0 ? "down" : "";
    const speed = s.speed != null ? s.speed.toFixed(2) : "–";
    const speedCls = s.speed > 0 ? "up" : s.speed < 0 ? "down" : "";
    const topName = s.topStockName || "–";
    const topPrice = s.topStockPrice != null ? s.topStockPrice.toFixed(2) : "–";
    const topPct = s.topStockChangePct != null ? (s.topStockChangePct > 0 ? "+" : "") + s.topStockChangePct.toFixed(2) + "%" : "–";
    const topCls = s.topStockChangePct > 0 ? "up" : s.topStockChangePct < 0 ? "down" : "";
    const pct5 = s.changePct5 != null ? (s.changePct5 > 0 ? "+" : "") + s.changePct5.toFixed(2) + "%" : "–";
    const pct5Cls = s.changePct5 > 0 ? "up" : s.changePct5 < 0 ? "down" : "";
    const pct20 = s.changePct20 != null ? (s.changePct20 > 0 ? "+" : "") + s.changePct20.toFixed(2) + "%" : "–";
    const pct20Cls = s.changePct20 > 0 ? "up" : s.changePct20 < 0 ? "down" : "";

    tr.innerHTML = "<td style='color:var(--natives-text-muted);'>" + (idx + 1) + "</td>" +
                   "<td><button type='button' class='sector-select'>" + escape(s.name) + "</button></td>" +
                   "<td class='num " + pctCls + "'>" + pct + "</td>" +
                   "<td class='num " + speedCls + "'>" + speed + "</td>" +
                   "<td>" + escape(topName) + "</td>" +
                   "<td class='num " + topCls + "'>" + topPrice + "</td>" +
                   "<td class='num " + topCls + "'>" + topPct + "</td>" +
                   "<td class='num " + pct5Cls + "'>" + pct5 + "</td>" +
                   "<td class='num " + pct20Cls + "'>" + pct20 + "</td>";

    tr.addEventListener("click", function() {
      selectSector(s);
    });

    tbody.appendChild(tr);
  });

  // 首次默认选中第 1 个板块
  if (!currentSelectedSector && items.length > 0) {
    selectSector(items[0]);
  }
}

function selectSector(s) {
  currentSelectedSector = s;
  document.querySelectorAll('[data-sector-code]').forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.sectorCode===s.code)));
  document.querySelectorAll("#sectors-table tbody tr").forEach(function(r) {
    r.classList.remove("selected-sector");
  });
  renderSectorsTableSelection();
  updateSectorSidebar(s);
  loadSectorStocks(s);
}

function updateSectorSidebar(sector) {
  const s = sector || currentSelectedSector;
  if (!s) return;
  const nameEl = document.getElementById("side-sector-name");
  const codeEl = document.getElementById("side-sector-code");
  const badgeEl = document.getElementById("side-sector-type-badge");
  const pctEl = document.getElementById("side-sector-pct");
  const speedEl = document.getElementById("side-sector-speed");
  const pct5El = document.getElementById("side-sector-pct5");
  const pct20El = document.getElementById("side-sector-pct20");
  const leaderNameEl = document.getElementById("side-sector-leader-name");
  const leaderCodeEl = document.getElementById("side-sector-leader-code");
  const leaderPriceEl = document.getElementById("side-sector-leader-price");
  const leaderPctEl = document.getElementById("side-sector-leader-pct");

  if (nameEl) nameEl.textContent = s.name || "–";
  if (codeEl) codeEl.textContent = s.code || "–";
  if (badgeEl) {
    const typeMap = { concept: "概念板块", industry: "行业板块", region: "地域板块", all: "综合板块" };
    badgeEl.textContent = typeMap[currentSectorType] || "板块";
  }
  if (pctEl) {
    const chg = s.changePct != null ? (s.changePct > 0 ? "+" : "") + s.changePct.toFixed(2) + "%" : "–";
    pctEl.textContent = chg;
    pctEl.className = "price-large " + (s.changePct > 0 ? "up" : s.changePct < 0 ? "down" : "");
  }
  if (speedEl) {
    speedEl.textContent = s.speed != null ? (s.speed > 0 ? "+" : "") + s.speed.toFixed(2) + "%" : "–";
    speedEl.className = "num " + (s.speed > 0 ? "up" : s.speed < 0 ? "down" : "");
  }
  if (pct5El) {
    pct5El.textContent = s.changePct5 != null ? (s.changePct5 > 0 ? "+" : "") + s.changePct5.toFixed(2) + "%" : "–";
    pct5El.className = "num " + (s.changePct5 > 0 ? "up" : s.changePct5 < 0 ? "down" : "");
  }
  if (pct20El) {
    pct20El.textContent = s.changePct20 != null ? (s.changePct20 > 0 ? "+" : "") + s.changePct20.toFixed(2) + "%" : "–";
    pct20El.className = "num " + (s.changePct20 > 0 ? "up" : s.changePct20 < 0 ? "down" : "");
  }
  if (leaderNameEl) leaderNameEl.textContent = s.topStockName || "–";
  if (leaderCodeEl) leaderCodeEl.textContent = s.topStockCode || "–";
  if (leaderPriceEl) leaderPriceEl.textContent = s.topStockPrice != null ? s.topStockPrice.toFixed(2) : "–";
  if (leaderPctEl) {
    leaderPctEl.textContent = s.topStockChangePct != null ? (s.topStockChangePct > 0 ? "+" : "") + s.topStockChangePct.toFixed(2) + "%" : "–";
    leaderPctEl.className = "num " + (s.topStockChangePct > 0 ? "up" : s.topStockChangePct < 0 ? "down" : "");
  }

  // 计算多空分布与力量比
  if (lastStocksData && lastStocksData.length > 0) {
    let up = 0, down = 0, flat = 0;
    lastStocksData.forEach(function(st) {
      if (st.changePct > 0) up++;
      else if (st.changePct < 0) down++;
      else if (st.changePct === 0) flat++;
    });
    const total = up + down + flat;
    const ratio = total > 0 ? Math.round((up / total) * 100) : null;

    const upEl = document.getElementById("side-sector-up-count");
    const downEl = document.getElementById("side-sector-down-count");
    const flatEl = document.getElementById("side-sector-flat-count");
    const ratioEl = document.getElementById("side-sector-bull-ratio");
    const fillEl = document.getElementById("side-sector-power-fill");
    const totalEl = document.getElementById("side-sector-total-stocks");

    if (upEl) upEl.textContent = up;
    if (downEl) downEl.textContent = down;
    if (flatEl) flatEl.textContent = flat;
    if (ratioEl) ratioEl.textContent = ratio == null ? "–" : ratio + "%";
    if (fillEl) fillEl.style.width = (ratio || 0) + "%";
    if (totalEl) totalEl.textContent = total;
  }
}
actions.updateSectorSidebar = updateSectorSidebar;

function updateSectorStockDetail(symbol, name) {
  const container = document.getElementById("side-sector-stock-content");
  if (!container) return;
  const st = (lastStocksData || []).find(function(it) { return it.code === symbol; });
  const displayName = name || (st && st.name) || symbol;
  const price = st && st.price != null ? st.price.toFixed(2) : "–";
  const pct = st && st.changePct != null ? (st.changePct > 0 ? "+" : "") + st.changePct.toFixed(2) + "%" : "–";
  const isUp = st && st.changePct >= 0;

  const isFav = state.watchlistItems.some(function(i) { return i.symbol === symbol; });

  container.innerHTML =
    "<div style='display:flex; justify-content:space-between; align-items:flex-start; margin-bottom:8px;'>" +
      "<div>" +
        "<div style='font-size:14px; font-weight:600;'>" + escape(displayName) + " <span style='font-size:var(--natives-fs-secondary); color:var(--natives-text-muted);'>" + escape(symbol) + "</span></div>" +
        "<div style='font-size:18px; font-weight:bold; margin-top:2px;' class='" + (isUp ? "up" : "down") + "'>" + price + " <span style='font-size:12px;'>" + pct + "</span></div>" +
      "</div>" +
      "<button class='btn " + (!isFav ? "btn-accent" : "") + "' id='btn-sector-stock-fav' style='padding:3px 8px; font-size:var(--natives-fs-secondary);'>" +
        (isFav ? "已自选" : "+ 加自选") +
      "</button>" +
    "</div>" +
    "<div style='display:grid; grid-template-columns:1fr 1fr; gap:6px; font-size:var(--natives-fs-secondary); color:var(--natives-text-muted); padding:6px 0; border-top:1px solid var(--natives-border);'>" +
      "<div>成交量: <span class='num' style='color:var(--natives-text-primary);'>" + (st && st.volume ? (st.volume > 10000 ? (st.volume / 10000).toFixed(1) + "万手" : st.volume + "手") : "–") + "</span></div>" +
      "<div>换手率: <span class='num' style='color:var(--natives-text-primary);'>" + (st && st.turnover != null ? st.turnover.toFixed(2) + "%" : "–") + "</span></div>" +
      "<div>振幅: <span class='num' style='color:var(--natives-text-primary);'>" + (st && st.amplitude != null ? st.amplitude.toFixed(2) + "%" : "–") + "</span></div>" +
      "<div>量比: <span class='num' style='color:var(--natives-text-primary);'>" + (st && st.volumeRatio != null ? st.volumeRatio.toFixed(2) : "–") + "</span></div>" +
    "</div>" +
    "<div style='margin-top:8px; text-align:right;'>" +
      "<button class='btn' id='btn-sector-stock-analyze' style='padding:3px 8px; font-size:var(--natives-fs-secondary);'>转到趋势分析 →</button>" +
    "</div>";

  const favBtn = document.getElementById("btn-sector-stock-fav");
  if (favBtn) {
    favBtn.addEventListener("click", function() {
      const nowFav = state.watchlistItems.some(function(i) { return i.symbol === symbol; });
      if (nowFav) {
        actions.setWatchlistMember({symbol},false).then(function() {
          updateSectorStockDetail(symbol, name);
        });
      } else {
        actions.setWatchlistMember({
          symbol: symbol,
          name: displayName
        },true).then(function() {
          updateSectorStockDetail(symbol, name);
        });
      }
    });
  }

  const analyzeBtn = document.getElementById("btn-sector-stock-analyze");
  if (analyzeBtn) {
    analyzeBtn.addEventListener("click", function() {
      const trendsTab = document.getElementById("tab-trends");
      if (trendsTab) trendsTab.click();
    });
  }
}
actions.updateSectorStockDetail = updateSectorStockDetail;

function renderSectorsTableSelection() {
  const rows = document.querySelectorAll("#sectors-table tbody tr");
  rows.forEach(function(r) {
    const nameCell = r.querySelector("td:nth-child(2)");
    if (nameCell && currentSelectedSector && nameCell.textContent.trim() === currentSelectedSector.name) {
      r.classList.add("selected-sector");
    }
  });
}

function loadSectorStocks(sector) {
  const titleEl = document.getElementById("sector-stocks-title");
  if (titleEl) {
    titleEl.textContent = sector.name;
  }
  const tbody = document.querySelector("#sector-stocks-table tbody");
  if (!tbody) return;
  stocksRequest?.abort();const request=new AbortController(),generation=++stocksGeneration;
  stocksRequest=request;
  lastStocksData=[];rowStatus(tbody,'fundHomeLoading',10);

  const url = "/api/market/sector/stocks?name=" + encodeURIComponent(sector.name) +
              "&code=" + encodeURIComponent(sector.code || "") +
              "&top=" + encodeURIComponent(sector.topStockCode || "");

  api("GET", url,null,{signal:request.signal}).then(function(res) {
    if(generation!==stocksGeneration)return;
    lastStocksData = res.quotes || [];
    if (titleEl) {
      titleEl.textContent = sector.name + " · " + t("fundMarketConstituents",{n:lastStocksData.length});
    }
    renderSectorStocksTable();
    updateSectorSidebar(sector);
  }).catch(function(err) {
    if(generation!==stocksGeneration||request.signal.aborted)return;
    rowStatus(tbody,'fundMarketReadFailed',10);
  });
}

function renderSectorStocksTable() {
  const tbody = document.querySelector("#sector-stocks-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "";

  const items = lastStocksData.slice();
  if (stockSort.key) {
    items.sort((a,b)=>compare(a,b,stockSort.key,stockSort.asc));
  }

  if (items.length === 0) {
    rowStatus(tbody,'fundMarketNoStocks',10);
    return;
  }

  items.forEach(function(st, idx) {
    const tr = document.createElement("tr");
    tr.className = "clickable";
    if (state.currentSymbol === st.code) {
      tr.classList.add("selected");
    }

    const price = st.price != null ? st.price.toFixed(2) : "–";
    const pct = st.changePct != null ? (st.changePct > 0 ? "+" : "") + st.changePct.toFixed(2) + "%" : "–";
    const change = st.change != null ? (st.change > 0 ? "+" : "") + st.change.toFixed(2) : "–";
    const cls = st.changePct > 0 ? "up" : st.changePct < 0 ? "down" : "";
    const vol = st.volume != null ? (st.volume >= 10000 ? (st.volume / 10000).toFixed(2) + "万" : st.volume.toFixed(0)) : "–";
    const turnover = st.turnover != null ? st.turnover.toFixed(2) : "–";
    const amp = st.amplitude != null ? st.amplitude.toFixed(2) : "–";
    const vratio = st.volumeRatio != null ? st.volumeRatio.toFixed(2) : "–";

    tr.dataset.symbol = st.code;
    tr.dataset.name = st.name;
    tr.innerHTML = "<td style='color:var(--natives-text-muted);'>" + (idx + 1) + "</td>" +
                   "<td><b>" + escape(st.code) + "</b></td>" +
                   "<td><button type='button' class='sector-select' data-stock-open>" + escape(st.name) + "</button></td>" +
                   "<td class='num " + cls + "'>" + price + "</td>" +
                   "<td class='num " + cls + "'>" + pct + "</td>" +
                   "<td class='num " + cls + "'>" + change + "</td>" +
                   "<td class='num'>" + vol + "</td>" +
                   "<td class='num'>" + turnover + "</td>" +
                   "<td class='num'>" + amp + "</td>" +
                   "<td class='num'>" + vratio + "</td>";

    tr.querySelector('[data-stock-open]').onclick=event=>{event.stopPropagation();actions.openStockDetail(st.code,st.name);};
    tr.addEventListener("click", function() {
      document.querySelectorAll("#sector-stocks-table tbody tr").forEach(function(r) { r.classList.remove("selected"); });
      tr.classList.add("selected");
      actions.selectSymbol(st.code, st.name, true);
    });

    tr.addEventListener("dblclick", function() {
      actions.openStockDetail(st.code, st.name);
    });

    tbody.appendChild(tr);
  });
}

document.querySelectorAll('#sectors-table th.sortable,#sector-stocks-table th.sortable').forEach(th=>{
 const key=th.querySelector('[data-i18n]')?.dataset.i18n;
 const button=document.createElement('button');button.type='button';button.className='table-sort';if(key)button.dataset.i18n=key;button.textContent=key?t(key):th.textContent;th.replaceChildren(button);
 th.setAttribute('aria-sort','none');
});
// 表头排序绑定
document.querySelectorAll("#sectors-table th.sortable").forEach(function(th) {
  th.addEventListener("click", function() {
    const key = th.dataset.key;
    if (sectorSort.key === key) { sectorSort.asc = !sectorSort.asc; }
    else { sectorSort = { key: key, asc: false }; }
    document.querySelectorAll("#sectors-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    th.classList.add(sectorSort.asc ? "sort-asc" : "sort-desc");
    document.querySelectorAll('#sectors-table th.sortable').forEach(o=>o.setAttribute('aria-sort',o===th?(sectorSort.asc?'ascending':'descending'):'none'));
    renderSectorsTable();
  });
});

document.querySelectorAll("#sector-stocks-table th.sortable").forEach(function(th) {
  th.addEventListener("click", function() {
    const key = th.dataset.key;
    if (stockSort.key === key) { stockSort.asc = !stockSort.asc; }
    else { stockSort = { key: key, asc: false }; }
    document.querySelectorAll("#sector-stocks-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    th.classList.add(stockSort.asc ? "sort-asc" : "sort-desc");
    document.querySelectorAll('#sector-stocks-table th.sortable').forEach(o=>o.setAttribute('aria-sort',o===th?(stockSort.asc?'ascending':'descending'):'none'));
    renderSectorStocksTable();
  });
});

})();
