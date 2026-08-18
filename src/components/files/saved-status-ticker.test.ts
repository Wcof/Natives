import assert from 'node:assert/strict';
import test from 'node:test';

import {
  formatSavedStatusLabel,
  startSavedStatusTicker,
  type SavedStatusTickerEnvironment,
} from './saved-status-ticker';

function createEnvironment(initialVisibility: 'hidden' | 'visible') {
  let visibility = initialVisibility;
  let nextTimerId = 0;
  const listeners = new Set<() => void>();
  const timers = new Map<number, () => void>();

  const environment: SavedStatusTickerEnvironment = {
    isVisible: () => visibility === 'visible',
    subscribeToVisibility: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    schedule: (callback) => {
      const id = ++nextTimerId;
      timers.set(id, callback);
      return id;
    },
    cancel: (id) => timers.delete(id),
  };

  return {
    environment,
    activeTimers: () => timers.size,
    listenerCount: () => listeners.size,
    fireNextTimer: () => {
      const next = timers.entries().next().value as [number, () => void] | undefined;
      assert.ok(next, 'expected an active timer');
      timers.delete(next[0]);
      next[1]();
    },
    setVisibility: (next: 'hidden' | 'visible') => {
      visibility = next;
      for (const listener of [...listeners]) listener();
    },
  };
}

test('visible ticker updates immediately and keeps exactly one scheduled tick', () => {
  const harness = createEnvironment('visible');
  let updates = 0;

  const stop = startSavedStatusTicker(100, () => { updates += 1; }, harness.environment);

  assert.equal(updates, 1);
  assert.equal(harness.activeTimers(), 1);
  assert.equal(harness.listenerCount(), 1);

  harness.fireNextTimer();
  assert.equal(updates, 2);
  assert.equal(harness.activeTimers(), 1);

  stop();
});

test('hidden ticker has no active timer and resumes with an immediate update', () => {
  const harness = createEnvironment('hidden');
  let updates = 0;

  const stop = startSavedStatusTicker(100, () => { updates += 1; }, harness.environment);

  assert.equal(updates, 0);
  assert.equal(harness.activeTimers(), 0);

  harness.setVisibility('visible');
  assert.equal(updates, 1, 'resume must refresh the relative time immediately');
  assert.equal(harness.activeTimers(), 1);

  harness.setVisibility('hidden');
  assert.equal(harness.activeTimers(), 0);

  stop();
});

test('repeated visibility events never create duplicate timers', () => {
  const harness = createEnvironment('visible');
  const stop = startSavedStatusTicker(100, () => undefined, harness.environment);

  harness.setVisibility('visible');
  harness.setVisibility('visible');

  assert.equal(harness.activeTimers(), 1);
  assert.equal(harness.listenerCount(), 1);

  stop();
});

test('cleanup cancels the timer and listener, including across input changes', () => {
  const harness = createEnvironment('visible');
  const labels: string[] = [];
  let savedAt = 100;
  let locale = 'en';

  let stop = startSavedStatusTicker(
    savedAt,
    () => labels.push(`${locale}:${savedAt}`),
    harness.environment,
  );
  stop();

  stop = startSavedStatusTicker(null, () => labels.push('unexpected'), harness.environment);
  assert.equal(harness.activeTimers(), 0, 'savedAt=null must not schedule work');
  assert.equal(harness.listenerCount(), 0, 'savedAt=null must not subscribe');
  stop();

  savedAt = 200;
  locale = 'zh';
  stop = startSavedStatusTicker(
    savedAt,
    () => labels.push(`${locale}:${savedAt}`),
    harness.environment,
  );

  assert.deepEqual(labels, ['en:100', 'zh:200']);
  assert.equal(harness.activeTimers(), 1);
  assert.equal(harness.listenerCount(), 1);

  stop();
  assert.equal(harness.activeTimers(), 0);
  assert.equal(harness.listenerCount(), 0);

  harness.setVisibility('visible');
  assert.deepEqual(labels, ['en:100', 'zh:200']);
});

test('savedAt calculation clamps future timestamps and applies current locale labels', () => {
  assert.equal(
    formatSavedStatusLabel(2_000, 1_000, {
      justNow: 'Auto-saved',
      secondsAgo: 'Saved {seconds}s ago',
    }),
    'Auto-saved',
  );
  assert.equal(
    formatSavedStatusLabel(1_000, 3_400, {
      justNow: '已自动保存',
      secondsAgo: '{seconds} 秒前已保存',
    }),
    '2 秒前已保存',
  );
});
