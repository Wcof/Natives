(() => {
const { state, actions, authFetch, api, themeColor } = globalThis.Fund;

// Fund 股票详情子系统：看盘全屏双栏工作台控制器（Stock Detail Controller）
// 核心职责：
// 1. 管理左侧 ECharts 大图区（分时 + VWAP、日/周/月 K 线、成交量副图、技术指标）
// 2. 挂载图二专业画图工具引擎（Stock Drawings Engine）
// 3. 1:1 驱动右侧盘口卡片流（复用 actions.renderDepth 共享渲染与算法）
// 4. 严禁任何硬编码 mock 数据，全动态驱动；切换 Tab 时自动暂停轮询防泄漏

const sdState = {
  active: false,
  symbol: '',
  name: '',
  period: 'minute', // minute | dayk | week | month
  range: 60,        // 20 | 60 | 120 | 250 | 0
  indicator: 'none',// none | macd | rsi | kdj | boll | wr
  fq: 'qfq',        // qfq | hfq | bf
  previousTab: 'watchlist',
  klineData: [],
  minuteData: null,
  detail: null,
};

let minuteChart = null;
let minuteVolChart = null;
let klineChart = null;
let volumeChart = null;
let indicatorChart = null;
let drawingsCtrl = null;
let _refreshTimer = null;
let _loadSeq = 0;

function getThemeColors() {
  const isLight = document.documentElement.dataset.theme === 'archive';
  const isIntl = document.documentElement.getAttribute('data-color-mode') === 'intl' || state.colorMode === 'intl';
  const colorUp = isIntl ? '#00b35c' : '#ff2d2d';
  const colorDown = isIntl ? '#ff2d2d' : '#00b35c';
  return {
    isLight,
    isIntl,
    colorUp,
    colorDown,
    axis: isLight ? '#d8cfbe' : '#262626',
    grid: isLight ? '#e8dfce' : '#141414',
    text: isLight ? '#1a1a18' : '#aaa',
    textDim: isLight ? '#6b6355' : '#666',
    avgLine: isLight ? '#cc785c' : '#cdf24b',
  };
}

function getCurrentZoom(chartInstance) {
  if (!chartInstance) return { start: 0, end: 100 };
  try {
    const opt = chartInstance.getOption();
    if (opt && opt.dataZoom && opt.dataZoom[0]) {
      const dz = opt.dataZoom[0];
      return {
        start: dz.start != null ? dz.start : 0,
        end: dz.end != null ? dz.end : 100,
      };
    }
  } catch (e) {}
  return { start: 0, end: 100 };
}

let _isZoomSyncing = false;
function bindMultiChartZoomSync() {
  const chartList = [
    { source: klineChart, targets: () => [volumeChart, indicatorChart] },
    { source: volumeChart, targets: () => [klineChart, indicatorChart] },
    { source: indicatorChart, targets: () => [klineChart, volumeChart] },
    { source: minuteChart, targets: () => [minuteVolChart] },
    { source: minuteVolChart, targets: () => [minuteChart] },
  ];

  chartList.forEach(({ source, targets }) => {
    if (!source || source._zoomSyncBound) return;
    source._zoomSyncBound = true;
    source.on('datazoom', () => {
      if (_isZoomSyncing) return;
      _isZoomSyncing = true;
      try {
        const { start, end } = getCurrentZoom(source);
        targets().forEach(t => {
          if (t && typeof t.dispatchAction === 'function') {
            t.dispatchAction({ type: 'dataZoom', start, end });
          }
        });
      } finally {
        _isZoomSyncing = false;
      }
    });
  });
}

// ---------- ECharts 初始化 ----------
function initCharts() {
  if (minuteChart && klineChart) return true;
  if (typeof echarts === 'undefined') return false;

  const mEl = document.getElementById('sd-minute-chart');
  const mvEl = document.getElementById('sd-minute-vol');
  const kEl = document.getElementById('sd-kline-chart');
  const vEl = document.getElementById('sd-volume-chart');
  const indEl = document.getElementById('sd-indicator-chart');
  if (!mEl || !kEl) return false;

  const tc = getThemeColors();
  const baseOpts = {
    backgroundColor: 'transparent',
    textStyle: { color: tc.text, fontFamily: 'Microsoft YaHei, sans-serif' },
    categoryAxis: {
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { color: tc.textDim, fontSize: 10 },
      splitLine: { show: false },
    },
    valueAxis: {
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { color: tc.textDim, fontSize: 10 },
      splitLine: { lineStyle: { color: tc.grid } },
    },
    tooltip: {
      backgroundColor: tc.isLight ? 'rgba(250,246,239,0.95)' : 'rgba(20,20,20,0.92)',
      borderColor: tc.axis,
      textStyle: { color: tc.text, fontSize: 11 },
    },
  };

  minuteChart = echarts.init(mEl);
  minuteVolChart = echarts.init(mvEl);
  klineChart = echarts.init(kEl);
  volumeChart = echarts.init(vEl);
  indicatorChart = echarts.init(indEl);

  minuteChart.setOption(baseOpts);
  minuteVolChart.setOption(baseOpts);
  klineChart.setOption(baseOpts);
  volumeChart.setOption(baseOpts);
  indicatorChart.setOption(baseOpts);

  // 多图光标十字轴联动
  try {
    echarts.connect([minuteChart, minuteVolChart]);
    echarts.connect([klineChart, volumeChart, indicatorChart]);
  } catch (e) {}

  // 多图 dataZoom 实时双向精确同步（彻底解决副图与主图错位不对应问题）
  bindMultiChartZoomSync();

  window.addEventListener('resize', resizeAllCharts);

  // 初始化画图控制器（传入 onZoomToRange 回调）
  drawingsCtrl = globalThis.Fund.createStockDrawings({
    getKlineData: () => (sdState.period === 'minute' ? ((sdState.minuteData && (sdState.minuteData.points || sdState.minuteData.items)) || []) : sdState.klineData),
    getSymbol: () => sdState.symbol,
    getPeriod: () => sdState.period,
    onZoomToRange: (d1, d2) => zoomToRange(d1, d2),
  });

  return true;
}

actions.resizeStockDetailCharts = resizeAllCharts;

// ---------- 局部框选缩放与全景重置 ----------
function zoomToRange(d1, d2) {
  const data = sdState.klineData;
  if (!data || !data.length || !klineChart) return;
  const idx1 = data.findIndex(k => (k.date || k.time) === d1);
  const idx2 = data.findIndex(k => (k.date || k.time) === d2);
  if (idx1 < 0 && idx2 < 0) return;
  const i1 = idx1 >= 0 ? idx1 : 0;
  const i2 = idx2 >= 0 ? idx2 : data.length - 1;
  const minI = Math.min(i1, i2);
  const maxI = Math.max(i1, i2);
  const total = data.length;
  const startPct = Math.max(0, (minI / total) * 100);
  const endPct = Math.min(100, ((maxI + 1) / total) * 100);

  klineChart.dispatchAction({ type: 'dataZoom', start: startPct, end: endPct });
  if (volumeChart) volumeChart.dispatchAction({ type: 'dataZoom', start: startPct, end: endPct });
  if (indicatorChart) indicatorChart.dispatchAction({ type: 'dataZoom', start: startPct, end: endPct });

  const resetBtn = document.getElementById('sd-zoom-reset-btn');
  if (resetBtn) resetBtn.style.display = 'inline-flex';
}

function resetZoomAll() {
  sdState.range = 0;
  if (klineChart) klineChart.dispatchAction({ type: 'dataZoom', start: 0, end: 100 });
  if (volumeChart) volumeChart.dispatchAction({ type: 'dataZoom', start: 0, end: 100 });
  if (indicatorChart) indicatorChart.dispatchAction({ type: 'dataZoom', start: 0, end: 100 });
  const resetBtn = document.getElementById('sd-zoom-reset-btn');
  if (resetBtn) resetBtn.style.display = 'none';

  const rangeGroup = document.getElementById('sd-range-group');
  if (rangeGroup) {
    rangeGroup.querySelectorAll('button[data-range]').forEach(b => {
      b.classList.toggle('active', b.dataset.range === '0');
    });
  }
}

function resizeAllCharts() {
  [minuteChart, minuteVolChart, klineChart, volumeChart, indicatorChart].forEach(c => {
    if (c && typeof c.resize === 'function') c.resize();
  });
  // graphic 元素是像素坐标，图表尺寸变化后立即按新坐标系重挂，防止画线飘移
  if (drawingsCtrl) drawingsCtrl.render();
}

// ---------- 视图与周期切换 ----------
function switchPeriod(p) {
  sdState.period = p;

  // 更新周期按钮状态
  const group = document.querySelector('.sd-chart-toolbar');
  if (group) {
    group.querySelectorAll('.sd-tb-group .tb-btn').forEach(btn => {
      btn.classList.toggle('active', btn.id === 'sd-view-' + p);
    });
  }

  const rangeGroup = document.getElementById('sd-range-group');
  const sepRange = document.getElementById('sd-sep-range');
  const indBar = document.getElementById('sd-indicator-bar');
  const sepInd = document.getElementById('sd-sep-ind');
  const fqGroup = document.getElementById('sd-fq-group');

  const mEl = document.getElementById('sd-minute-chart');
  const mvEl = document.getElementById('sd-minute-vol');
  const kEl = document.getElementById('sd-kline-chart');
  const vEl = document.getElementById('sd-volume-chart');
  const indEl = document.getElementById('sd-indicator-chart');

  if (p === 'minute') {
    if (rangeGroup) rangeGroup.style.display = 'none';
    if (sepRange) sepRange.style.display = 'none';
    if (indBar) indBar.style.display = 'none';
    if (sepInd) sepInd.style.display = 'none';
    if (fqGroup) fqGroup.style.display = 'none';

    if (mEl) mEl.style.display = 'block';
    if (mvEl) mvEl.style.display = 'block';
    if (kEl) kEl.style.display = 'none';
    if (vEl) vEl.style.display = 'none';
    if (indEl) indEl.style.display = 'none';

    if (drawingsCtrl && minuteChart) {
      drawingsCtrl.attachChart(minuteChart);
    }
  } else {
    if (rangeGroup) rangeGroup.style.display = 'inline-flex';
    if (sepRange) sepRange.style.display = 'inline-block';
    if (indBar) indBar.style.display = 'inline-flex';
    if (sepInd) sepInd.style.display = 'inline-block';
    if (fqGroup) fqGroup.style.display = 'inline-flex';

    if (mEl) mEl.style.display = 'none';
    if (mvEl) mvEl.style.display = 'none';
    if (kEl) kEl.style.display = 'block';
    if (vEl) vEl.style.display = 'block';
    if (indEl) indEl.style.display = sdState.indicator !== 'none' ? 'block' : 'none';

    if (drawingsCtrl && klineChart) {
      drawingsCtrl.attachChart(klineChart);
    }
  }

  resizeAllCharts();
  loadChartData(true);
}

// ---------- 渲染分时图 ----------
function renderMinute(data) {
  if (!minuteChart || !minuteVolChart || !data) return;
  const points = (data && (data.points || data.items)) || [];
  const tc = getThemeColors();

  if (!points.length) {
    minuteChart.setOption({
      title: { text: '暂无分时数据', left: 'center', top: 'center', textStyle: { color: tc.textDim, fontSize: 13 } }
    }, true);
    minuteVolChart.setOption({}, true);
    return;
  }

  const preClose = data.preClose || (sdState.detail ? Number(sdState.detail.prevClose) : (points[0] ? Number(points[0].price) : 100));
  const times = points.map(d => d.time);
  const prices = points.map(d => Number(d.price));
  const vwaps = points.map(d => Number(d.vwap || d.price));
  const vols = points.map(d => Number(d.volume || 0));

  let minP = Infinity, maxP = -Infinity;
  prices.concat(vwaps).forEach(p => {
    if (p != null && !isNaN(p)) { minP = Math.min(minP, p); maxP = Math.max(maxP, p); }
  });
  const maxDiff = Math.max(Math.abs(maxP - preClose), Math.abs(minP - preClose), preClose * 0.01);
  const yMin = preClose - maxDiff;
  const yMax = preClose + maxDiff;

  const minOpt = {
    animation: false,
    grid: { left: 55, right: 35, top: 10, bottom: 6 },
    xAxis: {
      type: 'category',
      data: times,
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { color: tc.textDim, fontSize: 10 },
      splitLine: { show: false },
    },
    yAxis: [
      {
        type: 'value',
        min: yMin, max: yMax,
        axisLine: { lineStyle: { color: tc.axis } },
        splitLine: { lineStyle: { color: tc.grid } },
        axisLabel: {
          color: function(val) {
            return val > preClose ? tc.colorUp : (val < preClose ? tc.colorDown : tc.textDim);
          },
          formatter: val => val.toFixed(2),
        },
      },
      {
        type: 'value',
        min: -((maxDiff / preClose) * 100),
        max: (maxDiff / preClose) * 100,
        position: 'right',
        splitLine: { show: false },
        axisLabel: {
          color: val => val > 0 ? tc.colorUp : (val < 0 ? tc.colorDown : tc.textDim),
          formatter: val => val.toFixed(2) + '%',
        },
      },
    ],
    series: [
      {
        name: '现价',
        type: 'line',
        data: prices,
        showSymbol: false,
        lineStyle: { width: 1.5, color: '#38bdf8' },
        areaStyle: {
          color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
            { offset: 0, color: 'rgba(56, 189, 248, 0.25)' },
            { offset: 1, color: 'rgba(56, 189, 248, 0.0)' },
          ]),
        },
      },
      {
        name: '均价',
        type: 'line',
        data: vwaps,
        showSymbol: false,
        lineStyle: { width: 1.2, color: tc.avgLine, type: 'dashed' },
      },
    ],
  };

  const volColors = prices.map((p, i) => {
    const prev = i === 0 ? preClose : prices[i-1];
    return p >= prev ? tc.colorUp : tc.colorDown;
  });

  const volOpt = {
    animation: false,
    grid: { left: 55, right: 35, top: 4, bottom: 16 },
    xAxis: {
      type: 'category',
      data: times,
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { show: false },
    },
    yAxis: {
      type: 'value',
      splitLine: { lineStyle: { color: tc.grid } },
      axisLabel: { color: tc.textDim, fontSize: 9, formatter: v => (v >= 1e4 ? (v / 1e4).toFixed(0) + '万' : v) },
    },
    series: [
      {
        name: '成交量',
        type: 'bar',
        data: vols.map((v, i) => ({ value: v, itemStyle: { color: volColors[i] } })),
      },
    ],
  };

  minuteChart.setOption(minOpt, { notMerge: true });
  minuteVolChart.setOption(volOpt, { notMerge: true });
}

