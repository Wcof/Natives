import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { parseUnifiedDiff, formatSize } from './diff-utils';

describe('DiffUtils', () => {
  describe('parseUnifiedDiff', () => {
    it('should parse a simple unified diff', () => {
      const diff = `--- a/file.ts
+++ b/file.ts
@@ -1,3 +1,4 @@
 line1
+added line
 line2
 line3`;
      const result = parseUnifiedDiff(diff);
      assert.ok(result);
      // Unified diff uses space prefix for unchanged lines
      assert.equal(result.original, ' line1\n line2\n line3');
      assert.equal(result.modified, ' line1\nadded line\n line2\n line3');
    });

    it('should handle diff with only additions', () => {
      const diff = `--- /dev/null
+++ b/new.ts
@@ -0,0 +1,2 @@
+line1
+line2`;
      const result = parseUnifiedDiff(diff);
      assert.ok(result);
      assert.equal(result.original, '');
      assert.equal(result.modified, 'line1\nline2');
    });

    it('should handle diff with only deletions', () => {
      const diff = `--- a/old.ts
+++ /dev/null
@@ -1,2 +0,0 @@
-line1
-line2`;
      const result = parseUnifiedDiff(diff);
      assert.ok(result);
      assert.equal(result.original, 'line1\nline2');
      assert.equal(result.modified, '');
    });

    it('should return null for empty diff', () => {
      const diff = `--- a/file.ts
+++ b/file.ts`;
      assert.equal(parseUnifiedDiff(diff), null);
    });
  });

  describe('formatSize', () => {
    it('should format bytes', () => assert.equal(formatSize(500), '500 B'));
    it('should format KB', () => assert.equal(formatSize(1536), '1.5 KB'));
    it('should format MB', () => assert.equal(formatSize(1048576), '1.0 MB'));
    it('should format GB', () => assert.equal(formatSize(1073741824), '1.0 GB'));
  });
});
