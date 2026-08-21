'use client';

/**
 * 用量统计 Widget（30 天 / 今日 Token 与会话概览）。
 * 复用 useUsageData + summarizeOverviewUsage。
 */

import { useCallback, useMemo } from 'react';
import { BarChart3 } from 'lucide-react';
import { t, useLocale, type Locale } from '@/i18n';
import { useUsageData } from '@/hooks/useUsageData';
import { summarizeOverviewUsage } from '@/lib/personal-overview-data';
import { fmtCount } from '@/lib/format';
import type { UsageViewRequest } from '@/types/usage';

function localDateKey(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function valueOrUnavailable(value: number | null, locale: Locale): string {
  return value === null ? t(locale, 'settings.overviewUnavailable') : fmtCount(value, locale);
}

export function TokenMetricsWidget() {
  const locale = useLocale();

  const buildViewRequest = useCallback(
    (): UsageViewRequest => ({
      preset: '30d',
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      projectPath: null,
    }),
    [],
  );

  const { state } = useUsageData(buildViewRequest);
  const usage = state.kind === 'ready' ? state.data : null;
  const summary = useMemo(
    () => (usage ? summarizeOverviewUsage(usage, localDateKey()) : null),
    [usage],
  );

  return (
    <div className="flex h-full flex-col justify-between gap-2 p-1">
      <div className="flex items-center gap-1.5 text-xs font-semibold text-[var(--text)]">
        <BarChart3 size={14} className="text-[var(--primary)]" />
        <span>{t(locale, 'home.widgetTokenMetrics')}</span>
      </div>
      <div className="grid grid-cols-2 gap-2 text-xs">
        <div className="rounded-lg bg-[var(--surface-hover)] p-2">
          <div className="text-[0.6875rem] text-[var(--text-secondary)]">{t(locale, 'settings.overviewThirtyDayTokens')}</div>
          <div className="mt-0.5 font-semibold text-[var(--text)]">{valueOrUnavailable(summary?.totalTokens ?? null, locale)}</div>
        </div>
        <div className="rounded-lg bg-[var(--surface-hover)] p-2">
          <div className="text-[0.6875rem] text-[var(--text-secondary)]">{t(locale, 'settings.overviewSessions')}</div>
          <div className="mt-0.5 font-semibold text-[var(--text)]">{fmtCount(summary?.sessions ?? 0, locale)}</div>
        </div>
      </div>
    </div>
  );
}
