// Chart Registry (ADR-0028 D4).
//
// Each chart is a pure renderer: (container, dataset, ui) -> void. It reads
// only aggregated datasets produced by the metric layer, styles exclusively
// via existing theme tokens, and leaves no timers or listeners behind.

import { escapeHtml } from '../plugins/sanitizer.js';

const SVG_NS = 'http://www.w3.org/2000/svg';

// R2（方案 §5.2）：不再向公共 shadowRoot 放探针取色。色阶直接引用
// 空间解析后的局部角色（--space-widget-accent，由卡片根注入），
// 浏览器实时解析，外观变化即时生效且无颜色快照。
function heatShades() {
  const base = 'var(--space-widget-accent, currentColor)';
  return [
    '',
    `color-mix(in srgb, ${base} 20%, transparent)`,
    `color-mix(in srgb, ${base} 45%, transparent)`,
    `color-mix(in srgb, ${base} 70%, transparent)`,
    base,
  ];
}

// R7（§5.1 第 5 条）：把当前卡片筛选 + 维度 key 组装成详情页筛选。
// range 原样透传；维度 key 按槽位映射到 usageFilter 字段。
function drillFilter(ui, key) {
  if (!ui || typeof ui.openDrilldown !== 'function') return;
  const filter = { range: ui.range };
  // 自定义周期：起止随钻取透传（用量详情沿用同一时间窗）。
  if (ui.range === 'custom' && ui.startTime && ui.endTime) {
    filter.startTime = ui.startTime;
    filter.endTime = ui.endTime;
  }
  if (key) {
    if (ui.dimension === 'provider') filter.provider = key;
    else if (ui.dimension === 'source') filter.source = key;
    else filter.model = key;
  }
  ui.openDrilldown(filter);
}

const numberCard = {
  id: 'number',
  render(container, dataset, ui) {
    container.replaceChildren();
    const card = document.createElement('div');
    card.className = 'ai-perf-number';
    // 协议单位：successRate 等百分比指标由 Host 以 0–100 提供，前端只格式化。
    const displayVal = dataset.number.isPercent
      ? `${Number(dataset.number.value ?? 0).toFixed(1)}%`
      : ui.fmtValue(dataset.number.value);
    const subLabelHtml = dataset.number.subLabel
      ? `<div class="ai-perf-number-sub">${escapeHtml(dataset.number.subLabel)}</div>`
      : '';
    card.innerHTML = `
      <div class="ai-perf-number-value">${escapeHtml(displayVal)}</div>
      <div class="ai-perf-number-label">${escapeHtml(ui.label)}</div>
      ${subLabelHtml}
    `;
    if (typeof ui.openDrilldown === 'function') {
      card.classList.add('ai-perf-drillable');
      card.setAttribute('role', 'button');
      card.tabIndex = 0;
      const open = () => drillFilter(ui, '');
      card.addEventListener('click', open);
      card.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open(); } });
    }
    container.append(card);
  },
};

const lineChart = {
  id: 'line',
  render(container, dataset, ui) {
    container.replaceChildren();
    const points = dataset.trend;
    const wrap = document.createElement('div');
    wrap.className = 'ai-perf-chart';
    if (!points || points.length < 2) {
      wrap.innerHTML = `<div class="ai-perf-empty">${escapeHtml(ui.t('aiPerfNoTrend', '时间段内暂无趋势数据'))}</div>`;
      container.append(wrap);
      return;
    }
    const w = 300;
    const h = 96;
    const pad = 4;
    const max = Math.max(...points.map((p) => p.v), 1);
    const step = (w - pad * 2) / (points.length - 1);
    const coords = points.map((p, i) => [pad + i * step, h - pad - (p.v / max) * (h - pad * 2)]);
    const path = coords.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
    const area = `${path} L${coords[coords.length - 1][0].toFixed(1)},${h - pad} L${coords[0][0].toFixed(1)},${h - pad} Z`;
    wrap.innerHTML = `
      <svg viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" class="ai-perf-line-svg" aria-hidden="true">
        <path class="ai-perf-line-area" d="${area}"></path>
        <path class="ai-perf-line-path" d="${path}"></path>
      </svg>
      <div class="ai-perf-chart-caption">
        <span>${escapeHtml(ui.label)}</span>
        <span class="ai-perf-chart-max">${escapeHtml(ui.fmtRank(max))}</span>
      </div>
    `;
    container.append(wrap);
  },
};

