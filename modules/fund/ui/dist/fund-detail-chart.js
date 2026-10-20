// 基金详情图表渲染器（分时走势、单位净值走势、个人收益走势）
(() => {
const { t } = globalThis.Fund;
const esc = globalThis.Fund.home?.escape || (s => String(s ?? ""));

let chartInstances = {
  minute: null,
  perf: null,
  profit: null,
};

function initECharts(el) {
  if (!el || !globalThis.echarts) return null;
  return echarts.getInstanceByDom(el) || echarts.init(el);
}

let generation = 0;
const tickets = {minute:0,perf:0,profit:0};
function getThemeColors() {
  return {accent:Fund.themeColor('--natives-accent'),text:Fund.themeColor('--natives-text'),muted:Fund.themeColor('--natives-text-muted'),border:Fund.themeColor('--natives-border'),grid:Fund.themeColor('--natives-border')};
}

async function ensureEchartsReady() {
  if (globalThis.echarts) return true;
  if (globalThis.Fund.ensureEcharts) {
    await globalThis.Fund.ensureEcharts();
  }
  if(!globalThis.echarts)throw Error(t('fundDetailChartUnavailable'));return true;
}

async function renderMinuteChart(chartBox, points, preClose) {
  if (!chartBox) return;
  const owner=generation,ticket=++tickets.minute;
  await ensureEchartsReady();
  if(owner!==generation||ticket!==tickets.minute)return;
  chartInstances.minute = initECharts(chartBox);
  if (!chartInstances.minute) return;

  if (!points || !points.length) {
    chartInstances.minute.clear();
    return;
  }
  const colors = getThemeColors();
  const times = points.map(i => i.time);
  const prices = points.map(i => preClose>0?(i.price/preClose-1)*100:i.price);

  chartInstances.minute.setOption({
    backgroundColor: "transparent",
    animation: false,
    grid: { left: 45, right: 20, top: 20, bottom: 25 },
    xAxis: {
      type: "category",
      data: times,
      axisLine: { lineStyle: { color: colors.border } },
      axisLabel: { color: colors.muted, fontSize: 12 },
    },
    yAxis: {
      type: "value",
      scale: true,
      splitLine: { lineStyle: { color: colors.grid } },
      axisLabel: { color: colors.muted, fontSize: 12 },
    },
    series: [{
      type: "line",
      data: prices,
      smooth: true,
      showSymbol: false,
      lineStyle: { color: colors.accent, width: 2 },
      areaStyle: {
        color: {
          type: "linear",
          x: 0,
          y: 0,
          x2: 0,
          y2: 1,
          colorStops: [
            { offset: 0, color: Fund.themeRgba?.("--natives-accent",0.15)||colors.accent },
            { offset: 1, color: "transparent" },
          ],
        },
      },
    }],
  });
}

async function renderPerfChart(box, filteredNav) {
  if (!box) return;
  const owner=generation,ticket=++tickets.perf;
  await ensureEchartsReady();
  if(owner!==generation||ticket!==tickets.perf)return;
  chartInstances.perf = initECharts(box);
  if (!chartInstances.perf) return;

  const colors = getThemeColors();
  const dates = filteredNav.map(n => n.navDate);
  const values = filteredNav.map(n => parseFloat(n.unitNav));

  chartInstances.perf.setOption({
    backgroundColor: "transparent",
    animation: false,
    grid: { left: 45, right: 20, top: 20, bottom: 25 },
    xAxis: {
      type: "category",
      data: dates,
      axisLine: { lineStyle: { color: colors.border } },
      axisLabel: { color: colors.muted, fontSize: 12 },
    },
    yAxis: {
      type: "value",
      scale: true,
      splitLine: { lineStyle: { color: colors.grid } },
      axisLabel: { color: colors.muted, fontSize: 12 },
    },
    series: [{
      type: "line",
      data: values,
      smooth: true,
      showSymbol: values.length === 1,
      symbolSize: 6,
      lineStyle: { color: colors.accent, width: 2 },
    }],
  });
}

async function renderProfitChart(box, profitPoints, isHidden) {
  if (!box) return;
  if (isHidden) {
    tickets.profit++;
    box.innerHTML = `<div class="history-empty" style="display:flex;align-items:center;justify-content:center;height:100%;color:var(--natives-text-muted)"><p>${t("fundHomeOverviewHistoryHidden")}</p></div>`;
    if (chartInstances.profit) {
      try { chartInstances.profit.dispose(); } catch (_) {}
      chartInstances.profit = null;
    }
    return;
  }

  const owner=generation,ticket=++tickets.profit;
  await ensureEchartsReady();
  if(owner!==generation||ticket!==tickets.profit)return;
  if(chartInstances.profit?.isDisposed?.())chartInstances.profit=null;
  if(!chartInstances.profit)box.replaceChildren?.();
  chartInstances.profit = initECharts(box);
  if (!chartInstances.profit) return;

  const colors = getThemeColors();
  const dates = profitPoints.map(p => p.date);
  const values=profitPoints.map(p=>p.amount==null?null:Number(p.amount));

  chartInstances.profit.setOption({
    backgroundColor: "transparent",
    animation: false,
    grid: { left: 55, right: 20, top: 20, bottom: 25 },
    xAxis: {
      type: "category",
      data: dates,
      axisLine: { lineStyle: { color: colors.border } },
      axisLabel: { color: colors.muted, fontSize: 12 },
    },
    yAxis: {
      type: "value",
      scale: true,
      splitLine: { lineStyle: { color: colors.grid } },
      axisLabel: { color: colors.muted, fontSize: 12 },
    },
    series: [{
      type: "line",
      data: values,
      smooth: true,
      showSymbol: values.length === 1,
      symbolSize: 6,
      lineStyle: { color: colors.accent, width: 2 },
    }],
  });
}

function resizeAll() {
  Object.values(chartInstances).forEach(c => {
    if (c) try { c.resize(); } catch (_) {}
  });
}

function disposeAll() {
  generation++;tickets.minute++;tickets.perf++;tickets.profit++;
  Object.values(chartInstances).forEach(c => {
    if (c) try { c.dispose(); } catch (_) {}
  });
  chartInstances = { minute: null, perf: null, profit: null };
}

function clearMinute() {
  tickets.minute++;
  if (chartInstances.minute) try { chartInstances.minute.clear(); } catch (_) {}
}

globalThis.Fund.holdingDetailCharts = {
  ensureEchartsReady,
  renderMinuteChart,
  renderPerfChart,
  renderProfitChart,
  resizeAll,
  disposeAll,
  clearMinute,
};
})();