// ---------- 渲染 K 线与技术指标 ----------
function renderKline(klines) {
  if (!klineChart || !volumeChart || !klines) return;
  const tc = getThemeColors();

  if (!klines.length) {
    klineChart.setOption({
      title: { text: '暂无K线数据', left: 'center', top: 'center', textStyle: { color: tc.textDim, fontSize: 13 } }
    }, true);
    volumeChart.setOption({}, true);
    return;
  }

  const dates = klines.map(d => d.date);
  const ohlc = klines.map(d => [d.open, d.close, d.low, d.high]);
  const vols = klines.map(d => d.volume || 0);

  // 计算 MA5, MA10, MA20, MA30, MA60
  function calcMA(days) {
    const res = [];
    for (let i = 0; i < klines.length; i++) {
      if (i < days - 1) { res.push(null); continue; }
      let sum = 0;
      for (let j = 0; j < days; j++) sum += klines[i - j].close;
      res.push(Number((sum / days).toFixed(2)));
    }
    return res;
  }

  const ma5 = calcMA(5);
  const ma10 = calcMA(10);
  const ma20 = calcMA(20);
  const ma30 = calcMA(30);
  const ma60 = calcMA(60);

  let startIdx = 0;
  if (sdState.range > 0 && klines.length > sdState.range) {
    startIdx = Math.round(((klines.length - sdState.range) / klines.length) * 100);
  } else {
    startIdx = 0;
  }

  const klineOpt = {
    animation: false,
    grid: { left: 55, right: 35, top: 12, bottom: 6 },
    xAxis: {
      type: 'category',
      data: dates,
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { color: tc.textDim, fontSize: 10 },
      splitLine: { show: false },
    },
    yAxis: {
      type: 'value',
      scale: true,
      axisLine: { lineStyle: { color: tc.axis } },
      splitLine: { lineStyle: { color: tc.grid } },
      axisLabel: { color: tc.textDim, fontSize: 10, formatter: v => v.toFixed(2) },
    },
    dataZoom: [
      { type: 'inside', start: startIdx, end: 100, zoomOnMouseWheel: true, moveOnMouseMove: true, xAxisIndex: [0] },
    ],
    series: [
      {
        name: 'K线',
        type: 'candlestick',
        data: ohlc,
        itemStyle: {
          color: tc.colorUp,
          color0: tc.colorDown,
          borderColor: tc.colorUp,
          borderColor0: tc.colorDown,
        },
      },
      { name: 'MA5', type: 'line', data: ma5, showSymbol: false, lineStyle: { width: 1.2, color: '#ffeb3b' } },
      { name: 'MA10', type: 'line', data: ma10, showSymbol: false, lineStyle: { width: 1.2, color: '#e040fb' } },
      { name: 'MA20', type: 'line', data: ma20, showSymbol: false, lineStyle: { width: 1.2, color: '#4fc3f7' } },
      { name: 'MA30', type: 'line', data: ma30, showSymbol: false, lineStyle: { width: 1.2, color: '#00e676' } },
      { name: 'MA60', type: 'line', data: ma60, showSymbol: false, lineStyle: { width: 1.2, color: '#ff9800' } },
    ],
  };

  const volBarColors = klines.map(d => (d.close >= d.open ? tc.colorUp : tc.colorDown));
  const volOpt = {
    animation: false,
    grid: { left: 55, right: 35, top: 4, bottom: 16 },
    xAxis: {
      type: 'category',
      data: dates,
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { show: false },
    },
    yAxis: {
      type: 'value',
      splitLine: { lineStyle: { color: tc.grid } },
      axisLabel: { color: tc.textDim, fontSize: 9, formatter: v => (v >= 1e4 ? (v / 1e4).toFixed(0) + '万' : v) },
    },
    dataZoom: [
      { type: 'inside', start: startIdx, end: 100, zoomOnMouseWheel: true, moveOnMouseMove: true, xAxisIndex: [0] },
    ],
    series: [
      {
        name: '成交量',
        type: 'bar',
        data: vols.map((v, i) => ({ value: v, itemStyle: { color: volBarColors[i] } })),
      },
    ],
  };

  klineChart.setOption(klineOpt, { notMerge: true });
  volumeChart.setOption(volOpt, { notMerge: true });

  // 渲染副图指标（复用 Fund.indicators，严格同步主图缩放范围）
  renderSubIndicator(klines);
}

