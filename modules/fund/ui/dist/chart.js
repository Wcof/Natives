(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：Trends 图表。职责一句话——在双层 Canvas 上渲染分时/K线
// 底图（含 MA 均线、成交量副图、极值标注）与划线交互层（磁吸、选中、拖拽）。
// import 方向：core（状态/api/主题取色）；跨域经 actions（theme 重绘由 app.js 触发）。

const mainCanvas = document.getElementById("chart-main");
const interCanvas = document.getElementById("chart-interactive");
const klineArea = document.getElementById("chart-kline-area");
const volCanvas = document.getElementById("chart-volume");
const chartContainer = document.getElementById("chart-container");
const compareCanvas = document.getElementById("chart-compare");
const mCtx = mainCanvas ? mainCanvas.getContext("2d") : null;
const iCtx = interCanvas ? interCanvas.getContext("2d") : null;
const vCtx = volCanvas ? volCanvas.getContext("2d") : null;
const cCtx = compareCanvas ? compareCanvas.getContext("2d") : null;

const bounds = { minPrice: 0, maxPrice: 0, chartW: 340, chartH: 200 };

// ---------- 坐标映射（价格↔像素、时间索引↔X，严格在 K 线主图区域内映射） ----------
function priceToY(p) {
  const range = (bounds.maxPrice - bounds.minPrice) || 1;
  return 15 + (1 - (p - bounds.minPrice) / range) * (bounds.chartH - 30);
}

function yToPrice(y) {
  const range = (bounds.maxPrice - bounds.minPrice) || 1;
  return bounds.minPrice + (1 - (y - 15) / (bounds.chartH - 30)) * range;
}

function timeIndexToX(idx, total) {
  if (total <= 1) return 10;
  return 10 + (idx / (total - 1)) * (bounds.chartW - 20);
}

function xToTimeIndex(x, total) {
  if (total <= 1) return 0;
  const norm = (x - 10) / (bounds.chartW - 20);
  return Math.max(0, Math.min(total - 1, Math.round(norm * (total - 1))));
}

function applyMagnet(x, y) {
  if (!state.magnetEnabled) return { x: x, y: y, price: yToPrice(y) };
  if (state.currentKlineData && state.currentKlineData.length > 0) {
    const idx = xToTimeIndex(x, state.currentKlineData.length);
    const bar = state.currentKlineData[idx];
    if (bar) {
      const snappedX = timeIndexToX(idx, state.currentKlineData.length);
      const candidates = [bar.open, bar.close, bar.high, bar.low];
      let bestY = y;
      let bestP = yToPrice(y);
      let minDiff = 12;
      candidates.forEach(function(cand) {
        const candY = priceToY(cand);
        if (Math.abs(candY - y) < minDiff) {
          minDiff = Math.abs(candY - y);
          bestY = candY;
          bestP = cand;
        }
      });
      return { x: snappedX, y: bestY, price: bestP, snapped: minDiff < 12 };
    }
  }
  return { x: x, y: y, price: yToPrice(y), snapped: false };
}

// ---------- 底图：网格 + 分时(VWAP) / K线(MA) ----------
function redrawChart() {
  if (!mainCanvas || !mCtx) return;
  const kRect = (klineArea || chartContainer || mainCanvas).getBoundingClientRect();
  bounds.chartW = kRect.width;
  bounds.chartH = kRect.height;

  mCtx.clearRect(0, 0, kRect.width, kRect.height);

  mCtx.strokeStyle = themeColor("--natives-border");
  mCtx.lineWidth = 1;
  for (let gridY = 25; gridY < bounds.chartH; gridY += 35) {
    mCtx.beginPath();
    mCtx.moveTo(0, gridY);
    mCtx.lineTo(bounds.chartW, gridY);
    mCtx.stroke();
  }

  if (state.currentPeriod === "minute" && state.currentMinuteData && state.currentMinuteData.length > 0) {
    renderMinuteChart();
  } else if (state.currentKlineData && state.currentKlineData.length > 0) {
    renderCandleChart();
  }
  redrawInteractive();
  renderVolumeChart();
}
actions.redrawChart = redrawChart;

function renderMinuteChart() {
  const pts = state.currentMinuteData;
  const prices = pts.map(function(p) { return p.price; });
  const vwapList = pts.map(function(p) { return p.vwap; });
  const maxP = Math.max.apply(null, prices.concat(vwapList));
  const minP = Math.min.apply(null, prices.concat(vwapList));
  bounds.minPrice = minP;
  bounds.maxPrice = maxP;

  // 1. 白色价格走势线
  mCtx.beginPath();
  mCtx.strokeStyle = themeColor("--natives-text-primary");
  mCtx.lineWidth = 1.5;
  for (let i = 0; i < pts.length; i++) {
    const x = timeIndexToX(i, pts.length);
    const y = priceToY(pts[i].price);
    if (i === 0) mCtx.moveTo(x, y);
    else mCtx.lineTo(x, y);
  }
  mCtx.stroke();

  // 2. 黄色 VWAP 日内均价线
  mCtx.beginPath();
  mCtx.strokeStyle = themeColor("--natives-accent");
  mCtx.lineWidth = 1.2;
  for (let j = 0; j < pts.length; j++) {
    const vx = timeIndexToX(j, pts.length);
    const vy = priceToY(pts[j].vwap);
    if (j === 0) mCtx.moveTo(vx, vy);
    else mCtx.lineTo(vx, vy);
  }
  mCtx.stroke();

  // 3. 当前价格水平虚线标尺 (token-monitor)
  const lastP = pts[pts.length - 1].price;
  const lastY = priceToY(lastP);
  mCtx.save();
  mCtx.strokeStyle = themeRgba("--natives-accent", 0.7);
  mCtx.setLineDash([4, 4]);
  mCtx.beginPath();
  mCtx.moveTo(0, lastY);
  mCtx.lineTo(bounds.chartW, lastY);
  mCtx.stroke();
  mCtx.restore();

  // 4. 极值标注 (High & Low)
  drawExtremaAnnotations(prices, pts.length);
}

function renderCandleChart() {
  const bars = state.currentKlineData;
  const highs = bars.map(function(b) { return b.high; });
  const lows = bars.map(function(b) { return b.low; });
  bounds.minPrice = Math.min.apply(null, lows);
  bounds.maxPrice = Math.max.apply(null, highs);

  const barW = Math.max(2, (bounds.chartW - 20) / bars.length * 0.7);

  // MA 均线（同花顺配色：MA5 白 / MA10 黄 / MA20 粉）
  const closes = bars.map(function(b) { return b.close; });
  const maDefs = [
    { n: 5, color: "--natives-text-primary", label: "MA5" },
    { n: 10, color: "--natives-accent", label: "MA10" },
    { n: 20, color: "#e58bc0", label: "MA20" },
  ];
  maDefs.forEach(function(def) {
    mCtx.beginPath();
    mCtx.strokeStyle = themeColor(def.color);
    mCtx.lineWidth = 1;
    let started = false;
    for (let i = def.n - 1; i < closes.length; i++) {
      let sum = 0;
      for (let j = i - def.n + 1; j <= i; j++) sum += closes[j];
      const ma = sum / def.n;
      // 均线不改变价格区间（超出时不裁剪视觉判断，仅按现有 bounds 绘制）
      const x = timeIndexToX(i, bars.length);
      const y = priceToY(ma);
      if (!started) { mCtx.moveTo(x, y); started = true; }
      else mCtx.lineTo(x, y);
    }
    mCtx.stroke();
  });
  // MA 图例（复用 closes，不重复 map）
  mCtx.font = "10px sans-serif";
  let legendX = 6;
  maDefs.forEach(function(def) {
    const i = closes.length - 1;
    let val = "–";
    if (i >= def.n - 1) {
      let sum = 0;
      for (let j = i - def.n + 1; j <= i; j++) sum += closes[j];
      val = (sum / def.n).toFixed(2);
    }
    mCtx.fillStyle = themeColor(def.color);
    mCtx.fillText(def.label + ": " + val, legendX, 12);
    legendX += 70;
  });

  for (let i = 0; i < bars.length; i++) {
    const b = bars[i];
    const x = timeIndexToX(i, bars.length);
    const openY = priceToY(b.open);
    const closeY = priceToY(b.close);
    const highY = priceToY(b.high);
    const lowY = priceToY(b.low);
    const isUp = b.close >= b.open;

    mCtx.strokeStyle = isUp ? themeColor("--color-up") : themeColor("--color-down");
    mCtx.fillStyle = isUp ? themeColor("--color-up") : themeColor("--color-down");

    mCtx.beginPath();
    mCtx.moveTo(x, highY);
    mCtx.lineTo(x, lowY);
    mCtx.stroke();

    const top = Math.min(openY, closeY);
    const h = Math.max(1, Math.abs(closeY - openY));
    mCtx.fillRect(x - barW / 2, top, barW, h);
  }

  // 现价虚线标尺
  if (bars.length > 0) {
    const lastBarClose = bars[bars.length - 1].close;
    const lastY = priceToY(lastBarClose);
    mCtx.save();
    mCtx.strokeStyle = themeRgba("--natives-accent", 0.7);
    mCtx.setLineDash([4, 4]);
    mCtx.beginPath();
    mCtx.moveTo(0, lastY);
    mCtx.lineTo(bounds.chartW, lastY);
    mCtx.stroke();
    mCtx.restore();
  }

  // 极值标注
  drawExtremaAnnotations(highs, bars.length, lows);
}

// ---------- 买量和卖量（成交量副图）独立绘制 ----------
function renderVolumeChart() {
  if (!volCanvas || !vCtx) return;
  const vRect = volCanvas.getBoundingClientRect();
  const vw = vRect.width;
  const vh = vRect.height;
  vCtx.clearRect(0, 0, vw, vh);

  let vols = [];
  let isUpList = [];
  let total = 0;
  let barW = 2;

  if (state.currentPeriod === "minute" && state.currentMinuteData && state.currentMinuteData.length > 0) {
    const pts = state.currentMinuteData;
    total = pts.length;
    vols = pts.map(function(p) { return p.volume || 0; });
    isUpList = pts.map(function(p, idx) {
      return idx === 0 || p.price >= pts[idx - 1].price;
    });
    barW = Math.max(1, (bounds.chartW - 20) / total * 0.6);
  } else if (state.currentKlineData && state.currentKlineData.length > 0) {
    const bars = state.currentKlineData;
    total = bars.length;
    vols = bars.map(function(b) { return b.volume || 0; });
    isUpList = bars.map(function(b) { return b.close >= b.open; });
    barW = Math.max(2, (bounds.chartW - 20) / total * 0.7);
  }

  if (total === 0 || vols.length === 0) return;
  const maxV = Math.max.apply(null, vols) || 1;
  const lastVol = vols[vols.length - 1];
  const volEl = document.getElementById("vol-indicator-val");
  if (volEl) {
    volEl.textContent = lastVol >= 10000 ? (lastVol / 10000).toFixed(2) + "万手" : lastVol.toFixed(0) + "手";
    volEl.className = "num " + (isUpList[isUpList.length - 1] ? "up" : "down");
  }

  for (let k = 0; k < total; k++) {
    const x = timeIndexToX(k, total);
    const h = Math.max(1, (vols[k] / maxV) * (vh - 4));
    vCtx.fillStyle = isUpList[k] ? themeColor("--color-up") : themeColor("--color-down");
    vCtx.fillRect(x - barW / 2, vh - h, barW, h);
  }
}

// 极值气泡标注
function drawExtremaAnnotations(highs, total, optionalLows) {
  let maxIdx = 0, minIdx = 0;
  let maxVal = highs[0], minVal = optionalLows ? optionalLows[0] : highs[0];
  for (let i = 1; i < highs.length; i++) {
    if (highs[i] > maxVal) { maxVal = highs[i]; maxIdx = i; }
    const lv = optionalLows ? optionalLows[i] : highs[i];
    if (lv < minVal) { minVal = lv; minIdx = i; }
  }

  mCtx.save();
  mCtx.font = "9px monospace";
  mCtx.fillStyle = themeColor("--natives-text-primary");

  // High 标注
  const hx = timeIndexToX(maxIdx, total);
  const hy = priceToY(maxVal);
  mCtx.fillText("H:" + maxVal.toFixed(2), hx - 15, Math.max(12, hy - 4));

  // Low 标注
  const lx = timeIndexToX(minIdx, total);
  const ly = priceToY(minVal);
  mCtx.fillText("L:" + minVal.toFixed(2), lx - 15, Math.min(bounds.chartH - 4, ly + 10));
  mCtx.restore();
}

// ---------- 大盘对照图 ----------
function renderCompareChart() {
  if (!compareCanvas || !cCtx) return;
  const cRect = compareCanvas.getBoundingClientRect();
  cCtx.clearRect(0, 0, cRect.width, cRect.height);
  if (!state.indexMinuteData || state.indexMinuteData.length < 2) return;

  const prices = state.indexMinuteData.map(function(p) { return p.price; });
  const min = Math.min.apply(null, prices);
  const max = Math.max.apply(null, prices);
  const range = (max - min) || 1;

  cCtx.beginPath();
  cCtx.strokeStyle = themeColor("--natives-accent");
  cCtx.lineWidth = 1.2;
  for (let i = 0; i < state.indexMinuteData.length; i++) {
    const x = (i / (state.indexMinuteData.length - 1)) * (cRect.width - 4) + 2;
    const y = (cRect.height - 4) - ((prices[i] - min) / range) * (cRect.height - 8);
    if (i === 0) cCtx.moveTo(x, y);
    else cCtx.lineTo(x, y);
  }
  cCtx.stroke();
}
actions.renderCompareChart = renderCompareChart;

// ---------- 数据加载 ----------
function resizeCanvas() {
  if (!mainCanvas || !mCtx || !interCanvas || !iCtx) return;
  const kRect = (klineArea || chartContainer || mainCanvas).getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  bounds.chartW = kRect.width;
  bounds.chartH = kRect.height;

  mainCanvas.width = kRect.width * dpr;
  mainCanvas.height = kRect.height * dpr;
  interCanvas.width = kRect.width * dpr;
  interCanvas.height = kRect.height * dpr;
  mCtx.setTransform(1, 0, 0, 1, 0, 0);
  iCtx.setTransform(1, 0, 0, 1, 0, 0);
  mCtx.scale(dpr, dpr);
  iCtx.scale(dpr, dpr);

  if (volCanvas && vCtx) {
    const vRect = volCanvas.getBoundingClientRect();
    volCanvas.width = vRect.width * dpr;
    volCanvas.height = vRect.height * dpr;
    vCtx.setTransform(1, 0, 0, 1, 0, 0);
    vCtx.scale(dpr, dpr);
  }

  if (compareCanvas && cCtx) {
    const cRect = compareCanvas.getBoundingClientRect();
    compareCanvas.width = cRect.width * dpr;
    compareCanvas.height = cRect.height * dpr;
    cCtx.setTransform(1, 0, 0, 1, 0, 0);
    cCtx.scale(dpr, dpr);
  }

  redrawChart();
  renderCompareChart();
}
actions.resizeCanvas = resizeCanvas;
window.addEventListener("resize", resizeCanvas);

function loadChartData() {
  if (!state.currentSymbol) return;
  if (state.currentPeriod === "minute") {
    api("GET", "/api/market/minute?symbol=" + state.currentSymbol).then(function(res) {
      state.currentMinuteData = res.points || [];
      state.currentKlineData = null;
      resizeCanvas();
    });
  } else {
    // 7d 复用日K接口，仅取最近 7 根日线展示
    const apiPeriod = state.currentPeriod === "7d" ? "day" : state.currentPeriod;
    api("GET", "/api/market/kline?symbol=" + state.currentSymbol + "&period=" + apiPeriod + "&fq=" + (state.fqType || "qfq")).then(function(res) {
      let candles = res.candles || [];
      if (state.currentPeriod === "7d") candles = candles.slice(-7);
      state.currentKlineData = candles;
      state.currentMinuteData = null;
      resizeCanvas();
    });
  }
}
actions.loadChartData = loadChartData;

function loadCompareIndex() {
  if (!compareCanvas || !cCtx) return;
  api("GET", "/api/market/minute?symbol=sh000001").then(function(res) {
    state.indexMinuteData = res.points || [];
    if (state.indexMinuteData.length > 0) {
      const last = state.indexMinuteData[state.indexMinuteData.length - 1];
      const compVal = document.getElementById("compare-index-val");
      if (compVal) compVal.textContent = last.price.toFixed(2);
    }
    renderCompareChart();
  }).catch(function() {});
}
actions.loadCompareIndex = loadCompareIndex;

function loadDrawings() {
  api("GET", "/api/drawings?symbol=" + state.currentSymbol + "&period=" + state.currentPeriod).then(function(res) {
    state.drawings = res.drawings || [];
    redrawInteractive();
  });
}
actions.loadDrawings = loadDrawings;

// ---------- 周期 / 工具 / 磁吸 / 删除划线 ----------
const pBtns = document.querySelectorAll("button.period-btn");
pBtns.forEach(function(b) {
  b.addEventListener("click", function() {
    pBtns.forEach(function(o) { o.classList.remove("active"); });
    b.classList.add("active");
    state.currentPeriod = b.dataset.period;
    loadChartData();
    loadDrawings();
  });
});

// 复权模式切换：qfq/hfq/bf，切换后重拉 K 线
const fqSelect = document.getElementById("fq-select");
if (fqSelect) {
  fqSelect.addEventListener("change", function() {
    state.fqType = fqSelect.value;
    loadChartData();
  });
}

const tBtns = document.querySelectorAll(".tool-btn[data-tool]");
tBtns.forEach(function(b) {
  b.addEventListener("click", function() {
    tBtns.forEach(function(o) { o.classList.remove("active"); });
    b.classList.add("active");
    state.currentTool = b.dataset.tool;
  });
});

const magnetBtn = document.getElementById("btn-magnet");
if (magnetBtn) {
  magnetBtn.addEventListener("click", function() {
    state.magnetEnabled = !state.magnetEnabled;
    magnetBtn.style.opacity = state.magnetEnabled ? "1" : "0.4";
  });
}

const clearDrawBtn = document.getElementById("btn-clear-draw");
if (clearDrawBtn) {
  clearDrawBtn.addEventListener("click", function() {
    if (state.selectedDrawing) {
      api("DELETE", "/api/drawings?id=" + state.selectedDrawing.id).then(function() {
        state.drawings = state.drawings.filter(function(d) { return d.id !== state.selectedDrawing.id; });
        state.selectedDrawing = null;
        redrawInteractive();
      });
    }
  });
}

// ---------- 划线层专业金融配色（支持亮暗自适应、避免与红绿K线混淆） ----------
function getInteractiveColors() {
  const isLight = document.documentElement.dataset.theme === "archive";
  return {
    lineDefault: isLight ? "#0284c7" : "#38bdf8",
    lineSelected: isLight ? "#d97706" : "#f59e0b",
    rectFill: isLight ? "rgba(2, 132, 199, 0.12)" : "rgba(56, 189, 248, 0.18)",
    anchorFill: "#ffffff",
    anchorStroke: isLight ? "#0284c7" : "#38bdf8",
    anchorSelectedStroke: isLight ? "#d97706" : "#f59e0b",
    crosshair: isLight ? "rgba(26, 26, 24, 0.28)" : "rgba(242, 242, 234, 0.32)",
  };
}

let currentCursor = null;

// ---------- 划线层渲染与交互（严格在 K 线主图层生效） ----------
function redrawInteractive() {
  if (!interCanvas || !iCtx) return;
  const kRect = (klineArea || chartContainer || interCanvas).getBoundingClientRect();
  iCtx.clearRect(0, 0, kRect.width, kRect.height);

  if (currentCursor) {
    renderCrosshair(currentCursor.x, currentCursor.y);
  }

  state.drawings.forEach(function(d) {
    renderDrawing(d, state.selectedDrawing && state.selectedDrawing.id === d.id);
  });

  if (state.tempDrawing) {
    renderDrawing(state.tempDrawing, true);
  }
}

function renderCrosshair(x, y) {
  if (x == null || y == null) return;
  const colors = getInteractiveColors();
  iCtx.save();
  iCtx.strokeStyle = colors.crosshair;
  iCtx.lineWidth = 1;
  iCtx.setLineDash([4, 4]);

  iCtx.beginPath();
  iCtx.moveTo(0, y);
  iCtx.lineTo(bounds.chartW, y);
  iCtx.stroke();

  iCtx.beginPath();
  iCtx.moveTo(x, 0);
  iCtx.lineTo(x, bounds.chartH);
  iCtx.stroke();
  iCtx.restore();
}

function renderDrawing(d, isSel) {
  if (!d.points || d.points.length < 2) return;
  const colors = getInteractiveColors();
  const p1 = d.points[0];
  const p2 = d.points[1];
  const x1 = pricePointToScreen(p1).x;
  const y1 = pricePointToScreen(p1).y;
  const x2 = pricePointToScreen(p2).x;
  const y2 = pricePointToScreen(p2).y;

  iCtx.save();
  const strokeColor = isSel ? colors.lineSelected : (d.options && d.options.color) || colors.lineDefault;
  iCtx.strokeStyle = strokeColor;
  iCtx.lineWidth = isSel ? 2.5 : 1.8;

  if (isSel) {
    iCtx.shadowColor = colors.lineSelected;
    iCtx.shadowBlur = 4;
  }

  if (d.toolType === "trendline") {
    iCtx.beginPath();
    iCtx.moveTo(x1, y1);
    iCtx.lineTo(x2, y2);
    iCtx.stroke();
  } else if (d.toolType === "horizontal") {
    iCtx.beginPath();
    iCtx.setLineDash([5, 4]);
    iCtx.moveTo(0, y1);
    iCtx.lineTo(bounds.chartW, y1);
    iCtx.stroke();
  } else if (d.toolType === "rectangle") {
    iCtx.beginPath();
    iCtx.fillStyle = colors.rectFill;
    iCtx.rect(Math.min(x1, x2), Math.min(y1, y2), Math.abs(x2 - x1), Math.abs(y2 - y1));
    iCtx.fill();
    iCtx.stroke();
  }

  if (isSel) {
    drawAnchor(x1, y1, colors.anchorSelectedStroke);
    drawAnchor(x2, y2, colors.anchorSelectedStroke);
  }
  iCtx.restore();
}

function drawAnchor(x, y, strokeColor) {
  const colors = getInteractiveColors();
  iCtx.save();
  iCtx.fillStyle = colors.anchorFill;
  iCtx.strokeStyle = strokeColor || colors.anchorStroke;
  iCtx.lineWidth = 2;
  iCtx.beginPath();
  iCtx.arc(x, y, 4.5, 0, Math.PI * 2);
  iCtx.fill();
  iCtx.stroke();
  iCtx.restore();
}

function pricePointToScreen(pt) {
  const total = (state.currentKlineData && state.currentKlineData.length) || (state.currentMinuteData && state.currentMinuteData.length) || 1;
  const x = timeIndexToX(pt.timeIndex || 0, total);
  const y = priceToY(pt.price);
  return { x: x, y: y };
}

function hitTestDrawings(mx, my) {
  for (let i = state.drawings.length - 1; i >= 0; i--) {
    const d = state.drawings[i];
    const p1 = pricePointToScreen(d.points[0]);
    const p2 = pricePointToScreen(d.points[1]);

    if (Math.hypot(mx - p1.x, my - p1.y) <= 8) return { drawing: d, anchor: 0 };
    if (Math.hypot(mx - p2.x, my - p2.y) <= 8) return { drawing: d, anchor: 1 };

    if (d.toolType === "horizontal" && Math.abs(my - p1.y) <= 6) return { drawing: d, anchor: -1 };
    if (d.toolType === "trendline") {
      if (distToSegment({ x: mx, y: my }, p1, p2) <= 6) return { drawing: d, anchor: -1 };
    }
    if (d.toolType === "rectangle") {
      const minX = Math.min(p1.x, p2.x), maxX = Math.max(p1.x, p2.x);
      const minY = Math.min(p1.y, p2.y), maxY = Math.max(p1.y, p2.y);
      if (mx >= minX && mx <= maxX && my >= minY && my <= maxY) return { drawing: d, anchor: -1 };
    }
  }
  return null;
}

function distToSegment(p, v, w) {
  const l2 = Math.hypot(v.x - w.x, v.y - w.y) ** 2;
  if (l2 === 0) return Math.hypot(p.x - v.x, p.y - v.y);
  const t = Math.max(0, Math.min(1, ((p.x - v.x) * (w.x - v.x) + (p.y - v.y) * (w.y - v.y)) / l2));
  return Math.hypot(p.x - (v.x + t * (w.x - v.x)), p.y - (v.y + t * (w.y - v.y)));
}

if (interCanvas) {
  interCanvas.addEventListener("mousedown", function(e) {
    const rect = (klineArea || chartContainer || interCanvas).getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const total = (state.currentKlineData && state.currentKlineData.length) || (state.currentMinuteData && state.currentMinuteData.length) || 1;

    if (state.currentTool === "cursor") {
      const hit = hitTestDrawings(mx, my);
      if (hit) {
        state.selectedDrawing = hit.drawing;
        state.isDraggingAnchor = hit.anchor;
      } else {
        state.selectedDrawing = null;
      }
      redrawInteractive();
      return;
    }

    const snap = applyMagnet(mx, my);
    state.isDrawing = true;
    const timeIdx = xToTimeIndex(snap.x, total);
    const colors = getInteractiveColors();
    state.tempDrawing = {
      id: "draw_" + Date.now(),
      symbol: state.currentSymbol,
      period: state.currentPeriod,
      toolType: state.currentTool,
      points: [
        { price: snap.price, timeIndex: timeIdx },
        { price: snap.price, timeIndex: timeIdx }
      ],
      options: { color: colors.lineDefault }
    };
    redrawInteractive();
  });

  interCanvas.addEventListener("mousemove", function(e) {
    const rect = (klineArea || chartContainer || interCanvas).getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const total = (state.currentKlineData && state.currentKlineData.length) || (state.currentMinuteData && state.currentMinuteData.length) || 1;
    const snap = applyMagnet(mx, my);

    currentCursor = { x: snap.x, y: snap.y };

    if (state.isDrawing && state.tempDrawing) {
      state.tempDrawing.points[1] = { price: snap.price, timeIndex: xToTimeIndex(snap.x, total) };
      redrawInteractive();
      return;
    }

    if (state.selectedDrawing && state.isDraggingAnchor >= 0) {
      state.selectedDrawing.points[state.isDraggingAnchor] = { price: snap.price, timeIndex: xToTimeIndex(snap.x, total) };
      redrawInteractive();
      return;
    }

    redrawInteractive();
    showCrosshairTooltip(snap.x, snap.y, snap.price);
  });

  interCanvas.addEventListener("mouseleave", function() {
    currentCursor = null;
    redrawInteractive();
    const tt = document.getElementById("chart-tooltip");
    if (tt) tt.style.display = "none";
  });
}

window.addEventListener("mouseup", function() {
  if (state.isDrawing && state.tempDrawing) {
    state.isDrawing = false;
    state.drawings.push(state.tempDrawing);
    state.selectedDrawing = state.tempDrawing;
    api("POST", "/api/drawings", state.tempDrawing);
    state.tempDrawing = null;
    state.currentTool = "cursor";
    document.querySelectorAll(".tool-btn").forEach(function(b) {
      b.classList.toggle("active", b.dataset.tool === "cursor");
    });
    redrawInteractive();
  }
  state.isDraggingAnchor = -1;
});

function showCrosshairTooltip(x, y, price) {
  const tt = document.getElementById("chart-tooltip");
  if (!tt) return;
  tt.style.display = "block";
  tt.textContent = "价格: " + price.toFixed(2);
}

})();
