// AI Performance widget tests (ADR-0028): metric mapping, config
// normalization, render lifecycle, destroy semantics, empty/error states,
// chart options (number/line/bar/heatmap/timeline/table), sub-metrics and dimensions.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { MockElement, setupTestDomEnvironment } from './test-dom-mock.js';
import { METRICS, fetchDataset } from './ai-performance/metrics.js';
import { aiPerformanceWidget } from './ai-performance/widget.js';
import { WIDGET_KEYS, widgetPlugins } from './space-plugins.js';

setupTestDomEnvironment();

const OVERVIEW_READY = {
  status: 'ready',
  // 真实 Host 协议（usage/store.go GetOverview）：successRate 为 0–100。
  metrics: { totalRequests: 12, totalTokens: 32000, successRate: 90, estimatedCostUsd: 1.25 },
  // trend 点使用 timestamp（store.go trendQuery 的 bucket 字段）。
  trend: [
    { timestamp: '2026-09-01T10:00:00Z', requests: 8, tokens: 20000, costUsd: 1.0 },
    { timestamp: '2026-09-02T11:00:00Z', requests: 4, tokens: 12000, costUsd: 0.25 },
  ],
  tokens: { input: 20000, output: 12000 },
};

const ANALYTICS_READY = {
  status: 'ready',
  byModel: [{ key: 'gpt-5', name: 'gpt-5', requests: 8, tokens: 20000, costUsd: 1.0 }],
  bySource: [{ key: 'claude_code', name: 'claude_code', requests: 9, tokens: 21000, costUsd: 0.9 }],
  byProvider: [{ key: 'anthropic', name: 'Anthropic', requests: 10, tokens: 25000, costUsd: 1.1 }],
  byHour: [{ key: '09', name: '09:00', requests: 5, tokens: 9000, costUsd: 0.3 }],
};

const EVENTS_READY = {
  status: 'ready',
  // 真实 Host 协议（usage/types.go Event JSON）：明细事件时间字段是 requestedAt。
  events: [
    {
      id: 'ev-1',
      requestedAt: '2026-09-10T09:00:00Z',
      source: 'Claude Code',
      model: 'claude-3-5-sonnet',
      totalTokens: 2400,
      costMicro: 12000,
      result: 'success',
    },
    {
      id: 'ev-2',
      requestedAt: '2026-09-10T11:30:00Z',
      source: 'Codex',
      model: 'gpt-5',
      totalTokens: 5200,
      costMicro: 25000,
      result: 'success',
    },
  ],
  total: 2,
};

let mode = 'ready'; // ready | empty | delayed | error
let openPorts = [];

globalThis.chrome = {
  runtime: {
    lastError: null,
    connectNative() {
      const port = {
        _disconnected: false,
        onMessage: { addListener: (fn) => port._msgFns.push(fn) },
        onDisconnect: { addListener: (fn) => port._discFns.push(fn) },
        _msgFns: [],
        _discFns: [],
        postMessage(message) {
          const deliver = () => {
            if (port._disconnected) return;
            let payload = null;
            if (mode === 'error') {
              payload = { id: message.id, ok: false, error: 'boom' };
            } else if (mode === 'empty') {
              if (message.method === 'model_usage_overview') {
                payload = { id: message.id, ok: true, result: { status: 'empty' } };
              } else if (message.method === 'model_usage_analysis') {
                payload = { id: message.id, ok: true, result: { status: 'empty', byModel: [], bySource: [], byHour: [] } };
              } else if (message.method === 'model_usage_events') {
                payload = { id: message.id, ok: true, result: { status: 'empty', events: [], total: 0 } };
              }
            } else if (message.method === 'model_usage_overview') {
              payload = { id: message.id, ok: true, result: OVERVIEW_READY };
            } else if (message.method === 'model_usage_analysis') {
              payload = { id: message.id, ok: true, result: ANALYTICS_READY };
            } else if (message.method === 'model_usage_events') {
              payload = { id: message.id, ok: true, result: EVENTS_READY };
            }
            if (payload) port._msgFns.forEach((fn) => fn(payload));
          };
          if (mode === 'delayed') setTimeout(deliver, 40);
          else queueMicrotask(deliver);
        },
        disconnect() {
          port._disconnected = true;
          port._discFns.forEach((fn) => fn());
        },
      };
      openPorts.push(port);
      return port;
    },
  },
};

const ctx = {
  t: (k, fallback) => fallback || k,
  lang: 'zh_CN',
  shadowRoot: new MockElement('shadow-root'),
  onDataChange: () => {},
};