function renderSubIndicator(klines) {
  if (!indicatorChart) return;
  const indEl = document.getElementById('sd-indicator-chart');
  if (sdState.indicator === 'none') {
    if (indEl) indEl.style.display = 'none';
    resizeAllCharts();
    return;
  }
  if (indEl) indEl.style.display = 'block';

  // 读取当前主图 K 线的实际数据缩放区间，保证副图与主图严格同步对齐！
  const zoom = getCurrentZoom(klineChart);
  const tc = getThemeColors();
  const dates = klines.map(d => d.date);
  const ind = sdState.indicator;
  const indEngine = globalThis.Fund.indicators;
  if (!indEngine) return;

  const closes = klines.map(d => d.close);
  const highs = klines.map(d => d.high);
  const lows = klines.map(d => d.low);

  let series = [];
  if (ind === 'macd') {
    const r = indEngine.calcMACD(closes);
    series = [
      { name: 'DIF', type: 'line', data: r.dif, showSymbol: false, lineStyle: { width: 1.2, color: '#4fc3f7' } },
      { name: 'DEA', type: 'line', data: r.dea, showSymbol: false, lineStyle: { width: 1.2, color: '#e040fb' } },
      {
        name: 'MACD', type: 'bar',
        data: r.macd.map(v => ({ value: v, itemStyle: { color: v >= 0 ? tc.colorUp : tc.colorDown } })),
      },
    ];
  } else if (ind === 'kdj') {
    const r = indEngine.calcKDJ(highs, lows, closes);
    series = [
      { name: 'K', type: 'line', data: r.k, showSymbol: false, lineStyle: { width: 1.2, color: tc.colorUp } },
      { name: 'D', type: 'line', data: r.d, showSymbol: false, lineStyle: { width: 1.2, color: '#4fc3f7' } },
      { name: 'J', type: 'line', data: r.j, showSymbol: false, lineStyle: { width: 1.2, color: '#e040fb' } },
    ];
  } else if (ind === 'rsi') {
    const rsi14 = indEngine.calcRSI(closes, 14);
    series = [
      { name: 'RSI(14)', type: 'line', data: rsi14, showSymbol: false, lineStyle: { width: 1.2, color: '#38bdf8' } },
    ];
  } else if (ind === 'wr') {
    const wr = indEngine.calcWR(highs, lows, closes, 14);
    series = [
      { name: 'WR(14)', type: 'line', data: wr, showSymbol: false, lineStyle: { width: 1.2, color: '#f59e0b' } },
    ];
  } else if (ind === 'boll') {
    const boll = indEngine.calcBOLL(closes, 20);
    series = [
      { name: 'UP', type: 'line', data: boll.upper, showSymbol: false, lineStyle: { width: 1, color: '#4fc3f7', type: 'dashed' } },
      { name: 'MID', type: 'line', data: boll.mid, showSymbol: false, lineStyle: { width: 1, color: '#ffeb3b' } },
      { name: 'LOW', type: 'line', data: boll.lower, showSymbol: false, lineStyle: { width: 1, color: '#4fc3f7', type: 'dashed' } },
    ];
  }

  const opt = {
    animation: false,
    grid: { left: 55, right: 35, top: 4, bottom: 16 },
    xAxis: { type: 'category', data: dates, axisLine: { lineStyle: { color: tc.axis } }, axisLabel: { color: tc.textDim, fontSize: 9 } },
    yAxis: { type: 'value', scale: true, splitLine: { lineStyle: { color: tc.grid } }, axisLabel: { color: tc.textDim, fontSize: 9 } },
    dataZoom: [{ type: 'inside', start: zoom.start, end: zoom.end, zoomOnMouseWheel: true, moveOnMouseMove: true, xAxisIndex: [0] }],
    series: series,
  };
  indicatorChart.setOption(opt, { notMerge: true });
  setTimeout(resizeAllCharts, 30);
}

