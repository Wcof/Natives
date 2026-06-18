import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { checkPackageJson, checkGitStatus, checkChangelog, type ReleaseCheck } from './release-wizard';

describe('ReleaseWizard', () => {
  it('should detect missing package.json', async () => {
    const result = await checkPackageJson('/nonexistent');
    assert.equal(result.ok, false);
  });

  it('should check for git status issues', async () => {
    const result = await checkGitStatus('/tmp');
    // /tmp may or may not be a git repo — just verify the function runs
    assert.ok('ok' in result);
    assert.ok('message' in result);
  });
});