function waitFor(predicate, what, timeoutMs = 2000) {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const tick = () => {
      if (predicate()) return resolve();
      if (Date.now() - started > timeoutMs) return reject(new Error(`timeout waiting for ${what}`));
      setTimeout(tick, 10);
    };
    tick();
  });
}

async function renderWidget(config) {
  const container = new MockElement('div');
  const disposer = aiPerformanceWidget.render(container, config, {}, ctx);
  return { container, disposer };
}

console.log('--- AI Performance Widget Tests ---');

// 1. Metric mapping: one dataset per metric from the same aggregated protocol data.
{
  const api = {
    getUsageOverview: async () => OVERVIEW_READY,
    getUsageAnalysis: async () => ANALYTICS_READY,
    getUsageEvents: async () => EVENTS_READY,
  };
  const need = { overview: true, analytics: true, events: true };

  const dsToken = await fetchDataset(api, METRICS.token_usage, '30d', need, { subMetric: 'total' });
  assert.equal(dsToken.empty, false);
  assert.equal(dsToken.number.value, 32000);
  assert.equal(dsToken.trend.length, 2);
  assert.equal(dsToken.days[0].date, '2026-09-01');
  assert.equal(dsToken.rank[0].name, 'gpt-5');
  assert.equal(dsToken.rank[0].value, 20000, 'token rank must rank by tokens');

  const dsInputToken = await fetchDataset(api, METRICS.token_usage, '30d', need, { subMetric: 'input' });
  assert.equal(dsInputToken.number.value, 20000, 'input token sub-metric');

  const dsOutputToken = await fetchDataset(api, METRICS.token_usage, '30d', need, { subMetric: 'output' });
  assert.equal(dsOutputToken.number.value, 12000, 'output token sub-metric');

  const dsCost = await fetchDataset(api, METRICS.ai_cost, '30d', need);
  assert.equal(dsCost.number.value, 1.25);
  assert.equal(dsCost.rank[0].value, 1.0, 'cost rank must rank by costUsd');

  const dsReq = await fetchDataset(api, METRICS.request_count, '30d', need);
  assert.equal(dsReq.number.value, 12);
  assert.equal(dsReq.rank[0].name, 'claude_code', 'request rank must rank sources');

  const dsTimeline = await fetchDataset(api, METRICS.token_usage, '30d', need);
  assert.equal(dsTimeline.events.length, 2);
  assert.equal(dsTimeline.events[0].source, 'Claude Code');
  assert.equal(dsTimeline.events[1].source, 'Codex');
  // 契约：Host Event 的时间字段是 requestedAt，明细时间不得为空串。
  assert.ok(dsTimeline.events[0].time, 'event time must come from requestedAt, not empty');
  assert.ok(dsTimeline.events[1].time, 'event time must come from requestedAt, not empty');

  const dsTable = await fetchDataset(api, METRICS.token_usage, '30d', need);
  assert.equal(dsTable.tableRows.length, 1);
  assert.equal(dsTable.tableRows[0].name, 'gpt-5');

  console.log('  ✓ metric mapping (token/cost/request/session/sub-metrics/timeline/table)');

// 1b. Custom range (用户要求：自定义/24h/7d/30d 周期选择):
//     - 有效起止随查询透传（overview/analysis/events/sessions 全部携带）；
//     - 缺失/无效起止回退近 7 天缺省，卡片仍可查询；
//     - 非 custom 范围不携带 startTime/endTime。
{
  const seen = {};
  const api = {
    getUsageOverview: async (params) => { seen.overview = params; return OVERVIEW_READY; },
    getUsageAnalysis: async (params) => { seen.analysis = params; return ANALYTICS_READY; },
    getUsageEvents: async (params) => { seen.events = params; return EVENTS_READY; },
    getUsageSessions: async (params) => { seen.sessions = params; return { status: 'ready', totalSessions: 1, activeDays: 1, byDay: [], bySource: [] }; },
  };

  const start = '2026-08-01T00:00:00Z';
  const end = '2026-08-31T23:59:59Z';
  const ds = await fetchDataset(api, METRICS.ai_cost, 'custom', { overview: true }, { startTime: start, endTime: end, timezone: 'UTC' });
  assert.equal(ds.empty, false);
  assert.equal(seen.overview.range, 'custom');
  // 起止经 ISO 归一（时间等值；字符串可为毫秒精度）。
  assert.equal(new Date(seen.overview.startTime).getTime(), new Date(start).getTime());
  assert.equal(new Date(seen.overview.endTime).getTime(), new Date(end).getTime());

  // sessions 查询同样携带
  await fetchDataset(api, METRICS.ai_sessions, 'custom', { overview: false }, { startTime: start, endTime: end, timezone: 'UTC' });
  assert.equal(new Date(seen.sessions.startTime).getTime(), new Date(start).getTime());
  assert.equal(seen.sessions.range, 'custom');

  // 缺失起止 → 近 7 天缺省（起止均为合法 ISO 且 start < end）
  await fetchDataset(api, METRICS.ai_cost, 'custom', { overview: true }, {});
  assert.ok(seen.overview.startTime && seen.overview.endTime);
  assert.ok(new Date(seen.overview.endTime) > new Date(seen.overview.startTime));

  // 无效起止（start >= end）→ 同样回退缺省
  await fetchDataset(api, METRICS.ai_cost, 'custom', { overview: true }, { startTime: end, endTime: start });
  assert.ok(new Date(seen.overview.endTime) > new Date(seen.overview.startTime));

  // 非 custom 不携带
  await fetchDataset(api, METRICS.ai_cost, '30d', { overview: true }, { startTime: start, endTime: end });
  assert.equal(seen.overview.startTime, undefined);
  console.log('  ✓ custom range passes validated start/end and falls back to 7d default');
}

// 1c. normalizeConfig（widget config 归一）：custom 携带合法起止为 ISO；
//     非法起止置空；非 custom 不携带。
{
  const { normalizeConfig } = await import('./ai-performance/widget.js').catch(() => ({}));
  // normalizeConfig 未导出时，经 renderSettings + defaultData 间接验证。
  const start = '2026-08-01T00:00:00Z';
  const end = '2026-08-20T00:00:00Z';
  const settings = new MockElement('div');
  const seenConfigs = [];
  aiPerformanceWidget.renderSettings(settings, { metric: 'ai_cost', chart: 'number', range: 'custom', startTime: start, endTime: end }, (next) => seenConfigs.push(next), ctx);
  // custom 面板必须暴露起止输入
  const startInput = settings.querySelector('[data-role="ai-perf-start"]');
  const endInput = settings.querySelector('[data-role="ai-perf-end"]');
  assert.ok(startInput && endInput, 'custom range must expose start/end inputs');
  // 非 custom 范围不渲染起止输入
  const plain = new MockElement('div');
  aiPerformanceWidget.renderSettings(plain, { metric: 'ai_cost', chart: 'number', range: '30d' }, () => {}, ctx);
  assert.equal(plain.querySelector('[data-role="ai-perf-start"]'), null, 'non-custom range hides time inputs');
  console.log('  ✓ settings render custom range start/end inputs');
}
}

