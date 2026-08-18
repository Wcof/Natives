import assert from 'node:assert/strict';
import test from 'node:test';
import { createHarnessNoticeBatcher } from './notice-batcher';

type Scheduled = { callback: () => void };

function makeBatcher(options: { detailVisible: boolean; dirty: boolean }) {
  const scheduled: Scheduled[] = [];
  const calls = { workspace: 0, runs: 0, notice: 0 };
  const batcher = createHarnessNoticeBatcher({
    ...options,
    refreshWorkspace: () => { calls.workspace += 1; },
    refreshRuns: () => { calls.runs += 1; },
    showRemoteChange: () => { calls.notice += 1; },
    schedule: (callback) => {
      scheduled.push({ callback });
      return scheduled.length as unknown as ReturnType<typeof setTimeout>;
    },
    cancel: () => undefined,
  });
  return { batcher, calls, scheduled };
}

test('replayed Harness notices trigger one workspace refresh per batch', () => {
  const { batcher, calls, scheduled } = makeBatcher({ detailVisible: true, dirty: false });
  for (let index = 0; index < 100; index += 1) batcher.notify({ kind: 'profile_updated' });

  assert.equal(scheduled.length, 1, 'one replay page schedules one refresh');
  assert.deepEqual(calls, { workspace: 0, runs: 0, notice: 0 }, 'refresh is deferred');
  scheduled[0]!.callback();
  assert.deepEqual(calls, { workspace: 1, runs: 0, notice: 0 });
});

test('workspace refresh wins over trace refresh and dirty drafts only show a notice', () => {
  const clean = makeBatcher({ detailVisible: true, dirty: false });
  clean.batcher.notify({ kind: 'trace_updated' });
  clean.batcher.notify({ kind: 'profile_updated' });
  clean.scheduled[0]!.callback();
  assert.deepEqual(clean.calls, { workspace: 1, runs: 0, notice: 0 });

  const dirty = makeBatcher({ detailVisible: true, dirty: true });
  dirty.batcher.notify({ kind: 'profile_updated' });
  dirty.scheduled[0]!.callback();
  assert.deepEqual(dirty.calls, { workspace: 0, runs: 0, notice: 1 });
});
