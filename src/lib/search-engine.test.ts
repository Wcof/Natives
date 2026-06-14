import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { calculateScore, findSubsequence, findStreaks } from './search-engine';

describe('SearchEngine', () => {
  describe('findSubsequence', () => {
    it('should find matching positions for "ts" in "main.ts"', () => {
      const result = findSubsequence('ts', 'main.ts');
      assert.ok(result);
      assert.deepEqual(result, [5, 6]);
    });

    it('should return null when no match', () => {
      const result = findSubsequence('xyz', 'main.ts');
      assert.equal(result, null);
    });
  });

  describe('findStreaks', () => {
    it('should identify a single streak of 2', () => {
      assert.deepEqual(findStreaks([5, 6]), [2]);
    });

    it('should identify two separate streaks', () => {
      assert.deepEqual(findStreaks([0, 1, 5, 6]), [2, 2]);
    });
  });

  describe('calculateScore', () => {
    it('should return -1 for no match', () => {
      assert.equal(calculateScore('xyz', 'main.ts'), -1);
    });

    it('should score exact match higher than subsequence', () => {
      const exact = calculateScore('main', 'main.ts');
      const partial = calculateScore('mn', 'main.ts');
      assert.ok(exact > partial);
    });

    it('should prefer consecutive matches over scattered matches', () => {
      const consecutive = calculateScore('ab', 'xabx.txt');
      const scattered = calculateScore('ab', 'xa_xb.txt');
      assert.ok(consecutive > scattered);
    });

    it('should prefer matches at word boundaries', () => {
      const wordBoundary = calculateScore('ts', 'my-file.ts');
      const midWord = calculateScore('ts', 'pots.ts'); // 'ts' is at end in both
      assert.ok(wordBoundary > midWord);
    });

    it('should prefer matches at start of filename', () => {
      const early = calculateScore('re', 'readme.md');
      const late = calculateScore('re', 'core.md');
      assert.ok(early > late);
    });

    it('should prefer shorter filenames', () => {
      const short = calculateScore('ab', 'ab.txt');
      const long = calculateScore('ab', 'a-very-long-filename-with-ab.txt');
      assert.ok(short > long);
    });

    it('should add bonus for directories', () => {
      const dirScore = calculateScore('src', 'src', true);
      const fileScore = calculateScore('src', 'src', false);
      assert.ok(dirScore > fileScore);
    });

    it('should add bonus for recently modified files', () => {
      const recent = calculateScore('test', 'test.ts', false, Date.now());
      const old = calculateScore('test', 'test.ts', false, Date.now() - 365 * 24 * 60 * 60 * 1000);
      assert.ok(recent > old);
    });

    it('should handle empty query', () => {
      const result = findSubsequence('', 'main.ts');
      assert.deepEqual(result, []);
      // Empty query matches everything
    });

    it('should handle case insensitive matching', () => {
      const lower = calculateScore('readme', 'README.md');
      const upper = calculateScore('README', 'readme.md');
      assert.ok(lower > -1);
      assert.ok(upper > -1);
    });

    it('should score exact filename match highest', () => {
      const exact = calculateScore('Readme.md', 'Readme.md');
      const partial = calculateScore('Readme', 'Readme.md');
      assert.ok(exact > partial);
    });
  });
});
