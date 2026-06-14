import * as fs from 'fs';
import * as path from 'path';
import { execFile } from 'child_process';

// ── Types ──

export interface ReleaseCheck {
  ok: boolean;
  message: string;
}

// ── Checks ──

/**
 * 检查 package.json 是否存在且有效
 */
export async function checkPackageJson(dirPath: string): Promise<ReleaseCheck> {
  const pkgPath = path.join(dirPath, 'package.json');
  try {
    const content = await fs.promises.readFile(pkgPath, 'utf-8');
    const pkg = JSON.parse(content);
    if (!pkg.version) return { ok: false, message: 'package.json missing version field' };
    return { ok: true, message: `Version ${pkg.version}` };
  } catch (err: any) {
    return { ok: false, message: err.code === 'ENOENT' ? 'package.json not found' : 'Invalid package.json' };
  }
}

/**
 * 检查 git 状态是否干净
 */
export async function checkGitStatus(dirPath: string): Promise<ReleaseCheck> {
  try {
    const stdout = await execPromise('git', ['status', '--porcelain'], dirPath);
    if (stdout.trim()) {
      const lines = stdout.trim().split('\n');
      return { ok: false, message: `${lines.length} uncommitted change(s)` };
    }
    return { ok: true, message: 'Working tree clean' };
  } catch {
    return { ok: false, message: 'Not a git repository' };
  }
}

/**
 * 检查 CHANGELOG.md 是否存在
 */
export async function checkChangelog(dirPath: string): Promise<ReleaseCheck> {
  const clPath = path.join(dirPath, 'CHANGELOG.md');
  try {
    await fs.promises.access(clPath);
    return { ok: true, message: 'CHANGELOG.md found' };
  } catch {
    return { ok: false, message: 'CHANGELOG.md not found' };
  }
}

/**
 * 检查 gh CLI 是否可用
 */
export async function checkGhCli(): Promise<ReleaseCheck> {
  try {
    const stdout = await execPromise('gh', ['--version'], '/tmp');
    if (stdout) return { ok: true, message: 'gh CLI available' };
    return { ok: false, message: 'gh CLI not found' };
  } catch {
    return { ok: false, message: 'gh CLI not found' };
  }
}

function execPromise(cmd: string, args: string[], cwd: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { cwd, timeout: 5000 }, (err, stdout) => {
      if (err) reject(err);
      else resolve(stdout);
    });
  });
}
