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
  const mo = document.getElementById('view-month');
  const mn = document.getElementById('view-minute');
  const kc = document.getElementById('kline-chart');
  const vc = document.getElementById('volume-chart');
  const mc = document.getElementById('minute-chart');
  const mv = document.getElementById('minute-vol');
  const sep = document.getElementById('sep-range');

  if (dk) dk.classList.remove('active');
  if (wk) wk.classList.remove('active');
  if (mo) mo.classList.remove('active');
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
    // dayk / week / month
    if (view === 'week') { if (wk) wk.classList.add('active'); }
    else if (view === 'month') { if (mo) mo.classList.add('active'); }
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

window.switchView = switchView;

// 挂载图表能力到 trends.chart（其余能力由 trends-kline/minute/interact 分区追加）
trends.chart = {
  initCharts,
  switchView,
};

})();
