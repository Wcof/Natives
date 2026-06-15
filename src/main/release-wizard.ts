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

// ── Release Inspection ──

export interface ReleaseInspection {
  currentVersion: string;
  checks: { name: string; ok: boolean; message: string }[];
}

/**
 * 检查项目发布就绪状态
 */
export async function inspectProject(projectPath: string): Promise<ReleaseInspection> {
  const pkgCheck = await checkPackageJson(projectPath);
  const gitCheck = await checkGitStatus(projectPath);
  const clCheck = await checkChangelog(projectPath);
  const ghCheck = await checkGhCli();

  let currentVersion = '0.0.0';
  try {
    const content = await fs.promises.readFile(path.join(projectPath, 'package.json'), 'utf-8');
    const pkg = JSON.parse(content);
    if (pkg.version) currentVersion = pkg.version;
  } catch { /* ignore */ }

  return {
    currentVersion,
    checks: [
      { name: 'package.json', ...pkgCheck },
      { name: 'git status', ...gitCheck },
      { name: 'CHANGELOG.md', ...clCheck },
      { name: 'gh CLI', ...ghCheck },
    ],
  };
}

// ── Command Sequence ──

export interface CommandStep {
  label: string;
  command: string;
}

/**
 * 生成发布命令序列
 */
export function getCommandSequence(projectPath: string, version: string): CommandStep[] {
  return [
    { label: 'Update version', command: `npm version ${version} --no-git-tag-version` },
    { label: 'Git commit', command: `git add -A && git commit -m "release: v${version}"` },
    { label: 'Git tag', command: `git tag v${version}` },
    { label: 'Push', command: 'git push && git push --tags' },
  ];
}

/**
 * 准备发布（更新版本号 + CHANGELOG）
 */
export async function prepareRelease(projectPath: string, version: string): Promise<void> {
  // 更新 package.json 版本
  const pkgPath = path.join(projectPath, 'package.json');
  const content = await fs.promises.readFile(pkgPath, 'utf-8');
  const pkg = JSON.parse(content);
  pkg.version = version;
  await fs.promises.writeFile(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf-8');

  // 更新 CHANGELOG.md 的 Unreleased 段落
  const clPath = path.join(projectPath, 'CHANGELOG.md');
  try {
    let changelog = await fs.promises.readFile(clPath, 'utf-8');
    const today = new Date().toISOString().slice(0, 10);
    changelog = changelog.replace(
      /## Unreleased/i,
      `## Unreleased\n\n## [${version}] - ${today}`,
    );
    await fs.promises.writeFile(clPath, changelog, 'utf-8');
  } catch { /* CHANGELOG.md may not exist */ }
}
