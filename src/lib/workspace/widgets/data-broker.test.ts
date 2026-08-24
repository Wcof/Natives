import { describe, it, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import {
  WorkspaceDataBroker,
  buildWidgetBrokerKey,
} from './data-broker';
import type { WidgetDataContext, WidgetDefinition } from './types';

describe('WorkspaceDataBroker', () => {
  let broker: WorkspaceDataBroker;

  beforeEach(() => {
    broker = new WorkspaceDataBroker();
  });

  it('provides initial idle snapshot before subscription loads', () => {
    const snap = broker.getSnapshot('test.key');
    assert.equal(snap.status, 'idle');
    assert.equal(snap.data, null);
    assert.equal(snap.lastUpdated, null);
    assert.equal(snap.refetching, false);
  });

  it('deduplicates in-flight loader calls for the same adapter key', async () => {
    let callCount = 0;
    const loader = async (_ctx: WidgetDataContext) => {
      callCount += 1;
      await new Promise((r) => setTimeout(r, 10));
      return { value: 42 };
    };

    const snapshots1: unknown[] = [];
    const snapshots2: unknown[] = [];

    const unsub1 = broker.subscribe('shared.key', loader, {
      onSnapshot: (s) => snapshots1.push(s.data),
    });
    const unsub2 = broker.subscribe('shared.key', loader, {
      onSnapshot: (s) => snapshots2.push(s.data),
    });

    await new Promise((r) => setTimeout(r, 50));

    assert.equal(callCount, 1);
    assert.deepEqual(broker.getSnapshot('shared.key').data, { value: 42 });
    assert.equal(broker.getSnapshot('shared.key').status, 'ready');

    unsub1();
    unsub2();
  });

  it('syncAll triggers refetch across all active subscribed keys without duplication', async () => {
    let loadA = 0;
    let loadB = 0;

    const loaderA = async () => {
      loadA += 1;
      return { id: 'A', count: loadA };
    };
    const loaderB = async () => {
      loadB += 1;
      return { id: 'B', count: loadB };
    };

    const unsubA1 = broker.subscribe('key.a', loaderA, { onSnapshot: () => {} });
    const unsubA2 = broker.subscribe('key.a', loaderA, { onSnapshot: () => {} });
    const unsubB = broker.subscribe('key.b', loaderB, { onSnapshot: () => {} });

    await new Promise((r) => setTimeout(r, 20));
    assert.equal(loadA, 1);
    assert.equal(loadB, 1);

    assert.equal(broker.lastSyncedAt, null);
    assert.equal(broker.syncing, false);

    let statusNotified = false;
    const unsubSync = broker.subscribeSyncStatus(() => {
      statusNotified = true;
    });

    await broker.syncAll();

    assert.equal(loadA, 2);
    assert.equal(loadB, 2);
    assert.notEqual(broker.lastSyncedAt, null);
    assert.equal(broker.syncing, false);
    assert.equal(statusNotified, true);

    unsubA1();
    unsubA2();
    unsubB();
    unsubSync();
  });

  it('syncAll reports ok/partial/failed and only advances lastSyncedAt on full success', async () => {
    let shouldFail = false;
    const okLoader = async () => ({ ok: true });
    const flakyLoader = async () => {
      if (shouldFail) throw new Error('boom');
      return { ok: true };
    };

    const unsubOk = broker.subscribe('ok.key', okLoader, { onSnapshot: () => {} });
    broker.subscribe('flaky.key', flakyLoader, { onSnapshot: () => {} });
    await new Promise((r) => setTimeout(r, 20));

    // Full success → outcome ok, lastSyncedAt advances.
    const okResult = await broker.syncAll();
    assert.equal(okResult.outcome, 'ok');
    assert.equal(okResult.failed, 0);
    const firstSyncedAt = broker.lastSyncedAt;
    assert.notEqual(firstSyncedAt, null);

    // Partial failure → outcome partial, lastSyncedAt must NOT advance.
    shouldFail = true;
    const partial = await broker.syncAll();
    assert.equal(partial.outcome, 'partial');
    assert.ok(partial.failed >= 1);
    assert.ok(partial.succeeded >= 1);
    assert.equal(broker.lastSyncedAt, firstSyncedAt, 'partial failure must not update last-synced time');

    // Total failure → outcome failed (both mounted components fail),
    // lastSyncedAt still frozen at last success.
    broker.subscribe('fail2.key', flakyLoader, { onSnapshot: () => {} });
    unsubOk();
    await new Promise((r) => setTimeout(r, 20));
    const failed = await broker.syncAll();
    assert.equal(failed.outcome, 'failed');
    assert.equal(failed.succeeded, 0);
    assert.equal(broker.lastSyncedAt, firstSyncedAt, 'total failure must not update last-synced time');
  });

  it('syncAll returns noop and does not advance lastSyncedAt when no data components are mounted', async () => {
    const before = broker.lastSyncedAt;
    const result = await broker.syncAll();
    assert.equal(result.outcome, 'noop');
    assert.equal(broker.lastSyncedAt, before);
  });

  it('syncAll is idempotent under concurrent calls', async () => {
    const loader = async () => { await new Promise((r) => setTimeout(r, 10)); return { v: 1 }; };
    broker.subscribe('c.key', loader, { onSnapshot: () => {} });
    await new Promise((r) => setTimeout(r, 20));
    const [a, b] = await Promise.all([broker.syncAll(), broker.syncAll()]);
    assert.equal(a.outcome, 'ok');
    assert.equal(b.outcome, 'noop');
  });

  it('handles timeRange properly in buildWidgetBrokerKey and context', async () => {
    const timeAwareDef: Pick<WidgetDefinition<unknown>, 'type' | 'timeAware' | 'adapterKeyBuilder'> = {
      type: 'token_metrics',
      timeAware: true,
      adapterKeyBuilder: () => 'usage.summary',
    };

    const nonTimeDef: Pick<WidgetDefinition<unknown>, 'type' | 'timeAware'> = {
      type: 'storage_overview',
    };

    const key7d = buildWidgetBrokerKey(timeAwareDef, { type: 'token_metrics', enabled: true, surface: 'auto', order: 0 }, '7d');
    const key30d = buildWidgetBrokerKey(timeAwareDef, { type: 'token_metrics', enabled: true, surface: 'auto', order: 0 }, '30d');
    const nonTimeKey = buildWidgetBrokerKey(nonTimeDef, { type: 'storage_overview', enabled: true, surface: 'auto', order: 1 }, '7d');

    assert.equal(key7d, 'usage.summary:7d');
    assert.equal(key30d, 'usage.summary:30d');
    assert.equal(nonTimeKey, 'storage_overview');

    broker.setTimeRange('30d');
    assert.equal(broker.timeRange, '30d');

    let receivedRange: string | undefined;
    const loader = async (ctx: WidgetDataContext) => {
      receivedRange = ctx.timeRange;
      return { ok: true };
    };

    const unsub = broker.subscribe('time.test', loader, { onSnapshot: () => {} });
    await new Promise((r) => setTimeout(r, 20));

    assert.equal(receivedRange, '30d');
    unsub();
  });

  it('pauses new loads while the document is hidden and drains on return', async () => {
    // Minimal in-test document stub (the Node test runner has no DOM).
    type VisHandler = () => void;
    let visible = false;
    const handlers: VisHandler[] = [];
    const fakeDoc = {
      get visibilityState() { return visible ? 'visible' : 'hidden'; },
      addEventListener: (_e: string, fn: VisHandler) => { handlers.push(fn); },
      removeEventListener: (_e: string, fn: VisHandler) => {
        const i = handlers.indexOf(fn);
        if (i >= 0) handlers.splice(i, 1);
      },
    };
    const origDoc = (globalThis as { document?: unknown }).document;
    (globalThis as { document?: unknown }).document = fakeDoc;
    try {
      const unsub = broker.bindVisibility();
      assert.equal(broker['_paused'], true);

      let loadCount = 0;
      const loader = async () => { loadCount += 1; await new Promise((r) => setTimeout(r, 10)); return { v: 1 }; };
      broker.subscribe('paused.key', loader, { onSnapshot: () => {} });
      await new Promise((r) => setTimeout(r, 30));
      // Hidden: load is parked, not started.
      assert.equal(loadCount, 0);

      // Become visible → queued load drains and runs.
      visible = true;
      for (const fn of handlers) fn();
      await new Promise((r) => setTimeout(r, 30));
      assert.equal(loadCount, 1);

      unsub();
    } finally {
      if (origDoc === undefined) delete (globalThis as { document?: unknown }).document;
      else (globalThis as { document?: unknown }).document = origDoc;
    }
  });

  it('invalidates cache properly', async () => {
    let fetchCount = 0;
    const loader = async () => {
      fetchCount += 1;
      return { version: fetchCount };
    };

    const unsub = broker.subscribe('inv.key', loader, { onSnapshot: () => {} });
    await new Promise((r) => setTimeout(r, 20));
    assert.equal(fetchCount, 1);

    broker.invalidate('inv.key');
    await new Promise((r) => setTimeout(r, 20));
    assert.equal(fetchCount, 2);

    unsub();
  });
});
