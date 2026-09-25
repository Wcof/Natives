(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：图表交互与划线层（Drawings）。职责一句话——在交互 Canvas 上
// 渲染十字光标、技术划线（趋势线/水平线/矩形）、锚点碰撞检测与划线增删持久化。
// import 方向：core（状态/api/取色）；跨域经 actions（chart 坐标映射与重绘联动）。

const interCanvas = document.getElementById("chart-interactive");
const klineArea = document.getElementById("chart-kline-area");
const chartContainer = document.getElementById("chart-container");
const iCtx = interCanvas ? interCanvas.getContext("2d") : null;
let currentCursor = null;

// 交互层主题色彩定义
function getInteractiveColors() {
  return {
    crosshair: themeRgba("--natives-text-muted", 0.35),
    lineDefault: themeColor("--natives-accent"),
    lineSelected: themeColor("--natives-accent-strong"),
    rectFill: themeRgba("--natives-accent", 0.08),
    anchorFill: themeColor("--natives-surface"),
    anchorStroke: themeColor("--natives-accent"),
    anchorSelectedStroke: themeColor("--natives-accent-strong"),
  };
}

function loadDrawings() {
  if (!state.currentSymbol) return;
  api("GET", "/api/drawings?symbol=" + state.currentSymbol + "&period=" + state.currentPeriod).then(function(res) {
    state.drawings = res.drawings || [];
    redrawInteractive();
  });
}
actions.loadDrawings = loadDrawings;

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
actions.redrawInteractive = redrawInteractive;

function renderCrosshair(x, y) {
  if (x == null || y == null) return;
  const bounds = actions.getChartBounds ? actions.getChartBounds() : { chartW: 340, chartH: 200 };
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
  const bounds = actions.getChartBounds ? actions.getChartBounds() : { chartW: 340, chartH: 200 };
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
  const x = actions.timeIndexToX ? actions.timeIndexToX(pt.timeIndex || 0, total) : 10;
  const y = actions.priceToY ? actions.priceToY(pt.price) : 50;
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

// 划线与工具栏事件监听
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

    const snap = actions.applyMagnet ? actions.applyMagnet(mx, my) : { x: mx, y: my, price: my };
    state.isDrawing = true;
    const timeIdx = actions.xToTimeIndex ? actions.xToTimeIndex(snap.x, total) : 0;
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
    const snap = actions.applyMagnet ? actions.applyMagnet(mx, my) : { x: mx, y: my, price: my };

    currentCursor = { x: snap.x, y: snap.y };

    if (state.isDrawing && state.tempDrawing) {
      state.tempDrawing.points[1] = { price: snap.price, timeIndex: actions.xToTimeIndex ? actions.xToTimeIndex(snap.x, total) : 0 };
      redrawInteractive();
      return;
    }

    if (state.selectedDrawing && state.isDraggingAnchor >= 0) {
      state.selectedDrawing.points[state.isDraggingAnchor] = { price: snap.price, timeIndex: actions.xToTimeIndex ? actions.xToTimeIndex(snap.x, total) : 0 };
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
  tt.textContent = "价格: " + (typeof price === "number" ? price.toFixed(2) : price);
}

})();
