import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { execSync } from 'child_process';
import { getGitStatus, getGitDiff } from './git';

const GIT_DIR = path.join(process.env.HOME || '~', '.natives-test', 'git-test');

function setupGitRepo() {
  fs.mkdirSync(GIT_DIR, { recursive: true });
  execSync('git init', { cwd: GIT_DIR, stdio: 'ignore' });
  execSync('git config user.email "test@test.com"', { cwd: GIT_DIR, stdio: 'ignore' });
  execSync('git config user.name "Test"', { cwd: GIT_DIR, stdio: 'ignore' });
  fs.writeFileSync(path.join(GIT_DIR, 'initial.txt'), 'initial', 'utf-8');
  execSync('git add . && git commit -m "initial"', { cwd: GIT_DIR, stdio: 'ignore' });
  // Create modified and untracked files
  fs.writeFileSync(path.join(GIT_DIR, 'initial.txt'), 'modified', 'utf-8');
  fs.writeFileSync(path.join(GIT_DIR, 'new.txt'), 'new file', 'utf-8');
}

function cleanGitRepo() {
  if (fs.existsSync(GIT_DIR)) {
    fs.rmSync(GIT_DIR, { recursive: true, force: true });
  }
}

describe('getGitStatus', () => {
  before(setupGitRepo);
  after(cleanGitRepo);

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

describe('getGitDiff', () => {
  before(setupGitRepo);
  after(cleanGitRepo);

  it('should return diff for modified file', async () => {
    const filePath = path.join(GIT_DIR, 'initial.txt');
    const result = await getGitDiff(filePath);
    assert.ok(result);
    assert.ok(result!.includes('diff --git') || (result!.includes('---') && result!.includes('+++')));
  });

  it('should return null for untracked file', async () => {
    const filePath = path.join(GIT_DIR, 'new.txt');
    const result = await getGitDiff(filePath);
    assert.equal(result, null);
  });

  it('should return null for non-git file', async () => {
    const filePath = '/tmp/nonexistent-git-file.txt';
    const result = await getGitDiff(filePath);
    assert.equal(result, null);
  });
});