function reloadStockDetailTheme() {
  if (minuteChart && klineChart) {
    const tc = getThemeColors();
    const baseOpts = {
      textStyle: { color: tc.text },
      categoryAxis: {
        axisLine: { lineStyle: { color: tc.axis } },
        axisLabel: { color: tc.textDim },
      },
      valueAxis: {
        axisLine: { lineStyle: { color: tc.axis } },
        axisLabel: { color: tc.textDim },
        splitLine: { lineStyle: { color: tc.grid } },
      },
    };
    [minuteChart, minuteVolChart, klineChart, volumeChart, indicatorChart].forEach(c => {
      if (c && typeof c.setOption === 'function') c.setOption(baseOpts);
    });
  }
  if (sdState.active && sdState.symbol) {
    if (sdState.period === 'minute' && sdState.minuteData) {
      renderMinute(sdState.minuteData);
    } else if (sdState.klineData && sdState.klineData.length) {
      renderKline(sdState.klineData);
    }
  }
}
actions.reloadStockDetailTheme = reloadStockDetailTheme;

function safeDecode(str) {
  if (!str) return '';
  try {
    return decodeURIComponent(str);
  } catch (e) {
    return str;
  }
}

let _lastMinuteSig = null;
let _lastKlineSig = null;
let _chartAbortCtrl = null;

function showChartError(text) {
  const errEl = document.getElementById('sd-chart-error');
  const txtEl = document.getElementById('sd-error-text');
  if (errEl && txtEl) {
    txtEl.textContent = text || '行情加载失败';
    errEl.style.display = 'flex';
  }
}

function hideChartError() {
  const errEl = document.getElementById('sd-chart-error');
  if (errEl) errEl.style.display = 'none';
}

