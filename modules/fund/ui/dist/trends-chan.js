(() => {
const { state, actions } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

// ===== 缠论分时分析面板 =====
function renderChanlun(data) {
  const card = document.getElementById('chanlun-card');
  const el = document.getElementById('chanlun-body');
  if (!card || !el) return;
  if (!data || data.error) {
    card.style.display = 'none';
    return;
  }
  card.style.display = 'block';

  const signals = data.signals || [];
  const stateText = data.current_state || '';

  let html = '';

  // 白话总结（小白模式显示）
  let plainSummary = '';
  if (signals.length > 0) {
    const lastSig = signals[0];
    plainSummary = `缠论分时：${lastSig.type_name}信号，${lastSig.description}`;
  } else if (data.kline_count < 5) {
    plainSummary = '缠论分时：数据不足，5分钟K线少于5根，无法分析';
  } else {
    plainSummary = '缠论分时：暂无买卖信号，等待背驰形成';
  }
  html += `<div class="cl-plain-summary"><b>白话总结：</b>${plainSummary}</div>`;

  if (stateText) {
    html += `<div class="cl-state">${stateText}</div>`;
  }

  if (signals.length > 0) {
    html += '<div class="cl-signals">';
    for (const sig of signals) {
      const isBuy = sig.type.startsWith('buy');
      const cls = isBuy ? 'cl-signal-buy' : 'cl-signal-sell';
      const tagCls = sig.type;
      html += `<div class="cl-signal-item ${cls}">
        <span class="cl-signal-tag ${tagCls}">${sig.type_name}</span>
        <span class="cl-signal-desc">${sig.description}</span>
        <span class="cl-signal-time">${sig.time}</span>
      </div>`;
    }
    html += '</div>';
  }

  html += `<div class="cl-stats">
    <span>5分K线: <b style="color:#ddd">${data.kline_count}</b></span>
    <span>分型: <b style="color:#ddd">${data.fractal_count}</b></span>
    <span>笔: <b style="color:#ddd">${data.stroke_count}</b></span>
    <span>信号: <b style="color:#ddd">${signals.length}</b></span>
  </div>`;

  el.innerHTML = html;
}

// ===== 缠论日线分析面板 =====
function renderChanlunDaily(data) {
  const card = document.getElementById('chanlun-daily-card');
  const el = document.getElementById('chanlun-daily-body');
  if (!card || !el) return;
  if (!data || data.error) {
    card.style.display = 'none';
    return;
  }
  card.style.display = 'block';

  const signals = data.signals || [];
  const zhongshus = data.zhongshus || [];
  const stateText = data.current_state || '';
  const currentPrice = data.current_price || (tState.klineData.length ? tState.klineData[tState.klineData.length - 1].close : null);

  let html = '';

  let plainSummary = '';
  const zsLabel = tState.currentView === 'week' ? '缠论周线' : '缠论日线';
  if (signals.length > 0) {
    const lastSig = signals[0];
    plainSummary = `${zsLabel}：${lastSig.type_name}信号，${lastSig.description}`;
  } else {
    plainSummary = `${zsLabel}：暂无买卖信号，等待背驰/中枢突破`;
  }
  if (currentPrice != null && zhongshus && zhongshus.length > 0) {
    const lastZs = zhongshus[zhongshus.length - 1];
    const zd = parseFloat(lastZs.zd), zg = parseFloat(lastZs.zg);
    if (!isNaN(zd) && !isNaN(zg)) {
      if (currentPrice >= zd && currentPrice <= zg) {
        plainSummary += `；当前在中枢[${zd}-${zg}]内震荡`;
      } else if (currentPrice > zg) {
        plainSummary += `；已突破中枢上沿${zg}`;
      } else {
        plainSummary += `；已跌破中枢下沿${zd}`;
      }
    }
  }
  html += `<div class="cl-plain-summary"><b>白话总结：</b>${plainSummary}</div>`;

  if (stateText) {
    html += `<div class="cl-state" style="border-left-color:#7F77DD">${stateText}</div>`;
  }

  if (signals.length > 0) {
    html += '<div class="cl-signals">';
    for (const sig of signals) {
      const isBuy = sig.type.startsWith('buy');
      const cls = isBuy ? 'cl-signal-buy' : 'cl-signal-sell';
      const tagCls = sig.type;
      html += `<div class="cl-signal-item ${cls}">
        <span class="cl-signal-tag ${tagCls}">${sig.type_name}</span>
        <span class="cl-signal-desc">${sig.description}</span>
        <span class="cl-signal-time">${sig.date}</span>
      </div>`;
    }
    html += '</div>';
  } else {
    html += '<div style="color:#555;font-size:12px;padding:6px 0">暂无买卖信号，等待背驰/中枢突破</div>';
  }

  let currentZs = null;
  if (currentPrice != null && zhongshus.length > 0) {
    for (const zs of zhongshus) {
      const zd = parseFloat(zs.zd);
      const zg = parseFloat(zs.zg);
      if (!isNaN(zd) && !isNaN(zg) && currentPrice >= zd && currentPrice <= zg) {
        currentZs = zs; break;
      }
    }
  }

  if (zhongshus.length > 0) {
    html += '<div style="margin-top:6px;font-size:11px;color:#666;border-top:1px solid #0d0d0d;padding-top:4px">中枢列表</div>';
    if (currentZs) {
      html += `<div style="margin:4px 0;padding:5px 8px;background:rgba(205,242,75,0.08);border:1px solid rgba(205,242,75,0.3);border-radius:4px;font-size:11px;color:#cdf24b">
        当前价格 <b>${currentPrice.toFixed(2)}</b> 处于中枢 <b>[${currentZs.zd} - ${currentZs.zg}]</b> 内，区间上沿压力 ${currentZs.zg}，下沿支撑 ${currentZs.zd}
      </div>`;
    } else if (currentPrice != null) {
      const lastZs = zhongshus[zhongshus.length - 1];
      const zd = parseFloat(lastZs.zd), zg = parseFloat(lastZs.zg);
      const pos = currentPrice > zg ? `上沿 ${zg} 之上` : `下沿 ${zd} 之下`;
      const hint = currentPrice > zg ? '已向上突破，关注回踩能否站稳' : '已跌破中枢，关注是否形成第三类卖点';
      html += `<div style="margin:4px 0;padding:5px 8px;background:#0d1a0d;border:1px solid #00b35c44;border-radius:4px;font-size:11px;color:#aaa">
        当前价格 <b>${currentPrice.toFixed(2)}</b> 位于最新中枢 [${lastZs.zd} - ${lastZs.zg}] ${pos}，${hint}
      </div>`;
    }
    for (const zs of zhongshus.slice(-4)) {
      const isCurrent = currentZs && currentZs === zs;
      const brokenCls = zs.is_broken ? 'cl-zhongshu-broken' : '';
      const currentCls = isCurrent ? 'cl-zhongshu-current' : '';
      const dirText = zs.is_broken ? (zs.break_direction === 'up' ? '向上突破' : '向下突破') : '震荡中';
      const currentTag = isCurrent ? '<span style="color:#cdf24b;font-weight:bold;margin-right:4px">[当前]</span>' : '';
      html += `<div class="cl-zhongshu ${brokenCls} ${currentCls}">
        ${currentTag}[${zs.zd} - ${zs.zg}] ${zs.start_date}~${zs.end_date} ${dirText}
      </div>`;
    }
  }

  html += `<div class="cl-stats">
    <span>${tState.currentView === 'week' ? '周K' : '日K'}: <b style="color:#ddd">${data.kline_count}</b></span>
    <span>合并: <b style="color:#ddd">${data.merged_count}</b></span>
    <span>分型: <b style="color:#ddd">${data.fractal_count}</b></span>
    <span>笔: <b style="color:#ddd">${data.stroke_count}</b></span>
    <span>中枢: <b style="color:#ddd">${data.zhongshu_count}</b></span>
    <span>信号: <b style="color:#ddd">${signals.length}</b></span>
  </div>`;

  el.innerHTML = html;
}

// ===== 日K线缠论图表叠加 =====
function applyChanlunDailyOverlay(data) {
  if (!data || !charts.klineChart) return;

  const chartSignals = data.chart_signals || [];
  const chartFractals = data.chart_fractals || [];
  const chartZhongshus = data.chart_zhongshus || [];
  const chartStrokes = data.chart_strokes || [];

  const opt = charts.klineChart.getOption();
  const klineSeries = opt.series.find(s => s.type === 'candlestick');
  if (!klineSeries) return;

  const existingMarkPoints = (klineSeries.markPoint && klineSeries.markPoint.data) || [];
  const clMarkPoints = [];

  const _chanlunExplain = {
    'buy1': '一类买点：价格创新低但MACD力度背驰（下上下三段，后段低点更低但力度更弱），认为是趋势底部。缠论中最强的买点。',
    'buy2': '二类买点：中枢形成后价格回踩不破中枢上沿。次强买点，出现在一类买点之后。',
    'buy3': '三类买点：中枢突破后价格回踩不破中枢上沿。确认性买点，趋势已确认向上。',
    'sell1': '一类卖点：价格创新高但MACD力度背驰（上下上三段，后段高点更高但力度更弱），认为是趋势顶部。缠论中最强的卖点。',
    'sell2': '二类卖点：中枢形成后价格反弹不破中枢下沿。次强卖点，出现在一类卖点之后。',
    'sell3': '三类卖点：中枢跌破后价格反弹不破中枢下沿。确认性卖点，趋势已确认向下。',
  };
  for (const sig of chartSignals) {
    const isBuy = sig.symbol === 'triangle';
    const sigDate = sig.coord[0];
    const sigPrice = sig.coord[1];
    const candle = tState.klineData.find(k => k.date === sigDate);
    let markerY = sigPrice;
    if (candle) {
      const range = candle.high - candle.low;
      const offset = Math.max(range * 0.6, sigPrice * 0.008);
      markerY = isBuy ? candle.low - offset : candle.high + offset;
    }
    clMarkPoints.push({
      coord: [sigDate, markerY],
      symbol: isBuy ? 'triangle' : 'triangle',
      symbolSize: 18,
      symbolRotate: isBuy ? 0 : 180,
      itemStyle: sig.itemStyle,
      label: {
        show: true,
        formatter: sig.label?.formatter || (isBuy ? '买' : '卖'),
        fontSize: 10,
        fontWeight: 'bold',
        color: sig.label?.color || (isBuy ? C.up : C.down),
        position: isBuy ? 'bottom' : 'top',
      },
    });
    const sigType = sig.type || (isBuy ? 'buy1' : 'sell1');
    tState.signalPoints.push({
      date: sigDate, price: markerY,
      title: sig.label?.formatter ? `缠论·${sig.label.formatter}` : (isBuy ? '缠论买点' : '缠论卖点'),
      formula: `信号价位 ${sigPrice}`,
      desc: _chanlunExplain[sigType] || (isBuy ? '缠论买点信号' : '缠论卖点信号'),
    });
  }

  for (const f of chartFractals) {
    clMarkPoints.push({
      coord: f.coord,
      symbol: f.symbol,
      symbolSize: f.symbolSize,
      itemStyle: f.itemStyle,
    });
  }

  const markAreas = chartZhongshus.map(zs => ([
    { xAxis: zs.xAxis[0], yAxis: zs.yAxis[0] },
    { xAxis: zs.xAxis[1], yAxis: zs.yAxis[1], itemStyle: zs.itemStyle },
  ]));

  const strokeLines = chartStrokes.map(s => ([
    { coord: s.coords[0] },
    { coord: s.coords[1], lineStyle: s.lineStyle },
  ]));

  const existingMarkLines = (klineSeries.markLine && klineSeries.markLine.data) || [];

  charts.klineChart.setOption({
    series: [{
      name: 'K线',
      markPoint: {
        data: [...existingMarkPoints, ...clMarkPoints],
        animation: false,
      },
      markLine: {
        silent: true,
        animation: false,
        symbol: 'none',
        data: [...existingMarkLines, ...strokeLines],
      },
      markArea: markAreas.length ? {
        silent: true,
        animation: false,
        data: markAreas,
      } : undefined,
    }],
  });
}

trends.chan = {
  renderChanlun,
  renderChanlunDaily,
  applyChanlunDailyOverlay,
};

})();
