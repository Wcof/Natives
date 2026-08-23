'use client';

/**
 * Token Metrics Widget（B-025）—— 迁移到 V2 Definition。
 * 与 Today Usage 共用 usage.summary adapter key（一次查询共享）。
 * 使用 Metric primitives（MetricBlock / AnimatedMetric）。
 */

import { z } from 'zod';
import { BarChart3 } from 'lucide-react';
import { t, useLocale, type Locale } from '@/i18n';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { fmtCount } from '@/lib/format';
import { AnimatedMetric } from '@/components/ui/design-system';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadUsageSummary,
  usageAdapterKey,
} from '@/lib/workspace/widgets/adapters/usage';
import type { UsageSummaryData } from '@/lib/workspace/widgets/adapters/usage';

type TokenMetricsSettings = Record<string, unknown>;

function valueOrUnavailable(value: number | null, locale: Locale): string {
  // 与迁移源一致：无数据时显示“不可用”
  return value === null ? '—' : fmtCount(value, locale);
}

function TokenMetricsView({ data }: WidgetProps<UsageSummaryData, TokenMetricsSettings>) {
  const locale = useLocale();
  const totalTokens = data?.totalTokens ?? null;
  const sessions = data?.sessions ?? 0;

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'space-between',
        gap: SPACING.sm,
        height: '100%',
        padding: 8,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text)' }}>
        <BarChart3 size={14} style={{ color: 'var(--primary)' }} />
        <span>{t(locale, 'home.widgetTokenMetrics')}</span>
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, minHeight: 0 }}>
        <div
          style={{
            borderRadius: 10,
            padding: SPACING.sm,
            background: 'var(--elevation-inset)',
            display: 'flex',
            flexDirection: 'column',
            gap: 2,
            minWidth: 0,
          }}
        >
          <div style={{ fontSize: 11, color: 'var(--text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {t(locale, 'settings.overviewThirtyDayTokens')}
          </div>
          <div style={{ fontWeight: 600, color: 'var(--text)', fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {totalTokens === null ? (
              valueOrUnavailable(null, locale)
            ) : (
              <AnimatedMetric value={totalTokens} format={(v) => fmtCount(v, locale)} duration={400} />
            )}
          </div>
        </div>
        <div
          style={{
            borderRadius: 10,
            padding: SPACING.sm,
            background: 'var(--elevation-inset)',
            display: 'flex',
            flexDirection: 'column',
            gap: 2,
            minWidth: 0,
          }}
        >
          <div style={{ fontSize: 11, color: 'var(--text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {t(locale, 'settings.overviewSessions')}
          </div>
          <div style={{ fontWeight: 600, color: 'var(--text)', fontSize: 13 }}>
            <AnimatedMetric value={sessions} format={(v) => fmtCount(v, locale)} duration={400} />
          </div>
        </div>
      </div>
    </div>
  );
}

export const tokenMetricsWidgetDefinition: WidgetDefinition<UsageSummaryData, TokenMetricsSettings> = {
  type: 'token_metrics',
  titleKey: 'home.widgetTokenMetrics',
  descriptionKey: 'home.widgetTokenMetrics',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'medium',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  timeAware: true,
  adapterKeyBuilder: usageAdapterKey,
  load: loadUsageSummary,
  Component: TokenMetricsView,
};

export default tokenMetricsWidgetDefinition;
