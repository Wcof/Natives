import assert from 'node:assert/strict';
import { test } from 'node:test';
import { shouldApplyHomeFallback } from './file-startup-navigation';

test('a quick-access navigation wins over the async home fallback', () => {
  assert.equal(shouldApplyHomeFallback('/', true), false);
  assert.equal(shouldApplyHomeFallback('/', false), true);
  assert.equal(shouldApplyHomeFallback('/Users/me/Downloads', false), false);
});
