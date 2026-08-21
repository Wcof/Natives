import assert from 'node:assert/strict';
import test from 'node:test';
import {
  activeOperations,
  canTransition,
  countByKind,
  createOperation,
  withStatus,
} from './activity-center';

test('state machine rejects illegal transitions', () => {
  assert.equal(canTransition('queued', 'running'), true);
  assert.equal(canTransition('queued', 'succeeded'), false);
  assert.equal(canTransition('running', 'failed'), true);
  assert.equal(canTransition('succeeded', 'running'), false);
  assert.equal(canTransition('running', 'cancelled'), true);
});

test('withStatus applies only legal transitions deterministically', () => {
  const op = createOperation('op-1', 'usage-sync', 'Sync usage');
  assert.equal(op.status, 'queued');

  const running = withStatus(op, 'running');
  assert.equal(running.status, 'running');
  assert.equal(running.finishedAt, null);

  // 非法转移（queued → succeeded）被静默拒绝
  const skipped = withStatus(op, 'succeeded');
  assert.equal(skipped.status, 'queued');

  const failed = withStatus(running, 'failed', 'timeout');
  assert.equal(failed.status, 'failed');
  assert.equal(failed.error, 'timeout');
  assert.ok(failed.finishedAt !== null);
});

test('activeOperations filters queued/running only', () => {
  const ops = [
    createOperation('a', 'usage-sync', 'A'),
    withStatus(withStatus(createOperation('b', 'app-start', 'B'), 'running'), 'succeeded'),
    withStatus(createOperation('c', 'proxy-apply', 'C'), 'running'),
  ];
  const active = activeOperations({ operations: ops });
  assert.deepEqual(active.map((op) => op.id), ['a', 'c']);
});

test('countByKind counts only active operations', () => {
  const ops = [
    withStatus(withStatus(createOperation('a', 'usage-sync', 'A'), 'running'), 'succeeded'),
    withStatus(createOperation('b', 'app-start', 'B'), 'running'),
    withStatus(withStatus(createOperation('c', 'app-start', 'C'), 'running'), 'failed'),
    createOperation('d', 'app-start', 'D'),
  ];
  const counts = countByKind({ operations: ops }, ['usage-sync', 'app-start']);
  assert.deepEqual(counts, { 'usage-sync': 0, 'app-start': 2 });
});
