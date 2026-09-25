(() => {
const trends = globalThis.Fund.trends;
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;
const { authFetch } = globalThis.Fund;
// Fund 趋势分析子系统：K线划线层（Drawings）。职责一句话——在 ECharts K线主图上
// 渲染用户划线（趋势线/水平线/矩形/通道），提供绘制、选中、拖拽锚点、Delete 删除
// 与 /api/drawings 按 symbol+period 持久化。
// 复用：图形持久化表与 API（chart_drawings，与原 Canvas 划线层同一后端）、
// 交互模型（原 chart-drawings.js 的像素级选中/拖拽/磁吸思路）、主题色（trends.C）。
// 渲染用 ECharts graphic 组件（replaceMerge 只替换 graphic 层，不触碰 K线/MA series），
// 数据坐标 → 像素经 convertToPixel 换算，dataZoom/resize/换标的时整层重绘。

const ACCENT = (C && C.avgLine) || '#cdf24b';
const SEL = '#ddff69';
const COLORS = { trendline: ACCENT, horizontal: '#4fc3f7', rectangle: '#ff9800', channel: '#e040fb' };

// d = { id, symbol, period, toolType, points:[{date, price}...], options:{color} }
// points 约定：trendline 2 点；horizontal 1 点（y 轴价）；rectangle 2 点（对角）；
// channel 3 点（p1/p2 定基线，p3 定平行通道的偏移方向与宽度）。
let drawings = [];
let tempDrawing = null;
let drawingTool = 'cursor';   // cursor | trendline | horizontal | rectangle | channel
let selected = null;          // { id, anchor }  anchor=-1 整体, 0/1/2 锚点
let dragging = false;

// ---------- 持久化（复用 /api/drawings） ----------
function periodKey() {
  return tState.currentView === 'week' ? 'week' : (tState.currentView === 'month' ? 'month' : 'day');
}
function listDrawings() {
  const sym = tState.currentSymbol;
  if (!sym) return;
  const period = periodKey();
  authFetch('/api/drawings?symbol=' + sym + '&period=' + period)
    .then(r => r.json())
    .then(res => {
      // 后端 normalize_symbol 会补 sh/sz/bj 前缀，前端统一归一化后再比较
      drawings = (res.drawings || []).filter(d => normSym(d.symbol) === normSym(sym) && d.period === period);
      selected = null;
      renderDrawings();
    })
    .catch(() => {});
}
function saveDrawing(d) {
  authFetch('/api/drawings', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id: d.id, symbol: d.symbol, period: d.period, toolType: d.toolType, points: d.points, options: d.options || {} }),
  }).catch(() => {});
}
function deleteDrawing(id) {
  authFetch('/api/drawings?id=' + encodeURIComponent(id), { method: 'DELETE' }).catch(() => {});
}

// 与后端 market::normalize_symbol 对齐的前端归一化（6位数字代码补交易所前缀）
function normSym(raw) {
  const s = String(raw || '').trim().toLowerCase();
  if (/^(sh|sz|bj|us)/.test(s)) return s;
  if (/^\d{6}$/.test(s)) {
    if (s === '000001') return 'sh' + s;
    if (/^(60|68|51|56|58)/.test(s)) return 'sh' + s;
    if (/^(00|30|15|16|39)/.test(s)) return 'sz' + s;
    if (/^(8|4|92)/.test(s)) return 'bj' + s;
  }
  return s;
}

// ---------- 坐标换算（数据坐标 ↔ 像素，基于 K线主图 grid） ----------
function pxToDate(px) {
  try {
    const v = charts.klineChart.convertFromPixel({ xAxisIndex: 0 }, px);
    const n = Math.max(0, Math.min(tState.klineData.length - 1, Math.round(v)));
    return tState.klineData[n] ? tState.klineData[n].date : null;
  } catch (e) { return null; }
}
function pxToPrice(py) {
  try { return charts.klineChart.convertFromPixel({ yAxisIndex: 0 }, py); } catch (e) { return null; }
}
function pointToPixel(pt) {
  try {
    return charts.klineChart.convertToPixel({ xAxisIndex: 0, yAxisIndex: 0 }, [pt.date, pt.price]);
  } catch (e) { return null; }
}

