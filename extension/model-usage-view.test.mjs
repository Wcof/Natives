import assert from 'node:assert/strict';
import { setupTestDomEnvironment } from './test-dom-mock.js';
import { renderUsageView } from './model-usage-view.js';

setupTestDomEnvironment();
const container = document.createElement('div');
const emptyRanks = { byModel: null, byProvider: null, bySource: null, byAccessKey: null };
for (const activeSubTab of ['overview', 'analytics', 'events', 'pricing', 'billing']) {
  renderUsageView(container, { activeSubTab, filterOptions: emptyRanks,
    overviewData: { status: 'empty', trend: null }, analyticsData: emptyRanks,
    eventsData: { events: null }, pricingData: { prices: null }, billingData: { summaries: null, entries: null },
    t: (_key, fallback) => fallback });
  assert.equal(container.querySelectorAll('[data-action="filter-usage-dimension"]').length, 5);
}
renderUsageView(container, { activeSubTab: 'analytics',
  analyticsData: { byModel: [{ key: 'model', requests: 1 }], byProvider: null, byAccessKey: null, byHour: null },
  t: (_key, fallback) => fallback });
assert.equal(container.querySelectorAll('.model-rank-card').length, 4);
assert.ok(container.textContent.includes('model'));
console.log('Usage view: nullable empty filter dimensions, all five empty tabs and partial rankings passed');

renderUsageView(container, { activeSubTab: 'overview',
  overviewData: { status: 'ready', metrics: { totalRequests: 1, totalTokens: 0, estimatedCostUsd: 0, priced: false, missingUsageRequests: 1 }, tokens: {}, trend: [] },
  t: (_key, fallback) => fallback });
assert.ok(container.textContent.includes('部分请求未报告用量'));
assert.ok(!container.textContent.includes('$0.0000'), 'missing usage cannot be rendered as free');
renderUsageView(container, { activeSubTab: 'overview',
  overviewData: { status: 'ready', metrics: { totalRequests: 2, totalTokens: 10, estimatedCostUsd: 0.25, priced: false, missingUsageRequests: 1 }, tokens: { input: 10 }, trend: [] },
  t: (_key, fallback) => fallback });
assert.ok(container.textContent.includes('10+'));
assert.ok(container.textContent.includes('≥$0.2500'), 'known cost subtotal is preserved');
