(() => {
const { state, actions, authFetch } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

let analyzeController = null;
let quotePending = false;
let minuteController = null, minuteGeneration = 0;

// ===== 行情显示 =====
function updateQuote(q) {
  const qb = document.getElementById('quote-bar');
  if (!qb) return;
  if (!q || q.error) {
    qb.textContent=Fund.t('fundTrendsMinuteUnavailable');
    return;
  }
  if (q.name) tState.currentStockName = q.name;
  const direction=q.change==null||q.change===0?'flat':q.change>0?'up':'down';
  const number=value=>Number.isFinite(value)?value.toFixed(2):'–';
  const signed=value=>Number.isFinite(value)?(value>0?'+':'')+value.toFixed(2):'–';
  qb.replaceChildren();
  const add=(className,text)=>{const span=document.createElement('span');span.className=className;span.textContent=text;qb.append(span);};
  add('qb-name',q.name||q.symbol||'–');
  add('qb-price '+direction,number(q.price));
  add('qb-change '+direction,`${signed(q.change)} (${signed(q.pct)}%)`);
  for(const [key,value] of [['high',number(q.high)],['low',number(q.low)],['volume',fmtVol(q.volume)]])add('qb-detail',`${Fund.t('fundChart'+key)} ${value}`);
  add('qb-time',q.time||'–');

}

function fmtVol(n) { return trends.chart.fmtVol(n); }

// Search owns its draft and cancels stale responses. Selecting never writes a watchlist.
const searchInput = document.getElementById('search-input');
const searchResults = document.getElementById('search-results');
let searchTimer = null, searchRequest = null, searchGeneration = 0, searchHits = [], searchIndex = -1;
function closeSearch() {
  clearTimeout(searchTimer);
  searchRequest?.abort(); searchRequest = null; searchGeneration++;
  searchHits = []; searchIndex = -1;
  searchResults.replaceChildren(); searchResults.style.display = 'none';
  searchInput.setAttribute('aria-expanded', 'false');
  searchInput.removeAttribute('aria-activedescendant');
}
function showSearchStatus(key) {
  searchResults.replaceChildren();
  const hint = document.createElement('p');
  hint.className = 'evidence-hint'; hint.textContent = Fund.t(key);
  searchResults.append(hint); searchResults.style.display = 'block';
  searchInput.setAttribute('aria-expanded', 'true');
}
function highlightSearch(index) {
  searchIndex = index;
  [...searchResults.querySelectorAll('button')].forEach((row, i) => {
    row.classList.toggle('active', i === index);
    row.setAttribute('aria-selected', String(i === index));
    if (i === index) searchInput.setAttribute('aria-activedescendant', row.id);
  });
}
async function doSuggest(keyword) {
  searchRequest?.abort();
  const request = new AbortController(), generation = ++searchGeneration;
  searchRequest = request; searchHits = []; searchIndex = -1;
  searchInput.removeAttribute('aria-activedescendant');
  showSearchStatus('fundFocusSearchLoading');
  try {
    const response = await authFetch('/api/trends/search?keyword=' + encodeURIComponent(keyword), { signal: request.signal });
    if (!response.ok) throw new Error('SEARCH_FAILED');
    const data = await response.json();
    if (generation !== searchGeneration || searchInput.value.trim() !== keyword) return;
    searchHits = (data.results || []).slice(0, 50);
    searchResults.replaceChildren();
    if (!searchHits.length) { showSearchStatus('fundLedgerNoResults'); return; }
    searchHits.forEach((hit, index) => {
      const row = document.createElement('button');
      row.type = 'button'; row.className = 'sr-item'; row.id = 'research-result-' + index;
      row.setAttribute('role', 'option'); row.setAttribute('aria-selected', 'false');
      row.textContent = [hit.name, hit.code, hit.market || hit.venue].filter(Boolean).join(' · ');
      row.addEventListener('click', () => selectStock(hit.symbol || hit.code, hit.name, hit.market));
      searchResults.append(row);
    });
  } catch (error) {
    if (generation === searchGeneration && !request.signal.aborted) showSearchStatus('fundFocusSearchFailed');
  } finally { if (searchRequest === request) searchRequest = null; }
}
if (searchInput && searchResults) {
  searchInput.setAttribute('role', 'combobox');
  searchInput.setAttribute('aria-controls', 'search-results');
  searchInput.setAttribute('aria-autocomplete', 'list');
  searchInput.setAttribute('aria-expanded', 'false');
  searchInput.setAttribute('aria-label', Fund.t('fundFocusSearchLabel'));
  searchInput.placeholder = Fund.t('fundFocusSearchLabel');
  searchResults.setAttribute('role', 'listbox');
  searchInput.addEventListener('input', () => {
    closeSearch();
    const keyword = searchInput.value.trim();
    if (keyword) searchTimer = setTimeout(() => doSuggest(keyword), 250);
  });
  searchInput.addEventListener('keydown', e => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeSearch(); }
    else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (!searchHits.length) return;
      e.preventDefault();
      highlightSearch(searchIndex < 0 ? (e.key === 'ArrowDown' ? 0 : searchHits.length - 1) : (searchIndex + (e.key === 'ArrowDown' ? 1 : searchHits.length - 1)) % searchHits.length);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const hit = searchHits[searchIndex];
      if (hit) selectStock(hit.symbol || hit.code, hit.name, hit.market);
      else if (searchInput.value.trim()) { clearTimeout(searchTimer); doSuggest(searchInput.value.trim()); }
    }
  });
  document.addEventListener('click', e => { if (!e.target.closest('.search-wrap')) closeSearch(); });
  document.addEventListener('fund:language-changed', () => {
    searchInput.setAttribute('aria-label', Fund.t('fundFocusSearchLabel'));
    searchInput.placeholder = Fund.t('fundFocusSearchLabel');
    closeSearch();
  });
  window.addEventListener('pagehide', closeSearch);
}
function selectStock(code, name, market, instrumentKind) {
  closeSearch();
  if (!code) { tState.currentSymbol = ''; tState.currentStockName = ''; return; }
  if (searchInput) searchInput.value = [code, name].filter(Boolean).join(' ');
  tState.currentStockName = name || code;
  tState.instrumentKind = Fund.researchSeries.kind(code, market, instrumentKind);
  if (tState.instrumentKind === 'fund' && tState.currentView === 'minute') {
    tState.currentView = 'dayk';
    document.getElementById('view-minute').classList.remove('active');
    document.getElementById('view-dayk').classList.add('active');
    document.getElementById('kline-chart').style.display = 'block';
    document.getElementById('minute-chart').style.display = 'none';
    document.getElementById('minute-vol').style.display = 'none';
  }
  if(document.getElementById('view-trends').hidden){tState.currentSymbol=code;return;}
  return analyze(code);
}

