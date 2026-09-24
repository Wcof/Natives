(() => {
const { state, actions, authFetch } = globalThis.Fund;

// Trends 趋势分析子系统共享命名空间
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = (trends.state = trends.state || {
  currentSymbol: '',
  currentStockName: '',
  currentView: 'dayk',
  mode: 'pro',
  klineData: [],
  lastSignalData: null,
  signalLines: [],
  signalPoints: [],
  dailyChanlun: null,
  minuteData: null,
  minuteChanlun: null,
  minuteYRange: null,
  dailyFlows: null,
  flowMode: 'realtime',
  currentIndicator: 'none',
  lastSignal: {},
});

// 6 个独立初始化的 ECharts 实例池
const charts = (trends.charts = trends.charts || {
  klineChart: null,
  volumeChart: null,
  flowChart: null,
  minuteChart: null,
  minuteVolChart: null,
  indicatorChart: null,
});

// 全局主题色
const C = (trends.C = {
  bg: 'transparent',
  up: '#ff2d2d',       // 涨-红
  down: '#00b35c',     // 跌-绿
  ma5: '#ffeb3b',      // MA5-黄
  ma10: '#e040fb',     // MA10-紫
  ma20: '#4fc3f7',     // MA20-蓝
  ma60: '#ff9800',     // MA60-橙
  text: '#aaa',
  textDim: '#666',
  grid: '#1a1a1a',
  axis: '#333',
  preClose: '#555',
  avgLine: '#cdf24b',
});

let _zoomBound = false;
let _tooltipBound = false;

// ===== 初始化 =====
function initCharts() {
  if (charts.klineChart) return true;
  if (typeof echarts === 'undefined') {
    return false;
  }
  const kEl = document.getElementById('kline-chart');
  if (!kEl) return false;

  try {
    const opts = {
      backgroundColor: C.bg,
      textStyle: { color: C.text, fontFamily: 'Microsoft YaHei' },
      categoryAxis: {
        axisLine: { lineStyle: { color: C.axis } },
        axisLabel: { color: C.textDim, fontSize: 10 },
        splitLine: { show: false },
      },
      valueAxis: {
        axisLine: { lineStyle: { color: C.axis } },
        axisLabel: { color: C.textDim, fontSize: 10 },
        splitLine: { lineStyle: { color: C.grid } },
      },
      tooltip: { backgroundColor: 'rgba(20,20,20,0.95)', borderColor: '#333', textStyle: { color: '#ddd', fontSize: 12 } },
    };

    charts.klineChart = echarts.init(document.getElementById('kline-chart'));
    charts.volumeChart = echarts.init(document.getElementById('volume-chart'));
    charts.flowChart = echarts.init(document.getElementById('flow-chart'));
    charts.minuteChart = echarts.init(document.getElementById('minute-chart'));
    charts.minuteVolChart = echarts.init(document.getElementById('minute-vol'));
    charts.indicatorChart = echarts.init(document.getElementById('indicator-chart'));

    charts.klineChart.setOption(opts);
    charts.volumeChart.setOption(opts);
    charts.flowChart.setOption(opts);
    charts.minuteChart.setOption(opts);
    charts.minuteVolChart.setOption(opts);
    charts.indicatorChart.setOption(opts);

    window.addEventListener('resize', () => {
      if (charts.klineChart) charts.klineChart.resize();
      if (charts.volumeChart) charts.volumeChart.resize();
      if (charts.flowChart) charts.flowChart.resize();
      if (charts.minuteChart) charts.minuteChart.resize();
      if (charts.minuteVolChart) charts.minuteVolChart.resize();
      if (charts.indicatorChart) charts.indicatorChart.resize();
    });

    // 视图切换
    const vd = document.getElementById('view-dayk');
    if (vd) vd.onclick = () => switchView('dayk');
    const vw = document.getElementById('view-week');
    if (vw) vw.onclick = () => switchView('week');
    const vm = document.getElementById('view-minute');
    if (vm) vm.onclick = () => switchView('minute');

    // 时间范围
    document.querySelectorAll('.tb-btn[data-range]').forEach(btn => {
      btn.onclick = () => {
        document.querySelectorAll('.tb-btn[data-range]').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        applyRange(parseInt(btn.dataset.range));
      };
    });

    bindChartTooltip();
    return true;
  } catch (err) {
    console.warn("initCharts error:", err);
    return false;
  }
}

function switchView(view) {
  const prevView = tState.currentView;
  tState.currentView = view;
  const dk = document.getElementById('view-dayk');
  const wk = document.getElementById('view-week');
  const mn = document.getElementById('view-minute');
  const kc = document.getElementById('kline-chart');
  const vc = document.getElementById('volume-chart');
  const mc = document.getElementById('minute-chart');
  const mv = document.getElementById('minute-vol');
  const sep = document.getElementById('sep-range');

  if (dk) dk.classList.remove('active');
  if (wk) wk.classList.remove('active');
  if (mn) mn.classList.remove('active');

  if (view === 'minute') {
    if (mn) mn.classList.add('active');
    if (kc) kc.style.display = 'none';
    if (vc) vc.style.display = 'none';
    if (mc) mc.style.display = 'block';
    if (mv) mv.style.display = 'block';
    if (sep) sep.style.display = 'none';
    document.querySelectorAll('.tb-btn[data-range]').forEach(b => b.style.display = 'none');
    const zi = document.getElementById('zoom-info');
    if (zi) zi.style.display = 'none';
    const cdc = document.getElementById('chanlun-daily-card');
    if (cdc) cdc.style.display = 'none';
    const it = document.getElementById('indicator-toolbar');
    if (it) it.style.display = 'none';
    const ic = document.getElementById('indicator-chart');
    if (ic) ic.style.display = 'none';
    setTimeout(() => {
      if (charts.minuteChart) charts.minuteChart.resize();
      if (charts.minuteVolChart) charts.minuteVolChart.resize();
      if (tState.currentSymbol && trends.coordinator?.loadMinute) {
        trends.coordinator.loadMinute(tState.currentSymbol);
      }
    }, 50);
  } else {
    // dayk 或 week
    if (view === 'week') { if (wk) wk.classList.add('active'); }
    else { if (dk) dk.classList.add('active'); }
    if (mc) mc.style.display = 'none';
    if (mv) mv.style.display = 'none';
    if (kc) kc.style.display = 'block';
    if (vc) vc.style.display = 'block';
    if (sep) sep.style.display = '';
    document.querySelectorAll('.tb-btn[data-range]').forEach(b => b.style.display = '');
    const zi = document.getElementById('zoom-info');
    if (zi) zi.style.display = '';
    // 指标副图工具栏显示（如果有指标）
    if (tState.currentIndicator !== 'none') {
      const it = document.getElementById('indicator-toolbar');
      if (it) it.style.display = 'flex';
      const ic = document.getElementById('indicator-chart');
      if (ic) ic.style.display = 'block';
      setTimeout(() => { if (charts.indicatorChart) charts.indicatorChart.resize(); }, 50);
    }
    // 切到日K/周K视图时隐藏分时缠论面板
    const cc = document.getElementById('chanlun-card');
    if (cc) cc.style.display = 'none';
    // 日K/周K视图显示缠论面板（如果有数据）
    if (tState.dailyChanlun && !tState.dailyChanlun.error) {
      const cdc = document.getElementById('chanlun-daily-card');
      if (cdc) cdc.style.display = 'block';
    }
    setTimeout(() => {
      if (charts.klineChart) charts.klineChart.resize();
      if (charts.volumeChart) charts.volumeChart.resize();
    }, 50);
    // 日K↔周K切换时重新分析
    if (prevView !== view && tState.currentSymbol && trends.coordinator?.analyze) {
      trends.coordinator.analyze(tState.currentSymbol);
    }
  }
}

// ===== MA计算 =====
function calcMA(data, period) {
  const result = new Array(data.length).fill(null);
  for (let i = period - 1; i < data.length; i++) {
    let sum = 0;
    for (let j = 0; j < period; j++) sum += data[i - j].close;
    result[i] = +(sum / period).toFixed(3);
  }
  return result;
}

// ===== K线图 =====
function renderKline(klines, signal) {
  tState.klineData = klines;
  tState.lastSignalData = signal;
  const dates = klines.map(k => k.date);
  const ohlc = klines.map(k => [k.open, k.close, k.low, k.high]);
  const ma5 = calcMA(klines, 5);
  const ma10 = calcMA(klines, 10);
  const ma20 = calcMA(klines, 20);
  const ma60 = calcMA(klines, 60);

  // 买卖点标记——买入▲在K线下方，卖出▼在K线上方
  const markPoints = [];
  tState.signalPoints = [];
  if (signal.breakouts) {
    for (const b of signal.breakouts) {
      if (b.signal === '买入') {
        const idx = findEntryIndex(klines, b.entry_price);
        const k = klines[idx];
        const candleLow = (k && k.low) ? k.low : (b.entry_price || b.breakout_price);
        const range = k ? (k.high - k.low) : 0;
        const markerY = candleLow - Math.max(range * 0.5, candleLow * 0.005);
        const dateStr = dates[idx] || dates[dates.length-1];
        markPoints.push({
          coord: [dateStr, markerY],
          symbol: 'triangle', symbolSize: 20, symbolRotate: 0,
          itemStyle: { color: C.up, borderWidth: 2, borderColor: '#fff' },
          label: { show: true, formatter: '买入', fontSize: 11, fontWeight: 'bold', color: '#fff',
                   backgroundColor: C.up, padding: [2,4], borderRadius: 3, position: 'bottom' },
        });
        tState.signalPoints.push({
          date: dateStr, price: markerY,
          title: '买入信号（海龟法则·做多）',
          formula: `突破${b.system || '20日'}最高点 ${b.breakout_price || b.channel_high}\n→ 入场 ${b.entry_price}，止损 ${b.stop_loss}（入场-2×N）`,
          desc: `唐奇安通道：股价突破过去N天最高点时触发买入信号。\nN值=${b.current_n || '?'}（ATR，反映日均波动幅度）。\n入场后止损价=入场价-2×N，跌破止损或触及反向通道退出。`,
        });
      } else if (b.signal === '卖出') {
        const idx = dates.length - 1;
        const k = klines[idx];
        const candleHigh = (k && k.high) ? k.high : b.breakout_price;
        const range = k ? (k.high - k.low) : 0;
        const markerY = candleHigh + Math.max(range * 0.5, candleHigh * 0.005);
        markPoints.push({
          coord: [dates[idx], markerY],
          symbol: 'triangle', symbolSize: 20, symbolRotate: 180,
          itemStyle: { color: C.down, borderWidth: 2, borderColor: '#fff' },
          label: { show: true, formatter: '卖出', fontSize: 11, fontWeight: 'bold', color: '#fff',
                   backgroundColor: C.down, padding: [2,4], borderRadius: 3, position: 'top' },
        });
        tState.signalPoints.push({
          date: dates[idx], price: markerY,
          title: '卖出信号（海龟法则·做空）',
          formula: `跌破${b.system || '20日'}最低点 ${b.breakout_price || b.channel_low}\n→ 入场 ${b.entry_price}，止损 ${b.stop_loss}（入场+2×N）`,
          desc: `唐奇安通道：股价跌破过去N天的最低点时触发做空信号。\nN值=${b.current_n || '?'}（ATR，反映日均波动幅度）。\n做空止损价=入场价+2×N，涨到止损或触及反向通道平仓。`,
        });
      }
    }
  }

  // 关键水平线（止损、目标价、支撑/压力）
  let isBearish = false;
  if (signal.breakouts) {
    for (const b of signal.breakouts) {
      if (b.entry_price && b.stop_loss && b.stop_loss > b.entry_price) {
        isBearish = true; break;
      }
    }
  }
  if (!isBearish && signal.action) {
    const a = String(signal.action);
    if (a.includes('卖') || a.includes('空') || a.includes('跌')) isBearish = true;
    else if (a.includes('买') || a.includes('涨')) isBearish = false;
  }

  const markLines = [];
  tState.signalLines = [];
  if (signal.breakouts) {
    for (const b of signal.breakouts) {
      if (b.stop_loss && b.stop_loss > 0) {
        const slLabel = isBearish
          ? `止损 ${b.stop_loss.toFixed(2)}\n涨到这里就止损`
          : `止损 ${b.stop_loss.toFixed(2)}\n跌到这里就卖`;
        markLines.push({ yAxis: b.stop_loss, lineStyle: { color: C.down, type: 'dashed', width: 2 },
          label: { formatter: slLabel, color: '#fff', fontSize: 11, fontWeight: 'bold',
            backgroundColor: C.down, padding: [3,6], borderRadius: 3, position: 'insideStartTop' } });
        const nVal = b.current_n || '?';
        tState.signalLines.push({
          value: b.stop_loss,
          title: isBearish ? '止损价（做空）' : '止损价（做多）',
          formula: isBearish
            ? `${b.entry_price} + 2 × ${nVal} = ${b.stop_loss}`
            : `${b.entry_price} - 2 × ${nVal} = ${b.stop_loss}`,
          desc: isBearish
            ? `海龟法则2N止损。N=${nVal}（ATR平均真实波幅，反映股票日均波动幅度）。\n做空止损在入场价上方：如果股价反弹到这里，说明判断错了，认亏平仓。`
            : `海龟法则2N止损。N=${nVal}（ATR平均真实波幅，反映股票日均波动幅度）。\n做多止损在入场价下方：如果股价跌到这里，说明判断错了，认亏卖出。`,
        });
      }
      if (b.entry_price && b.entry_price > 0 && b.signal !== '观望') {
        const entryColor = isBearish ? C.down : C.up;
        const entryText = isBearish ? `做空 ${b.entry_price.toFixed(2)}` : `入场 ${b.entry_price.toFixed(2)}`;
        markLines.push({ yAxis: b.entry_price, lineStyle: { color: entryColor, type: 'solid', width: 2 },
          label: { formatter: entryText, color: '#fff', fontSize: 11, fontWeight: 'bold',
            backgroundColor: entryColor, padding: [3,6], borderRadius: 3, position: 'insideStartTop' } });
        tState.signalLines.push({
          value: b.entry_price,
          title: isBearish ? '做空入场价（海龟法则）' : '做多入场价（海龟法则）',
          formula: isBearish
            ? `股价跌破${b.system || '20日'}最低点 → 做空入场 ${b.entry_price}\n当时通道下轨=${b.channel_low}，上轨=${b.channel_high}`
            : `股价突破${b.system || '20日'}最高点 → 做多入场 ${b.entry_price}\n当时通道上轨=${b.channel_high}，下轨=${b.channel_low}`,
          desc: isBearish
            ? `唐奇安通道做空：当股价跌破过去N天的最低点时，触发做空信号。\n入场价=突破时的通道下轨。N值=${b.current_n || '?'}。\n已持有对应天数，止损价在上方。`
            : `唐奇安通道做多：当股价突破过去N天的最高点时，触发做多信号。\n入场价=突破时的通道上轨。N值=${b.current_n || '?'}。\n已持有对应天数，止损价在下方。`,
        });
      }
    }
  }
  // 形态目标价
  if (signal.patterns) {
    for (const p of signal.patterns) {
      if (p.target_price && p.target_price > 0) {
        const targetLabel = isBearish
          ? `目标 ${p.target_price.toFixed(2)}\n跌到这里就止盈`
          : `目标 ${p.target_price.toFixed(2)}\n涨到这里就卖`;
        markLines.push({ yAxis: p.target_price, lineStyle: { color: '#cdf24b', type: 'dashed', width: 2 },
          label: { formatter: targetLabel, color: '#0b0c0a', fontSize: 11, fontWeight: 'bold',
            backgroundColor: '#cdf24b', padding: [3,6], borderRadius: 3, position: 'insideStartTop' } });
        tState.signalLines.push({
          value: p.target_price,
          title: `${p.name} 目标价`,
          formula: p.description || '',
          desc: isBearish
            ? `${p.name}是经典看跌形态。跌破颈线后，预计再跌一个头部高度的幅度。\n跌到目标价就止盈平仓。置信度${p.confidence || '?'}%。`
            : `${p.name}是经典看涨形态。突破颈线后，预计再涨一个头部高度的幅度。\n涨到目标价就止盈卖出。置信度${p.confidence || '?'}%。`,
        });
      }
    }
  }

  const total = dates.length;
  const defaultDays = Math.min(60, total);
  let ds = total > defaultDays ? (1 - defaultDays / total) * 100 : 0;
  let de = 100;
  // 同一标的重复渲染（页签重进/轮询触发）时保留用户当前缩放区间，避免图表跳变
  const sameSymbol = tState.currentRenderedSymbol === tState.currentSymbol && tState.klineData.length;
  if (sameSymbol && charts.klineChart) {
    try {
      const dz = charts.klineChart.getOption().dataZoom?.[0];
      if (dz && dz.start != null && dz.end != null) { ds = dz.start; de = dz.end; }
    } catch(e) {}
  }
  tState.currentRenderedSymbol = tState.currentSymbol;

  charts.klineChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    xAxis: {
      type: 'category', data: dates,
      axisLine: { lineStyle: { color: C.axis } },
      axisLabel: { color: C.textDim, fontSize: 10 },
      splitLine: { show: false },
    },
    yAxis: {
      scale: true,
      axisLine: { show: false },
      axisLabel: { color: C.textDim, fontSize: 11 },
      splitLine: { lineStyle: { color: C.grid } },
    },
    grid: { left: 60, right: 50, top: 30, bottom: 50 },
    legend: {
      data: ['MA5','MA10','MA20','MA60'],
      top: 4, left: 60,
      textStyle: { color: C.textDim, fontSize: 10 },
      itemWidth: 14, itemHeight: 2,
    },
    series: [
      {
        name: 'K线', type: 'candlestick', data: ohlc,
        itemStyle: { color: C.up, color0: C.down, borderColor: C.up, borderColor0: C.down },
        markPoint: markPoints.length ? { data: markPoints, animation: false } : undefined,
        markLine: markLines.length ? { silent: true, animation: false, data: markLines, symbol: 'none' } : undefined,
      },
      { name: 'MA5', type: 'line', data: ma5, symbol: 'none', lineStyle: { color: C.ma5, width: 1 } },
      { name: 'MA10', type: 'line', data: ma10, symbol: 'none', lineStyle: { color: C.ma10, width: 1 } },
      { name: 'MA20', type: 'line', data: ma20, symbol: 'none', lineStyle: { color: C.ma20, width: 1 } },
      { name: 'MA60', type: 'line', data: ma60, symbol: 'none', lineStyle: { color: C.ma60, width: 1 } },
    ],
    tooltip: {
      trigger: 'axis', axisPointer: { type: 'cross', lineStyle: { color: '#666' } },
      formatter: (params) => {
        if (!params || !params.length) return '';
        const idx = params[0].dataIndex;
        const k = klines[idx];
        if (!k) return '';
        const isUp = k.close >= k.open;
        const c = isUp ? 'up' : 'down';
        let html = `<div style="font-size:12px;line-height:1.6">
          <div style="color:${C.textDim}">${k.date}</div>
          <div>开 <span class="${c}" style="font-weight:bold">${k.open.toFixed(2)}</span>
               高 <span class="up" style="font-weight:bold">${k.high.toFixed(2)}</span></div>
          <div>收 <span class="${c}" style="font-weight:bold">${k.close.toFixed(2)}</span>
               低 <span class="down" style="font-weight:bold">${k.low.toFixed(2)}</span></div>`;
        if (k.pct) html += `<div style="color:${k.pct>=0?C.up:C.down}">${k.pct>=0?'+':''}${k.pct.toFixed(2)}%</div>`;
        html += `<div style="color:${C.textDim}">量 ${fmtVol(k.volume)}`;
        if (k.amount > 0) html += ` 额 ${fmtVol(k.amount)}`;
        if (k.turnover > 0) html += ` 换手 ${k.turnover.toFixed(1)}%`;
        html += '</div>';
        for (const p of params) {
          if (p.seriesName && p.seriesName.startsWith('MA') && p.value != null) {
            html += `<div style="color:${p.color}">${p.seriesName} ${p.value.toFixed(2)}</div>`;
          }
        }
        html += '</div>';
        return html;
      }
    },
    dataZoom: [
      { type: 'inside', start: ds, end: de, zoomOnMouseWheel: true, moveOnMouseMove: true, moveOnMouseWheel: false },
      { type: 'slider', start: ds, end: de, height: 28, bottom: 8,
        borderColor: '#222', backgroundColor: '#0a0a0a',
        fillerColor: 'rgba(205,242,75,0.1)',
        selectedDataBackground: { lineStyle: { color: '#cdf24b' }, areaStyle: { color: 'rgba(205,242,75,0.15)' } },
        dataBackground: { lineStyle: { color: '#222' }, areaStyle: { color: '#111' } },
        handleStyle: { color: '#cdf24b', borderColor: '#cdf24b' },
        moveHandleStyle: { color: '#cdf24b' },
        textStyle: { color: C.textDim, fontSize: 10 },
        brushSelect: false,
      },
    ],
  }, true);

  // 成交量
  charts.volumeChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    xAxis: { type: 'category', data: dates, show: false },
    yAxis: { type: 'value', axisLabel: { color: C.textDim, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
    grid: { left: 60, right: 50, top: 5, bottom: 5 },
    series: [{
      type: 'bar', data: klines.map(k => ({
        value: k.volume,
        itemStyle: { color: k.close >= k.open ? C.up + '88' : C.down + '88' }
      })),
    }],
    tooltip: { trigger: 'axis', formatter: p => p[0] ? `<div style="font-size:11px">${p[0].axisValue}<br/>量 ${fmtVol(p[0].value)}</div>` : '' },
    dataZoom: [
      { type: 'inside', start: ds, end: de, zoomOnMouseWheel: true, moveOnMouseMove: true },
      { type: 'slider', start: ds, end: de, show: false },
    ],
  }, true);

  bindZoomSync();
  updateZoomInfo(ds, de);
  if (trends.indicator?.renderIndicator) {
    trends.indicator.renderIndicator(tState.currentIndicator);
  }
}

