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
