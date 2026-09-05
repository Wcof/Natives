import { renderPricingTab } from './model-pricing-view.js';

export function renderUsageView(container, {
  activeSubTab = 'overview',
  overviewData,
  analyticsData,
  eventsData,
  pricingData,
  currentFilter = { range: '4h' },
  filterOptions,
  t,
  onAction,
}) {
  container.replaceChildren();

  // Subnav header
  const subnav = document.createElement('div');
  subnav.className = 'model-subnav-bar';
  subnav.innerHTML = `
    <div class="model-subnav-tabs">
      <button type="button" class="model-subnav-tab ${activeSubTab === 'overview' ? 'active' : ''}" data-action="select-usage-tab" data-tab="overview">${escapeText(t('modelUsageOverview', '总览'))}</button>
      <button type="button" class="model-subnav-tab ${activeSubTab === 'analytics' ? 'active' : ''}" data-action="select-usage-tab" data-tab="analytics">${escapeText(t('modelUsageAnalytics', '透视分析'))}</button>
      <button type="button" class="model-subnav-tab ${activeSubTab === 'events' ? 'active' : ''}" data-action="select-usage-tab" data-tab="events">${escapeText(t('modelUsageEvents', '明细记录'))}</button>
      <button type="button" class="model-subnav-tab ${activeSubTab === 'pricing' ? 'active' : ''}" data-action="select-usage-tab" data-tab="pricing">${escapeText(t('modelUsagePricing', '价格与费率'))}</button>
    </div>
  `;
  container.append(subnav);

  const filters = document.createElement('div');
  filters.className = 'model-filter-bar model-filter-bar-range';
  filters.innerHTML = `<div class="model-filter-group">${[
    ['4h', t('modelRange4h', '近 4 小时')], ['24h', t('modelRange24h', '近 24 小时')],
    ['today', t('modelRangeToday', '今日')], ['7d', t('modelRange7d', '近 7 天')],
    ['30d', t('modelRange30d', '近 30 天')], ['all', t('modelRangeAll', '全部')],
  ].map(([range, label]) => `<button type="button" class="model-filter-btn ${currentFilter.range === range ? 'active' : ''}" data-action="filter-usage-range" data-range="${range}">${escapeText(label)}</button>`).join('')}</div>
    <button type="button" class="model-filter-import-btn" data-action="pick-import-file">${escapeText(t('modelImportHistory', '导入历史记录'))}</button>
    <input type="file" accept=".db,.sqlite" data-role="importer-file-input" hidden>`;
  container.append(filters);

  const dimensions = document.createElement('div');
  dimensions.className = 'model-filter-bar model-filter-bar-dimensions';
  dimensions.innerHTML = [
    selectFilter('model', t('modelAllModels', '全部模型'), filterOptions?.byModel, currentFilter.model),
    selectFilter('provider', t('modelAllProviders', '全部供应商'), filterOptions?.byProvider, currentFilter.provider),
    selectFilter('source', t('modelAllSources', '全部来源'), filterOptions?.bySource, currentFilter.source),
    selectFilter('accessKeyId', t('modelAllKeys', '全部密钥'), filterOptions?.byAccessKey, currentFilter.accessKeyId),
    `<select data-action="filter-usage-dimension" data-filter="result"><option value="">${escapeText(t('modelAllResults', '全部结果'))}</option><option value="success" ${currentFilter.result === 'success' ? 'selected' : ''}>${escapeText(t('modelSuccess', '成功'))}</option><option value="failed" ${currentFilter.result === 'failed' ? 'selected' : ''}>${escapeText(t('modelFailed', '失败'))}</option><option value="cancelled" ${currentFilter.result === 'cancelled' ? 'selected' : ''}>${escapeText(t('modelCancelled', '已取消'))}</option></select>`,
    `<button type="button" class="model-filter-custom-btn" data-action="show-custom-range">${escapeText(t('modelCustomRange', '自定义时间'))}</button>`,
  ].join('');
  container.append(dimensions);

  const customRange = document.createElement('form');
  customRange.className = 'model-filter-bar model-filter-bar-custom';
  customRange.dataset.role = 'usage-custom-range-form';
  customRange.hidden = currentFilter.range !== 'custom';
  customRange.innerHTML = `<label>${escapeText(t('modelStartTime', '开始时间'))}<input type="datetime-local" name="startTime" required></label><label>${escapeText(t('modelEndTime', '结束时间'))}<input type="datetime-local" name="endTime" required></label><button class="primary" type="submit">${escapeText(t('apply', '应用'))}</button>`;
  container.append(customRange);

  const importer = document.createElement('div');
  importer.innerHTML = `<div class="model-importer-progress" data-role="importer-progress" hidden><p>${escapeText(t('modelUploading', '正在上传并校验…'))}</p><div class="model-progress-bar-bg"><div class="model-progress-bar-fill" data-role="progress-fill" style="width:0%"></div></div></div><div class="model-importer-preview" data-role="importer-preview" hidden></div>`;
  container.append(importer);

  const content = document.createElement('div');
  content.className = 'model-usage-content';

  if (activeSubTab === 'overview') {
    renderOverviewTab(content, overviewData, currentFilter, t);
  } else if (activeSubTab === 'analytics') {
    renderAnalyticsTab(content, analyticsData, currentFilter, t);
  } else if (activeSubTab === 'events') {
    renderEventsTab(content, eventsData, currentFilter, t);
  } else if (activeSubTab === 'pricing') {
    renderPricingTab(content, pricingData, t);
  }

  container.append(content);
}

