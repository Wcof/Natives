import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { parseClaudeStatsCache, parseRtkUsage } from './usage-tracker';

describe('UsageTracker', () => {
  describe('parseClaudeStatsCache', () => {
    it('should return null for invalid input', () => {
      assert.equal(parseClaudeStatsCache(null), null);
      assert.equal(parseClaudeStatsCache(undefined), null);
      assert.equal(parseClaudeStatsCache('string'), null);
    });

    it('should parse modelUsage correctly', () => {
      const result = parseClaudeStatsCache({
        modelUsage: {
          'claude-sonnet': {
            inputTokens: 1000,
            outputTokens: 200,
            cacheReadInputTokens: 500,
            cacheCreationInputTokens: 0,
            costUSD: 0.05,
          },
        },
        dailyModelTokens: [],
        totalSessions: 10,
        totalMessages: 500,
        firstSessionDate: '2026-01-01',
      });
      assert.ok(result);
      assert.deepEqual(result!.models['claude-sonnet'], {
        inputTokens: 1000,
        outputTokens: 200,
        cacheReadInputTokens: 500,
        cacheCreationInputTokens: 0,
        costUSD: 0.05,
      });
      assert.equal(result!.activity.totalSessions, 10);
      assert.equal(result!.activity.totalMessages, 500);
    });

    it('should aggregate dailyModelTokens for localTokens', () => {
      const result = parseClaudeStatsCache({
        modelUsage: {},
        dailyModelTokens: [
          { date: '2020-01-01', tokensByModel: { auto: 1000 } },
          { date: '2020-01-02', tokensByModel: { auto: 2000 } },
        ],
        totalSessions: 0,
        totalMessages: 0,
      });
      assert.ok(result);
      assert.equal(result!.localTokens.total, 3000);
    });

    it('should handle missing fields gracefully', () => {
      const result = parseClaudeStatsCache({});
      assert.ok(result);
      assert.deepEqual(result!.models, {});
      assert.equal(result!.localTokens.total, 0);
      assert.equal(result!.activity.totalSessions, 0);
    });
  });

  describe('parseRtkUsage', () => {
    it('should return zero stats for empty output', () => {
      const result = parseRtkUsage('');
      assert.equal(result.totalSaved, 0);
      assert.equal(result.totalCommands, 0);
      assert.equal(result.history.length, 0);
      assert.equal(result.topCommands.length, 0);
    });

    it('should parse pipe-delimited lines correctly', () => {
      const output = [
        'git status | 150 | 1718000000',
        'git diff | 300 | 1718000100',
        'cargo test | 500 | 1718000200',
      ].join('\n');
      const result = parseRtkUsage(output);
      assert.equal(result.totalSaved, 950);
      assert.equal(result.totalCommands, 3);
      assert.equal(result.history.length, 3);
      assert.equal(result.history[0]!.command, 'git status');
      assert.equal(result.history[0]!.tokensSaved, 150);
      assert.equal(result.history[0]!.timestamp, 1718000000);
    });

    it('should handle missing timestamp (defaults to Date.now())', () => {
      const before = Date.now();
      const result = parseRtkUsage('git status | 100');
      const after = Date.now();
      assert.equal(result.totalCommands, 1);
      assert.equal(result.history[0]!.tokensSaved, 100);
      assert.ok(result.history[0]!.timestamp >= before);
      assert.ok(result.history[0]!.timestamp <= after);
    });

    it('should handle non-numeric tokens_saved (defaults to 0)', () => {
      const result = parseRtkUsage('git status | abc | 1718000000');
      assert.equal(result.totalSaved, 0);
      assert.equal(result.totalCommands, 1);
      assert.equal(result.history[0]!.tokensSaved, 0);
    });

    it('should skip malformed lines with less than 2 parts', () => {
      const result = parseRtkUsage('single_field\ngit status | 100 | 1718000000');
      assert.equal(result.totalCommands, 1);
      assert.equal(result.totalSaved, 100);
    });

    it('should aggregate duplicate commands in topCommands', () => {
      const output = [
        'git status | 100 | 1718000000',
        'git status | 200 | 1718000100',
        'git diff | 50 | 1718000200',
      ].join('\n');
      const result = parseRtkUsage(output);
      assert.equal(result.topCommands.length, 2);
      // git status should be first (300 total) vs git diff (50 total)
      assert.equal(result.topCommands[0]!.command, 'git status');
      assert.equal(result.topCommands[0]!.count, 2);
      assert.equal(result.topCommands[0]!.totalSaved, 300);
      assert.equal(result.topCommands[1]!.command, 'git diff');
      assert.equal(result.topCommands[1]!.count, 1);
      assert.equal(result.topCommands[1]!.totalSaved, 50);
    });

    it('should limit topCommands to 10', () => {
      const lines = Array.from({ length: 15 }, (_, i) => `cmd${i} | ${i * 10} | 1718000000`);
      const result = parseRtkUsage(lines.join('\n'));
      assert.equal(result.totalCommands, 15);
      assert.equal(result.topCommands.length, 10);
      // Should be sorted by totalSaved descending — cmd14 (140) first
      assert.equal(result.topCommands[0]!.command, 'cmd14');
    });
  });
});
