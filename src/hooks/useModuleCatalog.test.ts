/**
 * Channel filter for Personal Creations catalog (R-T6).
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

function shouldReloadModuleCatalog(channel: string): boolean {
  return channel === 'module';
}

describe('module catalog channel filter (R-T6)', () => {
  it('accepts module channel', () => {
    assert.equal(shouldReloadModuleCatalog('module'), true);
  });

  it('ignores non-module channels', () => {
    for (const ch of ['notification', 'theme', 'locale', 'env', 'file:changed', 'module_data']) {
      assert.equal(shouldReloadModuleCatalog(ch), false, ch);
    }
  });
});
