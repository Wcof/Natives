import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  RUN_STATUS_KEYS,
  runStatusLabel,
  type DisplayRunStatus,
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

test('every display status maps to a non-empty label in both locales', () => {
  for (const status of ALL_STATUSES) {
    const key = RUN_STATUS_KEYS[status];
    assert.ok(key, `missing key entry for ${status}`);
    const zh = runStatusLabel('zh', status);
    const en = runStatusLabel('en', status);
    assert.notEqual(zh, key, `${status} resolves to its key in zh (missing dictionary value?)`);
    assert.notEqual(en, key, `${status} resolves to its key in en (missing dictionary value?)`);
    assert.ok(zh.trim().length > 0, `${status} zh must not be blank`);
    assert.ok(en.trim().length > 0, `${status} en must not be blank`);
    assert.notEqual(zh, en, `${status} zh/en labels should not be identical`);
  }
});

test('table has no extra keys beyond the known status list', () => {
  assert.deepEqual(Object.keys(RUN_STATUS_KEYS).sort(), [...ALL_STATUSES].sort());
});

test('runStatusLabel resolves locale-aware for every status', () => {
  for (const status of ALL_STATUSES) {
    assert.equal(runStatusLabel('zh', status), runStatusLabel('zh-CN', status));
    assert.notEqual(runStatusLabel('zh', status), runStatusLabel('en', status));
  }
});

test('runStatusLabel handles idle, unknown, and overrides', () => {
  assert.equal(runStatusLabel('zh', null), '待命');
  assert.equal(runStatusLabel('en', undefined), 'Idle');
  assert.equal(runStatusLabel('zh', 'made_up_status'), 'made_up_status');
  const overrides = { interrupted: 'runStatus.paused' } as const;
  assert.equal(runStatusLabel('zh', 'interrupted', overrides), '已暂停');
  assert.equal(runStatusLabel('en', 'interrupted', overrides), 'Paused');
  // Overrides never leak into other statuses.
  assert.equal(runStatusLabel('zh', 'failed', overrides), '失败');
});
