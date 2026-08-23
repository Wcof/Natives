// ── App Launcher Adapter（APP-062） ──
// 使用 appsApi.listViews() 从统一 applications 注册表获取，不直接查进程。

import { appsApi } from '@/lib/tauri/apps';
import type { WidgetConfig, WidgetDataContext } from '../types';

export interface AppLauncherItem {
  id: string;
  title: string;
  kind: string;
  state: string;
}

export interface AppLauncherData {
  apps: AppLauncherItem[];
  unavailable: boolean;
}

const LIMIT = 8;

export function appLauncherAdapterKey(_config: WidgetConfig): string {
  return `apps.recent:${LIMIT}`;
}

export async function loadAppLauncher(ctx: WidgetDataContext): Promise<AppLauncherData> {
  try {
    const list = await appsApi.listViews();
    if (ctx.signal.aborted) throw new DOMException('Aborted', 'AbortError');
    const items: AppLauncherItem[] = list.filter((a) => a.kind !== 'local_project').slice(0, LIMIT).map((a) => ({
      id: a.appId,
      title: a.title,
      kind: a.kind,
      state: a.runtimeState,
    }));
    return { apps: items, unavailable: false };
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    console.warn('[widget] app-launcher loader unavailable:', err);
    return { apps: [], unavailable: true };
  }
}
