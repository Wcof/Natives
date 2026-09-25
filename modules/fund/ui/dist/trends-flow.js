(() => {
const { authFetch } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

// ===== 资金流与盘中实时流动模块 (Trends Flow) =====
// 职责：负责日级资金流与分时盘中资金流图表渲染、实时数据拉取及模式切换
function renderFlow(flows) {
  tState.dailyFlows = flows;
  if (tState.flowMode === 'realtime') {
    loadRealtimeFlow(tState.currentSymbol);
    return;
  }
  _renderDailyFlow(flows);
}

function _renderDailyFlow(flows) {
  const fs = document.getElementById('flow-summary');
  if (!fs || !charts.flowChart) return;
  if (!flows || !flows.length) {
    fs.textContent = '';
    charts.flowChart.setOption({ title: { text: '无资金流数据', left: 'center', top: 'center', textStyle: { color: C.textDim, fontSize: 13 } } }, true);
    return;
  }
  const recent = flows.slice(-5);
  const totalMain = recent.reduce((s, f) => s + f.main_net, 0);
  const totalMainYi = (totalMain / 1e8).toFixed(2);
  fs.innerHTML = `近5日主力 <span style="color:${totalMain>=0?C.up:C.down};font-weight:bold">${totalMain>=0?'+':''}${totalMainYi}亿</span>`;

  const dates = flows.map(f => f.date);
  const mainNet = flows.map(f => +(f.main_net / 1e8).toFixed(3));
  const superLarge = flows.map(f => +(f.super_large_net / 1e8).toFixed(3));

  charts.flowChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    legend: { data: ['主力净流入', '超大单'], textStyle: { color: C.textDim, fontSize: 10 }, top: 2, itemWidth: 12, itemHeight: 8 },
    xAxis: { type: 'category', data: dates, axisLabel: { color: C.textDim, fontSize: 9 }, axisLine: { lineStyle: { color: C.axis } } },
    yAxis: { type: 'value', axisLabel: { color: C.textDim, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
    grid: { left: 50, right: 20, top: 22, bottom: 18 },
    series: [
      { name: '主力净流入', type: 'bar', data: mainNet.map(v => ({ value: v, itemStyle: { color: v >= 0 ? C.up + '88' : C.down + '88' } })) },
      { name: '超大单', type: 'line', data: superLarge, symbol: 'circle', symbolSize: 3, lineStyle: { color: C.ma20, width: 1 } },
    ],
    tooltip: { trigger: 'axis', formatter: p => {
      let html = `<div style="font-size:11px">${p[0].axisValue}</div>`;
      for (const x of p) html += `<div><span style="color:${x.color}">●</span> ${x.seriesName}: ${x.value>=0?'+':''}${x.value.toFixed(2)}亿</div>`;
      return html;
    }},
  }, true);
}

// ===== 盘中实时资金流 =====
function switchFlowMode(mode) {
  tState.flowMode = mode;
  const ftRt = document.getElementById('ft-rt');
  if (ftRt) ftRt.classList.toggle('ft-active', mode === 'realtime');
  const ftDaily = document.getElementById('ft-daily');
  if (ftDaily) ftDaily.classList.toggle('ft-active', mode === 'daily');
  if (mode === 'realtime') {
    loadRealtimeFlow(tState.currentSymbol);
  } else {
    _renderDailyFlow(tState.dailyFlows);
  }
}

async function loadRealtimeFlow(symbol) {
  if (!symbol) return;
  try {
    const r = await authFetch(`/api/trends/realtime_flow?symbol=${symbol}`);
    const data = await r.json();
    renderRealtimeFlow(data);
  } catch(e) {
    const fs = document.getElementById('flow-summary');
    if (fs) fs.textContent = '实时资金流获取失败';
  }
}

function renderRealtimeFlow(data) {
  const fs = document.getElementById('flow-summary');
  if (!fs || !charts.flowChart) return;
  if (data.error || !data.flows || !data.flows.length) {
    fs.textContent = data.error || '暂无实时资金流数据';
    charts.flowChart.setOption({ title: { text: '盘前/非交易日', left: 'center', top: 'center', textStyle: { color: C.textDim, fontSize: 13 } } }, true);
    return;
  }

  const flows = data.flows;
  const summary = data.summary || {};
  const mainNetYi = (summary.main_net / 1e8).toFixed(2);
  const superLargeYi = (summary.super_large_net / 1e8).toFixed(2);
  fs.innerHTML = `今日主力 <span style="color:${summary.main_net>=0?C.up:C.down};font-weight:bold">${summary.main_net>=0?'+':''}${mainNetYi}亿</span> | 超大单 <span style="color:${summary.super_large_net>=0?C.up:C.down};font-weight:bold">${summary.super_large_net>=0?'+':''}${superLargeYi}亿</span>`;

  const times = flows.map(f => f.time);
  const mainNet = flows.map(f => +(f.main_net / 1e8).toFixed(4));
  const superLarge = flows.map(f => +(f.super_large_net / 1e8).toFixed(4));

  charts.flowChart.setOption({
    backgroundColor: C.bg,
    animation: false,
    legend: { data: ['主力净流入(累计)', '超大单(累计)'], textStyle: { color: C.textDim, fontSize: 10 }, top: 2, itemWidth: 12, itemHeight: 8 },
    xAxis: { type: 'category', data: times, axisLabel: { color: C.textDim, fontSize: 9, interval: 29 }, axisLine: { lineStyle: { color: C.axis } } },
    yAxis: { type: 'value', axisLabel: { color: C.textDim, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
    grid: { left: 50, right: 20, top: 22, bottom: 18 },
    series: [
      { name: '主力净流入(累计)', type: 'bar', data: mainNet.map(v => ({ value: v, itemStyle: { color: v >= 0 ? C.up + '88' : C.down + '88' } })) },
      { name: '超大单(累计)', type: 'line', data: superLarge, symbol: 'none', lineStyle: { color: C.ma20, width: 1 } },
    ],
    tooltip: { trigger: 'axis', formatter: p => {
      let html = `<div style="font-size:11px">${p[0].axisValue}</div>`;
      for (const x of p) html += `<div><span style="color:${x.color}">●</span> ${x.seriesName}: ${x.value>=0?'+':''}${x.value.toFixed(3)}亿</div>`;
      return html;
    }},
  }, true);
}

// 绑定资金流模式切换
const ftRt = document.getElementById('ft-rt');
if (ftRt) ftRt.onclick = () => switchFlowMode('realtime');
const ftDaily = document.getElementById('ft-daily');
if (ftDaily) ftDaily.onclick = () => switchFlowMode('daily');

window.switchFlowMode = switchFlowMode;

trends.flow = {
  renderFlow,
  switchFlowMode,
  loadRealtimeFlow,
  renderRealtimeFlow,
};

})();