// ---------- graphic 层渲染 ----------
function renderDrawings() {
  if (!charts.klineChart || !isKlineView()) return;
  const all = tempDrawing ? drawings.concat([tempDrawing]) : drawings.slice();
  const els = [];
  all.forEach(d => buildGraphic(d, els));
  // replaceMerge: ['graphic'] —— 只替换 graphic 组件层，K线/MA/成交量 series 不受影响
  charts.klineChart.setOption({ graphic: els }, { replaceMerge: ['graphic'] });
}

function buildGraphic(d, els) {
  const isSel = selected && selected.id === d.id;
  const color = (d.options && d.options.color) || COLORS[d.toolType] || ACCENT;
  const stroke = isSel ? SEL : color;
  const width = isSel ? 2.5 : 1.8;
  const pts = d.points.map(pointToPixel);
  if (pts.some(p => !p)) return;
  const style = { stroke: stroke, lineWidth: width };
  const prefix = 'dw_' + d.id;

  if (d.toolType === 'trendline' && pts.length >= 2) {
    els.push({ id: prefix + '_l', type: 'line', shape: { x1: pts[0][0], y1: pts[0][1], x2: pts[1][0], y2: pts[1][1] },
      style: style, silent: true, z: 60 });
  } else if (d.toolType === 'horizontal' && pts.length >= 1) {
    els.push({ id: prefix + '_l', type: 'line', shape: { x1: 0, y1: pts[0][1], x2: chartWidthPx(), y2: pts[0][1] },
      style: Object.assign({ lineDash: [5, 4] }, style), silent: true, z: 60 });
  } else if (d.toolType === 'rectangle' && pts.length >= 2) {
    els.push({ id: prefix + '_r', type: 'rect',
      shape: { x: Math.min(pts[0][0], pts[1][0]), y: Math.min(pts[0][1], pts[1][1]),
               width: Math.abs(pts[1][0] - pts[0][0]), height: Math.abs(pts[1][1] - pts[0][1]) },
      style: Object.assign({ fill: color + '22' }, style), silent: true, z: 60 });
  } else if (d.toolType === 'channel' && pts.length >= 3) {
    // p1-p2 基线 + 过 p3 偏移的平行线
    const off = [pts[2][0] - pts[0][0], pts[2][1] - pts[0][1]];
    const c1 = [pts[0][0] + off[0], pts[0][1] + off[1]];
    const c2 = [pts[1][0] + off[0], pts[1][1] + off[1]];
    els.push({ id: prefix + '_l1', type: 'line', shape: { x1: pts[0][0], y1: pts[0][1], x2: pts[1][0], y2: pts[1][1] },
      style: style, silent: true, z: 60 });
    els.push({ id: prefix + '_l2', type: 'line', shape: { x1: c1[0], y1: c1[1], x2: c2[0], y2: c2[1] },
      style: Object.assign({ lineDash: [5, 4] }, style), silent: true, z: 60 });
  } else {
    return;
  }
  if (isSel) {
    pts.forEach((p, i) => {
      els.push({ id: prefix + '_a' + i, type: 'circle', shape: { cx: p[0], cy: p[1], r: 4.5 },
        style: { fill: '#0b0c0a', stroke: SEL, lineWidth: 2 }, silent: true, z: 61 });
    });
  }
}

function chartWidthPx() {
  try {
    const dom = charts.klineChart.getDom();
    return dom.clientWidth - 60; // grid left:60，横贯到右缘
  } catch (e) { return 400; }
}

