(() => {
const { authFetch } = globalThis.Fund;
const trends = globalThis.Fund.trends;
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

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

// 挂载分时渲染能力到 trends.chart
trends.chart.renderMinute = renderMinute;
trends.chart.refreshMinuteLight = refreshMinuteLight;
trends.chart.fmtVol = fmtVol;

})();