function findEntryIndex(klines, entryPrice) {
  if (!entryPrice) return klines.length - 1;
  let best = 0, bestDiff = Infinity;
  for (let i = 0; i < klines.length; i++) {
    const d = Math.abs(klines[i].close - entryPrice);
    if (d < bestDiff) { bestDiff = d; best = i; }
  }
  return best;
}

function bindZoomSync() {
  if (_zoomBound || !charts.klineChart) return;
  _zoomBound = true;
  charts.klineChart.on('datazoom', () => {
    const dz = charts.klineChart.getOption().dataZoom[0];
    if (dz) {
      if (charts.volumeChart) charts.volumeChart.dispatchAction({ type: 'dataZoom', start: dz.start, end: dz.end });
      if (charts.indicatorChart) charts.indicatorChart.dispatchAction({ type: 'dataZoom', start: dz.start, end: dz.end });
      updateZoomInfo(dz.start, dz.end);
      syncRangeBtns(dz.start, dz.end);
    }
  });
}

function applyRange(days) {
  const total = tState.klineData.length;
  if (!total || !charts.klineChart) return;
  let s, e;
  if (days === 0 || days >= total) { s = 0; e = 100; }
  else { s = Math.max(0, (1 - days / total) * 100); e = 100; }
  charts.klineChart.dispatchAction({ type: 'dataZoom', start: s, end: e });
  if (charts.volumeChart) charts.volumeChart.dispatchAction({ type: 'dataZoom', start: s, end: e });
  if (charts.indicatorChart) charts.indicatorChart.dispatchAction({ type: 'dataZoom', start: s, end: e });
  updateZoomInfo(s, e);
}