const barChart = {
  id: 'bar',
  render(container, dataset, ui) {
    container.replaceChildren();
    const items = (dataset.rank || []).slice(0, 6);
    const wrap = document.createElement('div');
    wrap.className = 'ai-perf-chart';
    if (!items.length) {
      wrap.innerHTML = `<div class="ai-perf-empty">${escapeHtml(ui.t('aiPerfNoRank', '时间段内暂无排行数据'))}</div>`;
      container.append(wrap);
      return;
    }
    const max = Math.max(...items.map((i) => i.value), 1);
    const rows = items.map((item) => {
      const pct = Math.max(2, Math.round((item.value / max) * 100));
      return `
        <div class="ai-perf-bar-row"${typeof ui.openDrilldown === 'function' ? ' role="button" tabindex="0" data-drill-key="' + escapeHtml(item.key || '') + '"' : ''}>
          <span class="ai-perf-bar-name" title="${escapeHtml(item.name)}">${escapeHtml(item.name || '—')}</span>
          <span class="ai-perf-bar-track"><span class="ai-perf-bar-fill" style="width:${pct}%"></span></span>
          <span class="ai-perf-bar-value">${escapeHtml(ui.fmtRank(item.value))}</span>
        </div>`;
    });
    wrap.innerHTML = `<div class="ai-perf-bars">${rows.join('')}</div>`;
    if (typeof ui.openDrilldown === 'function') {
      wrap.classList.add('ai-perf-drillable');
      for (const row of wrap.querySelectorAll('[data-drill-key]')) {
        const key = row.dataset.drillKey;
        const open = () => drillFilter(ui, key);
        row.addEventListener('click', open);
        row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open(); } });
      }
    }
    container.append(wrap);
  },
};

const heatmapChart = {
  id: 'heatmap',
  render(container, dataset, ui) {
    container.replaceChildren();
    const days = dataset.days || [];
    const wrap = document.createElement('div');
    wrap.className = 'ai-perf-chart';
    if (!days.length) {
      wrap.innerHTML = `<div class="ai-perf-empty">${escapeHtml(ui.t('aiPerfNoHeatmap', '时间段内暂无活跃数据'))}</div>`;
      container.append(wrap);
      return;
    }
    const shades = heatShades();
    const byDate = new Map(days.map((d) => [d.date, d.v]));
    const max = Math.max(...days.map((d) => d.v), 1);
    const end = new Date();
    const weeks = 18;
    const cell = 10;
    const gap = 3;
    const svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('class', 'ai-perf-heat-svg');
    svg.setAttribute('viewBox', `0 0 ${weeks * (cell + gap) + 4} ${7 * (cell + gap) + 4}`);
    svg.setAttribute('width', '100%');
    const start = new Date(end);
    start.setDate(start.getDate() - (weeks * 7 - 1));
    start.setDate(start.getDate() - ((start.getDay() + 6) % 7));
    for (let wk = 0; wk < weeks; wk += 1) {
      for (let dow = 0; dow < 7; dow += 1) {
        const d = new Date(start);
        d.setDate(start.getDate() + wk * 7 + dow);
        const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
        const v = byDate.get(key);
        const rect = document.createElementNS(SVG_NS, 'rect');
        rect.setAttribute('x', String(wk * (cell + gap)));
        rect.setAttribute('y', String(dow * (cell + gap)));
        rect.setAttribute('width', String(cell));
        rect.setAttribute('height', String(cell));
        rect.setAttribute('rx', '2.5');
        if (v == null) {
          rect.setAttribute('fill', 'var(--surface-3)');
        } else {
          const level = v <= 0 ? 1 : Math.min(4, 1 + Math.ceil((v / max) * 3));
          rect.setAttribute('fill', shades[level]);
          const title = document.createElementNS(SVG_NS, 'title');
          title.textContent = `${key} · ${ui.fmtRank(v)}`;
          rect.append(title);
        }
        svg.append(rect);
      }
    }
    wrap.append(svg);
    container.append(wrap);
  },
};

const timelineChart = {
  id: 'timeline',
  render(container, dataset, ui) {
    container.replaceChildren();
    const events = (dataset.events || []).slice(0, 6);
    const wrap = document.createElement('div');
    wrap.className = 'ai-perf-chart ai-perf-timeline-wrap';
    if (!events.length) {
      wrap.innerHTML = `<div class="ai-perf-empty">${escapeHtml(ui.t('aiPerfNoTimeline', '暂无近期 AI 调用事件'))}</div>`;
      container.append(wrap);
      return;
    }
    const rows = events.map((ev) => `
      <div class="ai-perf-timeline-item">
        <span class="ai-perf-timeline-time">${escapeHtml(ev.time || '—')}</span>
        <span class="ai-perf-timeline-dot ${ev.status}"></span>
        <div class="ai-perf-timeline-info">
          <div class="ai-perf-timeline-head">
            <span class="ai-perf-timeline-source">${escapeHtml(ev.source)}</span>
            <span class="ai-perf-timeline-model">${escapeHtml(ev.model)}</span>
          </div>
          <div class="ai-perf-timeline-meta">
            <span>${escapeHtml(ui.t('aiPerfTokens', 'Token'))}: ${escapeHtml(ui.fmtRank(ev.tokens))}</span>
            ${ev.costStatus === 'unpriced'
              ? `<span>${escapeHtml(ui.t('aiPerfUnpriced', '未计价'))}</span>`
              : (ev.cost > 0 ? `<span>$${ev.cost.toFixed(4)}</span>` : '')}
          </div>
        </div>
      </div>
    `);
    wrap.innerHTML = `<div class="ai-perf-timeline-list">${rows.join('')}</div>`;
    container.append(wrap);
  },
};