// ---------- 1:1 驱动右侧原始 Dashboard 卡片（复用 actions.renderDepth） ----------
function updateDetailPanel(detail) {
  if (!detail) return;
  sdState.detail = detail;

  const cleanName = safeDecode(detail.name || sdState.name);

  function setElText(el, text) {
    if (el && el.textContent !== text) el.textContent = text;
  }
  function setVal(id, v) {
    const el = document.getElementById(id);
    const str = (v != null && v !== '') ? String(v) : '-';
    if (el && el.textContent !== str) el.textContent = str;
  }

  // 1. 标的报价卡
  const isUp = (detail.change || 0) >= 0;
  const cClass = isUp ? 'up' : 'down';
  const sign = isUp ? '+' : '';

  const nameEl = document.getElementById('sd-dash-name');
  const codeEl = document.getElementById('sd-dash-code');
  const priceEl = document.getElementById('sd-dash-price');
  const changeEl = document.getElementById('sd-dash-change');

  setElText(nameEl, cleanName);
  setElText(codeEl, detail.symbol || sdState.symbol);
  if (priceEl) {
    const pStr = detail.price != null ? Number(detail.price).toFixed(2) : '--';
    if (priceEl.textContent !== pStr) priceEl.textContent = pStr;
    const targetCls = 'price-large ' + cClass;
    if (priceEl.className !== targetCls) priceEl.className = targetCls;
  }
  if (changeEl) {
    const ch = detail.change != null ? `${sign}${Number(detail.change).toFixed(2)}` : '';
    const pct = detail.changePct != null ? `${sign}${Number(detail.changePct).toFixed(2)}%` : '';
    const chgStr = `${ch} (${pct})`;
    if (changeEl.textContent !== chgStr) changeEl.textContent = chgStr;
    if (changeEl.className !== cClass) changeEl.className = cClass;
  }

  // 顶部行情条同步
  const tName = document.getElementById('sd-stock-name');
  const tCode = document.getElementById('sd-stock-code');
  const tPrice = document.getElementById('sd-stock-price');
  const tChange = document.getElementById('sd-stock-change');
  setElText(tName, cleanName);
  setElText(tCode, detail.symbol || sdState.symbol);
  if (tPrice) {
    const pStr = detail.price != null ? Number(detail.price).toFixed(2) : '--';
    if (tPrice.textContent !== pStr) tPrice.textContent = pStr;
    const targetCls = 'sd-price-num ' + cClass;
    if (tPrice.className !== targetCls) tPrice.className = targetCls;
  }
  if (tChange) {
    const ch = detail.change != null ? `${sign}${Number(detail.change).toFixed(2)}` : '';
    const pct = detail.changePct != null ? `${sign}${Number(detail.changePct).toFixed(2)}%` : '';
    const chgStr = `${ch} (${pct})`;
    if (tChange.textContent !== chgStr) tChange.textContent = chgStr;
    const targetCls = 'sd-change-num ' + cClass;
    if (tChange.className !== targetCls) tChange.className = targetCls;
  }

  // 涨跌停与均价辅助速览
  setVal('sd-limit-up', detail.limitUp ? Number(detail.limitUp).toFixed(2) : '--');
  setVal('sd-limit-down', detail.limitDown ? Number(detail.limitDown).toFixed(2) : '--');
  setVal('sd-avg-price', detail.avgPrice ? Number(detail.avgPrice).toFixed(2) : '--');

  // 动态概念/行情特征标签（指纹比对，内容不变不刷 DOM）
  const tagsEl = document.getElementById('sd-tags');
  if (tagsEl) {
    const liveTags = [];
    if (detail.pe && detail.pe > 0 && detail.pe < 28) liveTags.push(`低估值(PE:${detail.pe.toFixed(1)})`);
    if (detail.turnover && detail.turnover > 3.0) liveTags.push(`活跃换手(${detail.turnover.toFixed(1)}%)`);
    if (detail.volumeRatio && detail.volumeRatio > 1.6) liveTags.push(`温和放量(量比:${detail.volumeRatio.toFixed(1)})`);
    if (detail.changePct != null && Math.abs(detail.changePct) >= 2.5) {
      liveTags.push(detail.changePct > 0 ? `日内强势(+${detail.changePct.toFixed(1)}%)` : `日内走弱(${detail.changePct.toFixed(1)}%)`);
    }
    if (detail.amplitude && detail.amplitude >= 3.5) liveTags.push(`宽幅震荡(${detail.amplitude.toFixed(1)}%)`);
    const tagSig = liveTags.join('|');
    if (tagsEl._lastSig !== tagSig) {
      tagsEl._lastSig = tagSig;
      tagsEl.innerHTML = liveTags.map(t => `<span class="sd-tag">${t}</span>`).join('');
    }
  }

  // 2. 公司市值与估值股本（全量展开）
  function fmtShares(v) {
    if (v == null || isNaN(v) || v === 0) return '-';
    return (v / 1e8).toFixed(2) + '亿';
  }
  setVal('sd-v-mktcap', detail.marketCap ? Number(detail.marketCap).toFixed(2) : '-');
  setVal('sd-v-circ-mktcap', detail.circMarketCap ? Number(detail.circMarketCap).toFixed(2) : '-');
  setVal('sd-v-pe', detail.pe ? Number(detail.pe).toFixed(2) : '-');
  setVal('sd-v-pb', detail.pb ? Number(detail.pb).toFixed(2) : '-');
  setVal('sd-v-div', detail.dividendYield ? Number(detail.dividendYield).toFixed(2) + '%' : '-');
  setVal('sd-v-circ-shares', fmtShares(detail.circShares));
  setVal('sd-v-total-shares', fmtShares(detail.totalShares));
  setVal('sd-v-avg', detail.avgPrice ? Number(detail.avgPrice).toFixed(2) : '-');

  // 3. 五档盘口与买卖深度（复用 actions.renderDepth）
  if (actions.renderDepth) {
    actions.renderDepth(detail, 'sd-');
  }

  // 4. 盘口关键指标网格
  setVal('sd-m-high', detail.high != null ? Number(detail.high).toFixed(2) : '-');
  setVal('sd-m-low', detail.low != null ? Number(detail.low).toFixed(2) : '-');
  setVal('sd-m-open', detail.open != null ? Number(detail.open).toFixed(2) : '-');
  setVal('sd-m-prev', detail.prevClose != null ? Number(detail.prevClose).toFixed(2) : '-');
  setVal('sd-m-amount', detail.amount ? (Number(detail.amount) / 10000).toFixed(1) : '-');
  setVal('sd-m-turnover', detail.turnover != null ? Number(detail.turnover).toFixed(2) + '%' : '-');
  setVal('sd-m-vol-ratio', detail.volumeRatio != null ? Number(detail.volumeRatio).toFixed(2) : '-');
  setVal('sd-m-amplitude', detail.amplitude != null ? Number(detail.amplitude).toFixed(2) + '%' : '-');

  // 同步自选按钮文字
  updateFavButtons();
}

