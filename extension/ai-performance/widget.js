// widget/aiPerformance — config-driven AI usage widget (ADR-0028 D2).
//
// The widget persists only a configuration object:
//   { metric, subMetric, chart, dimension, range }
// Render pipeline: config → Metric Query (Model Host) → Chart Renderer → UI.
// The render() contract returns a disposer that cancels in-flight requests
// and releases the shared Model Host client.

import { escapeHtml } from '../plugins/sanitizer.js';
import { acquireModelApi, releaseModelApi, subscribeModelEvents } from './client.js';
import { sharedCall } from './shared-queries.js';
import { renderView, normalizeView } from './views.js';
import { METRICS, DEFAULT_METRIC, fetchDataset, userTimezone } from './metrics.js';
import { CHARTS, DEFAULT_CHART, CHART_NEEDS, CHART_OPTIONS } from './charts.js';

const RANGES = [
  ['24h', 'aiPerfRange24h', '近 24 小时'],
  ['72h', 'aiPerfRange72h', '近 72 小时'],
  ['7d', 'aiPerfRange7d', '近 7 天'],
  ['30d', 'aiPerfRange30d', '近 30 天'],
  ['custom', 'aiPerfRangeCustom', '自定义'],
  ['all', 'aiPerfRangeAll', '全部'],
];

// 自定义周期的起止校验与归一：合法则保留 ISO（UTC），否则置空由查询层
// 给出近 7 天缺省（§4.2 周期参数与 Host Filter.custom 对齐）。
function normalizeCustomTimes(startTime, endTime) {
  const start = Date.parse(startTime);
  const end = Date.parse(endTime);
  if (Number.isFinite(start) && Number.isFinite(end) && start < end) {
    return { startTime: new Date(start).toISOString(), endTime: new Date(end).toISOString() };
  }
  return { startTime: '', endTime: '' };
}

