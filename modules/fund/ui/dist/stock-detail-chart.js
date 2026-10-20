(() => {
const { state, actions } = globalThis.Fund;

// Fund 股票详情子系统 · 图表渲染层（Stock Detail Charts）
// 职责边界（从 stock-detail.js 控制器中按职责拆出，行为保持不变）：
// 1. ECharts 五图初始化、主题配色与多图 dataZoom 联动
// 2. 分时图 / K 线 / 成交量 / 副图技术指标的 option 构建与渲染
// 3. 局部框选缩放（zoomToRange）与全景重置（resetZoomAll）
// 4. 主题切换时按当前数据重绘（reloadStockDetailTheme）
// 共享状态（sdState、图表实例、画图控制器）挂载在 StockDetailShared，
// 由 stock-detail.js 控制器在同一全局作用域内读写。

globalThis.Fund.StockDetailShared = {
  sdState: {
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
  },
  charts: { minute: null, minuteVol: null, kline: null, volume: null, indicator: null },
  drawingsCtrl: null,
  // 以下函数在文件末尾赋值，供控制器调用
  initCharts: null,
  renderMinute: null,
  renderKline: null,
  renderSubIndicator: null,
  resizeAllCharts: null,
  resetZoomAll: null,
};

const S = globalThis.Fund.StockDetailShared;
const sdState = S.sdState;
const C = S.charts;

function getThemeColors() {
  // DES 02：语义角色统一经 CSS 变量解析（涨跌由 colorMode owner 控制
  // data-color-mode，变量级联已经处理反转），不再维护 isLight 分支色表。
  const isIntl = document.documentElement.getAttribute('data-color-mode') === 'intl' || state.colorMode === 'intl';
  return {
    isLight: document.documentElement.dataset.theme === 'archive',
    isIntl,
    colorUp: Fund.themeColor('--color-up'),
    colorDown: Fund.themeColor('--color-down'),
    axis: Fund.themeColor('--chart-axis'),
    grid: Fund.themeColor('--chart-grid'),
    text: Fund.themeColor('--chart-text'),
    textDim: Fund.themeColor('--chart-text-dim'),
    avgLine: Fund.themeColor('--natives-accent'),
    tooltipBg: Fund.themeRgba('--natives-surface', 0.95),
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

function formatVolume(value) {
  return new Intl.NumberFormat(state.language==='en'?'en':'zh-CN',{notation:'compact',maximumFractionDigits:1}).format(value);
}

// 三图缩放联动由 initCharts 里的 echarts.connect（kline/volume/indicator 组）承担：
// 此前这里还有一层手写 datazoom→dispatchAction 同步，其 _isZoomSyncing 标志会被
// 嵌套触发的事件 handler 的 finally 提前复位，与 connect 叠加后形成互相递归的
// 事件风暴（Maximum call stack size exceeded，滚轮缩放必现）。已删除手写层。

// ---------- ECharts 初始化 ----------
function initCharts() {
  if (C.minute && C.kline) return true;
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
      axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) },
      splitLine: { show: false },
    },
    valueAxis: {
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) },
      splitLine: { lineStyle: { color: tc.grid } },
    },
    tooltip: {
      backgroundColor: tc.tooltipBg,
      borderColor: tc.axis,
      textStyle: { color: tc.text, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) },
    },
  };

  C.minute = echarts.init(mEl);
  C.minuteVol = echarts.init(mvEl);
  C.kline = echarts.init(kEl);
  C.volume = echarts.init(vEl);
  C.indicator = echarts.init(indEl);

  for(const chart of [C.minute,C.kline])chart.on('datazoom',()=>document.dispatchEvent(new Event('fund:research-viewport-changed')));

  C.minute.setOption(baseOpts);
  C.minuteVol.setOption(baseOpts);
  C.kline.setOption(baseOpts);
  C.volume.setOption(baseOpts);
  C.indicator.setOption(baseOpts);

  // 多图光标十字轴联动 + dataZoom 双向同步（echarts.connect 原生承担）
  try {
    echarts.connect([C.minute, C.minuteVol]);
    echarts.connect([C.kline, C.volume, C.indicator]);
  } catch (e) {}

  window.addEventListener('resize', resizeAllCharts);

  // 初始化画图控制器（传入 onZoomToRange 回调）
  S.drawingsCtrl = globalThis.Fund.createStockDrawings({
    getKlineData: () => (sdState.period === 'minute' ? ((sdState.minuteData && (sdState.minuteData.points || sdState.minuteData.items)) || []) : sdState.klineData),
    getSymbol: () => sdState.symbol,
    getPeriod: () => sdState.period,
    getFq: () => sdState.fq,
    getBusinessDate: () => sdState.minuteData?.date,
    onZoomToRange: (d1, d2) => zoomToRange(d1, d2),
  });

  return true;
}

