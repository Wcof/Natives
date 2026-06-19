import * as fs from 'fs';
import * as path from 'path';
import { execFile } from 'child_process';
import { type DiskUsageItem } from '../types/file';

/**
 * 获取目录下各项的磁盘用量
 * 优化：du -d1 一次获取所有目录大小，stat 获取文件大小（无需子进程）
 */
export async function getDiskUsage(dirPath: string): Promise<DiskUsageItem[]> {
  const stat = await fs.promises.stat(dirPath);
  if (!stat.isDirectory()) {
    throw Object.assign(new Error(`Not a directory: ${dirPath}`), { code: 'ENOTDIR' });
  }

  const entries = await fs.promises.readdir(dirPath);
  const visibleEntries = entries.filter((name) => !name.startsWith('.'));

  if (visibleEntries.length === 0) return [];

  // Classify entries
  const filePaths: string[] = [];
  const dirNames: string[] = [];

  for (const name of visibleEntries) {
    const fullPath = path.join(dirPath, name);
    try {
      const entryStat = await fs.promises.stat(fullPath);
      if (entryStat.isDirectory()) {
        dirNames.push(name);
      } else {
        filePaths.push(fullPath);
      }
    } catch {
      // Skip entries we can't stat
    }
  }

  // Get file sizes via Node.js fs.stat (fast, no subprocess)
  const fileSizeMap = new Map<string, number>();
  const statResults = await Promise.allSettled(
    filePaths.map((fp) => fs.promises.stat(fp).then((s) => ({ path: fp, size: s.size }))),
  );
  for (const r of statResults) {
    if (r.status === 'fulfilled') {
      fileSizeMap.set(r.value.path, r.value.size);
    }
  }

  // Get directory sizes via single du call (one subprocess instead of N)
  let dirSizeMap: Map<string, number> | null = null;
  if (dirNames.length > 0) {
    try {
      dirSizeMap = await getAllDirSizes(dirPath, new Set(dirNames));
    } catch {
      // du failed — fall through with null sizes
    }
  }

  // Build result
  const items: DiskUsageItem[] = [];

  for (const name of visibleEntries) {
    const fullPath = path.join(dirPath, name);
    const isDir = dirNames.includes(name);
    const size = isDir
      ? (dirSizeMap?.get(name) ?? 0)
      : (fileSizeMap.get(fullPath) ?? 0);

    items.push({
      name,
      path: fullPath,
      isDir,
      size,
      sizeFormatted: formatSize(size),
    });
  }

  items.sort((a, b) => b.size - a.size);
  return items;
}

/**
 * 使用单次 du -d1 获取所有一级子目录的大小（字节）
 */
function getAllDirSizes(dirPath: string, targetDirs: Set<string>): Promise<Map<string, number>> {
  return new Promise((resolve, reject) => {
    execFile('du', ['-d1', '-k', '--', dirPath], { timeout: 30000 }, (err, stdout) => {
      if (err && !stdout) {
        reject(err);
        return;
      }

      const map = new Map<string, number>();
      for (const line of stdout.split('\n')) {
        const tabIdx = line.indexOf('\t');
        if (tabIdx === -1) continue;

        const sizeKB = parseInt(line.substring(0, tabIdx), 10);
        if (isNaN(sizeKB)) continue;

        const rawPath = line.substring(tabIdx + 1);
        const name = path.basename(rawPath);

        // 只收集目标目录（跳过 du 输出的第一行——即 dirPath 自身）
        if (targetDirs.has(name)) {
          map.set(name, sizeKB * 1024);
        }
      }

      resolve(map);
    });
  });
}

/**
 * 格式化大小（人类可读）
 */
function formatSize(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = bytes;
  let unitIdx = 0;

  while (size >= 1024 && unitIdx < units.length - 1) {
    size /= 1024;
    unitIdx++;
  }

  return `${size.toFixed(1)} ${units[unitIdx]!}`;
}
