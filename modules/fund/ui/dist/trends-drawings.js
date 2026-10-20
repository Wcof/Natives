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

// DES-D02：缺省绘图色消费语义角色（随主题解析），显式用户色永不被覆盖；
// 新建对象不再把当时解析的 HEX 伪存进 options.color。
const ACCENT = () => Fund.themeColor('--draw-line');
const SEL = () => Fund.themeColor('--draw-selected');
const ANCHOR = () => Fund.themeColor('--draw-anchor');

// d = { id, symbol, period, toolType, points:[{date, price}...], options:{color} }
// points 约定：trendline 2 点；horizontal 1 点（y 轴价）；rectangle 2 点（对角）；
// channel 3 点（p1/p2 定基线，p3 定平行通道的偏移方向与宽度）。
let drawings = [];
let tempDrawing = null;
let drawingTool = 'cursor';   // cursor | trendline | horizontal | rectangle | channel
let selected = null;          // { id, anchor }  anchor=-1 整体, 0/1/2 锚点
let dragging = false;
let dragConfirmed = null;
let dragStart = null;

function cancelDraft() {
  const pending = !!(tempDrawing || dragging);
  if (!pending) return false;
  if (dragConfirmed) {
    const index = drawings.findIndex(d => d.id === dragConfirmed.id);
    if (index >= 0) drawings[index] = dragConfirmed;
  }
  dragConfirmed = null; dragStart = null;
  tempDrawing = null;
  dragging = false;
  renderDrawings();
  return true;
}

// ---------- 持久化（复用 /api/drawings） ----------
function periodKey() {
  return tState.currentView === 'week' ? 'week' : (tState.currentView === 'month' ? 'month' : 'day');
}
const persistence = Fund.createDrawingSession({
  scope: () => ({ symbol: tState.currentSymbol, period: periodKey(), viewContext: { fq: tState.fq || 'qfq', seriesKind: 'OHLC' } }),
  replace: rows => { drawings = rows; if (selected && !rows.some(d => d.id === selected.id)) selected = null; renderDrawings(); },
  statusRoot: () => document.getElementById('trends-chart-toolbar'),
});
function listDrawings() { cancelDraft(); return persistence.load(); }
function saveDrawing(d) { return persistence.save(d); }
function deleteDrawing(id) { return persistence.remove(id); }

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
    const pixel = charts.klineChart.convertToPixel({ xAxisIndex: 0, yAxisIndex: 0 }, [pt.date, pt.price]);
    return Array.isArray(pixel) && pixel.every(Number.isFinite) ? pixel : null;
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
  Fund.StockDrawingsShapes.buildGraphic({
    selected, hoverHit: null, strokeColor: ACCENT(), selectedColor: SEL(), strokeWidth: 1.8,
    pointToPixel, chartWidthPx, getKlineData: () => tState.klineData,
    dashPattern: type => ({dashed:[5,4],'dashed-dense':[3,2],'dashed-sparse':[8,6]})[type] || null,
  }, d, els);
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
    if (!isKlineView() || !persistence.canEdit()) return;
    const x = e.offsetX, y = e.offsetY;

    if (drawingTool === 'cursor') {
      const hit = hitTest(x, y);
      selected = hit;
      dragging = !!hit && !drawings.find(d=>d.id===hit.id)?.options.locked;
      dragConfirmed = hit ? structuredClone(drawings.find(d => d.id === hit.id)) : null;
      dragStart = hit ? {x,y} : null;
      if (hit) renderDrawings();
      return;
    }

    // 绘制起点
    const date = pxToDate(x);
    const price = pxToPrice(y);
    if (date == null || price == null) return;
    const base = { date: date, price: price };
    if (tempDrawing?.toolType === 'channel' && tempDrawing._stage2) {
      tempDrawing.points[2] = base; finishTemp(); return;
    }
    if (tempDrawing?.toolType === 'polyline') {
      if (tempDrawing.points.length < 512) tempDrawing.points.push({...base});
      renderDrawings(); return;
    }
    const need = ['horizontal', 'text'].includes(drawingTool) ? 1 : (drawingTool === 'channel' ? 3 : 2);
    tempDrawing = {
      id: 'draw_' + Date.now(),
      symbol: tState.currentSymbol,
      period: periodKey(),
      toolType: drawingTool,
      points: new Array(need).fill(0).map(() => Object.assign({}, base)),
      options: {},
    };
    if (drawingTool === 'text') {
      const draft=tempDrawing; tempDrawing=null;
      const dialog=document.createElement('dialog'); dialog.className='home-dialog';
      const form=document.createElement('form'), input=document.createElement('input');
      input.type='text'; input.maxLength=256; input.required=true; input.setAttribute('aria-label',Fund.t('fundTrendsDrawingText'));
      const save=document.createElement('button'); save.type='submit'; save.textContent=Fund.t('fundHomeSave');
      const cancel=document.createElement('button'); cancel.type='button'; cancel.textContent=Fund.t('cancel');
      cancel.onclick=()=>dialog.close();
      form.append(input,save,cancel); dialog.append(form); document.body.append(dialog);
      const context=JSON.stringify({symbol:tState.currentSymbol,period:periodKey(),fq:tState.fq});
      form.onsubmit=event=>{event.preventDefault(); const text=input.value.trim();
        if(!text || context!==JSON.stringify({symbol:tState.currentSymbol,period:periodKey(),fq:tState.fq}))return;
        draft.options={text,name:text}; tempDrawing=draft; finishTemp(); dialog.close();
      };
      dialog.addEventListener('close',()=>{dialog.remove();setDrawingTool('cursor');},{once:true});
      dialog.showModal(); input.focus(); return;
    }
    if (need === 1) finishTemp();
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
      if (selected.anchor === -1 && dragConfirmed && dragStart) {
        Fund.StockDrawingsShapes.moveWholeDrawing({chart:charts.klineChart,getKlineData:()=>tState.klineData}, d, dragConfirmed.points, dragStart, {x,y});
      } else moveAnchor(d, selected.anchor, date, price);
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
      dragConfirmed = null;
      return;
    }
    if (tempDrawing?.toolType === 'polyline') return;
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
    if (tempDrawing?.toolType === 'polyline' && !document.querySelector('dialog[open]')) {
      if(e.key==='Enter'){ e.preventDefault(); tempDrawing.points.pop(); finishTemp(); return; }
      if(e.key==='Backspace'){e.preventDefault();if(tempDrawing.points.length>2)tempDrawing.points.splice(-2,1);renderDrawings();return;}
    }
    if (e.key !== 'Delete' && e.key !== 'Backspace') return;
    const tag = (document.activeElement && document.activeElement.tagName || '').toLowerCase();
    if (tag === 'input' || tag === 'textarea' || document.activeElement?.isContentEditable || document.querySelector('dialog[open]') || document.getElementById('view-trends')?.hidden) return;
    if (selected) {
      deleteDrawing(selected.id);
    }
  });

  zr.on('dblclick', () => { if(tempDrawing?.toolType==='polyline'){tempDrawing.points.pop();finishTemp();} });

  // datazoom / resize / 标的切换后按当前视窗重算像素坐标
  charts.klineChart.on('datazoom', function() { renderDrawings(); });
  window.addEventListener('resize', function() { renderDrawings(); });
}

