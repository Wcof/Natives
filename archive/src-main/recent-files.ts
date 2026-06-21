import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

/**
 * 展开路径中的 ~ 为用户主目录
 */
function expandTilde(p: string): string {
  if (p === '~' || p.startsWith('~/') || p.startsWith('~\\')) {
    return path.join(os.homedir(), p.slice(1));
  }
  return p;
}

// ── Constants ──

/** 跳过目录（Natives2 移植） */
const IGNORE_DIRS = new Set([
  'node_modules', '.git', '.svn', '.hg', '.next', '.cache',
  '__pycache__', '.DS_Store', 'dist', 'out', 'build',
  '.idea', '.vscode', '.atomcode', '.mimocode', '.rtk',
]);

/** 最大扫描文件数 */
const MAX_FILES = 30_000;

/** 最大扫描时间（墙钟） */
const MAX_TIME_MS = 3_500;

/** 返回文件数 */
const RESULT_LIMIT = 60;

// ── Types ──

export interface WalkFile {
  path: string;
  mtime: number;
  size: number;
}

// ── BFS Walk ──

/**
 * BFS 目录遍历（Natives2 移植）
 * 跳过 IGNORE_DIRS，带文件数/时间上限
 *
 * 改进点：
 * - O(1) 出队（索引指针代替 shift()）
 * - 单次 stat 调用（stat 解析符号链接，失败时回退 lstat）
 * - 符号链接指向目录时正确入队
 */
export function walk(
  root: string,
  onFile: (file: WalkFile) => void,
  onDir?: (dir: string) => void,
  options?: { limit?: number; deadline?: number },
): void {
  root = expandTilde(root);
  const fileLimit = options?.limit ?? MAX_FILES;
  const deadline = options?.deadline ?? (Date.now() + MAX_TIME_MS);
  let count = 0;
  const queue: string[] = [root];
  let ptr = 0; // O(1) 出队指针

  while (ptr < queue.length) {
    // 时间上限检查
    if (Date.now() > deadline) break;

    const dir = queue[ptr++]!;
    let entries: string[];

    try {
      entries = fs.readdirSync(dir);
    } catch {
      continue; // 无权限跳过
    }

    for (const name of entries) {
      if (count >= fileLimit) break;
      if (name.startsWith('.') && name !== '.gitkeep') continue; // 跳过隐藏文件
      if (IGNORE_DIRS.has(name)) continue;

      const fullPath = path.join(dir, name);
      let stat: fs.Stats;

      // 优先使用 stat（自动解析符号链接），失败时回退 lstat
      try {
        stat = fs.statSync(fullPath);
      } catch {
        try {
          stat = fs.lstatSync(fullPath);
        } catch {
          continue;
        }
      }

      if (stat.isDirectory()) {
        queue.push(fullPath);
        onDir?.(fullPath);
      } else if (stat.isFile()) {
        count++;
        onFile({ path: fullPath, mtime: stat.mtimeMs, size: stat.size });
      }
    }
  }
}

// ── Recent Modified Files ──

/**
 * 获取最近修改的前 N 个文件
 * @param rootPath 根目录
 * @returns 按 mtime 降序排列的文件列表
 */
export function getRecentModifiedFiles(rootPath: string): WalkFile[] {
  const files: WalkFile[] = [];

  walk(
    rootPath,
    (file) => files.push(file),
    undefined,
    { limit: RESULT_LIMIT * 10 }, // 收集更多再做排序
  );

  // 按 mtime 降序
  files.sort((a, b) => b.mtime - a.mtime);
  return files.slice(0, RESULT_LIMIT);
}