// ---------- zr 层交互（绘制/选中/拖拽，覆盖在 K 线主图上） ----------
function bindInteraction() {
  if (bindInteraction._bound || !charts.klineChart) return;
  bindInteraction._bound = true;
  const zr = charts.klineChart.getZr();
  if (!zr) return;

  zr.on('mousedown', function(e) {
    if (!isKlineView()) return;
    const x = e.offsetX, y = e.offsetY;

    if (drawingTool === 'cursor') {
      const hit = hitTest(x, y);
      selected = hit;
      dragging = !!hit;
      if (hit) renderDrawings();
      return;
    }

    // 绘制起点
    const date = pxToDate(x);
    const price = pxToPrice(y);
    if (date == null || price == null) return;
    const base = { date: date, price: price };
    const need = drawingTool === 'horizontal' ? 1 : (drawingTool === 'channel' ? 3 : 2);
    tempDrawing = {
      id: 'draw_' + Date.now(),
      symbol: tState.currentSymbol,
      period: periodKey(),
      toolType: drawingTool,
      points: new Array(need).fill(0).map(() => Object.assign({}, base)),
      options: { color: COLORS[drawingTool] },
    };
    if (need === 1) finishTemp(); // 水平线单击即成
    else renderDrawings();
  });

  zr.on('mousemove', function(e) {
    if (!isKlineView()) return;
    const x = e.offsetX, y = e.offsetY;

    if (dragging && selected) {
      const d = drawings.find(dd => dd.id === selected.id);
      if (!d) { dragging = false; return; }
      const date = pxToDate(x);
      const price = pxToPrice(y);
      if (date == null || price == null) return;
      moveAnchor(d, selected.anchor, date, price);
      renderDrawings();
      return;
    }

    if (tempDrawing) {
      const date = pxToDate(x);
      const price = pxToPrice(y);
      if (date == null || price == null) return;
      updateTemp(date, price);
      renderDrawings();
    }
  });

  zr.on('mouseup', function(e) {
    if (!isKlineView()) { dragging = false; return; }
    if (dragging && selected) {
      const d = drawings.find(dd => dd.id === selected.id);
      if (d) saveDrawing(d);
      dragging = false;
      return;
    }
    if (tempDrawing) {
      const x = e.offsetX, y = e.offsetY;
      const date = pxToDate(x);
      const price = pxToPrice(y);
      if (date == null || price == null) return;
      updateTemp(date, price);
      // channel 两段式：第一段拖出基线，第二段点击/拖动定平行线宽度
      if (tempDrawing.toolType === 'channel' && !tempDrawing._stage2) {
        tempDrawing._stage2 = true;
        return;
      }
      finishTemp();
    }
  });

  document.addEventListener('keydown', function(e) {
    if (e.key !== 'Delete' && e.key !== 'Backspace') return;
    const tag = (document.activeElement && document.activeElement.tagName || '').toLowerCase();
    if (tag === 'input' || tag === 'textarea') return;
    if (selected) {
      deleteDrawing(selected.id);
      drawings = drawings.filter(d => d.id !== selected.id);
      selected = null;
      renderDrawings();
    }
  });

  // datazoom / resize / 标的切换后按当前视窗重算像素坐标
  charts.klineChart.on('datazoom', function() { renderDrawings(); });
  window.addEventListener('resize', function() { renderDrawings(); });
}

function updateTemp(date, price) {
  const d = tempDrawing;
  if (!d) return;
  if (d.toolType === 'trendline' || d.toolType === 'rectangle') {
    d.points[1] = { date: date, price: price };
  } else if (d.toolType === 'channel') {
    d.points[1] = { date: date, price: price };
    if (d._stage2) d.points[2] = { date: date, price: price };
  }
}

function finishTemp() {
  const d = tempDrawing;
  tempDrawing = null;
  if (!d) return;
  // 过滤无效图形（点重合/无位移）
  const pts = d.points;
  const valid = d.toolType === 'horizontal' ||
    (pts.length === 2 && (pts[0].date !== pts[1].date || Math.abs(pts[0].price - pts[1].price) > 1e-9)) ||
    (pts.length === 3 && (Math.abs(pts[0].price - pts[1].price) > 1e-9 || pts[0].date !== pts[1].date));
  if (valid) {
    drawings.push(d);
    selected = { id: d.id, anchor: -1 };
    saveDrawing(d);
  }
  renderDrawings();
  setDrawingTool('cursor');
}

