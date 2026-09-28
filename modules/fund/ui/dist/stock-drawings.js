(() => {
const { authFetch } = globalThis.Fund;

// Fund 股票详情子系统：专业划线引擎（Stock Drawings Engine）
// 全生命周期健全性 CRUD + 专业看盘交互：
// 1. 自动轻微吸附力（Magnet Snap）：捕捉蜡烛 OHLC，帮助用户精准画线
// 2. 右键属性编辑（Context Menu）：自由设置线条名称备注、粗细、颜色、线型及删除
// 3. 图上线条名称渲染：设置的名称在图线上直观呈现
// 4. 空白处点击取消选中：不选中任何划线，恢复普通无锚点状态
// 5. 框选拖拽弹出“查看这部分”：快速引导局部区域缩放放大

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

  // 严格遵循 fund-chart-spec.md 规范：默认天蓝，选中琥珀金
  let strokeColor = '#38bdf8';
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
      return data[n] ? (data[n].date || data[n].time) : null;
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
      return chart.convertToPixel({ xAxisIndex: 0, yAxisIndex: 0 }, [pt.date, pt.price]);
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

  // ---------- API 持久化 (CRUD 通信) ----------
  function listDrawings() {
    const sym = getSymbol();
    if (!sym) return;
    const period = periodKey();
    authFetch('/api/drawings?symbol=' + encodeURIComponent(sym) + '&period=' + encodeURIComponent(period))
      .then(r => r.json())
      .then(res => {
        // 请求成功但业务报错（如 401/500 透传 JSON）时保留当前画布不清空，
        // 避免瞬态错误把已画好的线“切换后消失”；只有拿到合法列表才整幅替换。
        if (!res || res.error || !Array.isArray(res.drawings)) {
          console.warn('[sd-drawings] 划线列表加载失败，保留当前画布', res);
          return;
        }
        drawings = res.drawings.filter(d => normSym(d.symbol) === normSym(sym) && d.period === period);
        selected = null;
        hideLineEditPop();
        hideBoxPrompt();
        renderDrawings();
      })
      .catch(err => console.warn('[sd-drawings] 划线列表请求失败', err));
  }

  function saveDrawing(d) {
    if (!d || !d.id || !d.symbol) return;
    authFetch('/api/drawings', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        id: d.id,
        symbol: d.symbol,
        period: d.period,
        toolType: d.toolType,
        points: d.points,
        options: d.options || {},
      }),
    }).catch(err => console.warn('[sd-drawings] 保存划线失败', err));
  }

  function deleteDrawing(id) {
    if (!id) return;
    authFetch('/api/drawings?id=' + encodeURIComponent(id), { method: 'DELETE' }).catch(() => {});
  }

  // ---------- 图元构建与 ECharts Graphic 渲染 ----------
  function renderDrawings() {
    if (!chart) return;
    const all = tempDrawing ? drawings.concat([tempDrawing]) : drawings.slice();
    const els = [];
    all.forEach(d => buildGraphic(d, els));
    chart.setOption({ graphic: els }, { replaceMerge: ['graphic'] });
  }

  function buildGraphic(d, els) {
    const isSel = selected && selected.id === d.id;
    const isHover = hoverHit && hoverHit.id === d.id;
    // 用户在属性面板自定义的颜色/线宽永远优先呈现；选中/悬停只做宽度提升
    // 与光晕强调，不再用硬编码色整体覆盖（否则属性修改会“看起来不生效”）。
    // 仅在未自定义颜色时按 fund-chart-spec 规范切换：默认天蓝、选中琥珀金。
    const hasCustomColor = !!(d.options && d.options.color);
    const color = hasCustomColor ? d.options.color : strokeColor;
    const baseWidth = (d.options && d.options.width) || strokeWidth;
    const stroke = (isSel && !hasCustomColor) ? '#f59e0b' : color;
    const width = isSel ? Math.max(baseWidth, 2.5) : (isHover ? Math.max(baseWidth, 2.2) : baseWidth);
    const style = { stroke, lineWidth: width };
    if (isSel || isHover) {
      style.shadowBlur = isSel ? 10 : 7;
      style.shadowColor = stroke;
    }
    // 线条类型由用户属性决定（默认实线）；兼容旧数据的 'dashed' 与新增疏密变体
    const dash = dashPattern((d.options && d.options.dash) || 'solid');
    if (dash) style.lineDash = dash;

    const pts = (d.points || []).map(pointToPixel);
    if (!pts.length || pts.some(p => !p)) return;
    const prefix = 'sd_draw_' + d.id;

    // 1. 趋势线 (两点)
    if (d.toolType === 'trendline' && pts.length >= 2) {
      els.push({
        id: prefix + '_l', type: 'line',
        shape: { x1: pts[0][0], y1: pts[0][1], x2: pts[1][0], y2: pts[1][1] },
        style: style, silent: true, z: 60,
      });
      // 线条名称标签
      if (d.options && d.options.name) {
        const mx = (pts[0][0] + pts[1][0]) / 2;
        const my = (pts[0][1] + pts[1][1]) / 2 - 12;
        els.push({
          id: prefix + '_name', type: 'text',
          x: mx - 20, y: my,
          style: { text: d.options.name, fill: stroke, backgroundColor: 'rgba(16,16,16,0.85)', padding: [2, 6], borderRadius: 3, fontSize: 10 },
          silent: true, z: 62,
        });
      }
    }
    // 2. 水平线 (单点横贯全图)——线型随用户属性，不再强制虚线
    else if (d.toolType === 'horizontal' && pts.length >= 1) {
      els.push({
        id: prefix + '_l', type: 'line',
        shape: { x1: 0, y1: pts[0][1], x2: chartWidthPx(), y2: pts[0][1] },
        style: style, silent: true, z: 60,
      });
      const pVal = d.points[0].price != null ? Number(d.points[0].price).toFixed(2) : '';
      const nameTxt = (d.options && d.options.name) ? `${d.options.name}: ` : '';
      els.push({
        id: prefix + '_txt', type: 'text',
        x: chartWidthPx() - 65, y: pts[0][1] - 8,
        style: { text: `${nameTxt}${pVal}`, fill: stroke, fontSize: 10, backgroundColor: 'rgba(0,0,0,0.75)', padding: [1, 5], borderRadius: 2 },
        silent: true, z: 62,
      });
    }
    // 3. 通道线 (p1-p2基线 + p3平行线)
    else if (d.toolType === 'channel' && pts.length >= 3) {
      const off = [pts[2][0] - pts[0][0], pts[2][1] - pts[0][1]];
      const c1 = [pts[0][0] + off[0], pts[0][1] + off[1]];
      const c2 = [pts[1][0] + off[0], pts[1][1] + off[1]];
      els.push({
        id: prefix + '_l1', type: 'line',
        shape: { x1: pts[0][0], y1: pts[0][1], x2: pts[1][0], y2: pts[1][1] },
        style: style, silent: true, z: 60,
      });
      els.push({
        id: prefix + '_l2', type: 'line',
        shape: { x1: c1[0], y1: c1[1], x2: c2[0], y2: c2[1] },
        style: style, silent: true, z: 60,
      });
    }
    // 4. 连续折线 (多点)
    else if (d.toolType === 'polyline' && pts.length >= 2) {
      for (let i = 0; i < pts.length - 1; i++) {
        els.push({
          id: prefix + '_pl_' + i, type: 'line',
          shape: { x1: pts[i][0], y1: pts[i][1], x2: pts[i+1][0], y2: pts[i+1][1] },
          style: style, silent: true, z: 60,
        });
      }
    }
    // 5. 矩形 (两点对角)
    else if (d.toolType === 'rectangle' && pts.length >= 2) {
      const rx = Math.min(pts[0][0], pts[1][0]);
      const ry = Math.min(pts[0][1], pts[1][1]);
      const rw = Math.abs(pts[1][0] - pts[0][0]);
      const rh = Math.abs(pts[1][1] - pts[0][1]);
      els.push({
        id: prefix + '_r', type: 'rect',
        shape: { x: rx, y: ry, width: rw, height: rh },
        style: Object.assign({ fill: color + '22' }, style), silent: true, z: 60,
      });
      if (d.options && d.options.name) {
        els.push({
          id: prefix + '_name', type: 'text',
          x: rx + 6, y: ry + 6,
          style: { text: d.options.name, fill: stroke, backgroundColor: 'rgba(16,16,16,0.85)', padding: [2, 6], borderRadius: 3, fontSize: 10 },
          silent: true, z: 62,
        });
      }
    }
    // 6. 圆形 / 椭圆 (两点：p1中心，p2定半径)
    else if (d.toolType === 'circle' && pts.length >= 2) {
      const cx = pts[0][0], cy = pts[0][1];
      const rx = Math.abs(pts[1][0] - pts[0][0]) || 20;
      const ry = Math.abs(pts[1][1] - pts[0][1]) || 20;
      els.push({
        id: prefix + '_c', type: 'ellipse',
        shape: { cx, cy, rx, ry },
        style: Object.assign({ fill: color + '18' }, style), silent: true, z: 60,
      });
    }
    // 7. 斐波那契回调线 (p1, p2 两点极值)
    else if (d.toolType === 'fibonacci' && pts.length >= 2) {
      const p1 = d.points[0].price;
      const p2 = d.points[1].price;
      const levels = [0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
      const minX = Math.min(pts[0][0], pts[1][0]);
      const maxX = Math.max(pts[0][0], pts[1][0]) + 120;
      levels.forEach((lvl, idx) => {
        const pr = p1 + (p2 - p1) * lvl;
        const ptPix = pointToPixel({ date: d.points[0].date, price: pr });
        if (!ptPix) return;
        const py = ptPix[1];
        els.push({
          id: prefix + '_fib_' + idx, type: 'line',
          shape: { x1: minX, y1: py, x2: Math.max(maxX, chartWidthPx()), y2: py },
          style: { stroke: (lvl === 0.5 || lvl === 0.618) ? '#f59e0b' : color, lineWidth: 1.2, lineDash: [4, 4] },
          silent: true, z: 60,
        });
        els.push({
          id: prefix + '_fib_t_' + idx, type: 'text',
          x: minX + 6, y: py - 9,
          style: { text: `${(lvl * 100).toFixed(1)}% (${pr.toFixed(2)})`, fill: '#aaa', fontSize: 10 },
          silent: true, z: 62,
        });
      });
    }
    // 8. 价格标尺 / 测距 (p1 到 p2)
    else if (d.toolType === 'measure' && pts.length >= 2) {
      const rx = Math.min(pts[0][0], pts[1][0]);
      const ry = Math.min(pts[0][1], pts[1][1]);
      const rw = Math.abs(pts[1][0] - pts[0][0]);
      const rh = Math.abs(pts[1][1] - pts[0][1]);
      const pDiff = (d.points[1].price - d.points[0].price);
      const pctDiff = d.points[0].price ? (pDiff / d.points[0].price * 100) : 0;
      const sign = pDiff >= 0 ? '+' : '';
      const cLabel = pDiff >= 0 ? '#ff2d2d' : '#00b35c';

      const data = getKlineData();
      let barCountTxt = '';
      if (data && data.length) {
        const i0 = data.findIndex(k => (k.date || k.time) === d.points[0].date);
        const i1 = data.findIndex(k => (k.date || k.time) === d.points[1].date);
        if (i0 >= 0 && i1 >= 0) {
          barCountTxt = ` · ${Math.abs(i1 - i0) + 1}根K线`;
        }
      }

      els.push({
        id: prefix + '_ms_r', type: 'rect',
        shape: { x: rx, y: ry, width: rw, height: rh },
        style: { fill: (pDiff >= 0 ? 'rgba(255,45,45,0.12)' : 'rgba(0,179,92,0.12)'), stroke: cLabel, lineWidth: 1.2, lineDash: [3, 3] },
        silent: true, z: 60,
      });
      els.push({
        id: prefix + '_ms_t', type: 'text',
        x: rx + rw / 2 - 40, y: ry + rh / 2 - 8,
        style: {
          text: `Δ ${sign}${pDiff.toFixed(2)} (${sign}${pctDiff.toFixed(2)}%)${barCountTxt}`,
          fill: '#fff', backgroundColor: 'rgba(20,20,20,0.85)', padding: [2, 6], borderRadius: 3, fontSize: 10,
        },
        silent: true, z: 62,
      });
    }
    // 9. 文字标注 (单点)
    else if (d.toolType === 'text' && pts.length >= 1) {
      const txt = (d.options && d.options.text) || d.options?.name || '标记便签';
      els.push({
        id: prefix + '_txt_p', type: 'text',
        x: pts[0][0], y: pts[0][1],
        style: {
          text: txt, fill: stroke, backgroundColor: 'rgba(20,20,20,0.85)',
          borderColor: stroke, borderWidth: 1, padding: [3, 8], borderRadius: 3, fontSize: 11,
        },
        silent: true, z: 62,
      });
    }

    // 选中或悬停时渲染把手（悬停时端点浮现，鼠标 hover 到锚点时发光放大动效）
    if (isSel || isHover) {
      pts.forEach((p, i) => {
        const isAnchorHovered = isHover && hoverHit && hoverHit.anchor === i;
        if (isAnchorHovered) {
          // 外层呼吸发光光晕
          els.push({
            id: prefix + '_anchor_glow_' + i, type: 'circle',
            shape: { cx: p[0], cy: p[1], r: 11 },
            style: { fill: 'rgba(245, 158, 11, 0.28)', stroke: '#f59e0b', lineWidth: 1.5, lineDash: [3, 2] },
            silent: true, z: 64,
          });
          // 内层高亮把手实心圆
          els.push({
            id: prefix + '_anchor_' + i, type: 'circle',
            shape: { cx: p[0], cy: p[1], r: 6.5 },
            style: { fill: '#ffffff', stroke: '#f59e0b', lineWidth: 2.5, shadowBlur: 6, shadowColor: 'rgba(0,0,0,0.5)' },
            silent: true, z: 66,
          });
        } else {
          // 常规实心把手
          els.push({
            id: prefix + '_anchor_' + i, type: 'circle',
            shape: { cx: p[0], cy: p[1], r: isSel ? 5.0 : 4.0 },
            style: { fill: '#ffffff', stroke: isSel ? '#f59e0b' : color, lineWidth: 2, shadowBlur: 3, shadowColor: 'rgba(0,0,0,0.25)' },
            silent: true, z: 65,
          });
        }
      });
    }
  }

  // ---------- 碰撞与拾取检测 (全工具覆盖，18px 磁性灵敏感应) ----------
  function hitTest(x, y) {
    for (let i = drawings.length - 1; i >= 0; i--) {
      const d = drawings[i];
      const pts = (d.points || []).map(pointToPixel);
      if (!pts.length || pts.some(p => !p)) continue;

      // 锚点优先检测 (扩大到 18px 磁性感应半径，18*18=324)
      for (let j = 0; j < pts.length; j++) {
        const dx = pts[j][0] - x, dy = pts[j][1] - y;
        if (dx * dx + dy * dy <= 324) {
          return { id: d.id, anchor: j, pt: pts[j] };
        }
      }

      // 图形主体拾取 (10px 范围)
      if (d.toolType === 'horizontal') {
        if (Math.abs(pts[0][1] - y) <= 10) return { id: d.id, anchor: -1 };
      } else if (d.toolType === 'trendline' && pts.length >= 2) {
        if (distToSeg(x, y, pts[0][0], pts[0][1], pts[1][0], pts[1][1]) <= 10) return { id: d.id, anchor: -1 };
      } else if (d.toolType === 'channel' && pts.length >= 3) {
        const off = [pts[2][0] - pts[0][0], pts[2][1] - pts[0][1]];
        const c1 = [pts[0][0] + off[0], pts[0][1] + off[1]];
        const c2 = [pts[1][0] + off[0], pts[1][1] + off[1]];
        if (distToSeg(x, y, pts[0][0], pts[0][1], pts[1][0], pts[1][1]) <= 10 ||
            distToSeg(x, y, c1[0], c1[1], c2[0], c2[1]) <= 10) {
          return { id: d.id, anchor: -1 };
        }
      } else if (d.toolType === 'rectangle' && pts.length >= 2) {
        const minX = Math.min(pts[0][0], pts[1][0]), maxX = Math.max(pts[0][0], pts[1][0]);
        const minY = Math.min(pts[0][1], pts[1][1]), maxY = Math.max(pts[0][1], pts[1][1]);
        if (x >= minX - 6 && x <= maxX + 6 && y >= minY - 6 && y <= maxY + 6) return { id: d.id, anchor: -1 };
      } else if (d.toolType === 'circle' && pts.length >= 2) {
        const rx = Math.abs(pts[1][0] - pts[0][0]) || 20;
        const ry = Math.abs(pts[1][1] - pts[0][1]) || 20;
        const norm = Math.pow((x - pts[0][0]) / rx, 2) + Math.pow((y - pts[0][1]) / ry, 2);
        if (Math.abs(norm - 1) < 0.4) return { id: d.id, anchor: -1 };
      } else if (d.toolType === 'polyline' && pts.length >= 2) {
        for (let k = 0; k < pts.length - 1; k++) {
          if (distToSeg(x, y, pts[k][0], pts[k][1], pts[k+1][0], pts[k+1][1]) <= 10) return { id: d.id, anchor: -1 };
        }
      } else if (d.toolType === 'fibonacci' && pts.length >= 2) {
        const levels = [0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
        const p1 = d.points[0].price, p2 = d.points[1].price;
        const hitLvl = levels.some(lvl => {
          const pr = p1 + (p2 - p1) * lvl;
          const pix = pointToPixel({ date: d.points[0].date, price: pr });
          return pix && Math.abs(pix[1] - y) <= 8;
        });
        if (hitLvl) return { id: d.id, anchor: -1 };
      } else if (d.toolType === 'measure' && pts.length >= 2) {
        const minX = Math.min(pts[0][0], pts[1][0]), maxX = Math.max(pts[0][0], pts[1][0]);
        const minY = Math.min(pts[0][1], pts[1][1]), maxY = Math.max(pts[0][1], pts[1][1]);
        if (x >= minX - 6 && x <= maxX + 6 && y >= minY - 6 && y <= maxY + 6) return { id: d.id, anchor: -1 };
      } else if (d.toolType === 'text' && pts.length >= 1) {
        if (x >= pts[0][0] - 10 && x <= pts[0][0] + 100 && Math.abs(pts[0][1] - y) <= 20) {
          return { id: d.id, anchor: -1 };
        }
      }
    }
    return null;
  }

  function distToSeg(px, py, x1, y1, x2, y2) {
    const l2 = (x2 - x1) * (x2 - x1) + (y2 - y1) * (y2 - y1);
    if (l2 === 0) return Math.hypot(px - x1, py - y1);
    let t = ((px - x1) * (x2 - x1) + (py - y1) * (y2 - y1)) / l2;
    t = Math.max(0, Math.min(1, t));
    return Math.hypot(px - (x1 + t * (x2 - x1)), py - (y1 + t * (y2 - y1)));
  }

  // 单点锚点拖拽更新（磁吸贴合）
  function moveAnchor(d, anchor, newDate, newPrice) {
    if (locked || !d || !d.points) return;
    if (anchor >= 0 && anchor < d.points.length) {
      d.points[anchor] = snapPoint(newDate, newPrice);
    }
  }

  // 整线按像素位移整体平移（X轴按 K 线索引差值偏移，Y轴按价差偏移，完美保持几何形状）
  function moveWholeDrawing(d, origPoints, startPix, curPix) {
    if (locked || !d || !origPoints || !origPoints.length || !chart) return;
    const data = getKlineData();
    if (!data || !data.length) return;

    try {
      const startIdxFloat = chart.convertFromPixel({ xAxisIndex: 0 }, startPix.x);
      const curIdxFloat = chart.convertFromPixel({ xAxisIndex: 0 }, curPix.x);
      const barDelta = Math.round(curIdxFloat - startIdxFloat);

      const startPrice = chart.convertFromPixel({ yAxisIndex: 0 }, startPix.y);
      const curPrice = chart.convertFromPixel({ yAxisIndex: 0 }, curPix.y);
      const priceDelta = curPrice - startPrice;

      d.points = origPoints.map(origPt => {
        const origIdx = data.findIndex(k => (k.date || k.time) === origPt.date);
        let newDate = origPt.date;
        if (origIdx >= 0) {
          const targetIdx = Math.max(0, Math.min(data.length - 1, origIdx + barDelta));
          newDate = data[targetIdx].date || data[targetIdx].time;
        }
        return {
          date: newDate,
          price: Number((origPt.price + priceDelta).toFixed(3))
        };
      });
    } catch (e) {}
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
    input.value = '支撑位';
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
        options: { color: strokeColor, text: userText, name: userText },
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
        dragging = !locked;
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
            options: { color: strokeColor, width: strokeWidth, dash: strokeDash },
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
        options: { color: strokeColor, width: strokeWidth, dash: strokeDash },
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
      if (tag === 'input' || tag === 'textarea') return;

      if (e.key === 'Delete' || e.key === 'Backspace') {
        if (selected && selected.id) {
          deleteSelected();
        }
      } else if (e.key === 'Escape') {
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
  function deleteSelected() {
    if (!selected) return;
    const id = selected.id;
    drawings = drawings.filter(d => d.id !== id);
    selected = null;
    hideLineEditPop();
    hideBoxPrompt();
    deleteDrawing(id);
    renderDrawings();
  }

  function undoLast() {
    if (!drawings.length) return;
    const last = drawings.pop();
    if (last) {
      deleteDrawing(last.id);
      selected = null;
      hideLineEditPop();
      hideBoxPrompt();
      renderDrawings();
    }
  }

  function clearAll() {
    if (!drawings.length) return;
    if (!window.confirm('确定要清空当前股票的所有划线吗？')) return;
    drawings.forEach(d => deleteDrawing(d.id));
    drawings = [];
    selected = null;
    tempDrawing = null;
    hideLineEditPop();
    hideBoxPrompt();
    hideInlineTextPop();
    renderDrawings();
  }

  // ---------- 绑定工具栏与浮层按钮交互 ----------
  function bindToolbarUI() {
    const root = document.getElementById('sd-draw-toolbar');
    if (!root || root._bound) return;
    root._bound = true;

    // 内嵌便签输入事件绑定
    document.getElementById('sd-itp-ok')?.addEventListener('click', confirmInlineText);
    document.getElementById('sd-itp-cancel')?.addEventListener('click', hideInlineTextPop);
    document.getElementById('sd-itp-input')?.addEventListener('keydown', e => {
      if (e.key === 'Enter') {
        e.preventDefault();
        confirmInlineText();
      } else if (e.key === 'Escape') {
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
        magBtn.title = magnetEnabled ? '强磁吸开启（点击切换轻微吸附）' : '轻微吸附开启（点击切换强磁吸）';
      });
    }

    // 锁定切换
    const lockBtn = document.getElementById('sd-draw-lock');
    if (lockBtn) {
      lockBtn.addEventListener('click', () => {
        locked = !locked;
        lockBtn.classList.toggle('active', locked);
        lockBtn.title = locked ? '已锁定画线（点击解锁）' : '未锁定（点击锁定）';
      });
    }

    // 调色盘与全局属性更新
    const palBtn = document.getElementById('sd-palette-btn');
    const palPop = document.getElementById('sd-palette-pop');
    if (palBtn && palPop) {
      palBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        const isOpen = palPop.style.display !== 'none';
        palPop.style.display = isOpen ? 'none' : 'block';
        palBtn.setAttribute('aria-expanded', String(!isOpen));
      });
      document.addEventListener('click', (e) => {
        if (!e.target.closest('.sd-palette-wrap')) {
          palPop.style.display = 'none';
          palBtn.setAttribute('aria-expanded', 'false');
        }
      });

      palPop.querySelectorAll('.sd-c-dot').forEach(dot => {
        dot.addEventListener('click', () => {
          palPop.querySelectorAll('.sd-c-dot').forEach(d => d.classList.remove('active'));
          dot.classList.add('active');
          strokeColor = dot.dataset.color || '#38bdf8';
          const preview = document.getElementById('sd-color-preview');
          if (preview) preview.style.background = strokeColor;

          if (selected) {
            const d = drawings.find(dd => dd.id === selected.id);
            if (d) {
              d.options = d.options || {};
              d.options.color = strokeColor;
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

  return {
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