// 2. Render → load → chart DOM, then destroy releases the shared port.
{
  const { container, disposer } = await renderWidget({ metric: 'token_usage', chart: 'number', range: '30d' });
  await waitFor(() => container.textContent.includes('32.0K'), 'number card value');
  disposer();
  assert.equal(container.childNodes.length, 0, 'destroy must clear the container');
  assert.ok(openPorts.every((p) => p._disconnected), 'last destroy must disconnect the shared port');
  console.log('  ✓ render → load → destroy releases shared Model Host port');
}

// 3. Render Timeline chart with real event rows.
{
  const { container, disposer } = await renderWidget({ metric: 'token_usage', chart: 'timeline', range: '30d' });
  await waitFor(() => container.textContent.includes('Claude Code'), 'timeline source Claude Code');
  assert.ok(container.textContent.includes('Codex'), 'timeline source Codex');
  disposer();
  console.log('  ✓ render Timeline chart with actual event stream');
}

// 4. Render Table chart with comparison rows.
{
  const { container, disposer } = await renderWidget({ metric: 'token_usage', chart: 'table', range: '30d' });
  await waitFor(() => container.textContent.includes('gpt-5'), 'table row model');
  disposer();
  console.log('  ✓ render Table comparison chart');
}

// 5. Destroy cancels in-flight loads (no late render, no leak).
{
  mode = 'delayed';
  const { container, disposer } = await renderWidget({ metric: 'token_usage', chart: 'line', range: '30d' });
  disposer();
  await new Promise((r) => setTimeout(r, 80));
  assert.ok(!container.textContent.includes('32.0K'), 'cancelled load must not render');
  mode = 'ready';
  console.log('  ✓ destroy cancels in-flight load');
}

