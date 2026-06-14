import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { parseUsageWindow, calcRemaining, type UsageWindow } from './usage-tracker';

describe('UsageTracker', () => {
  describe('calcRemaining', () => {
    it('should return remaining percentage', () => {
      const w: UsageWindow = { used: 25000, limit: 50000, resetAt: Date.now() + 3600000 };
      assert.equal(calcRemaining(w), 50);
    });

    it('should return 0 when limit is 0', () => {
      const w: UsageWindow = { used: 0, limit: 0, resetAt: 0 };
      assert.equal(calcRemaining(w), 0);
    });
  });

  describe('parseUsageWindow', () => {
    it('should parse a usage window from API response', () => {
      const result = parseUsageWindow({ used: 10000, limit: 50000, reset_at: Date.now() + 3600000 });
      assert.equal(result.used, 10000);
      assert.equal(result.limit, 50000);
    });

    it('should return zeroed window on invalid input', () => {
      const result = parseUsageWindow(null);
      assert.equal(result.used, 0);
      assert.equal(result.limit, 0);
    });
  });
});
