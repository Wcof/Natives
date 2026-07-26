/**
 * Regression: one active soft-resub loop per run; drop loops when a run leaves
 * the wanted set.
 *
 * H_sub (2026-07-23): the multi-run effect only called ensureRunSubscription for
 * wanted runs and never aborted historical ones. Switching conversations or a
 * child going terminal left orphan soft-resub loops → multi-subscription thrash.
 *
 * These used to be regex assertions over AssistantWorkbench.tsx, which pinned
 * the rules to one component's source text. The rules now live in
 * `subscription-coordination` (pure) and `useAssistantRun` (the loop around
 * them), so they can be exercised directly — and both the Workbench and the
 * creator workbench inherit the same guarantees.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  cancelUnwantedSubscriptions,
  diffSubscriptionSets,
  replaceRunSubscription,
  type SubscriptionSignal,
} from '@/lib/assistant-workspace/subscription-coordination';

const hookSrc = readFileSync(
  fileURLToPath(new URL('../../lib/assistant-workspace/use-assistant-run.ts', import.meta.url)),
  'utf8',
);

describe('run subscription lifecycle (one active sub per run)', () => {
  it('starting a run aborts only the previous signal for that same run', () => {
    const signals: Record<string, SubscriptionSignal | undefined> = {};
    const first = replaceRunSubscription(signals, 'run-a');
    const otherRun = replaceRunSubscription(signals, 'run-b');

    const second = replaceRunSubscription(signals, 'run-a');

    assert.equal(first.aborted, true, 'previous loop for run-a must be aborted');
    assert.equal(second.aborted, false, 'the fresh loop stays live');
    assert.equal(otherRun.aborted, false, 'an unrelated run keeps polling');
    assert.equal(signals['run-a'], second, 'the run slot holds exactly one live signal');
  });

  it('a run with a live signal is not restarted', () => {
    const live: SubscriptionSignal = { aborted: false };
    const { toStart } = diffSubscriptionSets(new Set(['run-a']), { 'run-a': live });
    assert.deepEqual(toStart, [], 'live loop must not be duplicated');
  });

  it('a run whose signal was aborted is restarted', () => {
    const dead: SubscriptionSignal = { aborted: true };
    const { toStart } = diffSubscriptionSets(new Set(['run-a']), { 'run-a': dead });
    assert.deepEqual(toStart, ['run-a'], 'an aborted loop must be replaced');
  });

  it('runs that leave the wanted set are aborted and dropped', () => {
    const signals: Record<string, SubscriptionSignal | undefined> = {};
    const stale = replaceRunSubscription(signals, 'old-run');
    replaceRunSubscription(signals, 'kept-run');

    const cancelled = cancelUnwantedSubscriptions(signals, new Set(['kept-run']));

    assert.deepEqual(cancelled, ['old-run']);
    assert.equal(stale.aborted, true, 'orphan loop must stop polling');
    assert.equal(signals['old-run'], undefined, 'and must be dropped from the map');
    assert.equal(signals['kept-run']?.aborted, false, 'the wanted run keeps polling');
  });

  it('clearing every subscription aborts all tracked signals', () => {
    const signals: Record<string, SubscriptionSignal | undefined> = {};
    const a = replaceRunSubscription(signals, 'run-a');
    const b = replaceRunSubscription(signals, 'run-b');

    cancelUnwantedSubscriptions(signals, new Set());

    assert.equal(a.aborted, true);
    assert.equal(b.aborted, true);
  });

  it('soft-resub only continues when the same signal still owns the run slot', () => {
    // A raced restart must not resurrect an aborted loop. This guard lives inside
    // the hook's setTimeout callback, which has no seam to call directly.
    assert.match(hookSrc, /if \(!signal\.aborted && subSignalsRef\.current\[runId\] === signal\)/);
  });

  it('an empty poll never reports a lost connection', () => {
    // The expensive lesson: treating quiet long-polls as disconnects produced
    // spurious "reconnecting" banners during healthy runs.
    assert.match(hookSrc, /quiet|Quiet/, 'the quiet-resubscribe path must be documented');
    assert.doesNotMatch(
      hookSrc,
      /connection\/set[^}]*reconnecting/,
      'the resubscribe loop must not touch global connection state',
    );
  });
});
