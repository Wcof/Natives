'use client';

/**
 * 今日 AI 用量 Widget（默认 5 Widget 之一）。
 * 复用 `useUsageData` + `summarizeOverviewUsage`（现有查询/纯计算），
 * 不直接解析日志、不建第二套 usage 状态机。
 */

import { useCallback, useMemo } from 'react';
import { BarChart3 } from 'lucide-react';
import { t, useLocale } from '@/i18n';
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

export function TodayUsageWidget() {
  const locale = useLocale();

  // 与 Menubar/设置摘要一致的冻结视图：当前时区、30d、无项目过滤。
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

  const todayTokens = summary?.todayTokens ?? null;
  const sessions = summary?.sessions ?? 0;

  return (
    <div className="flex h-full items-center gap-3 px-1">
      <BarChart3 size={18} className="shrink-0 text-[var(--primary)]" />
      <div className="min-w-0">
        <div className="text-xs text-[var(--text-secondary)]">{t(locale, 'home.todayTokens')}</div>
        <div className="truncate text-sm font-semibold text-[var(--text)]">
          {todayTokens === null
            ? t(locale, 'home.usageUnavailable')
            : fmtCount(todayTokens, locale)}
        </div>
      </div>
      <div className="ml-auto text-right">
        <div className="text-xs text-[var(--text-secondary)]">{t(locale, 'home.sessions')}</div>
        <div className="text-sm font-semibold text-[var(--text)]">{fmtCount(sessions, locale)}</div>
      </div>
    </div>
  );
}