function updateFavButtons() {
  const isFav = state.watchlistItems && state.watchlistItems.some(i => i.symbol === sdState.symbol);
  const topStar = document.getElementById('sd-btn-star');
  const topText = document.getElementById('sd-star-text');
  const dashFav = document.getElementById('sd-btn-dash-fav');

  if (topText) {
    const str = isFav ? '已自选' : '自选';
    if (topText.textContent !== str) topText.textContent = str;
  }
  if (topStar) topStar.classList.toggle('btn-accent', !isFav);
  if (dashFav) {
    const favHtml = isFav
      ? '<svg class="svg-icon" aria-hidden="true"><use href="#i-star" /></svg> 已自选'
      : '<svg class="svg-icon" aria-hidden="true"><use href="#i-star" /></svg> 自选';
    if (dashFav.innerHTML !== favHtml) dashFav.innerHTML = favHtml;
    dashFav.classList.toggle('btn-accent', !isFav);
  }
}

function toggleFav() {
  const isFav = state.watchlistItems && state.watchlistItems.some(i => i.symbol === sdState.symbol);
  if (isFav) {
    api('DELETE', '/api/watchlists/items?watchlistId=' + state.activeGroup + '&symbol=' + sdState.symbol).then(() => {
      actions.refreshWatchlist();
      updateFavButtons();
    });
  } else {
    api('POST', '/api/watchlists/items', {
      watchlistId: state.activeGroup,
      symbol: sdState.symbol,
      name: sdState.name,
    }).then(() => {
      actions.refreshWatchlist();
      updateFavButtons();
    });
  }
}

// ---------- 加载数据核心流水线（带防闪烁静默刷新保护） ----------
async function loadChartData(showLoading = false) {
  const seq = ++_loadSeq;
  const loading = document.getElementById('sd-loading');

  // 中止上一个未完成的请求，避免并发竞态
  if (_chartAbortCtrl) {
    try { _chartAbortCtrl.abort(); } catch (e) {}
    _chartAbortCtrl = null;
  }
  const currentCtrl = new AbortController();
  _chartAbortCtrl = currentCtrl;

  if (showLoading) {
    hideChartError();
    if (loading) loading.style.display = 'flex';
  }

  // 10 秒超时防护
  const timeoutId = setTimeout(() => {
    if (_chartAbortCtrl === currentCtrl) {
      currentCtrl.abort('timeout');
    }
  }, 10000);

  try {
    if (sdState.period === 'minute') {
      const res = await authFetch('/api/market/minute?symbol=' + encodeURIComponent(sdState.symbol), {
        signal: currentCtrl.signal,
      });
      const json = await res.json();
      if (seq !== _loadSeq) return;

      if (!res.ok || (json && json.error)) {
        if (showLoading && !sdState.minuteData) {
          showChartError((json && json.message) || '分时数据获取失败');
        }
        return;
      }

      hideChartError();
      const points = (json && (json.points || json.items)) || [];
      const last = points.length ? points[points.length - 1] : null;
      const sig = last ? `${sdState.symbol}_min_${last.time}_${last.price}_${last.volume || 0}_${points.length}` : '';
      if (!showLoading && sig && sig === _lastMinuteSig) {
        return; // 数据无变动直接跳过，杜绝重绘闪烁
      }
      _lastMinuteSig = sig;

      sdState.minuteData = json;
      renderMinute(json);
      // renderMinute 内部 notMerge:true 会整体擦掉 graphic 层，立即用内存
      // 画线重挂；否则轮询期间（showLoading=false）划线会闪没
      if (drawingsCtrl) drawingsCtrl.render();
      if (showLoading) setTimeout(resizeAllCharts, 40);
    } else {
      const p = sdState.period === 'week' ? 'week' : (sdState.period === 'month' ? 'month' : 'day');
      const res = await authFetch(`/api/market/kline?symbol=${encodeURIComponent(sdState.symbol)}&period=${p}&fq=${sdState.fq}`, {
        signal: currentCtrl.signal,
      });
      const json = await res.json();
      if (seq !== _loadSeq) return;

      if (!res.ok || (json && json.error)) {
        if (showLoading && (!sdState.klineData || !sdState.klineData.length)) {
          showChartError((json && json.message) || 'K线数据获取失败');
        }
        return;
      }

      hideChartError();
      const rawList = (json && (json.candles || json.klines)) || [];
      const last = rawList.length ? rawList[rawList.length - 1] : null;
      const sig = last ? `${sdState.symbol}_${p}_${last.time || last.date}_${last.close}_${last.high || 0}_${last.low || 0}_${last.volume || 0}_${rawList.length}` : '';
      if (!showLoading && sig && sig === _lastKlineSig) {
        return;
      }
      _lastKlineSig = sig;

      sdState.klineData = rawList.map(c => ({
        date: c.time || c.date,
        open: Number(c.open),
        close: Number(c.close),
        high: Number(c.high),
        low: Number(c.low),
        volume: Number(c.volume || 0),
      }));
      renderKline(sdState.klineData);
      // 同上：renderKline notMerge 擦除后先以内存画线秒级恢复，reload 异步对账
      if (drawingsCtrl) drawingsCtrl.render();
      if (showLoading) setTimeout(resizeAllCharts, 40);
    }

    if (showLoading && drawingsCtrl) {
      drawingsCtrl.reload();
    }
  } catch (e) {
    if (seq === _loadSeq && showLoading) {
      if (e.name === 'AbortError' || currentCtrl.signal.aborted) {
        showChartError('行情数据加载超时，点击重试');
      } else {
        showChartError('行情数据连接异常，点击重试');
      }
    }
  } finally {
    clearTimeout(timeoutId);
    if (_chartAbortCtrl === currentCtrl) {
      _chartAbortCtrl = null;
    }
    if (seq === _loadSeq && loading) loading.style.display = 'none';
  }
}

