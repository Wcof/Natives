// ── AI / Proxy Status Adapter（B-026） ──
// 复用 provider.list() 与 providerRouting.getSettings()（现有 facade）。
// DB change/event 驱动：subscribe 挂载 onDbStateChanged('provider') 事件，
// 事件触发时 data-broker 只 refetch（保留旧数据，无闪烁）。

import { provider, providerRouting } from '@/lib/tauri/provider';
import type { WidgetConfig, WidgetDataContext } from '../types';

export interface AiStatusData {
  providers: number;
  keys: number;
  proxyEnabled: boolean;
  proxyPort: number | null;
}

interface ProviderLike {
  keys?: unknown[];
}

interface RoutingSettingsLike {
  enabled?: boolean;
  loopbackPort?: number | null;
}

export function aiStatusAdapterKey(_config: WidgetConfig): string {
  return 'providers.status';
}

export async function loadAiStatus(ctx: WidgetDataContext): Promise<AiStatusData> {
  const [providersResult, routingResult] = await Promise.all([
    provider
      .list()
      .then((p) => p as unknown as ProviderLike[])
      .catch(() => [] as ProviderLike[]),
    (providerRouting.getSettings?.() ?? Promise.resolve(null))
      .then((r) => r as unknown as RoutingSettingsLike | null)
      .catch(() => null),
  ]);

  if (ctx.signal.aborted) throw new DOMException('Aborted', 'AbortError');

  const keys = providersResult.reduce((total, item) => total + (item.keys?.length ?? 0), 0);
  return {
    providers: providersResult.length,
    keys,
    proxyEnabled: routingResult?.enabled ?? false,
    proxyPort: routingResult?.loopbackPort ?? null,
  };
}

/** DB change / domain 事件驱动刷新；返回取消订阅函数。 */
export function subscribeAiStatus(emit: () => void): (() => void) | void {
  if (typeof window === 'undefined' || !window.nativesAPI?.onDbStateChanged) return undefined;
  return window.nativesAPI.onDbStateChanged((_event, channel) => {
    if (channel === 'provider') emit();
  });
}
