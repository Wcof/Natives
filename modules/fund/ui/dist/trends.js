(() => {
const { state, actions, authFetch } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

let _refreshTimer = null;

// ===== 行情显示 =====
function updateQuote(q) {
  const qb = document.getElementById('quote-bar');
  if (!qb) return;
  if (!q || q.error) {
    qb.innerHTML = '<span class="qb-name flat">获取行情失败</span>';
    return;
  }
  if (q.name) tState.currentStockName = q.name;
  const isUp = (q.change || 0) >= 0;
  const c = isUp ? 'up' : 'down';
  const sign = isUp ? '+' : '';
  const price = q.price != null ? q.price.toFixed(2) : '--';
  const change = q.change != null ? `${sign}${q.change.toFixed(2)}` : '';
  const pct = q.pct != null ? `${sign}${q.pct.toFixed(2)}%` : '';
  const time = q.time || '';

  qb.innerHTML = `
    <span class="qb-name">${q.name || q.symbol}</span>
    <span class="qb-price ${c}">${price}</span>
    <span class="qb-change ${c}">${change} (${pct})</span>
    <span class="qb-detail">高 <b class="up">${q.high ? q.high.toFixed(2) : '--'}</b></span>
    <span class="qb-detail">低 <b class="down">${q.low ? q.low.toFixed(2) : '--'}</b></span>
    <span class="qb-detail">量 <b>${fmtVol(q.volume)}</b></span>
    <span class="qb-detail">额 <b>${fmtVol(q.amount)}</b></span>
    <span class="qb-detail">换手 <b>${q.turnover ? q.turnover.toFixed(1) + '%' : '--'}</b></span>
    <span class="qb-detail">PE <b>${q.pe ? q.pe.toFixed(1) : '--'}</b></span>
    <span class="qb-time">${time}</span>
  `;
}

function fmtVol(n) {
  if (n == null) return '--';
  if (n >= 1e8) return (n / 1e8).toFixed(2) + '亿';
  if (n >= 1e4) return (n / 1e4).toFixed(1) + '万';
  return String(n);
}

// ===== 搜索 =====
const searchInput = document.getElementById('search-input');
const searchResults = document.getElementById('search-results');
let searchTimer = null;

if (searchInput && searchResults) {
  searchInput.addEventListener('input', () => {
    clearTimeout(searchTimer);
    const kw = searchInput.value.trim();
    if (!kw) { searchResults.style.display = 'none'; return; }
    searchTimer = setTimeout(() => doSuggest(kw), 250);
  });

  searchInput.addEventListener('keydown', e => {
    if (e.key === 'Enter') {
      searchResults.style.display = 'none';
      const kw = searchInput.value.trim();
      if (/^\d{6}$/.test(kw)) analyze(kw);
      else doSearch();
    }
  });

  document.addEventListener('click', e => {
    if (!e.target.closest('.search-wrap')) searchResults.style.display = 'none';
  });
}

async function doSuggest(kw) {
  try {
    const r = await authFetch(`/api/trends/search?keyword=${encodeURIComponent(kw)}`);
    const data = await r.json();
    if (data.results && data.results.length) {
      searchResults.innerHTML = data.results.map(s =>
        `<div class="sr-item" onclick="selectStock('${s.code}','${s.name}')">
          <span class="code">${s.code}</span><span class="name">${s.name}</span>
        </div>`
      ).join('');
      searchResults.style.display = 'block';
    } else searchResults.style.display = 'none';
  } catch(e) {}
}

async function doSearch() {
  const kw = searchInput.value.trim();
  if (!kw) return;
  try {
    const r = await authFetch(`/api/trends/search?keyword=${encodeURIComponent(kw)}`);
    const data = await r.json();
    if (data.results && data.results.length) {
      const first = data.results[0];
      selectStock(first.code, first.name);
    }
  } catch(e) {}
}

function selectStock(code, name) {
  if (searchInput) searchInput.value = `${code} ${name}`;
  if (searchResults) searchResults.style.display = 'none';
  analyze(code);
}

// ===== 主分析流水线 =====
// 请求令牌：连续搜索/切换标的时，旧请求的晚到响应不得覆盖新图表（否则K线在
// 两个标的数据之间来回跳、出现"一闪一闪"的假大阳线）。
let _analyzeSeq = 0;
async function analyze(symbol) {
  tState.currentSymbol = symbol;
  clearInterval(_refreshTimer);
  const seq = ++_analyzeSeq;

  const qb = document.getElementById('quote-bar');
  if (qb) qb.innerHTML = '<span class="qb-name flat">正在分析中...</span>';
  const sb = document.getElementById('sum-body');
  if (sb) sb.innerHTML = '<div style="color:#666;font-size:13px;padding:12px 0">正在综合分析...</div>';

  try {
    const isWeek = tState.currentView === 'week';
    const analyzeUrl = `/api/trends/analyze?symbol=${symbol}${isWeek ? '&period=week' : ''}`;
    const chanlunUrl = `/api/trends/chanlun_daily?symbol=${symbol}${isWeek ? '&period=week' : ''}`;

    const [analyzeRes, chanlunRes] = await Promise.all([
      authFetch(analyzeUrl),
      authFetch(chanlunUrl),
    ]);

    const data = await analyzeRes.json();
    if (seq !== _analyzeSeq) return; // 已有更新的分析请求，丢弃旧响应
    if (data.error) {
      if (qb) qb.innerHTML = `<span class="qb-name" style="color:#ff2d2d">${data.error}</span>`;
      return;
    }

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
      trends.panel.renderCanslim(data.canslim);
    }
    if (trends.panel?.renderKeyLevels) {
      trends.panel.renderKeyLevels(data.key_levels);
    }
    if (trends.panel?.renderMarket) {
      trends.panel.renderMarket(data.signal, data.market_breadth);
    }

    if (trends.watch) {
      trends.watch.addHistory(data.symbol, data.name, data.signal.action, data.signal.score);
      trends.watch.updateStarButton(symbol);
      trends.watch.checkSignalChange(data.symbol, data.name, data.signal.action, data.signal.score, data.quote.price);
      trends.watch.recordSignal(data.symbol, data.name, data.signal.action, data.signal.score, data.quote.price);
      trends.watch.renderSignalAccuracy(data.symbol);
    }

    let chanlunData = null;
    try {
      chanlunData = await chanlunRes.json();
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
    _refreshTimer = setInterval(() => refreshQuote(symbol), 2000);
  } catch(e) {
    if (qb) qb.innerHTML = `<span class="qb-name" style="color:#ff2d2d">分析失败: ${e.message}</span>`;
  }
}

async function refreshQuote(symbol) {
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
}

async function loadMinute(symbol) {
  try {
    if (charts.minuteChart) charts.minuteChart.resize();
    if (charts.minuteVolChart) charts.minuteVolChart.resize();
    tState.minuteYRange = null;

    const [minuteRes, chanlunRes] = await Promise.all([
      authFetch(`/api/trends/minute?symbol=${symbol}`),
      authFetch(`/api/trends/chanlun_minute?symbol=${symbol}`),
    ]);
    // 切换标的后的在途分时响应直接丢弃，避免旧分时覆盖新标的图表
    if (tState.currentSymbol !== symbol) return;
    const data = await minuteRes.json();
    if (data.error) {
      if (charts.minuteChart) {
        charts.minuteChart.setOption({ title: { text: data.error, left: 'center', top: 'center', textStyle: { color: C.down, fontSize: 14 } } }, true);
      }
      const cc = document.getElementById('chanlun-card');
      if (cc) cc.style.display = 'none';
      return;
    }
    tState.minuteData = data;
    let chanlunData = null;
    try {
      chanlunData = await chanlunRes.json();
      tState.minuteChanlun = chanlunData;
    } catch(e) { chanlunData = null; }

    if (trends.chart?.renderMinute) {
      trends.chart.renderMinute(data, chanlunData);
    }
    if (trends.chan?.renderChanlun) {
      trends.chan.renderChanlun(chanlunData);
    }
  } catch(e) {
    if (charts.minuteChart) {
      charts.minuteChart.setOption({ title: { text: '分时数据获取失败', left: 'center', top: 'center', textStyle: { color: C.down, fontSize: 14 } } }, true);
    }
  }
}

// 暴露全局方法与内联兼容
window.selectStock = selectStock;
window.analyze = analyze;

// 导出与全局 Fund 体系联动
actions.initTrends = function() {
  try {
    const hist = trends.watch?.getHistory ? trends.watch.getHistory() : [];
    const lastSymbol = (hist.length > 0) ? hist[0].code : '600519';
    analyze(lastSymbol);
  } catch(e) {}
};

actions.loadTrendsStock = function(code) {
  if (code) {
    analyze(code);
  }
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

actions.refreshTrends = function() {
  const sym = state.currentSymbol || "600519";
  const cleanCode = sym.replace(/^(sh|sz|bj)/i, "");
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
  analyze(cleanCode);
};

trends.coordinator = {
  analyze,
  refreshQuote,
  loadMinute,
  selectStock,
  updateQuote,
};

})();