function bindChartTooltip() {
  if (_tooltipBound || !charts.klineChart || !charts.klineChart.getZr) return;
  _tooltipBound = true;

  const tooltipEl = document.getElementById('signal-tooltip');
  const chartDom = document.getElementById('kline-chart');
  if (!tooltipEl || !chartDom) return;

  const zr = charts.klineChart.getZr();
  if (!zr) return;
  zr.on('mousemove', function(e) {
    if (!tState.signalLines.length && !tState.signalPoints.length) {
      tooltipEl.style.display = 'none';
      return;
    }

    let yVal;
    try {
      yVal = charts.klineChart.convertFromPixel({ yAxisIndex: 0 }, e.offsetY);
    } catch(err) {
      tooltipEl.style.display = 'none';
      return;
    }
    if (yVal == null || isNaN(yVal)) {
      tooltipEl.style.display = 'none';
      return;
    }

    let found = null;
    for (const line of tState.signalLines) {
      if (line.value > 0 && Math.abs(yVal - line.value) / line.value < 0.012) {
        found = line;
        break;
      }
    }

    if (!found && tState.signalPoints.length) {
      let xIdx;
      try {
        xIdx = Math.round(charts.klineChart.convertFromPixel({ xAxisIndex: 0 }, e.offsetX));
      } catch(err) {}

      if (xIdx != null && !isNaN(xIdx)) {
        for (const pt of tState.signalPoints) {
          const idx = tState.klineData.findIndex(k => k.date === pt.date);
          if (idx >= 0 && Math.abs(idx - xIdx) <= 1 && pt.price > 0 && Math.abs(yVal - pt.price) / pt.price < 0.02) {
            found = pt;
            break;
          }
        }
      }
    }

    if (found) {
      tooltipEl.innerHTML =
        '<div class="stt-hint">信号说明（鼠标移开自动隐藏）</div>' +
        '<div class="stt-title">' + found.title + '</div>' +
        (found.formula ? '<div class="stt-formula">' + found.formula + '</div>' : '') +
        (found.desc ? '<div class="stt-desc">' + found.desc + '</div>' : '');
      tooltipEl.style.display = 'block';
    } else {
      tooltipEl.style.display = 'none';
    }
  });

  charts.klineChart.getZr().on('mouseout', function() {
    tooltipEl.style.display = 'none';
  });
}

