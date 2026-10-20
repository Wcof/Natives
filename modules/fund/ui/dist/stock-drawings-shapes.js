(() => {
// Fund 股票详情画图引擎 · 图元构建与拾取层（Stock Drawings Shapes）
// 职责边界（从 stock-drawings.js 工厂中按职责拆出，行为保持不变）：
// 1. buildGraphic：九类工具的 ECharts graphic 图元构建（线/矩形/圆/斐波那契/测距/便签等）
// 2. hitTest / distToSeg：锚点磁性感应与图形主体拾取（纯几何计算）
// 3. moveAnchor / moveWholeDrawing：锚点磁吸拖拽与整线像素平移（纯几何计算）
// 这些函数依赖工厂闭包内的共享可变状态（选中态、悬停态、磁吸开关等），
// 因此以 ctx 上下文对象形式接收，由 stock-drawings.js 在工厂内部绑定：
//   ctx = { chart, getKlineData, selected, hoverHit, magnetEnabled, locked,
//           strokeColor, strokeWidth, dashPattern, pointToPixel, chartWidthPx,
//           snapPoint, saveDrawing }
// 其中 selected/hoverHit/magnetEnabled/locked 为 getter，保持引用实时读取。

globalThis.Fund.StockDrawingsShapes = {
  buildGraphic(ctx, d, els) {
    const selected = ctx.selected;
    const hoverHit = ctx.hoverHit;
    if(d.options?.visible === false)return;
    const isSel = selected && selected.id === d.id;
    const isHover = hoverHit && hoverHit.id === d.id;
    // 用户在属性面板自定义的颜色/线宽永远优先呈现；选中/悬停只做宽度提升
    // 与光晕强调，不再用硬编码色整体覆盖（否则属性修改会“看起来不生效”）。
    // 仅在未自定义颜色时按 fund-chart-spec 规范切换：默认天蓝、选中琥珀金。
    const hasCustomColor = !!(d.options && d.options.color);
    const color = hasCustomColor ? d.options.color : ctx.strokeColor;
    const baseWidth = (d.options && d.options.width) || ctx.strokeWidth;
    const stroke = (isSel && !hasCustomColor) ? (ctx.selectedColor || color) : color;
    const width = isSel ? Math.max(baseWidth, 2.5) : (isHover ? Math.max(baseWidth, 2.2) : baseWidth);
    const style = { stroke, lineWidth: width };
    if (isSel || isHover) {
      style.shadowBlur = isSel ? 10 : 7;
      style.shadowColor = stroke;
    }
    // 线条类型由用户属性决定（默认实线）；兼容旧数据的 'dashed' 与新增疏密变体
    const dash = ctx.dashPattern((d.options && d.options.dash) || 'solid');
    if (dash) style.lineDash = dash;

    const pts = (d.points || []).map(ctx.pointToPixel);
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
          style: { text: d.options.name, fill: stroke, backgroundColor: Fund.themeRgba('--natives-surface', 0.95), padding: [2, 6], borderRadius: 3, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) },
          silent: true, z: 62,
        });
      }
    }
    // 2. 水平线 (单点横贯全图)——线型随用户属性，不再强制虚线
    else if (d.toolType === 'horizontal' && pts.length >= 1) {
      els.push({
        id: prefix + '_l', type: 'line',
        shape: { x1: 0, y1: pts[0][1], x2: ctx.chartWidthPx(), y2: pts[0][1] },
        style: style, silent: true, z: 60,
      });
      const pVal = d.points[0].price != null ? Number(d.points[0].price).toFixed(2) : '';
      const nameTxt = (d.options && d.options.name) ? `${d.options.name}: ` : '';
      els.push({
        id: prefix + '_txt', type: 'text',
        x: ctx.chartWidthPx() - 65, y: pts[0][1] - 8,
        style: { text: `${nameTxt}${pVal}`, fill: stroke, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')), backgroundColor: Fund.themeRgba('--natives-surface', 0.95), padding: [1, 5], borderRadius: 2 },
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
        style: Object.assign({ fill: hasCustomColor ? color + '22' : Fund.themeRgba('--draw-fill', 1) }, style), silent: true, z: 60,
      });
      if (d.options && d.options.name) {
        els.push({
          id: prefix + '_name', type: 'text',
          x: rx + 6, y: ry + 6,
          style: { text: d.options.name, fill: stroke, backgroundColor: Fund.themeRgba('--natives-surface', 0.95), padding: [2, 6], borderRadius: 3, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) },
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
        style: Object.assign({ fill: hasCustomColor ? color + '18' : Fund.themeRgba('--draw-fill', 1) }, style), silent: true, z: 60,
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
        const ptPix = ctx.pointToPixel({ date: d.points[0].date, price: pr });
        if (!ptPix) return;
        const py = ptPix[1];
        els.push({
          id: prefix + '_fib_' + idx, type: 'line',
          shape: { x1: minX, y1: py, x2: Math.max(maxX, ctx.chartWidthPx()), y2: py },
          style: { stroke: (lvl === 0.5 || lvl === 0.618) ? (ctx.selectedColor || color) : color, lineWidth: 1.2, lineDash: [4, 4] },
          silent: true, z: 60,
        });
        els.push({
          id: prefix + '_fib_t_' + idx, type: 'text',
          x: minX + 6, y: py - 9,
          style: { text: `${(lvl * 100).toFixed(1)}% (${pr.toFixed(2)})`, fill: Fund.themeColor('--chart-text-dim'), fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')) },
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
      const cLabel = pDiff >= 0 ? Fund.themeColor('--color-up') : Fund.themeColor('--color-down');

      const data = ctx.getKlineData();
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
        style: { fill: Fund.themeRgba(pDiff >= 0 ? '--color-up' : '--color-down', 0.12), stroke: cLabel, lineWidth: 1.2, lineDash: [3, 3] },
        silent: true, z: 60,
      });
      els.push({
        id: prefix + '_ms_t', type: 'text',
        x: rx + rw / 2 - 40, y: ry + rh / 2 - 8,
        style: {
          text: `Δ ${sign}${pDiff.toFixed(2)} (${sign}${pctDiff.toFixed(2)}%)${barCountTxt}`,
          fill: Fund.themeColor('--chart-text'), backgroundColor: Fund.themeRgba('--natives-surface', 0.95), padding: [2, 6], borderRadius: 3, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')),
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
          text: txt, fill: stroke, backgroundColor: Fund.themeRgba('--natives-surface', 0.95),
          borderColor: stroke, borderWidth: 1, padding: [3, 8], borderRadius: 3, fontSize: parseFloat(Fund.themeColor('--natives-fs-secondary')),
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
            style: { fill: Fund.themeRgba('--draw-selected', 0.28), stroke: ctx.selectedColor || Fund.themeColor('--draw-selected'), lineWidth: 1.5, lineDash: [3, 2] },
            silent: true, z: 64,
          });
          // 内层高亮把手实心圆
          els.push({
            id: prefix + '_anchor_' + i, type: 'circle',
            shape: { cx: p[0], cy: p[1], r: 6.5 },
            style: { fill: Fund.themeColor('--natives-surface'), stroke: ctx.selectedColor || Fund.themeColor('--draw-selected'), lineWidth: 2.5, shadowBlur: 6, shadowColor: Fund.themeRgba('--chart-text', 0.25) },
            silent: true, z: 66,
          });
        } else {
          // 常规实心把手
          els.push({
            id: prefix + '_anchor_' + i, type: 'circle',
            shape: { cx: p[0], cy: p[1], r: isSel ? 5.0 : 4.0 },
            style: { fill: Fund.themeColor('--natives-surface'), stroke: isSel ? ctx.selectedColor : color, lineWidth: 2, shadowBlur: 3, shadowColor: Fund.themeRgba('--chart-text', 0.12) },
            silent: true, z: 65,
          });
        }
      });
    }
  },

  // ---------- 碰撞与拾取检测 (全工具覆盖，18px 磁性灵敏感应) ----------
  hitTest(ctx, x, y) {
    const drawings = ctx.drawings;
    for (let i = drawings.length - 1; i >= 0; i--) {
      const d = drawings[i];
      if(d.options?.visible === false)continue;
      const pts = (d.points || []).map(ctx.pointToPixel);
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
          const pix = ctx.pointToPixel({ date: d.points[0].date, price: pr });
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
  },

  // 单点锚点拖拽更新（磁吸贴合）
  moveAnchor(ctx, d, anchor, newDate, newPrice) {
    if (ctx.locked || !d || !d.points) return;
    if (anchor >= 0 && anchor < d.points.length) {
      d.points[anchor] = ctx.snapPoint(newDate, newPrice);
    }
  },

  // 整线按像素位移整体平移（X轴按 K 线索引差值偏移，Y轴按价差偏移，完美保持几何形状）
  moveWholeDrawing(ctx, d, origPoints, startPix, curPix) {
    const chart = ctx.chart;
    if (ctx.locked || !d || !origPoints || !origPoints.length || !chart) return;
    const data = ctx.getKlineData();
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
  },
};

})();
