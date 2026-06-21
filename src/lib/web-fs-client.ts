/**
 * Web 模式下的文件系统客户端（退化方案）
 *
 * Natives2 Tauri 模式默认走 window.nativesAPI 通道。
 * 此模块仅作为浏览器 dev 模式的退化方案保留 —— 所有方法返回错误提示。
 * 实际文件操作走 Tauri 原生 IPC，不在前端实现 HTTP fallback。
 */

import type { FileEntry } from '@/types/file';
import type { DiskUsageItem } from '@/types/file';

const WEB_MODE_UNAVAILABLE = 'Browser mode: file operations require Tauri IPC';

async function webUnavailable<T>(): Promise<T> {
  throw new Error(WEB_MODE_UNAVAILABLE);
}

export const webFsClient = {
  listDir(_dirPath: string, _options?: { sortBy?: string; sortDir?: string; showHidden?: boolean }): Promise<FileEntry[]> {
    return webUnavailable();
  },

  recentFiles(_root: string): Promise<Array<{ path: string; mtime: number; size: number }>> {
    return webUnavailable();
  },

  renameEntry(_oldPath: string, _newPath: string): Promise<{ ok: boolean; error?: string }> {
    return webUnavailable();
  },

  trashEntry(_filePath: string): Promise<{ ok: boolean; error?: string }> {
    return webUnavailable();
  },

  createEntry(_targetPath: string, _type: string): Promise<{ ok: boolean; error?: string }> {
    return webUnavailable();
  },

  diskUsage(_dirPath: string): Promise<DiskUsageItem[]> {
    return webUnavailable();
  },
};