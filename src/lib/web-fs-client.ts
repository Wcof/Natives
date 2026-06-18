/**
 * Web 模式下的文件系统客户端
 * 当 window.nativesAPI 不可用时（浏览器 dev 模式），通过 Next.js API Routes 实现文件操作
 */

import type { FileEntry } from '@/types/file';
import type { DiskUsageItem } from '@/types/file';

async function fetchJSON<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `HTTP ${res.status}`);
  }
  return res.json();
}

export const webFsClient = {
  listDir(dirPath: string, options?: { sortBy?: string; sortDir?: string; showHidden?: boolean }): Promise<FileEntry[]> {
    const params = new URLSearchParams({ path: dirPath });
    if (options?.sortBy) params.set('sortBy', options.sortBy);
    if (options?.sortDir) params.set('sortDir', options.sortDir);
    if (options?.showHidden) params.set('showHidden', 'true');
    return fetchJSON<FileEntry[]>(`/api/fs/listDir?${params}`);
  },

  recentFiles(root: string): Promise<Array<{ path: string; mtime: number; size: number }>> {
    return fetchJSON(`/api/fs/recentFiles?path=${encodeURIComponent(root)}`);
  },

  async renameEntry(oldPath: string, newPath: string): Promise<{ ok: boolean; error?: string }> {
    try {
      return await fetchJSON('/api/fs/renameEntry', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ oldPath, newPath }),
      });
    } catch (err: any) {
      return { ok: false, error: err.message };
    }
  },

  async trashEntry(filePath: string): Promise<{ ok: boolean; error?: string }> {
    try {
      return await fetchJSON('/api/fs/trashEntry', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: filePath }),
      });
    } catch (err: any) {
      return { ok: false, error: err.message };
    }
  },

  async createEntry(targetPath: string, type: string): Promise<{ ok: boolean; error?: string }> {
    try {
      return await fetchJSON('/api/fs/createEntry', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: targetPath, type }),
      });
    } catch (err: any) {
      return { ok: false, error: err.message };
    }
  },

  diskUsage(dirPath: string): Promise<DiskUsageItem[]> {
    return fetchJSON<DiskUsageItem[]>(`/api/fs/du?path=${encodeURIComponent(dirPath)}`);
  },
};
