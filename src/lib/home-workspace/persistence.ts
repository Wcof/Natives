'use client';

/**
 * Home 文档持久化（Home Patch 决策 8）。
 *
 * 单份版本化 JSON 存 settings K/V（`settings:home_workspace`），V1 不新建
 * Workspace 表。拖动/缩放过程中**不写 SQLite** —— 只有 stop / debounce /
 * flush 才持久化（ADR-0020 §3）。
 */

import { DEFAULT_DOCUMENT, type HomeWorkspaceDocument } from './model';

const STORAGE_KEY = 'settings:home_workspace';
const SAVE_DEBOUNCE_MS = 800;

/** 从持久层加载文档；损坏/未知版本回退默认（可恢复，不崩溃）。 */
export async function loadHomeWorkspaceDocument(): Promise<HomeWorkspaceDocument> {
  try {
    const api = window.nativesAPI;
    if (!api?.db?.get) return DEFAULT_DOCUMENT;
    const raw = await api.db.get(STORAGE_KEY);
    if (typeof raw !== 'string' || !raw) return DEFAULT_DOCUMENT;
    const parsed = JSON.parse(raw) as HomeWorkspaceDocument;
    if (parsed.schemaVersion !== 1 || !parsed.instances || !parsed.layouts) {
      return DEFAULT_DOCUMENT;
    }
    return parsed;
  } catch {
    return DEFAULT_DOCUMENT;
  }
}

/** 生成带防抖的持久化器：连续 stop 事件合并为一次写。 */
export function createDocumentSaver() {
  let timer: ReturnType<typeof setTimeout> | null = null;

  const flush = async (document: HomeWorkspaceDocument) => {
    try {
      const api = window.nativesAPI;
      if (!api?.db?.set) return;
      await api.db.set(STORAGE_KEY, JSON.stringify(document));
    } catch {
      // 浏览器 dev 模式无 IPC；会话内状态不受影响
    }
  };

  const schedule = (document: HomeWorkspaceDocument) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void flush(document);
    }, SAVE_DEBOUNCE_MS);
  };

  return { schedule, flush };
}