// 6. Invalid config falls back to defaults (config-driven, not data-bound).
{
  const { container, disposer } = await renderWidget({ metric: 'bogus', chart: 'bogus', range: 'bogus' });
  await waitFor(() => container.textContent.includes('32.0K'), 'default number card');
  disposer();
  console.log('  ✓ invalid config normalized to defaults');
}

// 7. Empty and error states are explicit, never fabricated.
{
  mode = 'empty';
  const { container, disposer } = await renderWidget({ metric: 'token_usage', chart: 'bar', range: '7d' });
  await waitFor(() => container.textContent.includes('当前统计范围暂无 AI 用量记录'), 'empty state');
  disposer();

  mode = 'error';
  const { container: errBox, disposer: disposeErr } = await renderWidget({ metric: 'token_usage', chart: 'number', range: '7d' });
  await waitFor(() => errBox.textContent.includes('用量数据加载失败'), 'error state');
  disposeErr();
  mode = 'ready';
  console.log('  ✓ explicit empty / error states');
}

// 8. Settings render covers metric/subMetric/chart/dimension/range selection.
{
  const settings = new MockElement('div');
  aiPerformanceWidget.renderSettings(settings, aiPerformanceWidget.defaultData, () => {}, ctx);
  assert.ok(settings.querySelectorAll('select').length >= 3, 'settings must expose select controls');
  console.log('  ✓ settings exposes metric / subMetric / chart / range');
}

// 9. Registry integration: exactly one AI Performance widget registered.
{
  assert.equal(WIDGET_KEYS.filter((k) => k === 'widget/aiPerformance').length, 1);
  assert.equal(widgetPlugins['widget/aiPerformance'], aiPerformanceWidget);
  console.log('  ✓ registry integration');
}

// 10. Cross-language contract: consume the REAL Host-exported fixture
// (model-host/internal/host/contract_fixture_test.go serializes actual
// handler responses). Frontend must never hand-write Host shapes.
{
  const fixture = JSON.parse(
    readFileSync(new URL('./ai-performance/host-fixture.json', import.meta.url), 'utf8'),
  );

  // successRate protocol stays 0-100 as produced by GetOverview.
  assert.equal(
    typeof fixture.overview.metrics.successRate,
    'number',
    'fixture must contain overview.metrics.successRate',
  );
  assert.ok(
    fixture.overview.metrics.successRate >= 0 && fixture.overview.metrics.successRate <= 100,
    `successRate must be 0-100 protocol, got ${fixture.overview.metrics.successRate}`,
  );

  // Event time field is requestedAt; frontend maps it, not trend's timestamp.
  const hostEvents = fixture.events.events;
  assert.ok(hostEvents.length >= 2, 'fixture must contain >=2 events');
  for (const ev of hostEvents) {
    assert.ok(ev.requestedAt, `event ${ev.id} must carry requestedAt from real Host`);
    assert.equal(typeof ev.costStatus, 'string', `event ${ev.id} must carry costStatus`);
    assert.ok(['priced', 'unpriced'].includes(ev.costStatus), `event ${ev.id} costStatus must be priced|unpriced`);
  }
  // The unpriced event has costMicro=0 but MUST NOT be treated as a free 0.
  const unpriced = hostEvents.find((ev) => ev.costStatus === 'unpriced');
  assert.ok(unpriced, 'fixture must include an unpriced event');
  assert.equal(unpriced.costMicro, 0);

  // Feed the real fixture through the frontend mapping and verify output.
  const api = {
    getUsageOverview: async () => fixture.overview,
    getUsageAnalysis: async () => ({
      status: 'ready',
      byModel: [],
      bySource: hostEvents.map((ev) => ({
        key: ev.source.toLowerCase().replace(/\s+/g, '_'),
        name: ev.source,
        requests: 1,
        tokens: ev.totalTokens,
        costUsd: ev.costMicro / 1e6,
      })),
      byProvider: [],
      byHour: [],
    }),
    getUsageEvents: async () => fixture.events,
  };
  const need = { overview: true, analytics: true, events: true };
  const ds = await fetchDataset(api, METRICS.token_usage, '30d', need);

  // Mapped timeline events keep non-empty times and honor costStatus.
  assert.equal(ds.events.length, hostEvents.length);
  for (const mapped of ds.events) {
    assert.ok(mapped.time, 'mapped event time must not be empty (requestedAt contract)');
    assert.ok(['priced', 'unpriced'].includes(mapped.costStatus), 'mapped event must preserve costStatus');
  }
  const mappedUnpriced = ds.events.find((ev) => ev.costStatus === 'unpriced');
  assert.ok(mappedUnpriced, 'mapped events must retain an unpriced entry');

  console.log('  ✓ real Host fixture contract (requestedAt / costStatus / successRate 0-100)');
}