function updateTemp(date, price) {
  const d = tempDrawing;
  if (!d) return;
  if (['trendline', 'rectangle', 'circle', 'fibonacci', 'measure'].includes(d.toolType)) {
    d.points[1] = { date: date, price: price };
  } else if(d.toolType === 'polyline') { d.points[d.points.length-1]={date,price};
  } else if (d.toolType === 'channel') {
    d.points[d._stage2 ? 2 : 1] = {date,price};
  }
}

function finishTemp() {
  const d = tempDrawing;
  tempDrawing = null;
  if (!d) return;
  // 过滤无效图形（点重合/无位移）
  const pts = d.points;
  const areaValid=!['circle','rectangle'].includes(d.toolType) || (pts[0].date!==pts[1].date && Math.abs(pts[0].price-pts[1].price)>1e-9);
  const valid = areaValid && (['horizontal', 'text'].includes(d.toolType) ||
    (d.toolType === 'polyline' && pts.length>=2 && pts.some(p=>p.date!==pts[0].date || p.price!==pts[0].price)) ||
    (pts.length === 2 && (pts[0].date !== pts[1].date || Math.abs(pts[0].price - pts[1].price) > 1e-9)) ||
    (pts.length === 3 && (Math.abs(pts[0].price - pts[1].price) > 1e-9 || pts[0].date !== pts[1].date)));
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
  return Fund.StockDrawingsShapes.hitTest({ drawings, pointToPixel }, x, y);
}

function isKlineView() {
  return tState.instrumentKind !== 'fund' && charts.klineChart && tState.currentView !== 'minute' && tState.klineData.length > 0;
}

// ---------- 工具栏联动 ----------
function setDrawingTool(tool) {
  drawingTool = tool;
  cancelDraft();
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
  const objects=document.createElement('button');objects.type='button';objects.className='tb-btn';objects.textContent=Fund.t('fundChartDrawingObjects');objects.dataset.i18n='fundChartDrawingObjects';objects.onclick=()=>{cancelDraft();persistence.manage();};document.getElementById('trends-chart-toolbar').append(objects);
  const clearBtn = document.getElementById('btn-draw-clear');
  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      if (!selected) return;
      deleteDrawing(selected.id);
    });
  }
}

// ---------- 对外挂载 ----------
trends.drawings = {
  cancelDraft,
  pause() { cancelDraft(); persistence.pause(); },
  init() {
    if (this.initialized || !charts.klineChart) return;
    this.initialized = true;
    initToolbar();
    bindInteraction();
    listDrawings();
  },
  reload: listDrawings,
  render: renderDrawings,
  getTool: () => drawingTool,
};
window.addEventListener('blur', cancelDraft);
window.addEventListener('pagehide', cancelDraft);
document.addEventListener('pointercancel', cancelDraft);
document.addEventListener('keydown', e => {
  if (e.key === 'Escape' && !e.defaultPrevented && !document.querySelector('dialog[open]') && cancelDraft()) e.preventDefault();
});

})();
