'use client';

/**
 * files-api — 文件域访问 nativesAPI 的唯一入口
 *
 * 规则（前后端分离的前端侧约束）：
 * - 任何组件/hook 需要 fs / fsWatch / search / archive / disk / thumbnail
 *   能力时，必须 import 本模块，禁止直接触碰 `window.nativesAPI` 或
 *   `(window as any)`——接口类型在 tauri-adapter 的 NativesAPI 里只定义一遍，
 *   本模块负责把「可能不存在」收敛为显式的可用性检查 + 统一错误。
 * - 非 Tauri 环境（浏览器 dev）：`hasNativeFiles()` 返回 false，取用会抛
 *   `FilesApiUnavailableError`，调用方要么先探测、要么捕获降级。
 */

import type { NativesAPI } from '@/lib/tauri-adapter';

export class FilesApiUnavailableError extends Error {
  constructor(section: string) {
    super(`[files-api] nativesAPI.${section} not available (Tauri IPC required)`);
    this.name = 'FilesApiUnavailableError';
  }
}

function root(): NonNullable<Window['nativesAPI']> | null {
  if (typeof window === 'undefined') return null;
  return window.nativesAPI ?? null;
}

function section<K extends keyof NativesAPI>(key: K): NativesAPI[K] {
  const api = root()?.[key];
  if (!api) throw new FilesApiUnavailableError(String(key));
  return api;
}

/** 文件系统能力是否可用（浏览器 dev 模式为 false） */
export function hasNativeFiles(): boolean {
  return !!root()?.fs;
}

/** 文件系统读写/导航（listDir/read/write/rename/trash/move/copy/stat/roots/openWith…） */
export function fsApi(): NativesAPI['fs'] {
  return section('fs');
}

/** 文件变更监听（start/stop/onChange）；不可用时返回 null（调用方静默降级） */
export function fsWatchApiOrNull(): NativesAPI['fsWatch'] | null {
  return root()?.fsWatch ?? null;
}

/** 文件名模糊 / 全文 grep / Spotlight 搜索 */
export function searchApi(): NativesAPI['search'] {
  return section('search');
}

/** 压缩包只读清单 */
export function archiveApi(): NativesAPI['archive'] {
  return section('archive');
}

/** 磁盘占用 / 系统指标 */
export function diskApi(): NativesAPI['disk'] {
  return section('disk');
}

/** 缩略图生成 */
export function thumbnailApi(): NativesAPI['thumbnail'] {
  return section('thumbnail');
}