function renderOverviewTab(container, data, filter, t) {
  if (!data || data.status === 'empty') {
    container.append(emptyState(t('modelNoUsageRecords', '暂无使用记录'), t('modelNoUsageHint', '启动代理并使用 API 调用后即可查看调用统计。')));
    return;
  }

  const metrics = data.metrics || {};
  const tokens = data.tokens || {};

  // 6 Metric Cards
  const cardsGrid = document.createElement('div');
  cardsGrid.className = 'model-metric-grid';
  cardsGrid.innerHTML = `
    <div class="model-metric-card">
      <span class="muted">${escapeText(t('modelMetricTotalRequests', '总调用次数'))}</span>
      <strong>${formatNumber(metrics.totalRequests || 0)}</strong>
    </div>
    <div class="model-metric-card">
      <span class="muted">${escapeText(t('modelMetricTotalTokens', 'Token 消耗总量'))}</span>
      <strong>${formatNumber(metrics.totalTokens || 0)}</strong>
    </div>
    <div class="model-metric-card">
      <span class="muted">${escapeText(t('modelMetricSuccessRate', '请求成功率'))}</span>
      <strong class="${metrics.successRate >= 95 ? 'text-success' : 'text-warning'}">${(metrics.successRate || 0).toFixed(1)}%</strong>
    </div>
    <div class="model-metric-card">
      <span class="muted">${escapeText(t('modelMetricTPS', '平均吞吐量 (TPS)'))}</span>
      <strong>${(metrics.tps || 0).toFixed(1)} <small class="muted">tok/s</small></strong>
    </div>
    <div class="model-metric-card">
      <span class="muted">${escapeText(t('modelMetricCacheHitRate', 'Prompt 缓存命中率'))}</span>
      <strong>${(metrics.cacheHitRate || 0).toFixed(1)}%</strong>
    </div>
    <div class="model-metric-card">
      <span class="muted">${escapeText(t('modelMetricEstimatedCost', '估算总费用'))}</span>
      <strong class="text-primary">$${(metrics.estimatedCostUsd || 0).toFixed(4)}</strong>
    </div>
  `;
  container.append(cardsGrid);

  // Token Composition
  const tokenSection = document.createElement('div');
  tokenSection.className = 'model-usage-section';
  tokenSection.innerHTML = `
    <h4>${escapeText(t('modelTokenComposition', 'Token 构成分析'))}</h4>
    <div class="model-token-composition-grid">
      <div class="model-token-item">
        <span class="muted">${escapeText(t('modelTokenInput', '输入 Token'))}</span>
        <strong>${formatNumber(tokens.input || 0)}</strong>
      </div>
      <div class="model-token-item">
        <span class="muted">${escapeText(t('modelTokenOutput', '输出 Token'))}</span>
        <strong>${formatNumber(tokens.output || 0)}</strong>
      </div>
      <div class="model-token-item">
        <span class="muted">${escapeText(t('modelTokenCacheRead', '缓存读取'))}</span>
        <strong>${formatNumber(tokens.cacheRead || 0)}</strong>
      </div>
      <div class="model-token-item">
        <span class="muted">${escapeText(t('modelTokenCacheWrite', '缓存写入'))}</span>
        <strong>${formatNumber(tokens.cacheWrite || 0)}</strong>
      </div>
      <div class="model-token-item">
        <span class="muted">${escapeText(t('modelTokenReasoning', '思考 Token'))}</span>
        <strong>${formatNumber(tokens.reasoning || 0)}</strong>
      </div>
    </div>
  `;
  container.append(tokenSection);

  // Trend summary
  if (data.trend && data.trend.length > 0) {
    const trendSection = document.createElement('div');
    trendSection.className = 'model-usage-section';
    trendSection.innerHTML = `
      <h4>${escapeText(t('modelTrendBreakdown', '时段调用趋势'))}</h4>
      <div class="model-trend-list">
        ${data.trend.slice(-12).map(p => `
          <div class="model-trend-row">
            <span class="muted">${escapeText(p.timestamp.replace('T', ' ').replace('Z', ''))}</span>
            <span>${formatNumber(p.requests)} reqs</span>
            <span>${formatNumber(p.tokens)} tokens</span>
            <span class="text-primary">$${p.costUsd.toFixed(4)}</span>
          </div>
        `).join('')}
      </div>
    `;
    container.append(trendSection);
  }
}

