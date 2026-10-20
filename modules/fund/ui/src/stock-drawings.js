(() => {
const { authFetch } = globalThis.Fund;

// Fund 股票详情子系统：专业划线引擎（Stock Drawings Engine）
// 全生命周期健全性 CRUD + 专业看盘交互：
// 1. 自动轻微吸附力（Magnet Snap）：捕捉蜡烛 OHLC，帮助用户精准画线
// 2. 右键属性编辑（Context Menu）：自由设置线条名称备注、粗细、颜色、线型及删除
// 3. 图上线条名称渲染：设置的名称在图线上直观呈现
// 4. 空白处点击取消选中：不选中任何划线，恢复普通无锚点状态
// 5. 框选拖拽弹出“查看这部分”：快速引导局部区域缩放放大
// 图元构建、碰撞拾取与几何拖拽计算（纯计算层）已按职责拆分至
// stock-drawings-shapes.js，经 ctx 上下文对象与本工厂闭包共享状态。

function createStockDrawings(config) {
  let chart = null;
  let getKlineData = config.getKlineData || (() => []);
  let getSymbol = config.getSymbol || (() => '');
  let getPeriod = config.getPeriod || (() => 'dayk');
  let onZoomToRange = config.onZoomToRange || (() => {});

  let drawings = [];
  let tempDrawing = null;
  let historyStack = [];
  let currentTool = 'cursor';
  let selected = null; // { id, anchor }
  let hoverHit = null; // { id, anchor, pt } 当前鼠标悬停命中的画线或控制把手
  let dragOriginPoints = null; // 拖拽初始时刻的点数据副本
  let dragStartPix = null; // 拖拽按下时的屏幕像素位置 { x, y }
  let dragging = false;
  let magnetEnabled = false;
  let locked = false;
  let closePalette = () => {};

  // 临时挂起/恢复 ECharts inside dataZoom 的鼠标平移，防止拖拽线条或控制点时整张 K 线图跟滑
  function setChartPanEnabled(enabled) {
    if (!chart) return;
    try {
      chart.setOption({
        dataZoom: [
          { type: 'inside', moveOnMouseMove: enabled }
        ]
      });
    } catch (e) {}
  }

  // DES-D02：缺省色消费语义角色（随主题解析），显式色盘选择才算用户色。
  // userStrokeColor 为 null 表示「跟随主题」；解析值仅供渲染/预览，不落库。
  let userStrokeColor = null;
  const strokeColor = {
    get user() { return userStrokeColor; },
    valueOf() { return userStrokeColor || Fund.themeColor('--draw-line'); },
    toString() { return userStrokeColor || Fund.themeColor('--draw-line'); },
  };
  const selectedColor = () => Fund.themeColor('--draw-selected');
  const strokeColorStr = () => String(strokeColor);
  let strokeWidth = 2.0;
  let strokeDash = 'solid';

  // 线条类型 → ECharts lineDash 映射（唯一口径，渲染与默认值共用）：
  // solid=实线；dashed-dense=短虚线（间隔密）；dashed=常规虚线；dashed-sparse=长虚线（间隔疏）
  function dashPattern(type) {
    switch (type) {
      case 'dashed-dense': return [3, 3];
      case 'dashed': return [5, 4];
      case 'dashed-sparse': return [10, 6];
      default: return null; // solid 实线
    }
  }

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

  function periodKey() {
    const p = getPeriod();
    if (p === 'week') return 'week';
    if (p === 'month') return 'month';
    if (p === 'minute') return 'minute';
    return 'day';
  }

  // ---------- 坐标双向转换 (Data ↔ Screen Pixel) ----------
  function pxToDate(px) {
    if (!chart) return null;
    try {
      const v = chart.convertFromPixel({ xAxisIndex: 0 }, px);
      const data = getKlineData();
      if (!data || !data.length) return null;
      const n = Math.max(0, Math.min(data.length - 1, Math.round(v)));
      if (!data[n]) return null;
      const value = data[n].date || data[n].time;
      if (periodKey() !== 'minute' || /^\d{4}-\d{2}-\d{2}/.test(value)) return value;
      const day = config.getBusinessDate?.();
      return /^\d{4}-\d{2}-\d{2}$/.test(day || '') ? day + 'T' + value + ':00+08:00' : null;
    } catch (e) { return null; }
  }

  function pxToPrice(py) {
    if (!chart) return null;
    try {
      return chart.convertFromPixel({ yAxisIndex: 0 }, py);
    } catch (e) { return null; }
  }

  function pointToPixel(pt) {
    if (!chart || !pt || pt.date == null || pt.price == null) return null;
    try {
      let axisDate = pt.date;
      if (periodKey() === 'minute' && !getKlineData().some(row => (row.date || row.time) === axisDate)) {
        if (!axisDate.startsWith((config.getBusinessDate?.() || '') + 'T')) return null;
        axisDate = axisDate.slice(11, 16);
      }
      const pixel=chart.convertToPixel({ xAxisIndex: 0, yAxisIndex: 0 }, [axisDate, pt.price]);
      return Array.isArray(pixel)&&pixel.every(Number.isFinite)?pixel:null;
    } catch (e) { return null; }
  }

  function chartWidthPx() {
    try {
      const dom = chart.getDom();
      return (dom ? dom.clientWidth : 600) - 50;
    } catch (e) { return 500; }
  }

  // 智能辅助吸附力：距离蜡烛极值 ≤ 14px 时轻微吸附；强磁吸开启时扩大至 26px
  function snapPoint(date, rawPrice) {
    const data = getKlineData();
    if (!data || !data.length) return { date, price: rawPrice };
    const bar = data.find(d => (d.date || d.time) === date);
    if (!bar) return { date, price: rawPrice };

    const cands = [bar.open, bar.close, bar.high, bar.low].filter(v => v != null && !isNaN(v));
    if (!cands.length) return { date, price: rawPrice };

    const rawPix = pointToPixel({ date, price: rawPrice });
    if (!rawPix) return { date, price: rawPrice };

    const thresholdPx = magnetEnabled ? 28 : 14; // 默认带 14px 舒适轻微吸附力
    let bestP = rawPrice;
    let minDiffPx = thresholdPx;

    cands.forEach(p => {
      const pix = pointToPixel({ date, price: p });
      if (pix) {
        const diffPx = Math.abs(pix[1] - rawPix[1]);
        if (diffPx < minDiffPx) {
          minDiffPx = diffPx;
          bestP = p;
        }
      }
    });
    return { date, price: bestP };
  }

  const persistence = Fund.createDrawingSession({
    scope: () => ({ symbol: getSymbol(), period: periodKey(), viewContext: { fq: periodKey() === 'minute' ? 'bf' : (config.getFq?.() || 'qfq'), seriesKind: periodKey() === 'minute' ? 'minute' : 'OHLC' } }),
    replace: rows => { drawings = rows; if (selected && !rows.some(d => d.id === selected.id)) selected = null; renderDrawings(); },
    statusRoot: () => document.getElementById('sd-draw-toolbar'),
  });
  function cancelDraft() {
    const pending = !!(tempDrawing || dragging);
    if (dragging && selected && dragOriginPoints) {
      const d = drawings.find(x => x.id === selected.id); if (d) d.points = structuredClone(dragOriginPoints);
    }
    tempDrawing = null; dragging = false; dragOriginPoints = null; dragStartPix = null;
    setChartPanEnabled(true); hideInlineTextPop(); renderDrawings(); return pending;
  }
  function listDrawings() { cancelDraft(); return persistence.load(); }
  function saveDrawing(d) { return persistence.save(d); }
  function deleteDrawing(id) { return persistence.remove(id); }

  // ---------- 图元构建与 ECharts Graphic 渲染 ----------
  // 工厂上下文：把闭包内共享状态以 getter 形式实时暴露给纯计算层
  // （stock-drawings-shapes.js），保证选中态/磁吸/锁定等修改即时可见。
  const Shapes = globalThis.Fund.StockDrawingsShapes;
  const shapesCtx = {
    get chart() { return chart; },
    getKlineData: () => periodKey() === 'minute' ? getKlineData().map(row => ({...row,date:/^\d{4}-\d{2}-\d{2}/.test(row.date || row.time) ? row.date || row.time : config.getBusinessDate?.()+'T'+(row.date || row.time)+':00+08:00'})) : getKlineData(),
    get drawings() { return drawings; },
    get selected() { return selected; },
    get hoverHit() { return hoverHit; },
    get magnetEnabled() { return magnetEnabled; },
    get locked() { return locked; },
    get strokeColor() { return strokeColorStr(); },
    get selectedColor() { return selectedColor(); },
    get strokeWidth() { return strokeWidth; },
    dashPattern,
    pointToPixel,
    chartWidthPx,
    snapPoint,
  };

  function renderDrawings() {
    if (!chart) return;
    const all = tempDrawing ? drawings.concat([tempDrawing]) : drawings.slice();
    const els = [];
    all.forEach(d => buildGraphic(d, els));
    chart.setOption({ graphic: els }, { replaceMerge: ['graphic'] });
  }

  function buildGraphic(d, els) {
    Shapes.buildGraphic(shapesCtx, d, els);
  }

  // ---------- 碰撞拾取与几何拖拽（委托纯计算层 stock-drawings-shapes.js） ----------
  function hitTest(x, y) {
    return Shapes.hitTest(shapesCtx, x, y);
  }

  // 单点锚点拖拽更新（磁吸贴合）
  function moveAnchor(d, anchor, newDate, newPrice) {
    Shapes.moveAnchor(shapesCtx, d, anchor, newDate, newPrice);
  }

  // 整线按像素位移整体平移（X轴按 K 线索引差值偏移，Y轴按价差偏移，完美保持几何形状）
  function moveWholeDrawing(d, origPoints, startPix, curPix) {
    Shapes.moveWholeDrawing(shapesCtx, d, origPoints, startPix, curPix);
  }

  function finishTemp() {
    if (!tempDrawing) return;
    const d = tempDrawing;
    tempDrawing = null;

    const pts = d.points || [];
    const valid = d.toolType === 'horizontal' || d.toolType === 'text' ||
      (pts.length >= 2 && (pts[0].date !== pts[1].date || Math.abs(pts[0].price - pts[1].price) > 1e-6));

    if (valid) {
      drawings.push(d);
      historyStack.push(d.id);
      saveDrawing(d);
      selected = { id: d.id, anchor: -1 };

      // 画完后自动切回选择模式，避免用户拖动画布看盘时误画新线（对齐同花顺交互）
      currentTool = 'cursor';
      const root = document.getElementById('sd-draw-toolbar');
      if (root) {
        root.querySelectorAll('.sd-tool-btn').forEach(b => {
          b.classList.toggle('active', b.dataset.tool === 'cursor');
        });
      }

      // 如果画的是矩形，弹出“查看这部分”浮动卡片引导局部放大
      if (d.toolType === 'rectangle') {
        showBoxPrompt(d);
      }
    }
    renderDrawings();
  }

  // ---------- 内嵌便签输入框控制 (彻底取代 window.prompt) ----------
  let _pendingTextSnapped = null;
  function showInlineTextPop(snapped, x, y) {
    const pop = document.getElementById('sd-inline-text-pop');
    const input = document.getElementById('sd-itp-input');
    const stage = document.querySelector('.sd-chart-stage');
    if (!pop || !input || !stage) return;

    _pendingTextSnapped = snapped;
    input.value = Fund.t('fundDetailStockUiDefaultNote');
    const sRect = stage.getBoundingClientRect();
    pop.style.left = Math.max(10, Math.min(sRect.width - 240, x)) + 'px';
    pop.style.top = Math.max(10, Math.min(sRect.height - 40, y - 15)) + 'px';
    pop.style.display = 'flex';
    setTimeout(() => { input.focus(); input.select(); }, 40);
  }

  function hideInlineTextPop() {
    const pop = document.getElementById('sd-inline-text-pop');
    if (pop) pop.style.display = 'none';
    _pendingTextSnapped = null;
  }

  function confirmInlineText() {
    const input = document.getElementById('sd-itp-input');
    const userText = input ? input.value.trim() : '';
    if (userText && _pendingTextSnapped) {
      const d = {
        id: 'sd_' + Date.now(),
        symbol: getSymbol(),
        period: periodKey(),
        toolType: 'text',
        points: [_pendingTextSnapped],
        options: { ...(userStrokeColor ? { color: userStrokeColor } : {}), text: userText, name: userText },
      };
      drawings.push(d);
      historyStack.push(d.id);
      saveDrawing(d);
      selected = { id: d.id, anchor: -1 };

      currentTool = 'cursor';
      const root = document.getElementById('sd-draw-toolbar');
      if (root) {
        root.querySelectorAll('.sd-tool-btn').forEach(b => {
          b.classList.toggle('active', b.dataset.tool === 'cursor');
        });
      }
      renderDrawings();
    }
    hideInlineTextPop();
  }

  // ---------- 框选局部放大浮动按钮控制 ----------
  function showBoxPrompt(d) {
    const prompt = document.getElementById('sd-box-zoom-prompt');
    if (!prompt || !chart || !d.points || d.points.length < 2) return;
    const p1 = pointToPixel(d.points[0]);
    const p2 = pointToPixel(d.points[1]);
    if (!p1 || !p2) return;

    const dom = chart.getDom();
    const stage = dom.parentElement;
    if (!stage) return;

    const rx = Math.max(p1[0], p2[0]);
    const ry = Math.min(p1[1], p2[1]);

    prompt.style.left = Math.max(10, Math.min(dom.clientWidth - 140, rx - 90)) + 'px';
    prompt.style.top = Math.max(10, ry - 36) + 'px';
    prompt.style.display = 'inline-flex';
    prompt.dataset.drawingId = d.id;
  }

  function hideBoxPrompt() {
    const prompt = document.getElementById('sd-box-zoom-prompt');
    if (prompt) prompt.style.display = 'none';
  }

  // ---------- 右键编辑浮层控制 ----------
  function showLineEditPop(id, clickX, clickY) {
    const d = drawings.find(dd => dd.id === id);
    const pop = document.getElementById('sd-line-edit-pop');
    if (!d || !pop) return;

    const nameInput = document.getElementById('sd-lep-name');
    if (nameInput) nameInput.value = (d.options && d.options.name) || '';

    // 色块高亮
    const curColor = (d.options && d.options.color) || strokeColor;
    pop.querySelectorAll('.sd-c-dot').forEach(dot => {
      dot.classList.toggle('active', dot.dataset.color === curColor);
    });

    // 粗细高亮
    const curWidth = String((d.options && d.options.width) || strokeWidth);
    pop.querySelectorAll('.sd-opt-btn[data-width]').forEach(btn => {
      btn.classList.toggle('active', Math.abs(parseFloat(btn.dataset.width) - parseFloat(curWidth)) < 0.3);
    });

    // 虚实高亮
    const curDash = (d.options && d.options.dash) || 'solid';
    pop.querySelectorAll('.sd-opt-btn[data-dash]').forEach(btn => {
      btn.classList.toggle('active', btn.dataset.dash === curDash);
    });

    // 计算弹窗绝对位置在图表主舞台内
    const stage = document.querySelector('.sd-chart-stage');
    if (stage) {
      const sRect = stage.getBoundingClientRect();
      const posX = Math.max(10, Math.min(sRect.width - 230, clickX - sRect.left + 5));
      const posY = Math.max(10, Math.min(sRect.height - 180, clickY - sRect.top + 5));
      pop.style.left = posX + 'px';
      pop.style.top = posY + 'px';
    }
    pop.style.display = 'block';
    pop.dataset.drawingId = id;
  }

  function hideLineEditPop() {
    const pop = document.getElementById('sd-line-edit-pop');
    if (pop) pop.style.display = 'none';
  }

  // ---------- ZRender 底层事件监听 ----------
  // 每个图表实例的 zr 事件与工具栏只允许绑定一次：switchPeriod 反复
  // attachChart 时若重复 zr.on，会在同一次 mousedown 里触发多次画线/拖拽。
  const _boundCharts = new WeakSet();
  let _toolbarBound = false;
  function bindZrEvents() {
    if (!chart) return;
    const zr = chart.getZr();
    if (!zr) return;

    // 右键进入编辑状态
    zr.on('contextmenu', function(e) {
      e.event?.preventDefault?.();
      const x = e.offsetX, y = e.offsetY;
      const hit = hitTest(x, y);
      if (hit) {
        selected = hit;
        renderDrawings();
        showLineEditPop(hit.id, e.event.clientX || x, e.event.clientY || y);
      } else {
        selected = null;
        hideLineEditPop();
        renderDrawings();
      }
    });

    zr.on('mousedown', function(e) {
      if (!persistence.canEdit()) return;
      const x = e.offsetX, y = e.offsetY;

      // 1. 光标选择模式
      if (currentTool === 'cursor') {
        const hit = hitTest(x, y);
        if (!hit) {
          // 点击空白处：不选中任何划线，恢复普通无锚点状态
          selected = null;
          hoverHit = null;
          hideLineEditPop();
          hideBoxPrompt();
          renderDrawings();
          return;
        }

        selected = { id: hit.id, anchor: hit.anchor };
        dragging = !locked && !drawings.find(d=>d.id===hit.id)?.options.locked;
        dragStartPix = { x, y };

        const d = drawings.find(dd => dd.id === hit.id);
        dragOriginPoints = (d && d.points) ? JSON.parse(JSON.stringify(d.points)) : null;

        // 命中画线或控制点时，临时挂起 ECharts inside dataZoom 的鼠标平移，防止图表联动滑动
        setChartPanEnabled(false);
        zr.setCursorStyle('grabbing');
        renderDrawings();

        // 阻止事件继续穿透引发底层画布拖拽
        if (e.event) {
          e.event.preventDefault?.();
          e.event.stopPropagation?.();
        }

        // 若点击的是矩形，提示查看这部分
        if (d && d.toolType === 'rectangle') {
          showBoxPrompt(d);
        } else {
          hideBoxPrompt();
        }
        return;
      }

      const rawDate = pxToDate(x);
      const rawPrice = pxToPrice(y);
      if (rawDate == null || rawPrice == null) return;
      const snapped = snapPoint(rawDate, rawPrice);

      // 2. 文字标注：在画布位置弹出内嵌便签输入框（不再使用原生 window.prompt）
      if (currentTool === 'text') {
        showInlineTextPop(snapped, x, y);
        return;
      }

      // 3. 连续折线：多点连续增加
      if (currentTool === 'polyline') {
        if (!tempDrawing) {
          tempDrawing = {
            id: 'sd_' + Date.now(),
            symbol: getSymbol(),
            period: periodKey(),
            toolType: 'polyline',
            points: [snapped, Object.assign({}, snapped)],
            options: { ...(userStrokeColor ? { color: userStrokeColor } : {}), width: strokeWidth, dash: strokeDash },
          };
        } else {
          const lastPt = tempDrawing.points[tempDrawing.points.length - 2];
          if (!lastPt || lastPt.date !== snapped.date || Math.abs(lastPt.price - snapped.price) > 1e-4) {
            tempDrawing.points.push(Object.assign({}, snapped));
          }
        }
        renderDrawings();
        return;
      }

      // 4. 通道线两段式：阶段二点击完成绘制
      if (currentTool === 'channel' && tempDrawing && tempDrawing._stage2) {
        tempDrawing.points[2] = snapped;
        finishTemp();
        return;
      }

      // 5. 通用起画
      const need = currentTool === 'horizontal' ? 1 : (currentTool === 'channel' ? 3 : 2);
      tempDrawing = {
        id: 'sd_' + Date.now(),
        symbol: getSymbol(),
        period: periodKey(),
        toolType: currentTool,
        points: new Array(need).fill(0).map(() => Object.assign({}, snapped)),
        options: { ...(userStrokeColor ? { color: userStrokeColor } : {}), width: strokeWidth, dash: strokeDash },
      };

      if (need === 1) finishTemp();
      else renderDrawings();
    });

    zr.on('mousemove', function(e) {
      const x = e.offsetX, y = e.offsetY;

      // 悬停光标与把手高亮反馈
      if (currentTool === 'cursor' && !dragging) {
        const hit = hitTest(x, y);
        const changed = (!hoverHit && hit) || (hoverHit && !hit) ||
                        (hoverHit && hit && (hoverHit.id !== hit.id || hoverHit.anchor !== hit.anchor));
        hoverHit = hit;

        if (hit) {
          zr.setCursorStyle(hit.anchor >= 0 ? 'grab' : 'move');
        } else {
          zr.setCursorStyle('default');
        }

        if (changed) {
          renderDrawings();
        }
      }

      // 拖拽更新
      if (dragging && selected && !locked) {
        zr.setCursorStyle('grabbing');
        const d = drawings.find(dd => dd.id === selected.id);
        if (!d) {
          dragging = false;
          setChartPanEnabled(true);
          return;
        }

        if (selected.anchor >= 0) {
          // 拖拽控制点
          const rawDate = pxToDate(x);
          const rawPrice = pxToPrice(y);
          if (rawDate != null && rawPrice != null) {
            d.points[selected.anchor] = snapPoint(rawDate, rawPrice);
            renderDrawings();
            if (d.toolType === 'rectangle') {
              showBoxPrompt(d);
            }
          }
        } else if (selected.anchor === -1 && dragOriginPoints && dragStartPix) {
          // 整线平移
          moveWholeDrawing(d, dragOriginPoints, dragStartPix, { x, y });
          renderDrawings();
          if (d.toolType === 'rectangle') {
            showBoxPrompt(d);
          }
        }
        return;
      }

      // 绘制预览
      if (tempDrawing) {
        const rawDate = pxToDate(x);
        const rawPrice = pxToPrice(y);
        if (rawDate == null || rawPrice == null) return;
        const snapped = snapPoint(rawDate, rawPrice);

        if (tempDrawing.toolType === 'channel') {
          if (!tempDrawing._stage2) {
            tempDrawing.points[1] = snapped;
            tempDrawing.points[2] = snapped;
          } else {
            tempDrawing.points[2] = snapped;
          }
        } else {
          tempDrawing.points[tempDrawing.points.length - 1] = snapped;
        }
        renderDrawings();
      }
    });

    zr.on('mouseup', function() {
      if (dragging && selected && !locked) {
        dragging = false;
        dragOriginPoints = null;
        dragStartPix = null;
        setChartPanEnabled(true);

        const d = drawings.find(dd => dd.id === selected.id);
        if (d) saveDrawing(d);

        zr.setCursorStyle(hoverHit ? (hoverHit.anchor >= 0 ? 'grab' : 'move') : 'default');
        renderDrawings();
        return;
      }

      if (tempDrawing && tempDrawing.toolType === 'channel' && !tempDrawing._stage2) {
        tempDrawing._stage2 = true;
        return;
      }

      if (tempDrawing && currentTool !== 'polyline' && currentTool !== 'channel') {
        finishTemp();
      }
    });

    // 全局 mouseup 兜底，防止鼠标滑出画布释放后卡住 dragging 状态
    window.addEventListener('mouseup', function() {
      if (dragging) {
        dragging = false;
        dragOriginPoints = null;
        dragStartPix = null;
        setChartPanEnabled(true);
        if (selected && selected.id) {
          const d = drawings.find(dd => dd.id === selected.id);
          if (d) saveDrawing(d);
        }
        if (zr) zr.setCursorStyle('default');
        renderDrawings();
      }
    });

    zr.on('dblclick', function() {
      if (tempDrawing && currentTool === 'polyline') {
        if (tempDrawing.points.length > 2) {
          tempDrawing.points.pop();
          finishTemp();
        } else {
          tempDrawing = null;
          renderDrawings();
        }
      }
    });

    // 键盘 Delete / Backspace 快捷删除
    document.addEventListener('keydown', function(e) {
      const tag = (document.activeElement && document.activeElement.tagName || '').toLowerCase();
      if (tag === 'input' || tag === 'textarea' || document.activeElement?.isContentEditable || document.querySelector('dialog[open]') || !chart || chart.getDom().getClientRects().length === 0) return;

      if (e.key === 'Delete' || e.key === 'Backspace') {
        if (selected && selected.id) {
          deleteSelected();
        }
      } else if (e.key === 'Escape') {
        cancelDraft();
        hideLineEditPop();
        hideBoxPrompt();
        if (tempDrawing) {
          tempDrawing = null;
          renderDrawings();
        } else if (selected) {
          selected = null;
          renderDrawings();
        }
      } else if (e.key === 'Enter') {
        if (tempDrawing && currentTool === 'polyline') {
          if (tempDrawing.points.length > 2) {
            tempDrawing.points.pop();
            finishTemp();
          }
        }
      }
    });
  }

  // ---------- 删除与清空操作 (Delete) ----------
  async function deleteSelected() {
    if (!selected) return;
    if (await deleteDrawing(selected.id)) { selected = null; hideLineEditPop(); hideBoxPrompt(); renderDrawings(); }
  }
  async function undoLast() {
    const id = historyStack.at(-1); if (!id) return;
    if (await deleteDrawing(id)) { historyStack.pop(); selected = null; hideLineEditPop(); hideBoxPrompt(); renderDrawings(); }
  }
  async function clearAll() {
    if (!drawings.length || !await Fund.home.confirm(Fund.t('fundChartDrawingClearConfirm'))) return;
    for (const d of [...drawings]) { if (!await deleteDrawing(d.id)) break; }
    selected = null; cancelDraft(); hideLineEditPop(); hideBoxPrompt(); renderDrawings();
  }

  // ---------- 绑定工具栏与浮层按钮交互 ----------
  function bindToolbarUI() {
    const root = document.getElementById('sd-draw-toolbar');
    if (!root || root._bound) return;
    root._bound = true;
    const objects=document.createElement('button');objects.type='button';objects.className='sd-draw-btn';objects.dataset.i18n='fundChartDrawingObjects';objects.textContent=Fund.t('fundChartDrawingObjects');objects.onclick=()=>{cancelDraft();persistence.manage();};root.append(objects);

    // 内嵌便签输入事件绑定
    document.getElementById('sd-itp-ok')?.addEventListener('click', confirmInlineText);
    document.getElementById('sd-itp-cancel')?.addEventListener('click', hideInlineTextPop);
    document.getElementById('sd-itp-input')?.addEventListener('keydown', e => {
      if (e.key === 'Enter') {
        e.preventDefault();
        confirmInlineText();
      } else if (e.key === 'Escape') {
        cancelDraft();
        e.preventDefault();
        hideInlineTextPop();
      }
    });

    // 工具切换
    root.querySelectorAll('.sd-tool-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        root.querySelectorAll('.sd-tool-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        currentTool = btn.dataset.tool || 'cursor';
        selected = null;
        hideLineEditPop();
        hideBoxPrompt();
        if (tempDrawing) { tempDrawing = null; }
        renderDrawings();
      });
    });

    // 撤销与删除
    const undoBtn = document.getElementById('sd-draw-undo');
    if (undoBtn) undoBtn.addEventListener('click', undoLast);
    const delBtn = document.getElementById('sd-draw-delete');
    if (delBtn) delBtn.addEventListener('click', deleteSelected);
    const clearAllBtn = document.getElementById('sd-draw-clear-all');
    if (clearAllBtn) clearAllBtn.addEventListener('click', clearAll);

    // 磁吸切换
    const magBtn = document.getElementById('sd-draw-magnet');
    if (magBtn) {
      magBtn.addEventListener('click', () => {
        magnetEnabled = !magnetEnabled;
        magBtn.classList.toggle('active', magnetEnabled);
        magBtn.dataset.i18nTitle=magnetEnabled?'fundDetailStockUiStrongSnap':'fundDetailStockUiSoftSnap';magBtn.title=Fund.t(magBtn.dataset.i18nTitle);
      });
    }

    // 锁定切换
    const lockBtn = document.getElementById('sd-draw-lock');
    if (lockBtn) {
      lockBtn.addEventListener('click', () => {
        locked = !locked;
        lockBtn.classList.toggle('active', locked);
        lockBtn.dataset.i18nTitle=locked?'fundDetailStockUiLocked':'fundDetailStockUiUnlocked';lockBtn.title=Fund.t(lockBtn.dataset.i18nTitle);
      });
    }

    bindPaletteUI();
    bindDrawingPopupsUI();
  }

  function bindPaletteUI() {
    // 调色盘与全局属性更新
    const palBtn = document.getElementById('sd-palette-btn');
    const palPop = document.getElementById('sd-palette-pop');
    if (palBtn && palPop) {
      palBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        if (palPop.matches(':popover-open')) { palPop.hidePopover(); return; }
        const anchor=palBtn.getBoundingClientRect();
        palPop.showPopover();
        palPop.style.left=Math.max(8,Math.min(innerWidth-palPop.offsetWidth-8,anchor.right-palPop.offsetWidth))+'px';
        palPop.style.top=Math.max(8,Math.min(innerHeight-palPop.offsetHeight-8,anchor.bottom+4))+'px';
        palPop.querySelector('button')?.focus({preventScroll:true});
      });
      palPop.addEventListener('toggle',()=>palBtn.setAttribute('aria-expanded',String(palPop.matches(':popover-open'))));
      palPop.addEventListener('beforetoggle',event=>{
        if(event.newState==='closed'&&palPop.contains(document.activeElement))palBtn.focus({preventScroll:true});
      });
      closePalette=()=>{if(palPop.matches(':popover-open'))palPop.hidePopover();};
      document.getElementById('sd-draw-toolbar').addEventListener('scroll',closePalette);
      window.addEventListener('resize',closePalette);
      document.addEventListener('fund:detail-selection-changed',closePalette);

      palPop.querySelectorAll('.sd-c-dot').forEach(dot => {
        dot.addEventListener('click', () => {
          palPop.querySelectorAll('.sd-c-dot').forEach(d => d.classList.remove('active'));
          dot.classList.add('active');
          // 显式色盘选择才成为用户色；取消显式色（未来入口）回主题默认。
          userStrokeColor = dot.dataset.color || null;
          const preview = document.getElementById('sd-color-preview');
          if (preview) preview.style.background = userStrokeColor || Fund.themeColor('--draw-line');

          if (selected) {
            const d = drawings.find(dd => dd.id === selected.id);
            if (d) {
              d.options = d.options || {};
              if (userStrokeColor) d.options.color = userStrokeColor; else delete d.options.color;
              saveDrawing(d);
              renderDrawings();
            }
          }
        });
      });

      palPop.querySelectorAll('.sd-opt-btn').forEach(btn => {
        btn.addEventListener('click', () => {
          if (btn.dataset.width) {
            palPop.querySelectorAll('.sd-opt-btn[data-width]').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            strokeWidth = parseFloat(btn.dataset.width);
          }
          if (btn.dataset.dash) {
            palPop.querySelectorAll('.sd-opt-btn[data-dash]').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            strokeDash = btn.dataset.dash;
          }
          if (selected) {
            const d = drawings.find(dd => dd.id === selected.id);
            if (d) {
              d.options = d.options || {};
              d.options.width = strokeWidth;
              d.options.dash = strokeDash;
              saveDrawing(d);
              renderDrawings();
            }
          }
        });
      });
    }

  }

  function bindDrawingPopupsUI() {
    // ---------- 右键编辑浮层（.sd-line-edit-pop）交互绑定 ----------
    const editPop = document.getElementById('sd-line-edit-pop');
    if (editPop) {
      document.getElementById('sd-lep-close')?.addEventListener('click', hideLineEditPop);
      document.getElementById('sd-lep-save')?.addEventListener('click', hideLineEditPop);
      document.getElementById('sd-lep-del')?.addEventListener('click', deleteSelected);

      const nameInput = document.getElementById('sd-lep-name');
      if (nameInput) {
        nameInput.addEventListener('input', () => {
          const id = editPop.dataset.drawingId;
          const d = drawings.find(dd => dd.id === id);
          if (d) {
            d.options = d.options || {};
            d.options.name = nameInput.value.trim();
            saveDrawing(d);
            renderDrawings();
          }
        });
      }

      // 编辑浮层颜色
      editPop.querySelectorAll('#sd-lep-colors .sd-c-dot').forEach(dot => {
        dot.addEventListener('click', () => {
          editPop.querySelectorAll('#sd-lep-colors .sd-c-dot').forEach(d => d.classList.remove('active'));
          dot.classList.add('active');
          const id = editPop.dataset.drawingId;
          const d = drawings.find(dd => dd.id === id);
          if (d) {
            d.options = d.options || {};
            d.options.color = dot.dataset.color;
            saveDrawing(d);
            renderDrawings();
          }
        });
      });

      // 编辑浮层线宽与线型（粗细在 #sd-lep-widths，类型在 #sd-lep-styles）
      editPop.querySelectorAll('#sd-lep-widths .sd-opt-btn, #sd-lep-styles .sd-opt-btn').forEach(btn => {
        btn.addEventListener('click', () => {
          const id = editPop.dataset.drawingId;
          const d = drawings.find(dd => dd.id === id);
          if (!d) return;
          d.options = d.options || {};

          if (btn.dataset.width) {
            editPop.querySelectorAll('#sd-lep-widths .sd-opt-btn[data-width]').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            d.options.width = parseFloat(btn.dataset.width);
          }
          if (btn.dataset.dash) {
            editPop.querySelectorAll('#sd-lep-styles .sd-opt-btn[data-dash]').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            d.options.dash = btn.dataset.dash;
          }
          saveDrawing(d);
          renderDrawings();
        });
      });
    }

    // ---------- 框选局部放大浮窗交互绑定 ----------
    const boxPrompt = document.getElementById('sd-box-zoom-prompt');
    if (boxPrompt) {
      document.getElementById('sd-bzp-close')?.addEventListener('click', hideBoxPrompt);
      document.getElementById('sd-bzp-btn')?.addEventListener('click', () => {
        const id = boxPrompt.dataset.drawingId;
        const d = drawings.find(dd => dd.id === id);
        if (d && d.points && d.points.length >= 2) {
          hideBoxPrompt();
          onZoomToRange(d.points[0].date, d.points[1].date);
        }
      });
    }
  }

  window.addEventListener('blur', cancelDraft);
  window.addEventListener('pagehide', () => { cancelDraft(); closePalette(); persistence.pause(); });
  document.addEventListener('pointercancel', cancelDraft);
  return {
    cancelDraft,
    pause() { cancelDraft(); closePalette(); persistence.pause(); },
    attachChart(echartInstance) {
      chart = echartInstance;
      if (chart && !_boundCharts.has(chart)) {
        _boundCharts.add(chart);
        bindZrEvents();
        chart.on('datazoom', () => {
          renderDrawings();
          hideBoxPrompt();
        });
      }
      if (!_toolbarBound) {
        _toolbarBound = true;
        bindToolbarUI();
      }
    },
    reload() {
      listDrawings();
    },
    render() {
      renderDrawings();
    },
  };
}

globalThis.Fund = globalThis.Fund || {};
globalThis.Fund.createStockDrawings = createStockDrawings;

})();
