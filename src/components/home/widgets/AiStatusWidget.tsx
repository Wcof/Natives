'use client';

/**
 * AI / Proxy 状态 Widget（默认 5 Widget 之一）。
 *
 * 复用 `provider.list()` 与 `providerRouting.getSettings()`（现有 facade）。
 * 按 ADR-0020：Proxy 未启用时展示「未启用/尚未配置」真实状态，不 mock。
 * 无自建高频 Timer —— 仅在挂载与 DB 变更事件时刷新。
 */

import { useCallback, useEffect, useState } from 'react';
import { Activity, Server } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { provider, providerRouting } from '@/lib/tauri/provider';

interface AiStatus {
  providers: number;
  keys: number;
  proxyEnabled: boolean;
  proxyPort: number | null;
}

export function AiStatusWidget() {
  const locale = useLocale();
  const [status, setStatus] = useState<AiStatus | null>(null);

  const load = useCallback(async () => {
    try {
      const [providers, routing] = await Promise.all([
        provider.list(),
        (providerRouting.getSettings?.() ?? Promise.resolve(null)).catch(() => null),
      ]);
      const keys = providers.reduce((total, item) => total + (item.keys?.length ?? 0), 0);
      setStatus({
        providers: providers.length,
        keys,
        proxyEnabled: routing?.enabled ?? false,
        proxyPort: routing?.loopbackPort ?? null,
      });
    } catch {
      /* browser dev mode */
    }
  }, []);

  useEffect(() => {
    void load();
    let unsub: (() => void) | undefined;
    try {
      if (window.nativesAPI?.onDbStateChanged) {
        unsub = window.nativesAPI.onDbStateChanged((_event, channel) => {
          if (channel === 'provider') void load();
        });
      }
    } catch { /* browser dev mode */ }
    return () => { unsub?.(); };
  }, [load]);

  if (status === null) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
        {t(locale, 'common.loading')}
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col justify-center gap-1 px-1">
      <div className="flex items-center gap-2">
        <Activity size={14} className="text-[var(--primary)]" />
        <span className="text-xs text-[var(--text-secondary)]">{t(locale, 'home.aiResources')}</span>
        <span className="ml-auto text-xs font-semibold text-[var(--text)]">
          {t(locale, 'home.providerCount', { count: status.providers })}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <Server size={14} className="text-[var(--text-disabled)]" />
        <span className="text-xs text-[var(--text-secondary)]">{t(locale, 'home.localProxy')}</span>
        <span className="ml-auto text-xs font-semibold text-[var(--text)]">
          {status.proxyEnabled
            ? (status.proxyPort ? t(locale, 'home.proxyPort', { port: status.proxyPort }) : t(locale, 'home.proxyEnabled'))
            : t(locale, 'home.proxyDisabled')}
        </span>
      </div>
    </div>
  );
}