function renderNav() {
  tState.navVisible = Fund.researchSeries.navRows(tState.navData || [], tState.currentView);
  charts.klineChart?.setOption(Fund.researchSeries.navOption(tState.navVisible), true);
}
// ===== 主分析流水线 =====
// 请求令牌：连续搜索/切换标的时，旧请求的晚到响应不得覆盖新图表（否则K线在
// 两个标的数据之间来回跳、出现"一闪一闪"的假大阳线）。
let _analyzeSeq = 0;
async function analyze(symbol) {
  tState.currentSymbol = symbol;
  minuteController?.abort(); ++minuteGeneration; trends.chart.pauseMinute?.();
  document.getElementById('trends-empty').hidden = true;
  document.dispatchEvent(new Event('fund:research-selection-changed'));
  analyzeController?.abort();
  analyzeController = new AbortController();
  const signal = analyzeController.signal;
  const seq = ++_analyzeSeq;

  try {
    await Fund.ensureEcharts();
    if (seq !== _analyzeSeq || signal.aborted) return;
    if (!trends.chart.initCharts()) throw new Error('CHART_INIT_FAILED');
  } catch (error) {
    if (seq === _analyzeSeq && !signal.aborted) document.getElementById('quote-bar').textContent = Fund.localizeApiError(500, error.code);
    return;
  }

  const loadingEl = document.getElementById('loading');
  if (loadingEl) loadingEl.style.display = 'flex';

  const qb = document.getElementById('quote-bar');
  if (qb) qb.textContent = Fund.t('fundHomeLoading');
  for (const chart of Object.values(charts)) chart?.clear();
  tState.navData = []; tState.navVisible = []; tState.klineData = []; tState.lastSignalData = null; tState.minuteData = null; tState.dailyChanlun = null;
  const sb = document.getElementById('sum-body');
  if (sb) sb.textContent = Fund.t('fundHomeLoading');
  for(const id of ['plan-card','market-card','chanlun-daily-card','chanlun-card','accuracy-card','risk-card']) {const card=document.getElementById(id);if(card)card.style.display='none';}
  for(const id of ['module-list','sig-list','canslim-body','levels-body']) {const node=document.getElementById(id);if(node)node.textContent=Fund.t('fundHomeLoading');}

  const navMode = tState.instrumentKind === 'fund';
  document.getElementById('trends-fq-group').hidden = navMode || tState.currentView === 'minute';
  document.querySelectorAll('.td-btn, #view-minute').forEach(button => { button.disabled = navMode; button.title = navMode ? Fund.t('fundTrendsNavCapability') : ''; });
  document.querySelectorAll('#view-trends [data-card]').forEach(card => card.hidden = navMode || (tState.instrumentKind === 'etf' && card.dataset.card === 'canslim'));
  if (navMode) {
    try {
      const rows = await Fund.researchSeries.nav(symbol, signal);
      if (seq !== _analyzeSeq) return;
      tState.navData = rows;
      const latest = rows.at(-1);
      qb.textContent = latest ? `${tState.currentStockName} · NAV ${latest.unitNav} · ${latest.navDate}` : Fund.t('fundTrendsNavEmpty');
      sb.textContent = Fund.t('fundTrendsNavCapability');
      document.getElementById('trends-query-status').textContent = [...new Set(rows.map(r => r.source))].join(' · ') + (latest ? ' · ' + rows[0].navDate + ' – ' + latest.navDate : '');
      charts.volumeChart?.clear(); charts.indicatorChart?.clear();
      document.getElementById('volume-chart').style.display = 'none';
      document.getElementById('indicator-chart').style.display = 'none';
      renderNav();
    } catch(e) { if (seq === _analyzeSeq && !signal.aborted) { qb.textContent = Fund.localizeApiError(e.status || 500, e.code); sb.textContent = Fund.t('fundTrendsNavEmpty'); } }
    finally { if (seq === _analyzeSeq && loadingEl) loadingEl.style.display = 'none'; }
    return;
  }
  document.getElementById('volume-chart').style.display = tState.currentView === 'minute' ? 'none' : '';
  try {
    const isWeek = tState.currentView === 'week';
    const isMonth = tState.currentView === 'month';
    const periodParam = isWeek ? '&period=week' : (isMonth ? '&period=month' : '');
    const fq = tState.fq || 'qfq';
    const analyzeUrl = `/api/trends/analyze?symbol=${encodeURIComponent(symbol)}${periodParam}&fq=${fq}`;
    const chanlunUrl = `/api/trends/chanlun_daily?symbol=${encodeURIComponent(symbol)}${periodParam}&fq=${fq}`;

    const [analyzeRes, chanlunRes] = await Promise.all([
      authFetch(analyzeUrl, { signal }),
      authFetch(chanlunUrl, { signal }),
    ]);

    const data = await analyzeRes.json();
    if (seq !== _analyzeSeq) return; // 已有更新的分析请求，丢弃旧响应
    if (data.error) {
      if (qb) qb.textContent = Fund.localizeApiError(analyzeRes.status, data.error, data.message);
      return;
    }
    if (!analyzeRes.ok || !Array.isArray(data.klines)) throw new Error('SERIES_INVALID');

    const query = data.queryIdentity;
    if (query && (query.fq !== fq || query.period !== (isWeek ? 'week' : isMonth ? 'month' : 'day'))) throw new Error('QUERY_IDENTITY');
    tState.queryIdentity = query || null;
    document.getElementById('trends-query-status').textContent = query ? [query.source, query.fq, query.from, query.to, query.count].filter(v => v != null).join(' · ') : Fund.t('fundTrendsSourceUnknown');
    updateQuote(data.quote);
    if (trends.chart?.renderKline) {
      trends.chart.renderKline(data.klines, data.signal);
    }
    if (trends.panel?.renderFlow) {
      trends.panel.renderFlow(data.flows);
    }
    if (trends.panel?.renderSignal) {
      trends.panel.renderSignal(data.signal);
    }
    if (trends.panel?.renderCanslim) {
      trends.panel.renderCanslim(data.signal?.canslim || data.canslim);
    }
    if (trends.panel?.renderKeyLevels) {
      trends.panel.renderKeyLevels(data.signal?.key_levels || data.key_levels);
    }
    if (trends.panel?.renderMarket) {
      trends.panel.renderMarket(data.signal, data.breadth || data.market_breadth);
    }

    if (trends.watch) {
      const _sAction = data.signal ? data.signal.action : '';
      const _sScore = data.signal ? data.signal.score : 0;
      const _sPrice = data.quote ? data.quote.price : 0;
      trends.watch.addHistory(data.symbol, data.name, _sAction, _sScore);
      trends.watch.updateStarButton(symbol);
      if (_sAction && _sPrice > 0) {
        trends.watch.checkSignalChange(data.symbol, data.name, _sAction, _sScore, _sPrice);
        trends.watch.recordSignal(data.symbol, data.name, _sAction, _sScore, _sPrice);
      }
      trends.watch.renderSignalAccuracy(data.symbol);

    }

    const clDailyLabel = document.getElementById('chanlun-daily-label');
    if (clDailyLabel) {
      clDailyLabel.textContent = isMonth ? '缠论月线分析' : (isWeek ? '缠论周线分析' : '缠论日线分析');
    }

    let chanlunData = null;
    try {
      chanlunData = await chanlunRes.json();
      if (seq !== _analyzeSeq) return;
      tState.dailyChanlun = chanlunData;
    } catch(e) {
      chanlunData = null;
      tState.dailyChanlun = null;
    }
    if (seq !== _analyzeSeq) return; // 缠论晚到响应同样丢弃
    if (trends.chan) {
      trends.chan.renderChanlunDaily(chanlunData);
      trends.chan.applyChanlunDailyOverlay(chanlunData);
    }

    if (tState.currentView === 'minute' && tState.currentSymbol === symbol) {
      loadMinute(symbol);
    }

    if (seq !== _analyzeSeq) return; // 不为旧标的启动行情轮询

  } catch(e) {
    if (seq === _analyzeSeq && e.name !== 'AbortError' && qb) qb.textContent = Fund.localizeApiError(500, e.code);
  } finally {
    if (seq === _analyzeSeq && loadingEl) loadingEl.style.display = 'none';
  }
}

