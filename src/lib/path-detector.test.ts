import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { detectFilePaths } from './path-detector';

describe('PathDetector', () => {
  it('should detect absolute paths like /Users/name/file.ts', () => {
    const text = 'Error in /Users/test/src/main.ts: something went wrong';
    const matches = detectFilePaths(text, '/Users/test');
    assert.ok(matches.length >= 1);
    const m = matches.find((x) => x.path === '/Users/test/src/main.ts');
    assert.ok(m);
    assert.equal(m!.line, undefined);
  });

  it('should detect relative paths like ./src/file.ts', () => {
    const text = 'Found in ./src/utils/helper.ts line 42';
    const matches = detectFilePaths(text, '/project');
    assert.ok(matches.some((x) => x.path === '/project/src/utils/helper.ts'));
  });

  it('should detect paths with line numbers like file.ts:42', () => {
    const text = 'Error at app.ts:25:10';
    const matches = detectFilePaths(text, '/project');
    const m = matches.find((x) => x.path === '/project/app.ts');
    assert.ok(m);
    assert.equal(m!.line, 25);
    assert.equal(m!.column, 10);
  });

  it('should detect paths with ../ relative prefix', () => {
    const text = 'Import from ../lib/util.js';
    const matches = detectFilePaths(text, '/project/src');
    assert.ok(matches.some((x) => x.path === '/project/lib/util.js'));
  });

  it('should return empty array for text without paths', () => {
    const matches = detectFilePaths('Hello world, this is plain text', '/tmp');
    assert.equal(matches.length, 0);
  });

  it('should detect multiple paths in one line', () => {
    const text = 'Compare /tmp/a.ts and /tmp/b.ts';
    const matches = detectFilePaths(text, '/');
    assert.equal(matches.length, 2);
  });

  it('should handle paths with hyphens and underscores', () => {
    const text = 'See ./my-file_name.test.ts:1 and /tmp/my-test_file.txt';
    const matches = detectFilePaths(text, '/project');
    // Relative match
    assert.ok(matches.some((x) => x.path === '/project/my-file_name.test.ts'));
    // Absolute match
    assert.ok(matches.some((x) => x.path === '/tmp/my-test_file.txt'));
  });
});
