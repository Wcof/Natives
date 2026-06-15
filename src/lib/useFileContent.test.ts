import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { useFileContent } from './useFileContent';

describe('useFileContent', () => {
  it('should export useFileContent function', () => {
    assert.equal(typeof useFileContent, 'function');
  });

  it('should be a React hook (function name starts with use)', () => {
    assert.ok(useFileContent.name === 'useFileContent');
  });
});
