(() => {
const { state, actions } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

// ===== 技术指标计算与渲染 =====
function switchIndicator(ind) {
  tState.currentIndicator = ind;
  document.querySelectorAll('.it-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.ind === ind);
  });
  const chart = document.getElementById('indicator-chart');
  const toolbar = document.getElementById('indicator-toolbar');
  if (ind === 'none') {
    if (chart) chart.style.display = 'none';
    if (toolbar) toolbar.style.display = 'none';
    clearBolloverlay();
  } else {
    if (ind !== 'boll') clearBolloverlay();
    if (chart) chart.style.display = 'block';
    if (toolbar) toolbar.style.display = 'flex';
    setTimeout(() => { if (charts.indicatorChart) charts.indicatorChart.resize(); }, 50);
    renderIndicator(ind);
  }
}

function clearBolloverlay() {
  if (!charts.klineChart || !tState.klineData.length) return;
  const opt = charts.klineChart.getOption();
  const series = opt.series || [];
  // 如果有BOLL系列（超过5个系列：K线+MA5+MA10+MA20+MA60=5），重新渲染K线清除
  if (series.length > 5) {
    if (tState.lastSignalData && trends.chart?.renderKline) {
      trends.chart.renderKline(tState.klineData, tState.lastSignalData);
      if (tState.dailyChanlun && !tState.dailyChanlun.error && trends.chan?.applyChanlunDailyOverlay) {
        trends.chan.applyChanlunDailyOverlay(tState.dailyChanlun);
      }
    }
  }
}

function renderIndicator(ind) {
  if (!charts.indicatorChart) return;
  if (ind === 'none' || !tState.klineData.length) {
    charts.indicatorChart.setOption({}, true);
    return;
  }
  const dates = tState.klineData.map(k => k.date);
  const closes = tState.klineData.map(k => k.close);
  const highs = tState.klineData.map(k => k.high);
  const lows = tState.klineData.map(k => k.low);

  let series = [];
  let legend = [];
  let yMin, yMax;

  if (ind === 'macd') {
    const r = calcMACD(closes);
    series = [
      { name: 'DIF', type: 'line', data: r.dif, symbol: 'none', lineStyle: { color: C.ma20, width: 1 } },
      { name: 'DEA', type: 'line', data: r.dea, symbol: 'none', lineStyle: { color: C.ma10, width: 1 } },
      { name: 'MACD', type: 'bar', data: r.macd.map(v => ({
        value: v, itemStyle: { color: v >= 0 ? C.up + '88' : C.down + '88' }
      })) },
    ];
    legend = ['DIF', 'DEA', 'MACD'];
  } else if (ind === 'rsi') {
    const rsi6 = calcRSI(closes, 6);
    const rsi12 = calcRSI(closes, 12);
    const rsi24 = calcRSI(closes, 24);
    series = [
      { name: 'RSI6', type: 'line', data: rsi6, symbol: 'none', lineStyle: { color: C.up, width: 1 } },
      { name: 'RSI12', type: 'line', data: rsi12, symbol: 'none', lineStyle: { color: C.ma20, width: 1 } },
      { name: 'RSI24', type: 'line', data: rsi24, symbol: 'none', lineStyle: { color: C.ma10, width: 1 } },
    ];
    legend = ['RSI6', 'RSI12', 'RSI24'];
    yMin = 0; yMax = 100;
  } else if (ind === 'kdj') {
    const r = calcKDJ(highs, lows, closes);
    series = [
      { name: 'K', type: 'line', data: r.k, symbol: 'none', lineStyle: { color: C.up, width: 1 } },
      { name: 'D', type: 'line', data: r.d, symbol: 'none', lineStyle: { color: C.ma20, width: 1 } },
      { name: 'J', type: 'line', data: r.j, symbol: 'none', lineStyle: { color: C.ma10, width: 1 } },
    ];
    legend = ['K', 'D', 'J'];
    yMin = 0; yMax = 100;
  } else if (ind === 'boll') {
    const r = calcBOLL(closes, 20);
    // BOLL直接画在主K线图上
    if (charts.klineChart) {
      charts.klineChart.setOption({
        series: [
          {}, {}, {}, {}, {},
          { name: 'BOLL上轨', type: 'line', data: r.upper, symbol: 'none', lineStyle: { color: '#4fc3f7', width: 1, type: 'dashed' } },
          { name: 'BOLL中轨', type: 'line', data: r.mid, symbol: 'none', lineStyle: { color: '#ffeb3b', width: 1 } },
          { name: 'BOLL下轨', type: 'line', data: r.lower, symbol: 'none', lineStyle: { color: '#4fc3f7', width: 1, type: 'dashed' } },
        ]
      });
    }
    charts.indicatorChart.setOption({
      title: { text: 'BOLL已叠加到主图', left: 'center', top: 'center', textStyle: { color: '#666', fontSize: 13 } }
    }, true);
    return;
  } else if (ind === 'wr') {
    const wr6 = calcWR(highs, lows, closes, 6);
    const wr10 = calcWR(highs, lows, closes, 10);
    series = [
      { name: 'WR6', type: 'line', data: wr6, symbol: 'none', lineStyle: { color: C.up, width: 1 } },
      { name: 'WR10', type: 'line', data: wr10, symbol: 'none', lineStyle: { color: C.ma20, width: 1 } },
    ];
    legend = ['WR6', 'WR10'];
    yMin = 0; yMax = 100;
  }

  charts.indicatorChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    legend: { data: legend, textStyle: { color: C.textDim, fontSize: 10 }, top: 2, itemWidth: 12, itemHeight: 8 },
    xAxis: { type: 'category', data: dates, show: false },
    yAxis: {
      type: 'value',
      min: yMin, max: yMax,
      axisLabel: { color: C.textDim, fontSize: 9 },
      splitLine: { lineStyle: { color: C.grid } },
    },
    grid: { left: 60, right: 50, top: 20, bottom: 5 },
    series: series,
    tooltip: {
      trigger: 'axis', axisPointer: { type: 'cross', lineStyle: { color: '#666' } },
      formatter: (params) => {
        if (!params || !params.length) return '';
        let html = `<div style="font-size:11px">${params[0].axisValue}</div>`;
        for (const p of params) {
          if (p.value != null) html += `<div><span style="color:${p.color}">●</span> ${p.seriesName}: ${(+p.value).toFixed(3)}</div>`;
        }
        return html;
      }
    },
    dataZoom: [
      { type: 'inside', start: 0, end: 100, zoomOnMouseWheel: true, moveOnMouseMove: true },
      { type: 'slider', start: 0, end: 100, show: false },
    ],
  }, true);
}