actions.resizeStockDetailCharts = resizeAllCharts;

// ---------- 局部框选缩放与全景重置 ----------
function zoomToRange(d1, d2) {
  const data = sdState.klineData;
  if (!data || !data.length || !C.kline) return;
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

  C.kline.dispatchAction({ type: 'dataZoom', start: startPct, end: endPct });
  if (C.volume) C.volume.dispatchAction({ type: 'dataZoom', start: startPct, end: endPct });
  if (C.indicator) C.indicator.dispatchAction({ type: 'dataZoom', start: startPct, end: endPct });

  const resetBtn = document.getElementById('sd-zoom-reset-btn');
  if (resetBtn) resetBtn.style.display = 'inline-flex';
}

function resetZoomAll() {
  sdState.range = 0;
  if (C.kline) C.kline.dispatchAction({ type: 'dataZoom', start: 0, end: 100 });
  if (C.volume) C.volume.dispatchAction({ type: 'dataZoom', start: 0, end: 100 });
  if (C.indicator) C.indicator.dispatchAction({ type: 'dataZoom', start: 0, end: 100 });
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
  [C.minute, C.minuteVol, C.kline, C.volume, C.indicator].forEach(c => {
    if (c && typeof c.resize === 'function') c.resize();
  });
  // graphic 元素是像素坐标，图表尺寸变化后立即按新坐标系重挂，防止画线飘移
  if (S.drawingsCtrl) S.drawingsCtrl.render();
}

// ---------- 渲染分时图 ----------
// A 股全日 241 分钟格：09:30-11:30（121 格）+ 13:01-15:00（120 格）。
// 时间轴必须按开市时段固定，上午盘只占左半边、午休后下午盘在右半边，
// 不能把已发生的点均分拉满全宽（否则盘中/午休看图比例完全失真）。
function buildFullTimeline() {
  const times = [];
  const push = (t) => times.push(`${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`);
  for (let t = 9 * 60 + 30; t <= 11 * 60 + 30; t++) push(t);
  for (let t = 13 * 60 + 1; t <= 15 * 60; t++) push(t);
  return times;
}

const MINUTE_AXIS_TICKS = [0, 60, 120, 180, 240];

function renderMinute(data) {
  if (!C.minute || !C.minuteVol || !data) return;
  const minute = Fund.researchSeries.minute(data), points = data.points;
  const tc = getThemeColors();

  if (!minute.prices.some(Number.isFinite) || minute.pre_close === null) {
    C.minute.setOption({
      title: { text: Fund.t(minute.prices.some(Number.isFinite) ? 'fundTrendsMinuteReferenceMissing' : 'fundTrendsMinuteEmpty'), left: 'center', top: 'center', textStyle: { color: tc.textDim, fontSize: 13 } }
    }, true);
    C.minuteVol.setOption({}, true);
    return;
  }

  const preClose = minute.pre_close;
  const fullTimes = buildFullTimeline();
  const pointIndex = new Map(points.map((d, i) => [d.time, i]));
  const aligned = values => fullTimes.map(t => {
    const i = pointIndex.get(t);
    return i == null ? null : values[i];
  });
  const prices = aligned(minute.prices);
  const vwaps = aligned(minute.avg_prices);
  const vols = aligned(minute.volumes);
  // 时间标签跟随实际点数推进（上午盘 121 格处显示午休分隔）
  const lastIdx = (() => {
    for (let i = fullTimes.length - 1; i >= 0; i--) if (prices[i] != null) return i;
    return 0;
  })();

  let minP = Infinity, maxP = -Infinity;
  prices.concat(vwaps).forEach(p => {
    if (p != null && !isNaN(p)) { minP = Math.min(minP, p); maxP = Math.max(maxP, p); }
  });
  const maxDiff = Math.max(Math.abs(maxP - preClose), Math.abs(minP - preClose), preClose * 0.01);
  const yMin = preClose - maxDiff;
  const yMax = preClose + maxDiff;
  const xLabel = {
    color: tc.textDim,
    fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')),
    interval: (index) => MINUTE_AXIS_TICKS.includes(index) && index <= lastIdx,
    formatter: (val, index) => (index === 120 ? '11:30/13:00' : val),
  };

  const minOpt = {
    animation: false,
    grid: { left: 55, right: 48, top: 10, bottom: 6 },
    xAxis: {
      type: 'category',
      data: fullTimes,
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: { show: false },
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
        name: Fund.t('fundTrendsPrice'),
        type: 'line',
        data: prices,
        showSymbol: false,
        connectNulls: false,
        lineStyle: { width: 1.5, color: Fund.themeColor('--natives-accent') },
        areaStyle: {
          color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
            { offset: 0, color: Fund.themeRgba('--natives-accent', 0.25) },
            { offset: 1, color: Fund.themeRgba('--natives-accent', 0.0) },
          ]),
        },
      },
      {
        name: Fund.t('fundTrendsAveragePrice'),
        type: 'line',
        data: vwaps,
        showSymbol: false,
        connectNulls: false,
        lineStyle: { width: 1.2, color: tc.avgLine, type: 'dashed' },
      },
    ],
  };

  const volColors = prices.map((p, i) => {
    const prev = i === 0 || p == null ? preClose : prices[i-1];
    return p >= prev ? tc.colorUp : tc.colorDown;
  });

  const volOpt = {
    animation: false,
    grid: { left: 55, right: 35, top: 4, bottom: 16 },
    xAxis: {
      type: 'category',
      data: fullTimes,
      axisLine: { lineStyle: { color: tc.axis } },
      axisLabel: xLabel,
    },
    yAxis: {
      type: 'value',
      splitLine: { lineStyle: { color: tc.grid } },
      axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')), formatter: formatVolume },
    },
    series: [
      {
        name: Fund.t('fundChartvolume'),
        type: 'bar',
        data: vols.map((v, i) => (v == null ? null : { value: v, itemStyle: { color: volColors[i] } })),
      },
    ],
  };

  C.minute.setOption(minOpt, { notMerge: true });
  C.minuteVol.setOption(volOpt, { notMerge: true });
}