async function loadDetailQuote() {
  try {
    const res = await authFetch('/api/market/detail?symbol=' + encodeURIComponent(sdState.symbol) + '&name=' + encodeURIComponent(sdState.name));
    const json = await res.json();
    updateDetailPanel(json);
  } catch (e) {}
}

// ---------- 外部主入口：openStockDetail ----------
function openStockDetail(symbol, name) {
  if (!symbol) return;
  sdState.active = true;
  sdState.symbol = symbol;
  sdState.name = safeDecode(name || symbol);
  _lastMinuteSig = null;
  _lastKlineSig = null;
  hideChartError();

  // 记录先前的页签
  const activeTabBtn = document.querySelector('nav.main-nav button[aria-selected="true"]');
  if (activeTabBtn) {
    sdState.previousTab = activeTabBtn.id.replace('tab-', '');
  }

  // 隐藏其他所有普通视图
  document.querySelectorAll('.main-column > section').forEach(sec => {
    if (sec.id !== 'view-stock-detail') sec.hidden = true;
  });

  // 主导航取消选中高亮（保持原7个tab不变，不插入个股详情title）
  document.querySelectorAll('nav.main-nav button').forEach(b => {
    b.setAttribute('aria-selected', 'false');
  });

  // 隐藏外部侧边栏，启用全宽双栏看盘，右侧看板默认完全展开展示
  const sideBody = document.querySelector('.workspace-body');
  if (sideBody) {
    sideBody.classList.add('stock-detail-active');
    sideBody.classList.remove('sd-side-collapsed');
  }
  // 右栏折叠统一由顶栏 #btn-toggle-side 接管：进入详情时同步其视觉态
  const sideToggleBtn = document.getElementById('btn-toggle-side');
  if (sideToggleBtn) {
    sideToggleBtn.setAttribute('aria-pressed', 'false');
    sideToggleBtn.classList.remove('active');
    sideToggleBtn.title = '折叠右侧看板 (⌘B)';
  }

  const v = document.getElementById('view-stock-detail');
  if (v) v.hidden = false;

  // 确保 DOM 展开完成并计算出真实像素尺寸后再初始化图表
  requestAnimationFrame(() => {
    initCharts();
    switchPeriod(sdState.period || 'minute');
    resumeStockDetail();
    setTimeout(resizeAllCharts, 60);
  });
}

// ---------- 交易时段自适应调度 (工作日开市3秒秒级刷新 / 闭市单次定格静止) ----------
function isSessionOpen() {
  const ms = globalThis.Fund.marketSession;
  if (ms && typeof ms.isMarketOpen === 'function') {
    return ms.isMarketOpen();
  }
  const d = new Date();
  const day = d.getDay();
  if (day === 0 || day === 6) return false;
  const hm = d.getHours() * 100 + d.getMinutes();
  return (hm >= 915 && hm <= 1130) || (hm >= 1300 && hm <= 1505);
}

function pauseStockDetail() {
  if (_refreshTimer) {
    clearTimeout(_refreshTimer);
    clearInterval(_refreshTimer);
    _refreshTimer = null;
  }
}

function resumeStockDetail() {
  pauseStockDetail();
  if (!sdState.active || !sdState.symbol) return;

  // 1. 无论开闭市，进入或激活个股详情时必然请求一次最新数据
  loadDetailQuote();
  loadChartData(false);

  // 2. 自适应调度：
  //    - 开市期间：3000ms 秒级平滑静默刷新
  //    - 闭市期间：停止高频轮询，60s 仅做低频时钟换挡检查，页面完全保持静止
  function scheduleNextTick() {
    if (!sdState.active || !sdState.symbol) return;
    const open = isSessionOpen();
    const delay = open ? 3000 : 60000;

    _refreshTimer = setTimeout(() => {
      if (!sdState.active || !sdState.symbol) return;
      if (isSessionOpen()) {
        loadDetailQuote();
        loadChartData(false);
      }
      scheduleNextTick();
    }, delay);
  }

  scheduleNextTick();
}

// ---------- 返回上一页 ----------
function exitStockDetail() {
  sdState.active = false;
  pauseStockDetail();
  hideChartError();
  if (_chartAbortCtrl) {
    try { _chartAbortCtrl.abort(); } catch (e) {}
    _chartAbortCtrl = null;
  }

  const sideBody = document.querySelector('.workspace-body');
  if (sideBody) {
    sideBody.classList.remove('stock-detail-active');
    sideBody.classList.remove('sd-side-collapsed');
  }
  // 顶栏折叠按钮在详情期间托管了右栏态，退出时恢复其外层侧栏视觉
  if (typeof actions.syncStockDetailSideBtn === 'function') actions.syncStockDetailSideBtn();

  const v = document.getElementById('view-stock-detail');
  if (v) v.hidden = true;

  // 切换回先前的页签并激活对应主导航按钮
  const targetTab = document.getElementById('tab-' + (sdState.previousTab || 'watchlist'));
  if (targetTab) {
    targetTab.click();
  }
}