async function refreshQuote(symbol) {
  if (tState.instrumentKind === 'fund' || quotePending || document.hidden || document.getElementById('view-trends')?.hidden) return;
  quotePending = true;
  try {
    // 只处理当前标的的行情，切换标的后的在途旧响应直接丢弃
    if (symbol !== tState.currentSymbol) return;
    const r = await authFetch(`/api/trends/quote?symbol=${symbol}`);
    const q = await r.json();
    // 响应期间可能已切换标的，二次校验后再落图
    if (!q.error && symbol === tState.currentSymbol) {
      updateQuote(q);
      if (trends.chart?.refreshKlineLastCandle) {
        trends.chart.refreshKlineLastCandle(q);
      }
      if (tState.currentView === 'minute' && trends.chart?.refreshMinuteLight) {
        trends.chart.refreshMinuteLight(symbol);
      }
      if (tState.flowMode === 'realtime' && trends.panel?.loadRealtimeFlow) {
        trends.panel.loadRealtimeFlow(symbol);
      }
    }
  } catch(e) {}
  finally { quotePending = false; }
}

async function loadMinute(symbol) {
  minuteController?.abort(); minuteController = new AbortController();
  const signal = minuteController.signal, generation = ++minuteGeneration;
  const current = () => generation === minuteGeneration && tState.currentSymbol === symbol && tState.currentView === 'minute';
  try {
    if (charts.minuteChart) charts.minuteChart.resize();
    if (charts.minuteVolChart) charts.minuteVolChart.resize();
    tState.minuteYRange = null;

    const [minuteRes, chanlunRes] = await Promise.all([
      authFetch(`/api/trends/minute?symbol=${encodeURIComponent(symbol)}`, { signal }),
      authFetch(`/api/trends/chanlun_minute?symbol=${encodeURIComponent(symbol)}`, { signal }),
    ]);
    // 切换标的后的在途分时响应直接丢弃，避免旧分时覆盖新标的图表
    if (tState.currentSymbol !== symbol) return;
    const raw = await minuteRes.json();
    if (!current()) return;
    if (!minuteRes.ok || raw.error) {
      if (charts.minuteChart) {
        charts.minuteChart.setOption({ title: { text: Fund.t('fundTrendsMinuteUnavailable'), left: 'center', top: 'center', textStyle: { color: C.down, fontSize: 14 } } }, true);
      }
      const cc = document.getElementById('chanlun-card');
      if (cc) cc.style.display = 'none';
      return;
    }
    const data = Fund.researchSeries.minute(raw);
    tState.minuteData = data;
    let chanlunData = null;
    try {
      chanlunData = await chanlunRes.json();
      if (!current()) return;
      tState.minuteChanlun = chanlunData;
    } catch(e) { chanlunData = null; }

    if (trends.chart?.renderMinute) {
      trends.chart.renderMinute(data, chanlunData);
    }
    if (trends.chan?.renderChanlun) {
      trends.chan.renderChanlun(chanlunData);
    }
  } catch(e) {
    if (!current() || signal.aborted) return;
    if (charts.minuteChart) {
      charts.minuteChart.setOption({ title: { text: Fund.t('fundTrendsMinuteUnavailable'), left: 'center', top: 'center', textStyle: { color: C.down, fontSize: 14 } } }, true);
    }
  }
}

