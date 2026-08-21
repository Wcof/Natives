'use client';

/**
 * AI Status Widget（B-026）—— 迁移到 V2 Definition。
 * DB change/event 驱动刷新（subscribe → onDbStateChanged('provider')）。
 * 复用 provider.list() + providerRouting.getSettings()（现有 facade）。
 * 按 ADR-0020：Proxy 未启用时展示真实状态，不 mock。
 */

import { z } from 'zod';
import { Activity, Server } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { FONT_SIZE } from '@/lib/design-tokens';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadAiStatus,
  subscribeAiStatus,
  aiStatusAdapterKey,
} from '@/lib/workspace/widgets/adapters/ai-status';
import type { AiStatusData } from '@/lib/workspace/widgets/adapters/ai-status';

type AiStatusSettings = Record<string, unknown>;

function AiStatusView({ data }: WidgetProps<AiStatusData, AiStatusSettings>) {
  const locale = useLocale();
  const status = data;

  if (status == null) {
    return (
      <div className="ws-shell-state" style={{ color: 'var(--text-tertiary)', fontSize: FONT_SIZE.xs }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'center',
        gap: 6,
        height: '100%',
        padding: '0 12px',
        minWidth: 0,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
        <Activity size={14} style={{ flexShrink: 0, color: 'var(--primary)' }} />
        <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
          {t(locale, 'home.aiResources')}
        </span>
        <span
          style={{
            marginLeft: 'auto',
            fontSize: FONT_SIZE.xs,
            fontWeight: 600,
            color: 'var(--text)',
            whiteSpace: 'nowrap',
          }}
        >
          {t(locale, 'home.providerCount', { count: status.providers })}
        </span>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
        <Server size={14} style={{ flexShrink: 0, color: 'var(--text-tertiary)' }} />
        <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
          {t(locale, 'home.localProxy')}
        </span>
        <span
          style={{
            marginLeft: 'auto',
            fontSize: FONT_SIZE.xs,
            fontWeight: 600,
            color: 'var(--text)',
            whiteSpace: 'nowrap',
          }}
        >
          {status.proxyEnabled
            ? status.proxyPort
              ? t(locale, 'home.proxyPort', { port: status.proxyPort })
              : t(locale, 'home.proxyEnabled')
            : t(locale, 'home.proxyDisabled')}
        </span>
      </div>
    </div>
  );
}

export const aiStatusWidgetDefinition: WidgetDefinition<AiStatusData, AiStatusSettings> = {
  type: 'ai_status',
  titleKey: 'home.widgetAiStatus',
  descriptionKey: 'home.widgetAiStatus',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: aiStatusAdapterKey,
  load: loadAiStatus,
  subscribe: subscribeAiStatus,
  Component: AiStatusView,
};

export default aiStatusWidgetDefinition;
