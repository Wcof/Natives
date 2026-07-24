/**
 * Web 模式下的文件系统客户端（退化方案）
 *
 * Natives2 Tauri 模式默认走 window.nativesAPI 通道。
 * 此模块仅作为浏览器 dev 模式的退化方案保留 —— 所有方法返回错误提示。
 * 实际文件操作走 Tauri 原生 IPC，不在前端实现 HTTP fallback。
 */

import type { DiskUsageItem } from '@/types/file';

const WEB_MODE_UNAVAILABLE = 'Browser mode: file operations require Tauri IPC';

async function webUnavailable<T>(): Promise<T> {
  throw new Error(WEB_MODE_UNAVAILABLE);
}

export const webFsClient = {
  diskUsage(_dirPath: string): Promise<DiskUsageItem[]> {
    return webUnavailable();
  },
};