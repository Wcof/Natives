// ── Recent Files Adapter（B-022） ──
// 仍复用 recent-files/files domain（@/lib/recent-files-client）。
// 迁移源只暴露 useRecentFiles hook；此处动态解析同名 imperative 函数，
// 候选表见 pickImperative —— 命不中时降级为空列表（诚实空态）。

import type { WidgetConfig, WidgetDataContext } from '../types';
import { pickImperative, normalizePaths } from './_shared';

export interface RecentFilesData {
  paths: string[];
  unavailable: boolean;
}

const LIMIT = 8;

const IMPERATIVE_CANDIDATES = [
  'list',
  'getRecentFiles',
  'listRecentFiles',
  'loadRecentFiles',
  'fetchRecentFiles',
  'getRecent',
  'recent',
];

export function recentFilesAdapterKey(_config: WidgetConfig): string {
  return `files.recent:${LIMIT}`;
}

export async function loadRecentFiles(ctx: WidgetDataContext): Promise<RecentFilesData> {
  try {
    const mod = await import('@/lib/recent-files-client');
    const fn = pickImperative(mod, IMPERATIVE_CANDIDATES);
    if (typeof fn === 'function') {
      const result = await fn(LIMIT);
      if (ctx.signal.aborted) throw new DOMException('Aborted', 'AbortError');
      const paths = normalizePaths(result);
      return { paths, unavailable: false };
    }
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    console.warn('[widget] recent-files loader fallback (no imperative facade found):', err);
  }
  return { paths: [], unavailable: true };
}