const tableChart = {
  id: 'table',
  render(container, dataset, ui) {
    container.replaceChildren();
    const rows = (dataset.tableRows || []).slice(0, 5);
    const wrap = document.createElement('div');
    wrap.className = 'ai-perf-chart ai-perf-table-wrap';
    if (!rows.length) {
      wrap.innerHTML = `<div class="ai-perf-empty">${escapeHtml(ui.t('aiPerfNoTable', '暂无对比数据'))}</div>`;
      container.append(wrap);
      return;
    }
    // R2（方案 §5.4）：列由当前指标定义，不硬编码 requests/tokens 两列；
    // 成本卡显示金额列（带估算标注），Token 卡显示 Token 列，调用卡显示请求数。
    const cols = tableColumnsFor(ui.metricId, ui.t);
    const thead = `
      <div class="ai-perf-table-row ai-perf-table-head">
        <span class="ai-perf-col-name">${escapeHtml(ui.t('aiPerfColName', '项目'))}</span>
        ${cols.map((c) => `<span class="ai-perf-col-num">${escapeHtml(c.label)}</span>`).join('')}
      </div>
    `;
    const tbody = rows
      .map(
        (r) => `
      <div class="ai-perf-table-row"${typeof ui.openDrilldown === 'function' ? ' role="button" tabindex="0" data-drill-key="' + escapeHtml(r.key || '') + '"' : ''}>
        <span class="ai-perf-col-name" title="${escapeHtml(r.name)}">${escapeHtml(r.name || '—')}</span>
        ${cols.map((c) => `<span class="ai-perf-col-num">${escapeHtml(c.cell(r))}</span>`).join('')}
      </div>
    `
      )
      .join('');
    wrap.innerHTML = `<div class="ai-perf-table">${thead}${tbody}</div>`;
    if (typeof ui.openDrilldown === 'function') {
      wrap.classList.add('ai-perf-drillable');
      for (const row of wrap.querySelectorAll('[data-drill-key]')) {
        const key = row.dataset.drillKey;
        const open = () => drillFilter(ui, key);
        row.addEventListener('click', open);
        row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open(); } });
      }
    }
    container.append(wrap);
  },
};

function tableColumnsFor(metricId, t) {
  const fmtInt = (v) => Number(v || 0).toLocaleString();
  if (metricId === 'ai_cost') {
    // 金额列：本地估算值；未计价部分不由表格冒充 0（§9.1）。
    return [
      { label: t('aiPerfColCost', '费用(估算)'), cell: (r) => `$${Number(r.cost ?? 0).toFixed(4)}` },
      { label: t('aiPerfColTokens', 'Token'), cell: (r) => fmtInt(r.tokens) },
    ];
  }
  if (metricId === 'request_count' || metricId === 'session_duration' || metricId === 'ai_sessions') {
    return [
      { label: t('aiPerfColRequests', '请求'), cell: (r) => fmtInt(r.requests) },
      { label: t('aiPerfColTokens', 'Token'), cell: (r) => fmtInt(r.tokens) },
    ];
  }
  return [
    { label: t('aiPerfColTokens', 'Token'), cell: (r) => fmtInt(r.tokens) },
    { label: t('aiPerfColRequests', '请求'), cell: (r) => fmtInt(r.requests) },
  ];
}

export const CHARTS = {
  [numberCard.id]: numberCard,
  [lineChart.id]: lineChart,
  [barChart.id]: barChart,
  [heatmapChart.id]: heatmapChart,
  [timelineChart.id]: timelineChart,
  [tableChart.id]: tableChart,
};

export const DEFAULT_CHART = 'number';

export const CHART_OPTIONS = [
  ['number', 'aiPerfChartNumber', '数字卡片'],
  ['line', 'aiPerfChartLine', '趋势线'],
  ['bar', 'aiPerfChartBar', '排行'],
  ['heatmap', 'aiPerfChartHeatmap', '活跃热力图'],
  ['timeline', 'aiPerfChartTimeline', '调用时间线'],
  ['table', 'aiPerfChartTable', '明细对比表'],
];

export const CHART_NEEDS = {
  number: { overview: true, analytics: false, events: false },
  line: { overview: true, analytics: false, events: false },
  heatmap: { overview: true, analytics: false, events: false },
  bar: { overview: false, analytics: true, events: false },
  timeline: { overview: false, analytics: false, events: true },
  table: { overview: false, analytics: true, events: false },
};