function updateZoomInfo(start, end) {
  const total = tState.klineData.length;
  if (!total) return;
  const si = Math.floor(start / 100 * total);
  const ei = Math.min(total - 1, Math.floor(end / 100 * total));
  const sd = tState.klineData[si]?.date || '';
  const ed = tState.klineData[ei]?.date || '';
  const zi = document.getElementById('zoom-info');
  if (zi) zi.textContent = `${sd} ~ ${ed} (${ei - si + 1}根)`;
}

function syncRangeBtns(start, end) {
  const total = tState.klineData.length;
  if (!total) return;
  const days = Math.round((end - start) / 100 * total);
  document.querySelectorAll('.tb-btn[data-range]').forEach(b => {
    const r = parseInt(b.dataset.range);
    if (r === 0) b.classList.toggle('active', start === 0 && end === 100);
    else b.classList.toggle('active', Math.abs(days - r) < 5 && end > 99);
  });
}

function renderMinute(data, chanlunData) {
  if (!charts.minuteChart || !charts.minuteVolChart) return;
  const pre = data.pre_close;
  const prices = data.prices;
  const avgs = data.avg_prices;
  const times = data.times;
  const vols = data.volumes;

  if (!tState.minuteYRange || tState.minuteYRange.pre !== pre) {
    const is20pct = tState.currentSymbol && (
      tState.currentSymbol.startsWith('300') || tState.currentSymbol.startsWith('688') ||
      tState.currentSymbol.startsWith('920') || tState.currentSymbol.startsWith('8')
    );
    const pct = is20pct ? 0.20 : 0.10;
    tState.minuteYRange = {
      pre: pre,
      min: pre * (1 - pct),
      max: pre * (1 + pct),
    };
  }
  const yMin = tState.minuteYRange.min;
  const yMax = tState.minuteYRange.max;

  const lastP = prices.filter(p => p > 0).slice(-1)[0] || pre;
  const pColor = lastP >= pre ? C.up : C.down;

  const markPoints = [];
  if (chanlunData && chanlunData.signals) {
    for (const sig of chanlunData.signals) {
      const isBuy = sig.type.startsWith('buy');
      const timeIdx = times.indexOf(sig.time);
      const t = timeIdx >= 0 ? sig.time : times[times.length - 1];
      markPoints.push({
        coord: [t, sig.price],
        symbol: isBuy ? 'triangle' : 'pin',
        symbolSize: 16,
        symbolRotate: isBuy ? 0 : 180,
        itemStyle: { color: isBuy ? C.up : C.down, borderWidth: 2, borderColor: '#fff' },
        label: {
          show: true, formatter: sig.type_name, fontSize: 9, fontWeight: 'bold', color: '#fff',
          position: isBuy ? 'bottom' : 'top', backgroundColor: isBuy ? C.up : C.down,
          padding: [2, 4], borderRadius: 2,
        },
      });
    }
  }

  const fractalMarks = [];
  if (chanlunData && chanlunData.fractals) {
    for (const f of chanlunData.fractals) {
      const timeIdx = times.indexOf(f.time);
      if (timeIdx < 0) continue;
      fractalMarks.push({
        coord: [f.time, f.price],
        symbol: 'circle', symbolSize: 6,
        itemStyle: { color: f.type === 'top' ? '#00b35c' : '#ff2d2d', borderColor: '#fff', borderWidth: 1 },
      });
    }
  }

  charts.minuteChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    xAxis: {
      type: 'category', data: times,
      axisLine: { lineStyle: { color: C.axis } },
      axisLabel: { color: C.textDim, fontSize: 10, interval: Math.floor(times.length / 6) },
      splitLine: { show: true, lineStyle: { color: C.grid, type: 'dashed' } },
    },
    yAxis: {
      type: 'value', min: yMin, max: yMax,
      axisLine: { show: false },
      axisLabel: {
        color: C.textDim, fontSize: 11,
        formatter: v => {
          const pct = ((v - pre) / pre * 100).toFixed(2);
          return v.toFixed(2) + '\n' + (pct > 0 ? '+' : '') + pct + '%';
        }
      },
      splitLine: { lineStyle: { color: C.grid, type: 'dashed' } },
    },
    grid: { left: 70, right: 60, top: 10, bottom: 25 },
    series: [
      {
        name: '价格', type: 'line', data: prices, symbol: 'none',
        lineStyle: { color: pColor, width: 1.5 },
        areaStyle: { color: pColor === C.up ? 'rgba(255,45,45,0.08)' : 'rgba(0,179,92,0.08)' },
        markLine: { silent: true, symbol: 'none', animation: false,
          data: [{ yAxis: pre, lineStyle: { color: C.preClose, type: 'dashed', width: 1 },
            label: { formatter: '昨收 ' + pre.toFixed(3), color: C.textDim, fontSize: 10, position: 'insideEndTop' } }]
        },
        markPoint: (markPoints.length || fractalMarks.length) ? {
          data: [...markPoints, ...fractalMarks],
          animation: false,
        } : undefined,
      },
      { name: '均价', type: 'line', data: avgs, symbol: 'none', lineStyle: { color: C.avgLine, width: 1, type: 'dashed' } },
    ],
    tooltip: {
      trigger: 'axis', axisPointer: { type: 'cross', lineStyle: { color: '#666' } },
      formatter: (params) => {
        if (!params || !params.length) return '';
        let html = `<div style="font-size:12px;line-height:1.6"><div style="color:${C.textDim}">${params[0].axisValue}</div>`;
        for (const p of params) {
          if (p.value > 0) {
            const pct = ((p.value - pre) / pre * 100).toFixed(2);
            const c = p.value >= pre ? C.up : C.down;
            html += `<div><span style="color:${p.color}">●</span> ${p.seriesName} <span style="color:${c};font-weight:bold">${p.value.toFixed(3)}</span> <span style="color:${c}">(${pct>0?'+':''}${pct}%)</span></div>`;
          }
        }
        html += '</div>';
        return html;
      }
    },
  }, true);

  const vc = vols.map((v, i) => {
    if (v === 0) return { value: 0, itemStyle: { color: '#222' } };
    const p = prices[i] || 0;
    return { value: v, itemStyle: { color: p >= pre ? C.up + '66' : C.down + '66' } };
  });
  charts.minuteVolChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    xAxis: { type: 'category', data: times, show: false },
    yAxis: { type: 'value', axisLabel: { color: C.textDim, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
    grid: { left: 70, right: 60, top: 5, bottom: 5 },
    series: [{ type: 'bar', data: vc }],
    tooltip: { trigger: 'axis', formatter: p => p[0] ? `<div style="font-size:11px">${p[0].axisValue}<br/>量 ${fmtVol(p[0].value)}</div>` : '' },
  }, true);
}

