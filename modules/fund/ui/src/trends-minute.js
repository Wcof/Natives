(() => {
const { authFetch } = globalThis.Fund;
const trends = globalThis.Fund.trends;
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

function renderMinute(data, chanlunData) {
  if (!charts.minuteChart || !charts.minuteVolChart) return;
  const status=document.getElementById('trends-query-status');
  if(status)status.textContent=Fund.t('fundChartdate')+': '+(data.date||Fund.t('fundDetailMissing'));
  const pre = data.pre_close;
  const prices = data.prices;
  const avgs = data.avg_prices;
  const times = data.times;
  const vols = data.volumes;
  if (!prices.some(price=>price!==null) || pre===null) {
    charts.minuteChart.clear();charts.minuteVolChart.clear();tState.minuteYRange=null;
    charts.minuteChart.setOption({title:{text:Fund.t(pre===null?'fundTrendsMinuteReferenceMissing':'fundTrendsMinuteEmpty'),left:'center',top:'center',textStyle:{color:C.textDim,fontSize:14}}});
    return;
  }

  const span=Math.max(pre*0.001,...prices.concat(avgs).filter(price=>price!==null).map(price=>Math.abs(price-pre)))*1.1;
  tState.minuteYRange={pre,min:Math.max(0,pre-span),max:pre+span};
  const yMin = tState.minuteYRange.min;
  const yMax = tState.minuteYRange.max;
  const interval=count=>Math.max(0,Math.ceil(times.length/count)-1);
  const media=[
    {query:{maxWidth:479},option:{grid:{left:56,right:12},xAxis:{axisLabel:{interval:interval(2)}}}},
    {option:{grid:{left:70,right:60},xAxis:{axisLabel:{interval:interval(6)}}}},
  ];

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
        itemStyle: { color: isBuy ? C.up : C.down, borderWidth: 2, borderColor: Fund.themeColor('--natives-surface') },
        label: {
          show: true, formatter: sig.type_name, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')), fontWeight: 'bold', color: Fund.themeColor('--natives-on-accent'),
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
        itemStyle: { color: f.type === 'top' ? C.down : C.up, borderColor: Fund.themeColor('--natives-surface'), borderWidth: 1 },
      });
    }
  }

  charts.minuteChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    xAxis: {
      type: 'category', data: times,
      axisLine: { lineStyle: { color: C.axis } },
      axisLabel: { color: C.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')), interval: interval(6) },
      splitLine: { show: true, lineStyle: { color: C.grid, type: 'dashed' } },
    },
    yAxis: {
      type: 'value', min: yMin, max: yMax,
      axisLine: { show: false },
      axisLabel: {
        color: C.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')),
        formatter: v => {
          const pct = ((v - pre) / pre * 100).toFixed(2);
          return v.toFixed(2) + '\n' + (pct > 0 ? '+' : '') + pct + '%';
        }
      },
      splitLine: { lineStyle: { color: C.grid, type: 'dashed' } },
    },
    grid: { left:70, right:60, top: 10, bottom: 25 },
    media,
    series: [
      {
        name: Fund.t('fundTrendsPrice'), type: 'line', data: prices, symbol: 'none',
        lineStyle: { color: pColor, width: 1.5 },
        areaStyle: { color: Fund.themeRgba(pColor === C.up ? '--color-up' : '--color-down', 0.08) },
        markLine: { silent: true, symbol: 'none', animation: false,
          data: [{ yAxis: pre, lineStyle: { color: C.preClose, type: 'dashed', width: 1 },
            label: { formatter: Fund.t('fundTrendsPreviousClose')+' '+pre.toFixed(3), color: C.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')), position: 'insideEndTop' } }]
        },
        markPoint: (markPoints.length || fractalMarks.length) ? {
          data: [...markPoints, ...fractalMarks],
          animation: false,
        } : undefined,
      },
      { name: Fund.t('fundTrendsAveragePrice'), type: 'line', data: avgs, symbol: 'none', lineStyle: { color: C.avgLine, width: 1, type: 'dashed' } },
    ],
    tooltip: {
      trigger: 'axis', axisPointer: { type: 'cross', lineStyle: { color: C.axis } },
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
    if (v === 0) return { value: 0, itemStyle: { color: C.grid } };
    const p = prices[i] || 0;
    return { value: v, itemStyle: { color: Fund.themeRgba(p>=pre?'--color-up':'--color-down',0.4) } };
  });
  charts.minuteVolChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    xAxis: { type: 'category', data: times, show: false },
    yAxis: { type: 'value', axisLabel: { color: C.textDim, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) }, splitLine: { lineStyle: { color: C.grid } } },
    grid: { left:70, right:60, top: 5, bottom: 5 },
    media,
    series: [{ type: 'bar', data: vc }],
    tooltip: { trigger: 'axis', formatter: p => p[0] ? `<div style="font-size:var(--natives-fs-secondary)">${p[0].axisValue}<br/>${Fund.t('fundChartvolume')} ${fmtVol(p[0].value)}</div>` : '' },
  }, true);
}

let refreshController=null,refreshGeneration=0;
function pauseMinute(){refreshController?.abort();refreshController=null;refreshGeneration++;}
async function refreshMinuteLight(symbol) {
  pauseMinute();const controller=new AbortController(),generation=refreshGeneration;refreshController=controller;
  try {
    const response=await authFetch('/api/trends/minute?symbol='+encodeURIComponent(symbol),{signal:controller.signal});
    const raw=await response.json();
    if(controller.signal.aborted||generation!==refreshGeneration||symbol!==tState.currentSymbol||tState.currentView!=='minute')return;
    if(!response.ok||raw.error)throw Error('MINUTE_RESPONSE');
    const data=Fund.researchSeries.minute(raw),previous=tState.minuteData;
    tState.minuteData=data;
    if(previous?.date!==data.date)tState.minuteChanlun=null;
    renderMinute(data,tState.minuteChanlun);
  }catch(error){
    if(!controller.signal.aborted&&generation===refreshGeneration)document.getElementById('trends-query-status').textContent=Fund.t('fundTrendsMinuteUnavailable');
  }finally{if(refreshController===controller)refreshController=null;}
}

function fmtVol(n) {
  if (!Number.isFinite(n)) return '--';
  if (n >= 1e8) return Fund.t('fundChartQuantityHundredMillion',{value:(n/1e8).toFixed(2)});
  if (n >= 1e4) return Fund.t('fundChartQuantityTenThousand',{value:(n/1e4).toFixed(1)});
  return String(n);
}

// 挂载分时渲染能力到 trends.chart
trends.chart.renderMinute = renderMinute;
trends.chart.pauseMinute = pauseMinute;
trends.chart.refreshMinuteLight = refreshMinuteLight;
trends.chart.fmtVol = fmtVol;

})();
