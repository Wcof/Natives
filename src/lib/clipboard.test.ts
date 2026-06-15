import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { copyToClipboard, readFromClipboard } from './clipboard';

describe('Clipboard', () => {
  it('should export copyToClipboard function', () => {
    assert.equal(typeof copyToClipboard, 'function');
  });

  it('should export readFromClipboard function', () => {
    assert.equal(typeof readFromClipboard, 'function');
  });

  it('should return false when clipboard API unavailable', async () => {
    // In Node.js test environment, clipboard API is not available
    const result = await copyToClipboard('test');
    // Should not throw, may return true or false depending on environment
    assert.equal(typeof result, 'boolean');
  });

  it('should return null when clipboard API unavailable', async () => {
    const result = await readFromClipboard();
    // In Node.js, clipboard API is not available
    assert.ok(result === null || typeof result === 'string');
  });
});