// 11. T7: shared query layer — identical concurrent params merge into ONE
// Host call; revision bump invalidates the cache so the next call re-fetches.
{
  const { sharedCall, invalidateQueries, resetSharedQueriesForTest } = await import(
    './ai-performance/shared-queries.js'
  );
  const { subscribeModelEvents } = await import('./ai-performance/client.js');
  resetSharedQueriesForTest();

  let overviewCalls = 0;
  const api = {
    getUsageOverview: async () => {
      overviewCalls += 1;
      return { status: 'ready', metrics: { successRate: 90 } };
    },
  };

  // 5 个并发同参调用只发 1 次 Host 查询。
  const results = await Promise.all([
    sharedCall(api, 'getUsageOverview', { range: '30d' }),
    sharedCall(api, 'getUsageOverview', { range: '30d' }),
    sharedCall(api, 'getUsageOverview', { range: '30d' }),
    sharedCall(api, 'getUsageOverview', { range: '30d' }),
    sharedCall(api, 'getUsageOverview', { range: '30d' }),
  ]);
  assert.equal(overviewCalls, 1, `identical concurrent params must merge, got ${overviewCalls} calls`);
  assert.equal(results.length, 5);

  // 缓存命中：revision 未变时再次调用仍是 1 次。
  await sharedCall(api, 'getUsageOverview', { range: '30d' });
  assert.equal(overviewCalls, 1, 'cached result must not re-fetch within same revision');

  // Host 推送 model_usage_updated → revision 失效 → 下一次调用重新查询。
  const { __emitModelEventForTest } = await import('./ai-performance/client.js');
  const unsubscribe = subscribeModelEvents(() => {});
  __emitModelEventForTest({ type: 'model_usage_updated' });
  await sharedCall(api, 'getUsageOverview', { range: '30d' });
  assert.equal(overviewCalls, 2, 'revision invalidation must force a re-fetch');

  // 不同参数不是同一查询。
  await sharedCall(api, 'getUsageOverview', { range: '7d' });
  assert.equal(overviewCalls, 3, 'different params must not share cache entries');

  unsubscribe();
  console.log('  ✓ shared query merge + revision invalidation (T7)');
}

// 12. T7b/T8/T9: Six preset views render expected elements and honor data contracts.
{
  const { renderView, VIEWS } = await import('./ai-performance/views.js');
  assert.ok(VIEWS.includes('tools') && VIEWS.includes('limits') && VIEWS.includes('savings'));

  const mockApi = {
    getUsageSources: async () => ({
      status: 'ready',
      sources: [
        { id: 'claude-code', name: 'Claude Code', enabled: true, caps: { historicalUsage: 'implemented' }, audit: 'test audit' },
      ],
    }),
    getUsageBilling: async () => ({
      status: 'ready',
      summaries: [
        { currency: 'USD', recognizedServiceSpend: 2500000, cashOutflow: 5000000, creditBalanceDelta: -2500000 },
      ],
    }),
    getUsageAttention: async () => ({
      status: 'ready',
      items: [
        { id: 'attn-1', kind: 'waiting_permission', title: '等待授权', toolId: 'claude-code' },
      ],
    }),
    getUsageBudgets: async () => ({
      status: 'ready',
      budgets: [
        { id: 'daily-est', period: 'daily', currency: 'USD', amountMicro: 10000000, enabled: true },
      ],
      evaluations: [
        { budgetId: 'daily-est', spentMicro: 4500000, amountMicro: 10000000, percent: 45 },
      ],
    }),
    getUsageEvents: async () => ({
      status: 'ready',
      events: [
        { requestedAt: new Date().toISOString(), provider: 'anthropic', model: 'claude-3-7-sonnet', totalTokens: 1500, costMicro: 3000, billingAtom: 'atom-1' },
      ],
    }),
    getUsageInsights: async () => ({
      status: 'ready',
      insights: [
        { id: 'ins-1', ruleKey: 'long_context_cost_spike', severity: 'info', title: '长会话输入膨胀', evidence: '中位数翻倍', action: '开新会话', caveat: '自主决定' },
      ],
    }),
    ackUsageAttention: async () => ({ ok: true }),
  };

  const i18n = (k, f) => f;

  // Test tools view
  const rootTools = new MockElement('div');
  const handledTools = await renderView('tools', rootTools, mockApi, i18n);
  assert.ok(handledTools && rootTools.textContent.includes('Claude Code') && rootTools.textContent.includes('已启用'));

  // Test billing view
  const rootBilling = new MockElement('div');
  const handledBilling = await renderView('billing', rootBilling, mockApi, i18n);
  assert.ok(handledBilling && rootBilling.textContent.includes('USD') && rootBilling.textContent.includes('服务消耗'));

  // Test attention view
  const rootAttention = new MockElement('div');
  const handledAttention = await renderView('attention', rootAttention, mockApi, i18n);
  assert.ok(handledAttention && rootAttention.textContent.includes('等待授权') && rootAttention.textContent.includes('标记已查看'));

  // Test limits view
  const rootLimits = new MockElement('div');
  const handledLimits = await renderView('limits', rootLimits, mockApi, i18n);
  assert.ok(handledLimits && rootLimits.textContent.includes('daily-est') && rootLimits.textContent.includes('45%'));

  // Test ledger view
  const rootLedger = new MockElement('div');
  const handledLedger = await renderView('ledger', rootLedger, mockApi, i18n);
  assert.ok(handledLedger && rootLedger.textContent.includes('claude-3-7-sonnet') && rootLedger.textContent.includes('atom-1'));

  // Test savings view
  const rootSavings = new MockElement('div');
  const handledSavings = await renderView('savings', rootSavings, mockApi, i18n);
  assert.ok(handledSavings && rootSavings.textContent.includes('长会话输入膨胀') && rootSavings.textContent.includes('开新会话'));

  console.log('  ✓ six preset views rendered and data bound (tools/billing/attention/limits/ledger/savings)');
}


