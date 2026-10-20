(() => {
const trends = globalThis.Fund.trends;
const tState = trends.state;
const charts = trends.charts;


function activeRows() {
  if (tState.instrumentKind === 'fund') return (tState.navVisible || []).map(row => ({...row, date:row.navDate}));
  if (tState.currentView === 'minute') {
    const data=tState.minuteData || {};
    return (data.times || []).map((time,index)=>({date:[data.date,time].filter(Boolean).join(' '),price:data.prices?.[index],volume:data.volumes?.[index]}));
  }
  return tState.klineData;
}
function activeChart() { return tState.currentView === 'minute' ? charts.minuteChart : charts.klineChart; }
function columnKeys() { return tState.instrumentKind === 'fund' ? ['date','unitNav','source','fetchedAt'] : tState.currentView === 'minute' ? ['date','price','volume'] : ['date','open','high','low','close','volume']; }


let _zoomBound = false;
let _tooltipBound = false;

function bindZoomSync() {
  if (_zoomBound || !charts.klineChart) return;
  _zoomBound = true;
  charts.klineChart.on('datazoom', () => {
    document.dispatchEvent(new Event('fund:research-viewport-changed'));
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
  const total = activeRows().length;
  if (!total || !activeChart()) return;
  let s=0, e=100;
  if (days !== 0) {
    const months={20:1,60:3,120:6,250:12}[days];
    const latest=new Date(activeRows()[total-1].date.slice(0,10)+'T00:00:00Z');
    if (months && Number.isFinite(latest.getTime())) {
      const cutoff=new Date(latest);cutoff.setUTCMonth(cutoff.getUTCMonth()-months);
      const first=activeRows().findIndex(row=>row.date>=cutoff.toISOString().slice(0,10));
      s=Math.max(0,first)/Math.max(1,total-1)*100;
    } else s=Math.max(0,(1-days/total)*100);
  }
  activeChart().dispatchAction({ type: 'dataZoom', start: s, end: e });
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
      tooltipEl.replaceChildren();
      for(const [kind,value] of [['hint',Fund.t('fundTrendsUi48')],['title',found.title],['formula',found.formula],['desc',found.desc]]) {
        if(!value)continue;const node=document.createElement('div');node.className='stt-'+kind;node.textContent=value;tooltipEl.append(node);
      }
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
  const total = activeRows().length;
  if (!total) return;
  const si = Math.floor(start / 100 * total);
  const ei = Math.min(total - 1, Math.floor(end / 100 * total));
  const sd = activeRows()[si]?.date || '';
  const ed = activeRows()[ei]?.date || '';
  const zi = document.getElementById('zoom-info');
  if (zi) zi.textContent = `${sd} ~ ${ed} (${ei - si + 1})`;
}

function syncRangeBtns(start, end) {
  const total = activeRows().length;
  if (!total) return;
  const days = Math.round((end - start) / 100 * total);
  document.querySelectorAll('.tb-btn[data-range]').forEach(b => {
    const r = parseInt(b.dataset.range);
    if (r === 0) b.classList.toggle('active', start === 0 && end === 100);
    else b.classList.toggle('active', Math.abs(days - r) < 5 && end > 99);
  });
}

const reader=Fund.researchSeries.reader({id:'trends',toolbar:document.getElementById('trends-chart-toolbar'),controls:document.querySelector('#view-trends .chart-container'),rows:activeRows,chart:activeChart,keys:columnKeys,chartNodes:[document.getElementById('kline-chart'),document.getElementById('minute-chart')]});
document.addEventListener('fund:research-selection-changed',reader.reset);

window.applyRange = applyRange;

// 挂载缩放/提示交互能力到 trends.chart
trends.chart.bindZoomSync = bindZoomSync;
trends.chart.applyRange = applyRange;
trends.chart.bindChartTooltip = bindChartTooltip;
trends.chart.updateZoomInfo = updateZoomInfo;
trends.chart.syncRangeBtns = syncRangeBtns;

})();
