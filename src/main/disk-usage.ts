import * as fs from 'fs';
import * as path from 'path';
import { execFile } from 'child_process';
import { type DiskUsageItem } from '../types/file';

/**
 * 获取目录下各项的磁盘用量
 * @param dirPath 目录路径
 * @returns 用量列表（按大小降序）
 */
export async function getDiskUsage(dirPath: string): Promise<DiskUsageItem[]> {
  const stat = await fs.promises.stat(dirPath);
  if (!stat.isDirectory()) {
    throw Object.assign(new Error(`Not a directory: ${dirPath}`), { code: 'ENOTDIR' });
  }

  const entries = await fs.promises.readdir(dirPath);
  const items: DiskUsageItem[] = [];

  for (const name of entries) {
    if (name.startsWith('.')) continue; // 跳过隐藏文件
    const fullPath = path.join(dirPath, name);

    try {
      const entryStat = await fs.promises.stat(fullPath);

      if (entryStat.isDirectory()) {
        // 目录用 du -sk
        const sizeKB = await getDirSizeKB(fullPath);
        const sizeBytes = sizeKB * 1024;
        items.push({
          name,
          path: fullPath,
          isDir: true,
          size: sizeBytes,
          sizeFormatted: formatSize(sizeBytes),
        });
      } else {
        items.push({
          name,
          path: fullPath,
          isDir: false,
          size: entryStat.size,
          sizeFormatted: formatSize(entryStat.size),
        });
      }
    } catch {
      // 跳过无权限条目
    }
  }

  // 按大小降序排序
  items.sort((a, b) => b.size - a.size);

  return items;
}

/**
 * 使用 du -sk 获取目录大小（KB）
 */
function getDirSizeKB(dirPath: string): Promise<number> {
  return new Promise((resolve, reject) => {
    execFile('du', ['-sk', dirPath], { timeout: 10000 }, (err, stdout) => {
      if (err) {
        reject(err);
        return;
      }
      const match = stdout.match(/^(\d+)/);
      if (match) {
        resolve(parseInt(match[1]!, 10));
      } else {
        resolve(0);
      }
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