// EMA计算
function calcEMA(data, period) {
  const result = new Array(data.length).fill(null);
  if (data.length < period) return result;
  const k = 2 / (period + 1);
  let ema = data.slice(0, period).reduce((a, b) => a + b, 0) / period;
  result[period - 1] = ema;
  for (let i = period; i < data.length; i++) {
    ema = data[i] * k + ema * (1 - k);
    result[i] = ema;
  }
  return result;
}

// MACD计算
function calcMACD(closes) {
  const fast = 12, slow = 26, signal = 9;
  const emaFast = calcEMA(closes, fast);
  const emaSlow = calcEMA(closes, slow);
  const dif = closes.map((_, i) => {
    if (emaFast[i] == null || emaSlow[i] == null) return null;
    return +(emaFast[i] - emaSlow[i]).toFixed(4);
  });
  const difValid = dif.map(v => v == null ? 0 : v);
  const dea = calcEMA(difValid, signal).map(v => v == null ? null : +v.toFixed(4));
  const macd = dif.map((d, i) => {
    if (d == null || dea[i] == null) return null;
    return +((d - dea[i]) * 2).toFixed(4);
  });
  return { dif, dea, macd };
}

// RSI计算
function calcRSI(closes, period) {
  const result = new Array(closes.length).fill(null);
  if (closes.length < period + 1) return result;
  let avgGain = 0, avgLoss = 0;
  for (let i = 1; i <= period; i++) {
    const change = closes[i] - closes[i - 1];
    if (change >= 0) avgGain += change; else avgLoss -= change;
  }
  avgGain /= period;
  avgLoss /= period;
  result[period] = avgLoss === 0 ? 100 : +(100 - 100 / (1 + avgGain / avgLoss)).toFixed(2);
  for (let i = period + 1; i < closes.length; i++) {
    const change = closes[i] - closes[i - 1];
    const gain = change >= 0 ? change : 0;
    const loss = change < 0 ? -change : 0;
    avgGain = (avgGain * (period - 1) + gain) / period;
    avgLoss = (avgLoss * (period - 1) + loss) / period;
    result[i] = avgLoss === 0 ? 100 : +(100 - 100 / (1 + avgGain / avgLoss)).toFixed(2);
  }
  return result;
}

