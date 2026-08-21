// ── App Launcher Adapter（B-023） ──
// 复用 creativeApp.list()（现有 facade），不复制 Apps domain，不直接查进程。

import { creativeApp } from '@/lib/tauri/creative';
import type { CreativeAppSummary } from '@/lib/tauri/types-creative-app';
import type { WidgetConfig, WidgetDataContext } from '../types';

export interface AppLauncherData {
  apps: CreativeAppSummary[];
  unavailable: boolean;
}

const LIMIT = 8;

export function appLauncherAdapterKey(_config: WidgetConfig): string {
  return `apps.recent:${LIMIT}`;
}

export async function loadAppLauncher(ctx: WidgetDataContext): Promise<AppLauncherData> {
  try {
    const list = await creativeApp.list();
    if (ctx.signal.aborted) throw new DOMException('Aborted', 'AbortError');
    return { apps: list.slice(0, LIMIT), unavailable: false };
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    console.warn('[widget] app-launcher loader unavailable:', err);
    return { apps: [], unavailable: true };
  }
}