function refreshKlineLastCandle(q) {
  if (!tState.klineData.length || !q || !q.price || !charts.klineChart) return;
  if (tState.currentView === 'week') return;
  const last = tState.klineData[tState.klineData.length - 1];
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;
  if (last.date !== today) return;

  // 行情源与K线源价格基准不一致（如K线走了不复权fallback、行情走了另一数据源）
  // 时，直接合并会把最后一根K线变成脱离真实走势的假大阳/大阴线。偏离超过该
  // 板块单日涨跌停极限判定为基准错位，跳过本次合并，保持上一帧K线，等待下一
  // 次 analyze 全量重绘对齐。主板±10%、创业板/科创板±20%、北交所±30%。
  const base = last.close > 0 ? last.close : (last.open || 0);
  if (base > 0) {
    const s = tState.currentSymbol || '';
    const limit = (s.startsWith('300') || s.startsWith('688')) ? 0.21
      : (s.startsWith('920') || s.startsWith('8')) ? 0.31 : 0.11;
    if (Math.abs(q.price - base) / base > limit) {
      return;
    }
  }

  last.close = q.price;
  last.high = Math.max(last.high, q.high || q.price);
  last.low = Math.min(last.low, q.low || q.price);
  if (q.volume) last.volume = q.volume;
  if (q.amount) last.amount = q.amount;

  const ma5 = calcMA(tState.klineData, 5);
  const ma10 = calcMA(tState.klineData, 10);
  const ma20 = calcMA(tState.klineData, 20);
  const ma60 = calcMA(tState.klineData, 60);

  charts.klineChart.setOption({
    series: [
      { name: 'K线', data: tState.klineData.map(k => [k.open, k.close, k.low, k.high]) },
      { name: 'MA5', data: ma5 },
      { name: 'MA10', data: ma10 },
      { name: 'MA20', data: ma20 },
      { name: 'MA60', data: ma60 },
    ]
  }, false);

  if (charts.volumeChart) {
    charts.volumeChart.setOption({
      series: [{
        data: tState.klineData.map(k => ({
          value: k.volume,
          itemStyle: { color: k.close >= k.open ? C.up + '88' : C.down + '88' }
        }))
      }]
    }, false);
  }
}

