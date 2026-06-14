import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { execSync } from 'child_process';
import { getGitStatus } from './git';

const GIT_DIR = path.join(process.env.HOME || '~', '.natives-test', 'git-status-test');

describe('getGitStatus', () => {
  before(() => {
    fs.mkdirSync(GIT_DIR, { recursive: true });
    execSync('git init', { cwd: GIT_DIR });
    execSync('git config user.email "test@test.com"', { cwd: GIT_DIR });
    execSync('git config user.name "Test"', { cwd: GIT_DIR });
    fs.writeFileSync(path.join(GIT_DIR, 'initial.txt'), 'initial', 'utf-8');
    execSync('git add . && git commit -m "initial"', { cwd: GIT_DIR });
    // Create modified and untracked files
    fs.writeFileSync(path.join(GIT_DIR, 'initial.txt'), 'modified', 'utf-8');
    fs.writeFileSync(path.join(GIT_DIR, 'new.txt'), 'new file', 'utf-8');
  });

  after(() => {
    if (fs.existsSync(GIT_DIR)) {
      fs.rmSync(GIT_DIR, { recursive: true, force: true });
    }
  });

  it('should return GitStatus with branch and files', async () => {
    const result = await getGitStatus(GIT_DIR);
    assert.ok(result);
    assert.ok(['master', 'main'].includes(result!.branch));
    assert.ok(result!.files.length >= 1);
  });

  it('should detect modified (M) files', async () => {
    const result = await getGitStatus(GIT_DIR);
    const modified = result!.files.find((f) => f.status === 'M');
    assert.ok(modified);
    assert.ok(modified!.path.includes('initial.txt'));
  });

  it('should detect untracked (??) files', async () => {
    const result = await getGitStatus(GIT_DIR);
    const untracked = result!.files.find((f) => f.status === '??');
    assert.ok(untracked);
    assert.ok(untracked!.path.includes('new.txt'));
  });

  it('should return null for non-git directory', async () => {
    const nonGitDir = path.join(process.env.HOME || '~', '.natives-test', 'not-a-git-repo');
    fs.mkdirSync(nonGitDir, { recursive: true });
    try {
      const result = await getGitStatus(nonGitDir);
      assert.equal(result, null);
    } finally {
      fs.rmSync(nonGitDir, { recursive: true, force: true });
    }
  });
});