function renderAnalyticsTab(container, data, filter, t) {
  if (!data || (!data.byModel?.length && !data.byProvider?.length)) {
    container.append(emptyState(t('modelNoAnalyticsData', '暂无分析数据'), t('modelNoUsageHint', '产生调用记录后将在此展示透视分析。')));
    return;
  }

  const grid = document.createElement('div');
  grid.className = 'model-analytics-grid';

  grid.append(createRankCard(t('modelRankByModel', '模型调用排行'), data.byModel, t));
  grid.append(createRankCard(t('modelRankByProvider', '供应商分布'), data.byProvider, t));
  grid.append(createRankCard(t('modelRankByAccessKey', '访问密钥分布'), data.byAccessKey, t));
  grid.append(createRankCard(t('modelRankByHour', '24小时活跃时段'), data.byHour, t));

  container.append(grid);
}

function createRankCard(title, list = [], t) {
  const card = document.createElement('div');
  card.className = 'model-rank-card';
  card.innerHTML = `
    <h4>${escapeText(title)}</h4>
    <div class="model-rank-list">
      ${list.length === 0 ? `<p class="muted">${escapeText(t('modelNoData', '暂无数据'))}</p>` : list.slice(0, 10).map(item => `
        <div class="model-rank-row">
          <div class="model-rank-info">
            <span class="model-rank-name" title="${escapeText(item.name || item.key)}">${escapeText(item.name || item.key)}</span>
            <span class="model-rank-val">${formatNumber(item.requests)} reqs (${(item.percent || 0).toFixed(1)}%)</span>
          </div>
          <div class="model-rank-bar-bg">
            <div class="model-rank-bar-fill" style="width: ${Math.min(100, Math.max(2, item.percent || 0))}%"></div>
          </div>
        </div>
      `).join('')}
    </div>
  `;
  return card;
}

