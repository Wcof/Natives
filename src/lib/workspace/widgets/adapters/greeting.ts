// ── Greeting Adapter ──
// 复用现有设置（settings:username），无 IPC 轮询、无 Timer。
// 数据源：window.nativesAPI.db.get（既有渲染层 facade，迁移源同款）。

import type { WidgetConfig, WidgetDataContext } from '../types';

export interface GreetingData {
  username: string | null;
}

export function greetingAdapterKey(_config: WidgetConfig): string {
  return 'greeting:username';
}

export async function loadGreeting(ctx: WidgetDataContext): Promise<GreetingData> {
  const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
  if (!api?.db?.get) {
    return { username: null };
  }
  const value = await api.db.get('settings:username');
  if (ctx.signal.aborted) {
    throw new DOMException('Aborted', 'AbortError');
  }
  return {
    username: typeof value === 'string' && value.trim() ? value.trim() : null,
  };
}
