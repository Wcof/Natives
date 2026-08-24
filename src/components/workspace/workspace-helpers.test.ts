/**
 * QA-01 regression tests — workspace toolbar / rename / sync label / i18n integrity.
 *
 * Covers the pure logic behind WS-04 (rename validation), WS-05 (sync relative-time
 * label) and the time-range localization, without spinning up a DOM. The component
 * barrel + widget-keys.test.ts already guard widget i18n completeness; this file
 * guards the toolbar-facing pieces.
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { relativeTime, validateRename } from './workspace-helpers';
import { t } from '@/i18n';
import type { TimeRange } from '@/lib/workspace/widgets/types';

const TIME_RANGES: TimeRange[] = ['today', '7d', '30d', '90d'];

describe('relativeTime (WS-05 sync label)', () => {
  it('formats seconds/minutes/hours/days per locale', () => {
    assert.equal(relativeTime('zh', 0), '0 秒');
    assert.equal(relativeTime('zh', 45_000), '45 秒');
    assert.equal(relativeTime('zh', 5 * 60_000), '5 分钟');
    assert.equal(relativeTime('zh', 3 * 3_600_000), '3 小时');
    assert.equal(relativeTime('zh', 2 * 86_400_000), '2 天');
    assert.equal(relativeTime('en', 45_000), '45s');
    assert.equal(relativeTime('en', 5 * 60_000), '5m');
    assert.equal(relativeTime('en', 3 * 3_600_000), '3h');
    assert.equal(relativeTime('en', 2 * 86_400_000), '2d');
  });

  it('clamps negative deltas to zero (clock skew)', () => {
    assert.equal(relativeTime('en', -5000), '0s');
    assert.equal(relativeTime('zh', -5000), '0 秒');
  });
});

describe('validateRename (WS-04)', () => {
  it('rejects empty / whitespace-only names', () => {
    assert.equal(validateRename('', 'Old'), 'required');
    assert.equal(validateRename('   ', 'Old'), 'required');
  });

  it('rejects names longer than 80 characters', () => {
    const over = 'x'.repeat(81);
    const exact = 'x'.repeat(80);
    assert.equal(validateRename(over, 'Old'), 'tooLong');
    assert.equal(validateRename(exact, 'Old'), null);
  });

  it('treats an unchanged name as a no-op, not an error', () => {
    assert.equal(validateRename('Same', 'Same'), null);
    // trimmed-equality: leading/trailing whitespace equal after trim is still a no-op
    assert.equal(validateRename('  Same  ', 'Same'), null);
  });

  it('accepts a valid new name', () => {
    assert.equal(validateRename('New Name', 'Old'), null);
  });
});

describe('time-range localization (WS-01)', () => {
  it('every time-range key resolves in zh and en without falling back to the key', () => {
    const keys = TIME_RANGES.map((r) => `workspace.timeRange${r === 'today' ? 'Today' : r}`);
    for (const key of keys) {
      for (const locale of ['zh', 'en'] as const) {
        const resolved = t(locale, key);
        assert.notEqual(resolved, key, `time-range key "${key}" missing in ${locale}`);
        assert.ok(typeof resolved === 'string' && resolved.trim().length > 0, `time-range key "${key}" empty in ${locale}`);
      }
    }
  });
});

describe('sync result + rename error i18n keys (WS-04/WS-05)', () => {
  const KEYS = [
    'workspace.syncBtn',
    'workspace.syncBtnRunning',
    'workspace.syncResultOk',
    'workspace.syncResultPartial',
    'workspace.syncResultFailed',
    'workspace.syncRetry',
    'workspace.lastSynced',
    'workspace.neverSynced',
    'workspace.renameErrorRequired',
    'workspace.renameErrorTooLong',
    'workspace.renameErrorConflict',
  ];
  it('all toolbar/sync/rename keys resolve in zh and en', () => {
    for (const key of KEYS) {
      for (const locale of ['zh', 'en'] as const) {
        const resolved = t(locale, key);
        assert.notEqual(resolved, key, `key "${key}" missing in ${locale}`);
        assert.ok(typeof resolved === 'string' && resolved.trim().length > 0, `key "${key}" empty in ${locale}`);
      }
    }
  });
});