function isoToLocalInputValue(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function customTimesInputs(config, t = (k, f) => f || k) {
  return `
    <label class="inspector-field"><span>${escapeHtml(t('aiPerfRangeStart', '开始时间'))}</span>
      <input type="datetime-local" data-role="ai-perf-start" value="${escapeHtml(isoToLocalInputValue(config.startTime))}">
    </label>
    <label class="inspector-field"><span>${escapeHtml(t('aiPerfRangeEnd', '结束时间'))}</span>
      <input type="datetime-local" data-role="ai-perf-end" value="${escapeHtml(isoToLocalInputValue(config.endTime))}">
    </label>`;
}

const DIMENSION_OPTIONS = [
  ['model', 'aiPerfDimModel', '按模型 (Model)'],
  ['source', 'aiPerfDimSource', '按工具来源 (Source)'],
  ['provider', 'aiPerfDimProvider', '按供应商 (Provider)'],
];

const VIEW_OPTIONS = [
  ['usage', 'aiPerfViewUsage', '用量与去向 (默认)'],
  ['tools', 'aiPerfViewTools', '工具与账户目录'],
  ['billing', 'aiPerfViewBilling', '账单与对账'],
  ['attention', 'aiPerfViewAttention', '待我处理'],
  ['limits', 'aiPerfViewLimits', '额度与预算'],
  ['ledger', 'aiPerfViewLedger', '统一用量账本'],
  ['savings', 'aiPerfViewSavings', '降本建议'],
];

const WIDGET_STYLES = `
  /* R2（方案 §5.2/§5.3）：全部消费空间解析后的局部角色（--space-widget-*，
     由 space-dashboard:applyWidgetDisplayStyles 注入本卡片根）；
     不再回读全局 --text/--accent/--display-font/--muted，不覆盖空间外观。 */
  .ai-perf { display:flex; flex-direction:column; gap:10px; width:100%; height:100%; min-width:0; color:var(--space-widget-text, inherit); box-sizing:border-box; }
  .ai-perf-number { display:flex; flex-direction:column; gap:4px; align-items:flex-start; box-sizing:border-box; width:100%; padding:14px 16px; border-radius:16px; background:color-mix(in srgb, var(--space-widget-text, #ffffff) 8%, transparent); backdrop-filter:blur(20px); -webkit-backdrop-filter:blur(20px); border:1px solid color-mix(in srgb, var(--space-widget-text, #ffffff) 16%, transparent); }
  .ai-perf-number.ai-perf-drillable { cursor:pointer; }
  .ai-perf-number-value { font-family:inherit; font-size:var(--space-widget-metric-size, 22px); line-height:1.15; color:currentColor; font-weight:var(--space-widget-weight, 700); letter-spacing:-0.02em; font-variant-numeric:tabular-nums; }
  .ai-perf-number-label { font-size:12px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); font-weight:500; }
  .ai-perf-number-sub { font-size:12px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); margin-top:2px; font-variant-numeric:tabular-nums; }
  .ai-perf-chart { width:100%; min-width:0; }
  .ai-perf-line-svg { width:100%; height:auto; display:block; }
  .ai-perf-line-path { fill:none; stroke:currentColor; stroke-width:2; stroke-linecap:round; stroke-linejoin:round; }
  .ai-perf-line-area { fill:color-mix(in srgb, currentColor 14%, transparent); stroke:none; }
  .ai-perf-heat-svg { width:100%; height:auto; display:block; }
  .ai-perf-chart-caption { display:flex; justify-content:space-between; margin-top:6px; font-size:12px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); }
  .ai-perf-chart-max { color:currentColor; font-weight:550; font-variant-numeric:tabular-nums; }
  .ai-perf-bars { display:flex; flex-direction:column; gap:6px; }
  .ai-perf-bar-row { display:grid; grid-template-columns:minmax(48px,32%) 1fr auto; align-items:center; gap:8px; font-size:12px; line-height:1.4; }
  .ai-perf-bar-name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:currentColor; font-weight:500; }
  .ai-perf-bar-track { height:8px; border-radius:4px; background:color-mix(in srgb, currentColor 12%, transparent); overflow:hidden; }
  .ai-perf-bar-fill { display:block; height:100%; border-radius:inherit; background:currentColor; }
  .ai-perf-bar-value { color:var(--space-widget-text-secondary, inherit); font-variant-numeric:tabular-nums; font-size:12px; }

  /* Timeline */
  .ai-perf-timeline-list { display:flex; flex-direction:column; gap:8px; width:100%; }
  .ai-perf-timeline-item { display:grid; grid-template-columns:auto auto 1fr; align-items:center; gap:8px; font-size:12px; line-height:1.4; }
  .ai-perf-timeline-time { color:var(--space-widget-text-secondary, inherit); font-variant-numeric:tabular-nums; font-size:12px; min-width:34px; }
  .ai-perf-timeline-dot { width:6px; height:6px; border-radius:50%; background:currentColor; }
  .ai-perf-timeline-dot.error { background:var(--space-widget-danger, var(--danger, #ff7f8f)); }
  .ai-perf-timeline-info { display:flex; justify-content:space-between; align-items:center; min-width:0; gap:6px; }
  .ai-perf-timeline-head { display:flex; gap:4px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .ai-perf-timeline-source { font-weight:600; color:currentColor; }
  .ai-perf-timeline-model { color:var(--space-widget-text-secondary, inherit); }
  .ai-perf-timeline-meta { display:flex; gap:6px; font-size:12px; color:var(--space-widget-text-secondary, inherit); flex-shrink:0; font-variant-numeric:tabular-nums; }

  /* Table */
  .ai-perf-table { display:flex; flex-direction:column; gap:4px; width:100%; font-size:12px; }
  .ai-perf-table-row { display:grid; grid-template-columns:1fr 50px 60px; gap:6px; align-items:center; padding:3px 0; border-bottom:1px solid color-mix(in srgb, currentColor 14%, transparent); }
  .ai-perf-table-head { font-weight:600; color:var(--space-widget-text-secondary, inherit); font-size:12px; }
  .ai-perf-col-name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:currentColor; }
  .ai-perf-col-num { text-align:right; font-variant-numeric:tabular-nums; color:var(--space-widget-text-secondary, inherit); }

  /* Tools & Capabilities */
  .ai-perf-tools { display:flex; flex-direction:column; gap:8px; width:100%; font-size:12px; }
  .ai-perf-source { padding:6px 8px; border:1px solid color-mix(in srgb, currentColor 16%, transparent); border-radius:6px; background:color-mix(in srgb, currentColor 5%, transparent); }
  .ai-perf-source-head { display:flex; justify-content:space-between; align-items:center; margin-bottom:4px; }
  .ai-perf-caps { display:flex; flex-wrap:wrap; gap:4px; margin:4px 0; }
  .ai-perf-cap { font-size:11px; line-height:1.4; padding:1px 5px; border-radius:3px; }
  .ai-perf-cap-ok { background:color-mix(in srgb, currentColor 18%, transparent); color:currentColor; }
  .ai-perf-cap-warn { background:color-mix(in srgb, var(--space-widget-warning, var(--warning, #e6c07b)) 20%, transparent); color:var(--space-widget-warning, var(--warning, #e6c07b)); }
  .ai-perf-cap-off { background:color-mix(in srgb, currentColor 8%, transparent); color:var(--space-widget-text-secondary, inherit); }
  .ai-perf-on { font-size:12px; line-height:1.4; color:currentColor; font-weight:500; }
  .ai-perf-audit { font-size:11px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); margin-top:3px; }

  /* Billing */
  .ai-perf-billing { display:flex; flex-direction:column; gap:6px; width:100%; font-size:12px; }
  .ai-perf-bill-row { display:flex; flex-direction:column; gap:2px; padding:6px; border-radius:6px; background:color-mix(in srgb, currentColor 5%, transparent); border:1px solid color-mix(in srgb, currentColor 12%, transparent); }

  /* Attention */
  .ai-perf-attention { display:flex; flex-direction:column; gap:6px; width:100%; font-size:12px; }
  .ai-perf-attention-row { display:grid; grid-template-columns:1fr auto auto; gap:8px; align-items:center; padding:5px 8px; border-radius:6px; background:color-mix(in srgb, currentColor 5%, transparent); border:1px solid color-mix(in srgb, currentColor 12%, transparent); }
  .ai-perf-ack { font-size:12px; line-height:1.4; padding:2px 8px; border-radius:4px; border:1px solid color-mix(in srgb, currentColor 30%, transparent); background:transparent; color:currentColor; cursor:pointer; }
  .ai-perf-ack:focus-visible { outline:2px solid currentColor; outline-offset:2px; }

  /* Limits & Budgets */
  .ai-perf-limits { display:flex; flex-direction:column; gap:8px; width:100%; font-size:12px; }
  .ai-perf-budget-row { display:flex; flex-direction:column; gap:4px; padding:6px 8px; border-radius:6px; background:color-mix(in srgb, currentColor 5%, transparent); border:1px solid color-mix(in srgb, currentColor 12%, transparent); }
  .ai-perf-budget-head { display:flex; justify-content:space-between; align-items:center; }
  .ai-perf-budget-period { font-size:12px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); }
  .ai-perf-budget-val { font-variant-numeric:tabular-nums; font-weight:600; }
  .ai-perf-budget-meta { display:flex; justify-content:space-between; font-size:12px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); }
  .ai-perf-fill-ok { background:currentColor; }
  .ai-perf-fill-warn { background:var(--space-widget-warning, var(--warning, #e6c07b)); }
  .ai-perf-fill-danger { background:var(--space-widget-danger, var(--danger, #ff7f8f)); }

  /* Ledger */
  .ai-perf-ledger { display:flex; flex-direction:column; gap:4px; width:100%; font-size:12px; }
  .ai-perf-ledger-row { display:grid; grid-template-columns:45px 1fr auto auto 60px; gap:6px; align-items:center; padding:3px 0; border-bottom:1px solid color-mix(in srgb, currentColor 12%, transparent); }
  .ai-perf-ledger-time { color:var(--space-widget-text-secondary, inherit); font-variant-numeric:tabular-nums; }
  .ai-perf-ledger-model { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:currentColor; font-weight:500; }
  .ai-perf-ledger-tokens { color:var(--space-widget-text-secondary, inherit); font-variant-numeric:tabular-nums; }
  .ai-perf-ledger-cost { font-weight:600; font-variant-numeric:tabular-nums; color:currentColor; }
  .ai-perf-ledger-atom { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--space-widget-text-secondary, inherit); font-size:11px; text-align:right; }

  /* Savings */
  .ai-perf-savings { display:flex; flex-direction:column; gap:8px; width:100%; font-size:12px; }
  .ai-perf-insight-card { padding:8px; border-radius:6px; border:1px solid color-mix(in srgb, currentColor 16%, transparent); background:color-mix(in srgb, currentColor 5%, transparent); display:flex; flex-direction:column; gap:4px; }
  .ai-perf-insight-head { display:flex; align-items:center; gap:6px; }
  .ai-perf-badge { font-size:11px; line-height:1.4; padding:1px 5px; border-radius:3px; font-weight:600; }
  .ai-perf-badge-warn { background:color-mix(in srgb, var(--space-widget-warning, var(--warning, #e6c07b)) 20%, transparent); color:var(--space-widget-warning, var(--warning, #e6c07b)); }
  .ai-perf-badge-info { background:color-mix(in srgb, currentColor 18%, transparent); color:currentColor; }
  .ai-perf-insight-evidence { font-size:12px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); }
  .ai-perf-insight-action { font-size:12px; line-height:1.4; color:currentColor; font-weight:500; }
  .ai-perf-insight-foot { display:flex; justify-content:space-between; align-items:center; gap:8px; }

  .ai-perf-empty { padding:12px; border:1px dashed color-mix(in srgb, currentColor 30%, transparent); border-radius:6px; color:var(--space-widget-text-secondary, inherit); font-size:12px; text-align:center; }
  .ai-perf-error { padding:10px 12px; border:1px solid var(--space-widget-danger, var(--danger, #ff7f8f)); border-radius:6px; color:var(--space-widget-danger, var(--danger, #ff7f8f)); font-size:12px; }
  .ai-perf-status { font-size:12px; line-height:1.4; color:var(--space-widget-text-secondary, inherit); min-height:14px; }
  .ai-perf.is-loading .ai-perf-status { opacity:.7; }
  /* R2：AI 图表不继承公共 .Widgets svg 的装饰性 drop-shadow。 */
  .ai-perf svg { filter:none; }
`;

function normalizeConfig(config = {}) {
  const metricId = METRICS[config.metric] ? config.metric : DEFAULT_METRIC;
  const chartId = CHARTS[config.chart] ? config.chart : DEFAULT_CHART;
  const range = RANGES.some(([id]) => id === config.range) ? config.range : '30d';
  const dimension = DIMENSION_OPTIONS.some(([id]) => id === config.dimension) ? config.dimension : 'model';
  const subMetric = config.subMetric || 'total';
  // toolIds：显式工具范围多选；空/缺失 = 全部已接入来源。仅保留合法字符串。
  const toolIds = Array.isArray(config.toolIds)
    ? config.toolIds.filter((id) => typeof id === 'string' && id)
    : [];
  // 周期：custom 携带经校验的起止时间（ISO/UTC）；无效则置空（查询层缺省）。
  const { startTime, endTime } = range === 'custom'
    ? normalizeCustomTimes(config.startTime, config.endTime)
    : { startTime: '', endTime: '' };
  return { metric: metricId, chart: chartId, range, dimension, subMetric, toolIds, startTime, endTime, view: normalizeView(config.view) };
}

export const aiPerformanceWidget = {
  key: 'widget/aiPerformance',
  name: 'AI Performance',
  description: 'AI 用量与效能统计卡片',
  defaultData: {
    metric: DEFAULT_METRIC,
    chart: DEFAULT_CHART,
    range: '30d',
    dimension: 'model',
    subMetric: 'total',
  },
  styles: WIDGET_STYLES,

  render(container, data = {}, display = {}, { t = (k, f) => f || k, shadowRoot, openDrilldown: envOpenDrilldown } = {}) {
    const config = normalizeConfig(data);
    container.className = 'Widget AiPerformance';
    container.replaceChildren();

    const metric = METRICS[config.metric];
    const chart = CHARTS[config.chart];
    const ui = {
      t,
      shadowRoot,
      label: t(metric.labelKey, metric.fallback),
      // R2（§5.4）：表格列由当前指标决定，消除 cost/token 列串线。
      metricId: metric.id,
      fmtValue: metric.fmt.value,
      fmtRank: metric.fmt.rank,
      // R7（§5.1 第 5 条）：点击金额/行进入带原筛选的用量详情。
      range: config.range,
      // 自定义周期：起止随钻取透传（§4.2 周期参数一致性）。
      startTime: config.startTime,
      endTime: config.endTime,
      dimension: config.dimension,
      openDrilldown: typeof envOpenDrilldown === 'function' ? envOpenDrilldown : null,
    };

    const root = document.createElement('div');
    root.className = 'ai-perf is-loading';
    root.innerHTML = `<div class="ai-perf-status">${escapeHtml(t('aiPerfLoading', '加载用量数据…'))}</div>`;
    container.append(root);

    const api = acquireModelApi();
    let cancelled = false;

    // T7：查询经共享层合并（同参并发只发 1 次）+ revision 缓存；
    // Host 推送 model_usage_updated 后自动补查一次。
    const sharedApi = new Proxy(api, {
      get(target, prop) {
        if (prop === 'getUsageOverview' || prop === 'getUsageAnalysis' || prop === 'getUsageEvents') {
          return (params) => sharedCall(target, prop, params);
        }
        return target[prop];
      },
    });
    const unsubscribe = subscribeModelEvents((evt) => {
      if (cancelled) return;
      if (evt?.type === 'model_usage_updated' || evt?.method === 'model_usage_updated') {
        // R3（§3.2）：非 usage 视图由 views.js 接管刷新，事件回调不得把
        // 内容换回旧指标的 usage 渲染。
        if (normalizeView(config.view) === 'usage') {
          load();
        }
      }
    });

    // T7b：view≠usage 的固定视图由 views.js 接管（tools/billing/attention/…）。
    const view = normalizeView(config.view);
    if (view !== 'usage') {
      root.classList.remove('is-loading');
      root.querySelector('.ai-perf-status')?.remove();
      // render 保持同步契约：异步渲染用 then/catch，不 await。
      renderView(view, root, sharedApi, t, { aborted: false }).catch((err) => {
        if (cancelled) return;
        root.innerHTML = `<div class="ai-perf-error">${escapeHtml(t('aiPerfLoadFailed', '用量数据加载失败'))}: ${escapeHtml(err?.message || String(err))}</div>`;
      });
      return () => {
        cancelled = true;
        unsubscribe();
        releaseModelApi();
        container.replaceChildren();
      };
    }

    async function load() {
      root.classList.add('is-loading');
      try {
        const dataset = await fetchDataset(sharedApi, metric, config.range, CHART_NEEDS[config.chart], {
          dimension: config.dimension,
          subMetric: config.subMetric,
          toolIds: config.toolIds,
        });
        if (cancelled) return;
        if (dataset.empty) {
          root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t('aiPerfEmpty', '当前统计范围暂无 AI 用量记录'))}</div>`;
          return;
        }
        root.classList.remove('is-loading');
        root.querySelector('.ai-perf-status')?.remove();
        chart.render(root, dataset, ui);
      } catch (err) {
        if (cancelled) return;
        root.classList.remove('is-loading');
        root.innerHTML = `<div class="ai-perf-error">${escapeHtml(t('aiPerfLoadFailed', '用量数据加载失败'))}: ${escapeHtml(err?.message || String(err))}</div>`;
      }
    }

    load();

    return () => {
      cancelled = true;
      unsubscribe();
      releaseModelApi();
      container.replaceChildren();
    };
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    const config = normalizeConfig(data);
    container.replaceChildren();

    const currentMetric = METRICS[config.metric];
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';

    const subMetricSelect =
      currentMetric?.subMetrics && currentMetric.subMetrics.length > 1
        ? `
        <label class="inspector-field">
          <span>${escapeHtml(t('aiPerfSettingSubMetric', '子指标 / 口径'))}</span>
          <select data-role="ai-perf-submetric">
            ${currentMetric.subMetrics
              .map(
                (sm) =>
                  `<option value="${sm.id}" ${sm.id === config.subMetric ? 'selected' : ''}>${escapeHtml(
                    t(sm.labelKey, sm.fallback)
                  )}</option>`
              )
              .join('')}
          </select>
        </label>
      `
        : '';

    const dimensionSelect =
      config.chart === 'bar' || config.chart === 'table'
        ? `
        <label class="inspector-field">
          <span>${escapeHtml(t('aiPerfSettingDimension', '分析维度'))}</span>
          <select data-role="ai-perf-dimension">
            ${DIMENSION_OPTIONS.map(
              ([id, key, fallback]) =>
                `<option value="${id}" ${id === config.dimension ? 'selected' : ''}>${escapeHtml(
                  t(key, fallback)
                )}</option>`
            ).join('')}
          </select>
        </label>
      `
        : '';

    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${escapeHtml(t('aiPerfSettingView', '视图模式'))}</span>
        <select data-role="ai-perf-view">
          ${VIEW_OPTIONS.map(
            ([id, key, fallback]) =>
              `<option value="${id}" ${id === config.view ? 'selected' : ''}>${escapeHtml(
                t(key, fallback)
              )}</option>`
          ).join('')}
        </select>
      </label>
      <label class="inspector-field">
        <span>${escapeHtml(t('aiPerfSettingMetric', '核心指标'))}</span>
        <select data-role="ai-perf-metric">
          ${Object.values(METRICS)
            .map(
              (m) =>
                `<option value="${m.id}" ${m.id === config.metric ? 'selected' : ''}>${escapeHtml(
                  t(m.labelKey, m.fallback)
                )}</option>`
            )
            .join('')}
        </select>
      </label>
      ${subMetricSelect}
      <label class="inspector-field">
        <span>${escapeHtml(t('aiPerfSettingChart', '可视化形态'))}</span>
        <select data-role="ai-perf-chart">
          ${CHART_OPTIONS.map(
            ([id, key, fallback]) =>
              `<option value="${id}" ${id === config.chart ? 'selected' : ''}>${escapeHtml(
                t(key, fallback)
              )}</option>`
          ).join('')}
        </select>
      </label>
      ${dimensionSelect}
      <label class="inspector-field">
        <span>${escapeHtml(t('aiPerfSettingRange', '统计范围'))}</span>
        <select data-role="ai-perf-range">
          ${RANGES.map(
            ([id, key, fallback]) =>
              `<option value="${id}" ${id === config.range ? 'selected' : ''}>${escapeHtml(
                t(key, fallback)
              )}</option>`
          ).join('')}
        </select>
      </label>
      ${config.range === 'custom' ? customTimesInputs(config, t) : ''}
      <p class="inspector-hint">${escapeHtml(
        t('aiPerfSettingHint', '数据来自 Model Host 用量统计，可在设置 › 数据与用量 中导入历史记录')
      )}</p>
    `;

    wrap.querySelector('[data-role="ai-perf-view"]').onchange = (e) =>
      onChange({ ...config, view: e.target.value });

    wrap.querySelector('[data-role="ai-perf-metric"]').onchange = (e) =>
      onChange({ ...config, metric: e.target.value, subMetric: 'total' });

    const subEl = wrap.querySelector('[data-role="ai-perf-submetric"]');
    if (subEl) {
      subEl.onchange = (e) => onChange({ ...config, subMetric: e.target.value });
    }

    wrap.querySelector('[data-role="ai-perf-chart"]').onchange = (e) =>
      onChange({ ...config, chart: e.target.value });

    const dimEl = wrap.querySelector('[data-role="ai-perf-dimension"]');
    if (dimEl) {
      dimEl.onchange = (e) => onChange({ ...config, dimension: e.target.value });
    }

    wrap.querySelector('[data-role="ai-perf-range"]').onchange = (e) => {
      const next = { ...config, range: e.target.value };
      if (e.target.value === 'custom') {
        const times = normalizeCustomTimes(next.startTime, next.endTime);
        if (!times.startTime) {
          const end = Date.now();
          next.startTime = new Date(end - 7 * 86400000).toISOString();
          next.endTime = new Date(end).toISOString();
        }
      }
      onChange(next);
    };
    const startEl = wrap.querySelector('[data-role="ai-perf-start"]');
    const endEl = wrap.querySelector('[data-role="ai-perf-end"]');
    if (startEl) startEl.onchange = (e) => onChange({ ...config, ...normalizeCustomTimes(new Date(e.target.value).toISOString(), config.endTime) });
    if (endEl) endEl.onchange = (e) => onChange({ ...config, ...normalizeCustomTimes(config.startTime, new Date(e.target.value).toISOString()) });

    container.append(wrap);
  },
};

// ---------------------------------------------------------------------------
// R1（方案 §4.1/§8.1）：七个独立目录组件。
//
// 每项是独立注册 key（独立添加/多实例/设置/删除），共享同一 renderer、
// 查询层与样式；差别只在锁定的 metric/view、允许的图表形态与默认配置。
// 旧 widget/aiPerformance 保留为兼容 renderer（仍在白名单，从目录隐藏）。

function renderLockedSettings(def, container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
  container.replaceChildren();
  const wrap = document.createElement('div');
  wrap.className = 'inspector-field-group';

  const metric = def.metric ? METRICS[def.metric] : null;
  const config = { ...def.defaultData, ...data };
  const allowedCharts = (def.allowedCharts || []).filter((id) => CHARTS[id]);
  const subMetrics = metric?.subMetrics || [];

  const subMetricSelect =
    subMetrics.length > 1
      ? `
      <label class="inspector-field">
        <span>${escapeHtml(t('aiPerfSettingSubMetric', '子指标 / 口径'))}</span>
        <select data-role="ai-perf-submetric">
          ${subMetrics
            .map(
              (sm) =>
                `<option value="${sm.id}" ${sm.id === config.subMetric ? 'selected' : ''}>${escapeHtml(
                  t(sm.labelKey, sm.fallback)
                )}</option>`
            )
            .join('')}
        </select>
      </label>
    `
      : '';

  const chartSelect =
    allowedCharts.length > 1
      ? `
      <label class="inspector-field">
        <span>${escapeHtml(t('aiPerfSettingChart', '可视化形态'))}</span>
        <select data-role="ai-perf-chart">
          ${CHART_OPTIONS.filter(([id]) => allowedCharts.includes(id))
            .map(
              ([id, key, fallback]) =>
                `<option value="${id}" ${id === config.chart ? 'selected' : ''}>${escapeHtml(
                  t(key, fallback)
                )}</option>`
            )
            .join('')}
        </select>
      </label>
    `
      : '';

  // 用量类锁定组件（有 metric）与旧总览一样提供周期选择；
  // limits/attention/savings 视图无时间窗语义，保持隐藏。
  const rangeSelect = def.metric
    ? `
      <label class="inspector-field">
        <span>${escapeHtml(t('aiPerfSettingRange', '统计范围'))}</span>
        <select data-role="ai-perf-range">
          ${RANGES.map(
            ([id, key, fallback]) =>
              `<option value="${id}" ${id === config.range ? 'selected' : ''}>${escapeHtml(
                t(key, fallback)
              )}</option>`
          ).join('')}
        </select>
      </label>
      ${config.range === 'custom' ? customTimesInputs(config, t) : ''}
    `
    : '';

  wrap.innerHTML = `${chartSelect}${subMetricSelect}${rangeSelect}
    <p class="inspector-hint">${escapeHtml(
      t('aiPerfSettingHint', '数据来自 Model Host 用量统计，可在设置 › 数据与用量 中导入历史记录')
    )}</p>
  `;

  const chartEl = wrap.querySelector('[data-role="ai-perf-chart"]');
  if (chartEl) chartEl.onchange = (e) => onChange({ ...config, chart: e.target.value });
  const subEl = wrap.querySelector('[data-role="ai-perf-submetric"]');
  if (subEl) subEl.onchange = (e) => onChange({ ...config, subMetric: e.target.value });
  const rangeEl = wrap.querySelector('[data-role="ai-perf-range"]');
  if (rangeEl) {
    rangeEl.onchange = (e) => {
      const next = { ...config, range: e.target.value };
      if (e.target.value === 'custom') {
        const times = normalizeCustomTimes(next.startTime, next.endTime);
        if (!times.startTime) {
          const end = Date.now();
          next.startTime = new Date(end - 7 * 86400000).toISOString();
          next.endTime = new Date(end).toISOString();
        }
      }
      onChange(next);
    };
  }
  const startEl = wrap.querySelector('[data-role="ai-perf-start"]');
  const endEl = wrap.querySelector('[data-role="ai-perf-end"]');
  if (startEl) startEl.onchange = (e) => onChange({ ...config, ...normalizeCustomTimes(new Date(e.target.value).toISOString(), config.endTime) });
  if (endEl) endEl.onchange = (e) => onChange({ ...config, ...normalizeCustomTimes(config.startTime, new Date(e.target.value).toISOString()) });

  container.append(wrap);
}

function defineAiComponent(def) {
  return {
    key: def.key,
    name: def.name,
    description: def.description,
    defaultData: { ...def.defaultData },
    styles: WIDGET_STYLES,
    render(container, data = {}, display = {}, env = {}) {
      const merged = { ...def.defaultData, ...data };
      if (def.metric) merged.metric = def.metric;
      if (def.view) merged.view = def.view;
      return aiPerformanceWidget.render(container, merged, display, env);
    },
    renderSettings(container, data = {}, onChange = () => {}, env = {}) {
      renderLockedSettings(def, container, data, onChange, env);
    },
  };
}

export const aiComponentDefinitions = {
  aiCost: defineAiComponent({
    key: 'widget/aiCost',
    name: 'AI Cost',
    description: 'AI 成本：模型用量估算金额与计费口径',
    metric: 'ai_cost',
    view: 'usage',
    allowedCharts: ['number', 'line', 'bar', 'heatmap', 'table'],
    defaultData: { metric: 'ai_cost', view: 'usage', chart: 'number', range: '30d', dimension: 'model', subMetric: 'total' },
  }),
  aiTokens: defineAiComponent({
    key: 'widget/aiTokens',
    name: 'Token Usage',
    description: 'Token 用量：输入/输出/总量',
    metric: 'token_usage',
    view: 'usage',
    allowedCharts: ['number', 'line', 'bar', 'heatmap', 'table'],
    defaultData: { metric: 'token_usage', view: 'usage', chart: 'number', range: '30d', dimension: 'model', subMetric: 'total' },
  }),
  aiSessions: defineAiComponent({
    key: 'widget/aiSessions',
    name: 'AI Sessions',
    description: '会话活跃：真实会话聚合（按工具与实例三元去重）',
    metric: 'ai_sessions',
    view: 'usage',
    allowedCharts: ['number', 'heatmap', 'line', 'table', 'timeline'],
    defaultData: { metric: 'ai_sessions', view: 'usage', chart: 'heatmap', range: '30d', dimension: 'source', subMetric: 'total' },
  }),
  aiRequests: defineAiComponent({
    key: 'widget/aiRequests',
    name: 'Request Stats',
    description: '调用统计：请求数与成功率',
    metric: 'request_count',
    view: 'usage',
    allowedCharts: ['number', 'line', 'bar', 'heatmap', 'timeline', 'table'],
    defaultData: { metric: 'request_count', view: 'usage', chart: 'number', range: '30d', dimension: 'source', subMetric: 'total' },
  }),
  aiLimits: defineAiComponent({
    key: 'widget/aiLimits',
    name: 'Quota & Budget',
    description: '额度与预算：账户窗口与预算阈值',
    view: 'limits',
    defaultData: { view: 'limits' },
  }),
  aiAttention: defineAiComponent({
    key: 'widget/aiAttention',
    name: 'Needs Attention',
    description: '任务提醒：等待许可/输入、错误与轮次结束',
    view: 'attention',
    defaultData: { view: 'attention' },
  }),
  aiSavings: defineAiComponent({
    key: 'widget/aiSavings',
    name: 'Savings Insights',
    description: '降本建议：有证据的节省动作',
    view: 'savings',
    defaultData: { view: 'savings' },
  }),
};
