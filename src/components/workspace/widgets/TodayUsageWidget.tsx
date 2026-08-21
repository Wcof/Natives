'use client';

/**
 * Today Usage Widget（B-024）—— 迁移到 V2 Definition。
 * 与 Token Metrics 共用 usage.summary adapter key（data-broker 共享一次查询）。
 * 复用 summarizeOverviewUsage（现有纯计算），不建第二套 usage 状态机。
 */

import { z } from 'zod';
import { BarChart3 } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { FONT_SIZE } from '@/lib/design-tokens';
import { fmtCount } from '@/lib/format';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadUsageSummary,
  usageAdapterKey,
} from '@/lib/workspace/widgets/adapters/usage';
import type { UsageSummaryData } from '@/lib/workspace/widgets/adapters/usage';

type TodayUsageSettings = Record<string, unknown>;

function TodayUsageView({ data }: WidgetProps<UsageSummaryData, TodayUsageSettings>) {
  const locale = useLocale();
  const todayTokens = data?.todayTokens ?? null;
  const sessions = data?.sessions ?? 0;

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        height: '100%',
        padding: '0 12px',
        minWidth: 0,
      }}
    >
      <BarChart3 size={18} style={{ flexShrink: 0, color: 'var(--primary)' }} />
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>
          {t(locale, 'home.todayTokens')}
        </div>
        <div
          style={{
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            fontSize: 14,
            fontWeight: 600,
            color: 'var(--text)',
          }}
        >
          {todayTokens === null
            ? t(locale, 'home.usageUnavailable')
            : fmtCount(todayTokens, locale)}
        </div>
      </div>
      <div style={{ marginLeft: 'auto', textAlign: 'right' }}>
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>
          {t(locale, 'home.sessions')}
        </div>
        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)' }}>
          {fmtCount(sessions, locale)}
        </div>
      </div>
    </div>
  );
}

export const todayUsageWidgetDefinition: WidgetDefinition<UsageSummaryData, TodayUsageSettings> = {
  type: 'today_usage',
  titleKey: 'home.widgetTodayUsage',
  descriptionKey: 'home.widgetTodayUsage',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: usageAdapterKey,
  load: loadUsageSummary,
  Component: TodayUsageView,
};

export default todayUsageWidgetDefinition;