// KDJ计算
function calcKDJ(highs, lows, closes, period = 9) {
  const k = new Array(closes.length).fill(null);
  const d = new Array(closes.length).fill(null);
  const j = new Array(closes.length).fill(null);
  let prevK = 50, prevD = 50;
  for (let i = period - 1; i < closes.length; i++) {
    let hh = -Infinity, ll = Infinity;
    for (let m = 0; m < period; m++) {
      hh = Math.max(hh, highs[i - m]);
      ll = Math.min(ll, lows[i - m]);
    }
    const rsv = hh === ll ? 50 : ((closes[i] - ll) / (hh - ll)) * 100;
    const curK = (2 / 3) * prevK + (1 / 3) * rsv;
    const curD = (2 / 3) * prevD + (1 / 3) * curK;
    k[i] = +curK.toFixed(2);
    d[i] = +curD.toFixed(2);
    j[i] = +(3 * curK - 2 * curD).toFixed(2);
    prevK = curK;
    prevD = curD;
  }
  return { k, d, j };
}

// BOLL计算
function calcBOLL(closes, period = 20, mult = 2) {
  const mid = new Array(closes.length).fill(null);
  const upper = new Array(closes.length).fill(null);
  const lower = new Array(closes.length).fill(null);
  for (let i = period - 1; i < closes.length; i++) {
    const slice = closes.slice(i - period + 1, i + 1);
    const ma = slice.reduce((a, b) => a + b, 0) / period;
    const variance = slice.reduce((a, b) => a + (b - ma) ** 2, 0) / period;
    const std = Math.sqrt(variance);
    mid[i] = +ma.toFixed(3);
    upper[i] = +(ma + mult * std).toFixed(3);
    lower[i] = +(ma - mult * std).toFixed(3);
  }
  return { mid, upper, lower };
}

// WR (威廉指标) 计算
function calcWR(highs, lows, closes, period) {
  const result = new Array(closes.length).fill(null);
  for (let i = period - 1; i < closes.length; i++) {
    let hh = -Infinity, ll = Infinity;
    for (let m = 0; m < period; m++) {
      hh = Math.max(hh, highs[i - m]);
      ll = Math.min(ll, lows[i - m]);
    }
    result[i] = hh === ll ? 0 : +(((hh - closes[i]) / (hh - ll)) * 100).toFixed(2);
  }
  return result;
}

// 绑定指标切换按钮事件与全局导出
document.querySelectorAll('.it-btn').forEach(btn => {
  btn.onclick = () => switchIndicator(btn.dataset.ind);
});
window.switchIndicator = switchIndicator;

trends.indicator = {
  switchIndicator,
  clearBolloverlay,
  renderIndicator,
  calcEMA,
  calcMACD,
  calcRSI,
  calcKDJ,
  calcBOLL,
  calcWR,
};

})();