function renderEventsTab(container, data, filter, t) {
  if (!data || !data.events || data.events.length === 0) {
    container.append(emptyState(t('modelNoEventsFound', '未找到匹配的调用记录'), t('modelNoEventsHint', '尝试调整筛选条件或发起新的 API 请求。')));
    return;
  }

  const tableWrapper = document.createElement('div');
  tableWrapper.className = 'model-table-wrapper';
  tableWrapper.innerHTML = `
    <table class="model-events-table">
      <thead>
        <tr>
          <th class="model-col-time align-center">${escapeText(t('modelEventTime', '时间'))}</th>
          <th class="model-col-model">${escapeText(t('modelEventModel', '模型'))}</th>
          <th class="model-col-input align-center">${escapeText(t('modelEventInput', '输入'))}</th>
          <th class="model-col-output align-center">${escapeText(t('modelEventOutput', '输出'))}</th>
          <th class="model-col-cache align-center">${escapeText(t('modelEventCache', '缓存'))}</th>
          <th class="model-col-cache-rate align-center">${escapeText(t('modelEventCacheRate', '缓存率'))}</th>
          <th class="model-col-total align-center">${escapeText(t('modelEventTotal', '总计'))}</th>
          <th class="model-col-speed align-center">${escapeText(t('modelEventSpeed', '生成速度'))}</th>
          <th class="model-col-ttft align-center">${escapeText(t('modelEventTtft', '首字延迟'))}</th>
          <th class="model-col-latency align-center">${escapeText(t('modelEventLatency', '总耗时'))}</th>
          <th class="model-col-cost align-center">${escapeText(t('modelEventCost', '费用'))}</th>
          <th class="model-col-status align-center">${escapeText(t('modelEventStatus', '状态'))}</th>
        </tr>
      </thead>
      <tbody>
        ${data.events.map(e => `
          <tr>
            <td class="model-col-time align-center"><small class="muted">${formatTime(e.requestedAt)}</small></td>
            <td class="model-col-model" title="${escapeText(e.model || '')}">
              <strong class="model-name-text">${escapeText(e.model || '—')}</strong>
              ${e.provider ? `<span class="usage-tag-pill">${escapeText(e.provider)}</span>` : ''}
            </td>
            <td class="model-col-input align-center" title="${(e.inputTokens || 0).toLocaleString()} tokens">
              ${compactNumber(e.inputTokens || 0)}
            </td>
            <td class="model-col-output align-center" title="${(e.outputTokens || 0).toLocaleString()} tokens">
              ${compactNumber(e.outputTokens || 0)}
            </td>
            <td class="model-col-cache align-center" title="Read: ${(e.cacheReadTokens || 0).toLocaleString()} tokens${e.cacheWriteTokens > 0 ? ` / Creation: ${(e.cacheWriteTokens || 0).toLocaleString()} tokens` : ''}">
              ${compactNumber(e.cacheReadTokens || 0)}
            </td>
            <td class="model-col-cache-rate align-center">
              ${formatCacheRate(e.inputTokens, e.cacheReadTokens)}
            </td>
            <td class="model-col-total align-center" title="${(e.totalTokens || 0).toLocaleString()} tokens">
              <strong>${compactNumber(e.totalTokens || 0)}</strong>
            </td>
            <td class="model-col-speed align-center">
              ${formatSpeed(e.outputTokens, e.latencyMs, e.ttftMs)}
            </td>
            <td class="model-col-ttft align-center" title="${e.ttftMs > 0 ? `${e.ttftMs} ms` : ''}">
              ${e.ttftMs > 0 ? `${compactNumber(e.ttftMs)} ms` : '—'}
            </td>
            <td class="model-col-latency align-center" title="${e.latencyMs || 0} ms">
              ${compactNumber(e.latencyMs || 0)} ms
            </td>
            <td class="model-col-cost align-center">
              <span class="text-primary">$${((e.costMicro || 0) / 1000000.0).toFixed(4)}</span>
            </td>
            <td class="model-col-status align-center">
              <span class="model-status-badge ${e.result === 'success' ? 'status-ok' : 'status-err'}">
                ${e.httpStatus || (e.result === 'success' ? '200' : 'ERR')}
              </span>
            </td>
          </tr>
        `).join('')}
      </tbody>
    </table>
  `;
  container.append(tableWrapper);

  // Pagination bar
  const pageBar = document.createElement('div');
  pageBar.className = 'model-pagination-bar';
  pageBar.innerHTML = `
    <span class="muted">${escapeText(t('modelTotalRecords', '共 $1 条记录').replace('$1', data.total))} · 第 ${data.page} 页</span>
    <div class="model-pagination-actions">
      <button type="button" data-action="prev-events-page" ${data.page <= 1 ? 'disabled' : ''}>← ${escapeText(t('prevPage', '上一页'))}</button>
      <button type="button" data-action="next-events-page" ${!data.hasMore ? 'disabled' : ''}>${escapeText(t('nextPage', '下一页'))} →</button>
    </div>
  `;
  container.append(pageBar);
}

function selectFilter(name, label, items = [], selected = '') {
  return `<select data-action="filter-usage-dimension" data-filter="${name}"><option value="">${escapeText(label)}</option>${items.map((item) => `<option value="${escapeText(item.key)}" ${item.key === selected ? 'selected' : ''}>${escapeText(item.name || item.key)}</option>`).join('')}</select>`;
}

function emptyState(title, hint) {
  const node = document.createElement('div');
  node.className = 'model-empty-state';
  node.innerHTML = `<strong>${escapeText(title)}</strong><p class="muted">${escapeText(hint)}</p>`;
  return node;
}

function formatNumber(val) {
  return Number(val || 0).toLocaleString();
}

function compactNumber(val) {
  const num = Number(val) || 0;
  const abs = Math.abs(num);
  if (abs >= 1_000_000_000) {
    return `${(num / 1_000_000_000).toFixed(1)}B`;
  }
  if (abs >= 1_000_000) {
    return `${(num / 1_000_000).toFixed(1)}M`;
  }
  return num.toLocaleString();
}

function formatCacheRate(inputTokens, cacheReadTokens) {
  const input = Number(inputTokens) || 0;
  const cache = Math.max(0, Number(cacheReadTokens) || 0);
  if (input <= 0) return '—';
  const rate = (Math.min(cache, input) / input) * 100;
  return `${rate.toFixed(2)}%`;
}

function formatSpeed(outputTokens, latencyMs, ttftMs) {
  const output = Number(outputTokens) || 0;
  const latency = Number(latencyMs) || 0;
  const ttft = Number(ttftMs) || 0;
  if (output <= 0 || latency <= 0 || ttft <= 0 || latency <= ttft) {
    return '—';
  }
  const speed = output / ((latency - ttft) / 1000);
  return Number.isFinite(speed) && speed > 0 ? `${speed.toFixed(1)} t/s` : '—';
}

function formatTime(isoStr) {
  if (!isoStr) return '-';
  try {
    const d = new Date(isoStr);
    return d.toLocaleString();
  } catch {
    return isoStr;
  }
}

function escapeText(value) {
  return String(value ?? '').replace(/[&<>'"]/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]);
}
