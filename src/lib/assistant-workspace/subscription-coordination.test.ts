import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  cancelUnwantedSubscriptions,
  computeWantedRunIds,
  diffSubscriptionSets,
  replaceRunSubscription,
  type SubscriptionSignal,
} from './subscription-coordination';

const isActive = (s: string) =>
  s === 'running' || s === 'queued' || s === 'preparing' || s === 'waiting_permission';

describe('subscription coordination', () => {
  it('wanted set only includes active runs', () => {
    const wanted = computeWantedRunIds(
      {
        a: { id: 'a', status: 'running' },
        b: { id: 'b', status: 'completed' },
        c: { id: 'c', status: 'waiting_permission' },
      },
      isActive,
    );
    assert.deepEqual([...wanted].sort(), ['a', 'c']);
  });

  it('diff starts missing and cancels unwanted', () => {
    const signals: Record<string, SubscriptionSignal | undefined> = {
      old: { aborted: false },
      keep: { aborted: false },
    };
    const wanted = new Set(['keep', 'new']);
    const { toStart, toCancel } = diffSubscriptionSets(wanted, signals);
    assert.deepEqual(toCancel.sort(), ['old']);
    assert.deepEqual(toStart.sort(), ['new']);
  });

  it('replaceRunSubscription aborts previous for same run (single sub)', () => {
    const signals: Record<string, SubscriptionSignal | undefined> = {};
    const first = replaceRunSubscription(signals, 'r1');
    assert.equal(first.aborted, false);
    const second = replaceRunSubscription(signals, 'r1');
    assert.equal(first.aborted, true);
    assert.equal(second.aborted, false);
    assert.equal(signals.r1, second);
  });

  it('cancelUnwantedSubscriptions drops and aborts', () => {
    const signals: Record<string, SubscriptionSignal | undefined> = {
      a: { aborted: false },
      b: { aborted: false },
    };
    const cancelled = cancelUnwantedSubscriptions(signals, new Set(['b']));
    assert.deepEqual(cancelled, ['a']);
    assert.equal(signals.a, undefined);
    assert.equal(signals.b?.aborted, false);
  });
});