document.getElementById('trends-fq').addEventListener('change', event => {
  trends.drawings?.cancelDraft(); tState.fq = event.target.value;
  if (tState.currentSymbol) analyze(tState.currentSymbol);
});
document.getElementById('trends-ai-review').addEventListener('click',()=>{
  const identity=()=>JSON.stringify({symbol:tState.currentSymbol,period:tState.currentView,fq:tState.fq,range:(tState.currentView==='minute'?charts.minuteChart:charts.klineChart)?.getOption().dataZoom?.[0]});
  Fund.home.openAI({research:{identity,capture:()=>{
    const nav=tState.instrumentKind==='fund',minute=tState.currentView==='minute';
    const rows=nav?(tState.navVisible || []).map(row=>({date:row.navDate,unitNav:row.unitNav})):minute?(tState.minuteData?.times || []).map((time,index)=>({date:[tState.minuteData.date,time].filter(Boolean).join(' '),price:tState.minuteData.prices[index],volume:tState.minuteData.volumes[index]})):tState.klineData;
    return Fund.researchSeries.capture({symbol:tState.currentSymbol,period:tState.currentView,fq:nav?'not-applicable':minute?'bf':tState.fq,seriesKind:nav?'NAV':minute?'minute':'OHLC'},rows,minute?charts.minuteChart:charts.klineChart,nav?tState.navVisible?.at(-1)?.source:minute?tState.minuteData?.source:tState.queryIdentity?.source);
  }}});
});
// 暴露全局方法与内联兼容
window.selectStock = selectStock;
window.analyze = analyze;

