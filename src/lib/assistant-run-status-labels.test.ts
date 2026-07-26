import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  RUN_STATUS_LABELS,
  runStatusLabel,
  type DisplayRunStatus,
  type RunStatusLabel,
} from './assistant-run-status-labels';

// Wire RunStatus union + display-only pseudo statuses; keep in sync with
// assistant-protocol RunStatus when the protocol grows.
const ALL_STATUSES: DisplayRunStatus[] = [
  'created',
  'connecting',
  'queued',
  'preparing',
  'reasoning',
  'generating',
  'running',
  'running_tool',
  'waiting_permission',
  'waiting_user',
  'waiting_subagent',
  'compacting',
  'reconnecting',
  'recovering',
  'cancelling',
  'completed',
  'failed',
  'cancelled',
  'interrupted',
  'background_watching',
];

test('every display status has non-empty zh and en labels', () => {
  for (const status of ALL_STATUSES) {
    const label: RunStatusLabel | undefined = RUN_STATUS_LABELS[status];
    assert.ok(label, `missing label entry for ${status}`);
    assert.equal(typeof label.zh, 'string', `${status}.zh must be string`);
    assert.equal(typeof label.en, 'string', `${status}.en must be string`);
    assert.ok(label.zh.trim().length > 0, `${status}.zh must not be blank`);
    assert.ok(label.en.trim().length > 0, `${status}.en must not be blank`);
  }
});

test('table has no extra keys beyond the known status list', () => {
  assert.deepEqual(Object.keys(RUN_STATUS_LABELS).sort(), [...ALL_STATUSES].sort());
});

test('runStatusLabel resolves zh/en for every status', () => {
  for (const status of ALL_STATUSES) {
    assert.equal(runStatusLabel(status, true), RUN_STATUS_LABELS[status].zh);
    assert.equal(runStatusLabel(status, false), RUN_STATUS_LABELS[status].en);
  }
});

test('runStatusLabel handles idle, unknown, and overrides', () => {
  assert.equal(runStatusLabel(null, true), '待命');
  assert.equal(runStatusLabel(undefined, false), 'Idle');
  assert.equal(runStatusLabel('made_up_status', true), 'made_up_status');
  const overrides = { interrupted: { zh: '已暂停', en: 'Paused' } } as const;
  assert.equal(runStatusLabel('interrupted', true, overrides), '已暂停');
  assert.equal(runStatusLabel('interrupted', false, overrides), 'Paused');
  // Overrides never leak into other statuses.
  assert.equal(runStatusLabel('failed', true, overrides), '失败');
});
