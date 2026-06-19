import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { compareVersions, parseGithubRelease } from './update-checker';

describe('UpdateChecker', () => {
  it('should compare versions correctly', () => {
    assert.ok(compareVersions('1.2.0', '1.1.0') > 0);
    assert.ok(compareVersions('1.1.0', '1.2.0') < 0);
    assert.ok(compareVersions('1.1.0', '1.1.0') === 0);
    assert.ok(compareVersions('1.1.1', '1.1.0') > 0);
    assert.ok(compareVersions('2.0.0', '1.9.9') > 0);
  });

  it('should parse GitHub release tag to version', () => {
    assert.equal(parseGithubRelease('v1.2.3'), '1.2.3');
    assert.equal(parseGithubRelease('1.2.3'), '1.2.3');
    assert.equal(parseGithubRelease('release-1.0.0'), '1.0.0');
  });
});
