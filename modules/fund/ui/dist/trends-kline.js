(() => {
const trends = globalThis.Fund.trends;
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

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
  // 划线层联动：标的/周期变化后重新加载该 symbol+period 的划线并重绘
  if (trends.drawings) {
    trends.drawings.reload();
  }
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

// 挂载 K 线渲染能力到 trends.chart
trends.chart.calcMA = calcMA;
trends.chart.renderKline = renderKline;
trends.chart.findEntryIndex = findEntryIndex;
trends.chart.refreshKlineLastCandle = refreshKlineLastCandle;

})();
