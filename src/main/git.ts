import { execFile } from 'child_process';
import * as path from 'path';
import { type GitStatus, type GitFileStatus } from '../types/file';

/**
 * 获取 Git 状态（porcelain 格式解析）
 * @param dirPath 目录路径
 * @returns Git 状态信息，非 git 目录返回 null
 */
export async function getGitStatus(dirPath: string): Promise<GitStatus | null> {
  try {
    const output = await execFilePromise('git', ['status', '--porcelain', '-b'], dirPath);
    const lines = output.split('\n').filter((l) => l.trim());

    // 解析分支行: ## main...origin/main
    const branchLine = lines.find((l) => l.startsWith('## '));
    if (!branchLine) return null;

    const branch = parseBranch(branchLine);

    // 解析文件状态
    const fileLines = lines.filter((l) => !l.startsWith('## '));
    const files: GitFileStatus[] = [];

    for (const line of fileLines) {
      const parsed = parseFileStatus(line);
      if (parsed) files.push(parsed);
    }

    return { root: dirPath, branch, files };
  } catch {
    return null; // 非 git 目录或 git 未安装
  }
}

function parseBranch(branchLine: string): string {
  // ## main...origin/main [ahead 1]
  // ## HEAD (no branch)
  const rest = branchLine.slice(3).trim(); // 去掉 "## "
  const aheadIdx = rest.indexOf('...');
  if (aheadIdx !== -1) return rest.slice(0, aheadIdx);
  return rest;
}

function parseFileStatus(line: string): GitFileStatus | null {
  // XY filename
  // R  old -> new
  // ?? filename
  const xy = line.slice(0, 2).trim();
  const rest = line.slice(3).trim();

  if (!xy) return null;

  // 映射 X/Y 状态到合并状态
  const status = mapStatus(xy);
  if (!status) return null;

  if (status === 'R' && rest.includes('->')) {
    const [oldPath, newPath] = rest.split('->').map((s) => s.trim());
    return { path: newPath || rest, status, oldPath };
  }

  return { path: rest, status };
}

function mapStatus(xy: string): GitFileStatus['status'] | null {
  // 取右侧（working tree）状态，优先 staged 状态
  // 标准 porcelain: XY where X=staged, Y=working
  // 合并: UU = unmerged
  if (xy === '??') return '??';
  if (xy === 'UU') return 'UU';
  if (xy === 'R ') return 'R';
  if (xy === 'RM') return 'R';
  if (xy.includes('R')) return 'R';
  if (xy.includes('M')) return 'M';
  if (xy.includes('A')) return 'A';
  if (xy.includes('D')) return 'D';
  return null;
}

function execFilePromise(cmd: string, args: string[], cwd: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { cwd, timeout: 10000 }, (err, stdout) => {
      if (err) reject(err);
      else resolve(stdout);
    });
  });
}

/**
 * 获取文件的 Git diff（HEAD vs working tree）
 * @param filePath 文件路径
 * @returns diff 文本，未跟踪或非 git 返回 null
 */
export async function getGitDiff(filePath: string): Promise<string | null> {
  const dir = path.dirname(filePath);

  try {
    // 先检查是否在 git 仓库中
    await execFilePromise('git', ['rev-parse', '--show-toplevel'], dir);
  } catch {
    return null;
  }

  try {
    const diff = await execFilePromise('git', ['diff', 'HEAD', '--', filePath], dir);
    if (!diff.trim()) return null;
    return diff;
  } catch {
    return null;
  }
}