// ---------- 绑定 UI 事件 ----------
function bindEvents() {
  const backBtn = document.getElementById('sd-btn-back');
  if (backBtn) backBtn.addEventListener('click', exitStockDetail);

  const resetBtn = document.getElementById('sd-zoom-reset-btn');
  if (resetBtn) resetBtn.addEventListener('click', resetZoomAll);

  const starBtn = document.getElementById('sd-btn-star');
  if (starBtn) starBtn.addEventListener('click', toggleFav);
  const dashFavBtn = document.getElementById('sd-btn-dash-fav');
  if (dashFavBtn) dashFavBtn.addEventListener('click', toggleFav);

  const refreshBtn = document.getElementById('sd-btn-refresh');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => {
      loadDetailQuote();
      loadChartData(true);
    });
  }
  const retryBtn = document.getElementById('sd-retry-btn');
  if (retryBtn) {
    retryBtn.addEventListener('click', () => {
      loadDetailQuote();
      loadChartData(true);
    });
  }

  // 周期按钮
  ['minute', 'dayk', 'week', 'month'].forEach(p => {
    const btn = document.getElementById('sd-view-' + p);
    if (btn) btn.addEventListener('click', () => switchPeriod(p));
  });

  // 缩放范围按钮（支持全部 800 根展示）
  const rangeGroup = document.getElementById('sd-range-group');
  if (rangeGroup) {
    rangeGroup.querySelectorAll('button[data-range]').forEach(btn => {
      btn.addEventListener('click', () => {
        rangeGroup.querySelectorAll('button[data-range]').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        sdState.range = parseInt(btn.dataset.range, 10) || 0;
        if (sdState.range === 0) {
          resetZoomAll();
        } else {
          const total = sdState.klineData.length;
          if (total > 0 && klineChart) {
            const startPct = Math.max(0, (1 - sdState.range / total) * 100);
            klineChart.dispatchAction({ type: 'dataZoom', start: startPct, end: 100 });
            if (volumeChart) volumeChart.dispatchAction({ type: 'dataZoom', start: startPct, end: 100 });
            if (indicatorChart) indicatorChart.dispatchAction({ type: 'dataZoom', start: startPct, end: 100 });
          }
        }
      });
    });
  }

  // 副图指标按钮
  const indBar = document.getElementById('sd-indicator-bar');
  if (indBar) {
    indBar.querySelectorAll('.it-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        indBar.querySelectorAll('.it-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        sdState.indicator = btn.dataset.ind || 'none';
        renderSubIndicator(sdState.klineData);
      });
    });
  }

  // 复权方式按钮切换 (qfq/hfq/bf)
  const fqGroup = document.getElementById('sd-fq-group');
  if (fqGroup) {
    fqGroup.querySelectorAll('button[data-fq]').forEach(btn => {
      btn.addEventListener('click', () => {
        fqGroup.querySelectorAll('button[data-fq]').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        sdState.fq = btn.dataset.fq || 'qfq';
        loadChartData(true);
      });
    });
  }

  // 搜索框联想与切换标的
  const searchInput = document.getElementById('sd-search-input');
  const searchResults = document.getElementById('sd-search-results');
  let searchTimer = null;
  if (searchInput && searchResults) {
    searchInput.addEventListener('input', () => {
      clearTimeout(searchTimer);
      const kw = searchInput.value.trim();
      if (!kw) { searchResults.style.display = 'none'; return; }
      searchTimer = setTimeout(async () => {
        try {
          const res = await authFetch('/api/market/suggest?input=' + encodeURIComponent(kw));
          const list = await res.json();
          if (list && list.length) {
            searchResults.innerHTML = list.map(item => `
              <div class="sd-search-item" data-code="${item.code}" data-name="${item.name}">
                <span>${item.name}</span>
                <span style="color:#777;font-family:var(--font-mono)">${item.code}</span>
              </div>
            `).join('');
            searchResults.style.display = 'block';
            searchResults.querySelectorAll('.sd-search-item').forEach(el => {
              el.addEventListener('click', () => {
                const code = el.dataset.code;
                const name = el.dataset.name;
                searchInput.value = '';
                searchResults.style.display = 'none';
                openStockDetail(code, name);
              });
            });
          } else {
            searchResults.style.display = 'none';
          }
        } catch (e) {}
      }, 200);
    });

    document.addEventListener('click', e => {
      if (!e.target.closest('.sd-search-wrap')) {
        searchResults.style.display = 'none';
      }
    });
  }

  // 右栏折叠统一由顶栏 #btn-toggle-side 接管（app.js 统一分发点击与 ⌘B）：
  // 个股详情激活时切换单独的右栏（sd-side-collapsed），不触碰外层侧栏状态
  function setSdSideCollapsed(collapsed) {
    const wb = document.querySelector('.workspace-body');
    if (!wb) return;
    wb.classList.toggle('sd-side-collapsed', collapsed);
    const btn = document.getElementById('btn-toggle-side');
    if (btn) {
      btn.setAttribute('aria-pressed', String(collapsed));
      btn.classList.toggle('active', collapsed);
      btn.title = (collapsed ? '展开右侧看板' : '折叠右侧看板') + ' (⌘B)';
    }
    resizeAllCharts();
    setTimeout(resizeAllCharts, 50);
  }
  actions.isStockDetailActive = () => sdState.active;
  actions.toggleStockDetailSide = () => {
    const wb = document.querySelector('.workspace-body');
    setSdSideCollapsed(!(wb && wb.classList.contains('sd-side-collapsed')));
  };
  actions.syncStockDetailSideBtn = () => {
    // 退出个股详情时恢复顶栏按钮对外层侧栏的视觉态
    const btn = document.getElementById('btn-toggle-side');
    if (!btn) return;
    const collapsed = document.querySelector('.workspace-body')?.classList.contains('side-collapsed');
    btn.setAttribute('aria-pressed', String(!!collapsed));
    btn.classList.toggle('active', !!collapsed);
    btn.title = (collapsed ? '展开看板' : '折叠看板') + ' (⌘B)';
  };

  // 左右双栏拖拽调宽分隔条 (#sd-side-resizer)
  const sdResizer = document.getElementById('sd-side-resizer');
  if (sdResizer) {
    let sdResizing = false;
    sdResizer.addEventListener('mousedown', e => {
      sdResizing = true;
      sdResizer.classList.add('dragging');
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      e.preventDefault();
    });
    document.addEventListener('mousemove', e => {
      if (!sdResizing) return;
      const newW = window.innerWidth - e.clientX;
      const boundedW = Math.max(260, Math.min(680, newW));
      document.documentElement.style.setProperty('--fund-side-width', boundedW + 'px');
      resizeAllCharts();
    });
    document.addEventListener('mouseup', () => {
      if (!sdResizing) return;
      sdResizing = false;
      sdResizer.classList.remove('dragging');
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      const w = parseInt(getComputedStyle(document.documentElement).getPropertyValue('--fund-side-width'), 10);
      try { if (!isNaN(w)) localStorage.setItem('natives-fund-side-width', String(w)); } catch (e) {}
      resizeAllCharts();
    });
  }

  // 右侧盘口卡片流折叠手风琴交互
  document.querySelectorAll('.sd-right-panel .side-card-header').forEach(header => {
    header.addEventListener('click', () => {
      header.closest('.side-card')?.classList.toggle('collapsed');
    });
  });
}

// 注册全局动作
actions.openStockDetail = openStockDetail;
actions.openStockDetailView = openStockDetail;
actions.exitStockDetail = exitStockDetail;
actions.pauseStockDetail = pauseStockDetail;
actions.resumeStockDetail = resumeStockDetail;

document.addEventListener('DOMContentLoaded', bindEvents);

})();