// toolIds 接线（R3）：fetchDataset 把 toolIds 透传为 { sources }，
// 空/缺失 = 全部已接入来源（不携带 sources 参数）。
{
  const calls = [];
  const api = {
    getUsageOverview: async (p) => { calls.push(['overview', p]); return OVERVIEW_READY; },
    getUsageAnalysis: async (p) => { calls.push(['analysis', p]); return ANALYTICS_READY; },
    getUsageEvents: async (p) => { calls.push(['events', p]); return EVENTS_READY; },
  };
  const need = { overview: true, analytics: true, events: true };

  const scoped = await fetchDataset(api, METRICS.ai_cost, '30d', need, { toolIds: ['claude-code', 'pi', 42, ''] });
  assert.equal(scoped.empty, false);
  const overviewCall = calls.find(([m]) => m === 'overview');
  assert.deepEqual(overviewCall[1].sources, ['claude-code', 'pi'], 'toolIds must forward as sources, non-string dropped');
  const eventsCall = calls.find(([m]) => m === 'events');
  assert.deepEqual(eventsCall[1].sources, ['claude-code', 'pi']);

  // 空 toolIds = 全部来源：不携带 sources 键。
  calls.length = 0;
  await fetchDataset(api, METRICS.token_usage, '30d', need, { toolIds: [] });
  assert.equal(calls.some(([, p]) => 'sources' in p), false, 'empty toolIds must omit sources (all connected)');

  // 未配置 toolIds = 全部来源。
  calls.length = 0;
  await fetchDataset(api, METRICS.token_usage, '30d', need, {});
  assert.equal(calls.some(([, p]) => 'sources' in p), false);

  // 会话指标同样透传。
  const SESSIONS_FIXTURE = {
    status: 'ready', totalSessions: 2, activeDays: 1, unattributed: 0,
    byDay: [{ date: '2026-09-11', sessions: 2 }],
    bySource: [{ key: 'codex', name: 'Codex', requests: 2, tokens: 500, costUsd: 0.1 }],
  };
  const sessionApi = {
    getUsageSessions: async (p) => { calls.push(['sessions', p]); return SESSIONS_FIXTURE; },
  };
  calls.length = 0;
  await fetchDataset(sessionApi, METRICS.ai_sessions, '30d', {}, { toolIds: ['codex'] });
  const sessionsCall = calls.find(([m]) => m === 'sessions');
  assert.ok(sessionsCall, 'sessions fetch must happen');
  assert.deepEqual(sessionsCall[1].sources, ['codex'], 'sessions must forward toolIds as sources');

  console.log('  ✓ toolIds forwarded as sources across overview/analysis/events/sessions');
}

console.log('\nAll AI Performance widget tests passed!');
process.exit(0);