// 导出与全局 Fund 体系联动
actions.initTrends = function() {
  try {
    const hist = trends.watch?.getHistory ? trends.watch.getHistory() : [];
    const lastSymbol = tState.currentSymbol || state.currentSymbol || hist[0]?.code;
    if (lastSymbol) selectStock(lastSymbol, tState.currentStockName || hist[0]?.name, '', tState.instrumentKind);
  } catch(e) {}
};

actions.loadTrendsStock = function(code, name, instrumentKind) {
  if (code) selectStock(code, name, '', instrumentKind);
};

// 启动入口
try {
  if (trends.chart?.initCharts) trends.chart.initCharts();
  if (trends.watch?.updateBadges) trends.watch.updateBadges();
  if (trends.panel?.loadMode) trends.panel.loadMode();
  setTimeout(() => {
    if (state.token && actions.initTrends) {
      actions.initTrends();
    }
  }, 100);
} catch(e) {}

actions.pauseTrends = function() { closeSearch(); trends.watch?.closePanel(); trends.chart.pauseMinute?.(); minuteController?.abort(); ++minuteGeneration; analyzeController?.abort(); ++_analyzeSeq; trends.drawings?.pause(); };
window.addEventListener('pagehide', actions.pauseTrends);

actions.refreshTrends = function() {
  const sym = tState.currentSymbol || state.currentSymbol;
  if (!sym) return;
  if (trends.chart?.initCharts) {
    trends.chart.initCharts();
  }
  setTimeout(() => {
    if (charts.klineChart) charts.klineChart.resize();
    if (charts.volumeChart) charts.volumeChart.resize();
    if (charts.flowChart) charts.flowChart.resize();
    if (charts.minuteChart) charts.minuteChart.resize();
    if (charts.minuteVolChart) charts.minuteVolChart.resize();
    if (charts.indicatorChart) charts.indicatorChart.resize();
  }, 50);
  analyze(sym);
};

trends.coordinator = {
  selection: () => ({ symbol: tState.currentSymbol, name: tState.currentStockName, instrumentKind: tState.instrumentKind }),
  analyze,
  renderNav,
  refreshQuote,
  loadMinute,
  selectStock,
  updateQuote,
};

})();