// ---------- 渲染 K 线与技术指标 ----------
function renderKline(klines) {
  if (!C.kline || !C.volume || !klines) return;
  const tc = getThemeColors();

  if (!klines.length) {
    C.kline.setOption({
      title: { text: Fund.t('fundChartCandleEmpty'), left: 'center', top: 'center', textStyle: { color: tc.textDim, fontSize: 13 } }
    }, true);
    C.volume.setOption({}, true);
    return;
  }

  const dates = klines.map(d => d.date);
  const ohlc = klines.map(d => [d.open, d.close, d.low, d.high]);
  const vols = klines.map(d => Number.isFinite(d.volume) && d.volume>=0 ? d.volume : null);

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
      axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) },
      splitLine: { show: false },
    },
    yAxis: {
      type: 'value',
      scale: true,
      axisLine: { lineStyle: { color: tc.axis } },
      splitLine: { lineStyle: { color: tc.grid } },
      axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')), formatter: v => v.toFixed(2) },
    },
    dataZoom: [
      { type: 'inside', start: startIdx, end: 100, zoomOnMouseWheel: true, moveOnMouseMove: true, xAxisIndex: [0] },
    ],
    series: [
      {
        name: Fund.t('fundChartCandle'),
        type: 'candlestick',
        data: ohlc,
        itemStyle: {
          color: tc.colorUp,
          color0: tc.colorDown,
          borderColor: tc.colorUp,
          borderColor0: tc.colorDown,
        },
      },
      { name: 'MA5', type: 'line', data: ma5, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-4') } },
      { name: 'MA10', type: 'line', data: ma10, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-5') } },
      { name: 'MA20', type: 'line', data: ma20, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-6') } },
      { name: 'MA30', type: 'line', data: ma30, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-3') } },
      { name: 'MA60', type: 'line', data: ma60, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-2') } },
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
      axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')), formatter: formatVolume },
    },
    dataZoom: [
      { type: 'inside', start: startIdx, end: 100, zoomOnMouseWheel: true, moveOnMouseMove: true, xAxisIndex: [0] },
    ],
    series: [
      {
        name: Fund.t('fundChartvolume'),
        type: 'bar',
        data: vols.map((v, i) => v===null ? null : { value: v, itemStyle: { color: volBarColors[i] } }),
      },
    ],
  };

  C.kline.setOption(klineOpt, { notMerge: true });
  C.volume.setOption(volOpt, { notMerge: true });

  // 渲染副图指标（复用 Fund.indicators，严格同步主图缩放范围）
  renderSubIndicator(klines);
}

function renderSubIndicator(klines) {
  if (!C.indicator) return;
  const indEl = document.getElementById('sd-indicator-chart');
  if (sdState.indicator === 'none') {
    if (indEl) indEl.style.display = 'none';
    resizeAllCharts();
    return;
  }
  if (indEl) indEl.style.display = 'block';

  // 读取当前主图 K 线的实际数据缩放区间，保证副图与主图严格同步对齐！
  const zoom = getCurrentZoom(C.kline);
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
      { name: 'DIF', type: 'line', data: r.dif, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-6') } },
      { name: 'DEA', type: 'line', data: r.dea, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-5') } },
      {
        name: 'MACD', type: 'bar',
        data: r.macd.map(v => ({ value: v, itemStyle: { color: v >= 0 ? tc.colorUp : tc.colorDown } })),
      },
    ];
  } else if (ind === 'kdj') {
    const r = indEngine.calcKDJ(highs, lows, closes);
    series = [
      { name: 'K', type: 'line', data: r.k, showSymbol: false, lineStyle: { width: 1.2, color: tc.colorUp } },
      { name: 'D', type: 'line', data: r.d, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-6') } },
      { name: 'J', type: 'line', data: r.j, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-5') } },
    ];
  } else if (ind === 'rsi') {
    const rsi14 = indEngine.calcRSI(closes, 14);
    series = [
      { name: 'RSI(14)', type: 'line', data: rsi14, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--natives-accent') } },
    ];
  } else if (ind === 'wr') {
    const wr = indEngine.calcWR(highs, lows, closes, 14);
    series = [
      { name: 'WR(14)', type: 'line', data: wr, showSymbol: false, lineStyle: { width: 1.2, color: Fund.themeColor('--series-4') } },
    ];
  } else if (ind === 'boll') {
    const boll = indEngine.calcBOLL(closes, 20);
    series = [
      { name: 'UP', type: 'line', data: boll.upper, showSymbol: false, lineStyle: { width: 1, color: Fund.themeColor('--series-6'), type: 'dashed' } },
      { name: 'MID', type: 'line', data: boll.mid, showSymbol: false, lineStyle: { width: 1, color: Fund.themeColor('--series-4') } },
      { name: 'LOW', type: 'line', data: boll.lower, showSymbol: false, lineStyle: { width: 1, color: Fund.themeColor('--series-6'), type: 'dashed' } },
    ];
  }

  const opt = {
    animation: false,
    grid: { left: 55, right: 35, top: 4, bottom: 16 },
    xAxis: { type: 'category', data: dates, axisLine: { lineStyle: { color: tc.axis } }, axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) } },
    yAxis: { type: 'value', scale: true, splitLine: { lineStyle: { color: tc.grid } }, axisLabel: { color: tc.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) } },
    dataZoom: [{ type: 'inside', start: zoom.start, end: zoom.end, zoomOnMouseWheel: true, moveOnMouseMove: true, xAxisIndex: [0] }],
    series: series,
  };
  C.indicator.setOption(opt, { notMerge: true });
  setTimeout(resizeAllCharts, 30);
}

function reloadStockDetailTheme() {
  const zooms = Object.entries(C).map(([key, chart]) => [key, chart?.getOption().dataZoom]);
  if (C.minute && C.kline) {
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
    [C.minute, C.minuteVol, C.kline, C.volume, C.indicator].forEach(c => {
      if (c && typeof c.setOption === 'function') c.setOption(baseOpts);
    });
  }
  if (sdState.active && sdState.symbol) {
    if (sdState.instrumentKind === 'fund') { C.kline?.setOption(Fund.researchSeries.navOption(Fund.researchSeries.navRows(sdState.navData || [], sdState.period)), true); for (const [key, zoom] of zooms) if (zoom?.length) C[key]?.setOption({dataZoom:zoom}); return; }
    if (sdState.period === 'minute' && sdState.minuteData) {
      renderMinute(sdState.minuteData);
    } else if (sdState.klineData && sdState.klineData.length) {
      renderKline(sdState.klineData);
    }
  }
  for (const [key, zoom] of zooms) {
    if (zoom?.length) C[key].setOption({ dataZoom: zoom.map(({ start, end, startValue, endValue }) => ({ start, end, startValue, endValue })) });
  }
  S.drawingsCtrl?.render();
}
actions.reloadStockDetailTheme = reloadStockDetailTheme;

S.initCharts = initCharts;
S.renderMinute = renderMinute;
S.renderKline = renderKline;
S.renderSubIndicator = renderSubIndicator;
S.resizeAllCharts = resizeAllCharts;
S.resetZoomAll = resetZoomAll;

})();
