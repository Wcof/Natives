import assert from 'node:assert/strict';
import { MockElement, setupTestDomEnvironment } from './test-dom-mock.js';
import { sharedCall, invalidateQueries, resetSharedQueriesForTest } from './ai-usage-client/shared-queries.js';
import { __emitModelEventForTest } from './ai-usage-client/client.js';
import { aiPerformanceWidget } from './ai-performance/widget.js';

setupTestDomEnvironment();
const tick = () => new Promise(resolve => setImmediate(resolve));
// MockElement's regex parser drops nested div text; capture the actual renderer
// output here, while the browser regression verifies its DOM separately.
function captureHtml(element) {
  let html = element.innerHTML;
  const descriptor = Object.getOwnPropertyDescriptor(MockElement.prototype, 'innerHTML');
  Object.defineProperty(element, 'innerHTML', {
    get: () => html,
    set(value) { html = value; descriptor.set.call(element, value); },
  });
}

// An invalidation during a request must issue a new query; either completion
// order must preserve the new request and cache.
for (const oldFirst of [true, false]) {
  resetSharedQueriesForTest();
  const pending = [];
  const api = { getUsageOverview: () => new Promise(resolve => pending.push(resolve)) };
  const old = sharedCall(api, 'getUsageOverview');
  invalidateQueries();
  const fresh = sharedCall(api, 'getUsageOverview');
  assert.equal(pending.length, 2, 'revision invalidation must not reuse the old request');
  if (oldFirst) {
    pending[0]('old'); await old;
    assert.equal(sharedCall(api, 'getUsageOverview'), fresh, 'old finally must not remove the new request');
  }
  pending[1]('fresh'); await fresh;
  if (!oldFirst) { pending[0]('old'); await old; }
  assert.equal(await sharedCall(api, 'getUsageOverview'), 'fresh');
}

const sent = [];
const delayed = [];
let delayReplies = false;
let budgetId = 'initial-budget';
let remainingPercent = 0;
let failQuota = false;
globalThis.chrome = { runtime: {
  lastError: null,
  connectNative() {
    const listeners = [], disconnects = [];
    const port = {
      closed: false,
      onMessage: { addListener: fn => listeners.push(fn) },
      onDisconnect: { addListener: fn => disconnects.push(fn) },
      postMessage(message) {
        sent.push(message);
        const result = message.method === 'model_usage_sessions'
          ? { status: 'ready', totalSessions: 3, activeDays: 2, byDay: [], bySource: [] }
          : message.method === 'model_usage_budgets'
            ? { budgets: [{ id: budgetId, period: 'monthly', currency: 'USD', amountMicro: 1000000, enabled: true }], evaluations: [] }
            : message.method === 'model_quota_cached'
              ? { status: 'ready', entries: { example: { provider: 'isolated-provider', status: 'success', fetchedAt: 1, windows: [{ name: 'weekly', remainingPercent }] } } }
              : { status: 'ready', metrics: { totalTokens: 1 }, tokens: {}, trend: [] };
        const deliver = () => { if (!port.closed) listeners.forEach(fn => fn(failQuota && message.method === 'model_quota_cached'
          ? { id: message.id, ok: false, error: { code: 'unavailable', message: 'isolated cache failure' } }
          : { id: message.id, ok: true, result })); };
        if (delayReplies) delayed.push(deliver); else queueMicrotask(deliver);
      },
      disconnect() { port.closed = true; disconnects.forEach(fn => fn()); },
    };
    return port;
  },
} };

resetSharedQueriesForTest();
const cards = Array.from({ length: 20 }, () => new MockElement('div'));
const dispose = cards.map(card => aiPerformanceWidget.render(card, { metric: 'ai_sessions', chart: 'number', range: '30d' }));
await tick();
assert.equal(sent.filter(m => m.method === 'model_usage_sessions').length, 1, '20 session cards share one query');
__emitModelEventForTest({ ok: true, event: 'model_usage_updated' });
await tick();
assert.equal(sent.filter(m => m.method === 'model_usage_sessions').length, 2, 'one shared refresh per revision');
dispose.forEach(fn => fn());

const custom = new MockElement('div');
const cleanupCustom = aiPerformanceWidget.render(custom, {
  metric: 'token_usage', chart: 'number', range: 'custom',
  startTime: '2026-01-01T00:00:00Z', endTime: '2026-01-02T00:00:00Z',
});
await tick();
const request = sent.findLast(m => m.method === 'model_usage_overview');
assert.equal(request.params.startTime, '2026-01-01T00:00:00.000Z');
assert.equal(request.params.endTime, '2026-01-02T00:00:00.000Z');
cleanupCustom();

resetSharedQueriesForTest();
const limits = new MockElement('div');
const cleanupLimits = aiPerformanceWidget.render(limits, { view: 'limits' });
captureHtml(limits.children[0]);
await tick();
assert.ok(limits.children[0].innerHTML.includes('initial-budget'));
assert.ok(limits.children[0].innerHTML.includes('isolated-provider'), 'quota uses the same Model Host authority');
assert.ok(limits.children[0].innerHTML.includes('0.0%'), `zero remaining is valid, not missing: ${limits.children[0].innerHTML}`);
assert.equal(sent.filter(m => m.method === 'model_quota_query').length, 0, 'render must not probe upstream quota');
budgetId = 'updated-budget';
__emitModelEventForTest({ ok: true, event: 'model_usage_updated' });
await tick();
assert.ok(limits.children[0].innerHTML.includes('updated-budget'), 'non-usage view refreshes on Host events');
remainingPercent = null;
__emitModelEventForTest({ ok: true, event: 'model_quota_updated' });
await tick();
assert.ok(!limits.children[0].innerHTML.includes('0.0%'), 'missing quota must not appear as zero remaining');
failQuota = true;
__emitModelEventForTest({ ok: true, event: 'model_quota_updated' });
await tick();
assert.ok(limits.children[0].innerHTML.includes('updated-budget'), 'quota failure must not hide the independent budget');
assert.ok(limits.children[0].innerHTML.includes('aiPerfQuotaUnavailable'));
failQuota = false;
cleanupLimits();

// Destroy while a non-usage view is waiting: no late DOM rendering.
resetSharedQueriesForTest();
delayReplies = true;
const removed = new MockElement('div');
const cleanupRemoved = aiPerformanceWidget.render(removed, { view: 'limits' });
const detachedRoot = removed.children[0];
const before = detachedRoot.innerHTML;
cleanupRemoved();
delayed.splice(0).forEach(fn => fn());
await tick();
assert.equal(detachedRoot.innerHTML, before);
assert.equal(removed.children.length, 0);
console.log('AI shared query, session deduplication, custom range, quota, refresh and disposal regressions passed');