async function refreshMinuteLight(symbol) {
  try {
    const r = await authFetch(`/api/trends/minute?symbol=${symbol}`);
    const data = await r.json();
    if (data.error || !tState.minuteData || !charts.minuteChart) return;
    const oldLen = tState.minuteData.prices.length;
    const newLen = data.prices.length;
    const oldLast = tState.minuteData.prices[oldLen - 1];
    const newLast = data.prices[newLen - 1];
    if (oldLen === newLen && oldLast === newLast) return;

    tState.minuteData = data;
    const pre = data.pre_close;
    const pColor = newLast >= pre ? C.up : C.down;

    charts.minuteChart.setOption({
      series: [
        { name: '价格', data: data.prices, lineStyle: { color: pColor, width: 1.5 } },
        { name: '均价', data: data.avg_prices },
      ],
    });

    if (charts.minuteVolChart) {
      const vc = data.volumes.map((v, i) => {
        if (v === 0) return { value: 0, itemStyle: { color: '#222' } };
        const p = data.prices[i] || 0;
        return { value: v, itemStyle: { color: p >= pre ? C.up + '66' : C.down + '66' } };
      });
      charts.minuteVolChart.setOption({
        series: [{ data: vc }],
      });
    }
  } catch(e) {}
}

function fmtVol(n) {
  if (n == null) return '--';
  if (n >= 1e8) return (n / 1e8).toFixed(2) + '亿';
  if (n >= 1e4) return (n / 1e4).toFixed(1) + '万';
  return String(n);
}

window.switchView = switchView;
window.applyRange = applyRange;

// 挂载图表能力到 trends.chart
trends.chart = {
  initCharts,
  switchView,
  calcMA,
  renderKline,
  findEntryIndex,
  applyRange,
  bindZoomSync,
  bindChartTooltip,
  updateZoomInfo,
  syncRangeBtns,
  renderMinute,
  refreshKlineLastCandle,
  refreshMinuteLight,
  fmtVol,
};

})();
