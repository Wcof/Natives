/**
 * Regression: one active soft-resub loop per run; drop loops when run leaves wanted set.
 *
 * H_sub (2026-07-23): multi-run effect only called ensureRunSubscription for wanted
 * runs and never aborted historical ones. Switching conversations / child terminal
 * left orphan soft-resub loops → multi-subscription thrash.
 *
 * Also guards: ensureRunSubscription short-circuits when signal is live;
 * unmount aborts every tracked signal.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const workbenchSrc = readFileSync(
  fileURLToPath(new URL('./AssistantWorkbench.tsx', import.meta.url)),
  'utf8',
);

describe('run subscription lifecycle (one active sub per run)', () => {
  it('startSubscription aborts only the previous signal for the same runId', () => {
    const startIdx = workbenchSrc.indexOf('const startSubscription = useCallback');
    assert.ok(startIdx >= 0, 'startSubscription must exist');
    const chunk = workbenchSrc.slice(startIdx, startIdx + 2200);
    assert.match(chunk, /const prev = subSignalsRef\.current\[runId\]/);
    assert.match(chunk, /if \(prev\) prev\.aborted = true/);
    assert.match(chunk, /subSignalsRef\.current\[runId\] = signal/);
  });

  it('ensureRunSubscription is a no-op when a live signal already tracks the run', () => {
    const ensureIdx = workbenchSrc.indexOf('const ensureRunSubscription = useCallback');
    assert.ok(ensureIdx >= 0);
    const chunk = workbenchSrc.slice(ensureIdx, ensureIdx + 900);
    assert.match(
      chunk,
      /if \(subSignalsRef\.current\[runId\] && !subSignalsRef\.current\[runId\]!\.aborted\) return/,
    );
  });

  it('multi-run effect aborts signals for runs that leave the wanted set', () => {
    // Wanted-set pruning is the switch-session / terminal-child recovery path.
    assert.match(workbenchSrc, /for \(const runId of Object\.keys\(subSignalsRef\.current\)\)/);
    assert.match(workbenchSrc, /if \(!wanted\.has\(runId\)\)/);
    assert.match(workbenchSrc, /delete subSignalsRef\.current\[runId\]/);
    assert.match(workbenchSrc, /delete resubAttemptsRef\.current\[runId\]/);
  });

  it('unmount / bootstrap cleanup aborts every tracked subscription signal', () => {
    assert.match(
      workbenchSrc,
      /for \(const signal of Object\.values\(subSignalsRef\.current\)\)\s*\{\s*signal\.aborted = true/,
    );
    assert.match(workbenchSrc, /subSignalsRef\.current = \{\}/);
  });

  it('soft-resub only continues when the same signal still owns the run slot', () => {
    // Prevents a raced restart from resurrecting an aborted loop.
    assert.match(
      workbenchSrc,
      /if \(!signal\.aborted && subSignalsRef\.current\[runId\] === signal\)/,
    );
  });
});
