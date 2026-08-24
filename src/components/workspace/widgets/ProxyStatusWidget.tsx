'use client';

/**
 * Local Proxy Status Widget (B-033).
 * Read-only status for the Local Model Proxy Engine.
 */

import { z } from 'zod';
import { useLocale, t } from '@/i18n';
import { ShieldCheck, ShieldAlert } from 'lucide-react';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadAiStatus,
  aiStatusAdapterKey,
  type AiStatusData,
} from '@/lib/workspace/widgets/adapters/ai-status';

type ProxyStatusSettings = Record<string, unknown>;

function ProxyStatusView({ data }: WidgetProps<AiStatusData, ProxyStatusSettings>) {
  const locale = useLocale();
  const proxyOnline = data?.proxyEnabled ?? false;

  return (
    <div className="flex h-full w-full flex-col justify-between">
      <div className="flex items-center justify-end">
        <span
          className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${
            proxyOnline
              ? 'bg-[var(--success)]/10 text-[var(--success)]'
              : 'bg-[var(--surface-hover)] text-[var(--text-disabled)]'
          }`}
        >
          {proxyOnline ? <ShieldCheck size={10} /> : <ShieldAlert size={10} />}
          {proxyOnline ? t(locale, 'workspace.proxyActive') : t(locale, 'workspace.proxyStandby')}
        </span>
      </div>

      <div className="rounded-lg bg-[var(--surface-hover)] p-2 text-xs">
        <div className="flex justify-between text-[var(--text-secondary)]">
          <span>{t(locale, 'workspace.proxyPort')}</span>
          <span className="font-mono text-[var(--text)]">{data?.proxyPort ? String(data.proxyPort) : (proxyOnline ? '8080' : '—')}</span>
        </div>
        <div className="flex justify-between text-[var(--text-secondary)] mt-1">
          <span>{t(locale, 'workspace.proxyProtocol')}</span>
          <span className="text-[var(--text)]">OpenAI / Anthropic</span>
        </div>
      </div>
    </div>
  );
}

export const proxyStatusWidgetDefinition: WidgetDefinition<AiStatusData, ProxyStatusSettings> = {
  type: 'proxy_status',
  titleKey: 'ai.proxy',
  descriptionKey: 'ai.proxy',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: aiStatusAdapterKey,
  load: loadAiStatus,
  Component: ProxyStatusView,
};

export default proxyStatusWidgetDefinition;