function moveAnchor(d, anchor, date, price) {
  if (anchor >= 0 && anchor < d.points.length) {
    d.points[anchor] = { date: date, price: price };
  }
}

function hitTest(x, y) {
  for (let i = drawings.length - 1; i >= 0; i--) {
    const d = drawings[i];
    const pts = d.points.map(pointToPixel);
    if (pts.some(p => !p)) continue;
    for (let a = 0; a < pts.length; a++) {
      if (Math.hypot(x - pts[a][0], y - pts[a][1]) <= 8) return { id: d.id, anchor: a };
    }
    if (d.toolType === 'horizontal' && Math.abs(y - pts[0][1]) <= 6) return { id: d.id, anchor: -1 };
    if (d.toolType === 'trendline' && pts.length >= 2 && distToSegment(x, y, pts[0], pts[1]) <= 6) {
      return { id: d.id, anchor: -1 };
    }
    if (d.toolType === 'rectangle' && pts.length >= 2) {
      const minX = Math.min(pts[0][0], pts[1][0]), maxX = Math.max(pts[0][0], pts[1][0]);
      const minY = Math.min(pts[0][1], pts[1][1]), maxY = Math.max(pts[0][1], pts[1][1]);
      if (x >= minX && x <= maxX && y >= minY && y <= maxY) return { id: d.id, anchor: -1 };
    }
    if (d.toolType === 'channel' && pts.length >= 3) {
      const off = [pts[2][0] - pts[0][0], pts[2][1] - pts[0][1]];
      const c1 = [pts[0][0] + off[0], pts[0][1] + off[1]];
      const c2 = [pts[1][0] + off[0], pts[1][1] + off[1]];
      if (distToSegment(x, y, pts[0], pts[1]) <= 6 || distToSegment(x, y, c1, c2) <= 6) {
        return { id: d.id, anchor: -1 };
      }
    }
  }
  return null;
}

function distToSegment(px, py, v, w) {
  const l2 = Math.pow(v[0] - w[0], 2) + Math.pow(v[1] - w[1], 2);
  if (l2 === 0) return Math.hypot(px - v[0], py - v[1]);
  const t = Math.max(0, Math.min(1, ((px - v[0]) * (w[0] - v[0]) + (py - v[1]) * (w[1] - v[1])) / l2));
  return Math.hypot(px - (v[0] + t * (w[0] - v[0])), py - (v[1] + t * (w[1] - v[1])));
}

function isKlineView() {
  return charts.klineChart && tState.currentView !== 'minute' && tState.klineData.length > 0;
}

// ---------- 工具栏联动 ----------
function setDrawingTool(tool) {
  drawingTool = tool;
  if (tool !== 'cursor') selected = null;
  document.querySelectorAll('.td-btn[data-tool]').forEach(b => {
    b.classList.toggle('active', b.dataset.tool === tool);
  });
  try { charts.klineChart.getZr().setCursor(tool === 'cursor' ? 'default' : 'crosshair'); } catch (e) {}
}

function initToolbar() {
  document.querySelectorAll('.td-btn[data-tool]').forEach(b => {
    b.addEventListener('click', () => setDrawingTool(b.dataset.tool));
  });
  const clearBtn = document.getElementById('btn-draw-clear');
  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      if (!selected) return;
      deleteDrawing(selected.id);
      drawings = drawings.filter(d => d.id !== selected.id);
      selected = null;
      renderDrawings();
    });
  }
}

// ---------- 对外挂载 ----------
trends.drawings = {
  init() {
    initToolbar();
    bindInteraction();
    listDrawings();
  },
  reload: listDrawings,
  render: renderDrawings,
  getTool: () => drawingTool,
};

})();
