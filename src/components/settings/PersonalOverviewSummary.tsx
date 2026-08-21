'use client';

/**
 * 设置 → 个人概览摘要（决策 10）。
 *
 * 完整 Usage Dashboard 已迁至「数据/用量」页（`/usage`，MainContent activeView
 * 'usage'）。设置个人概览只保留摘要：复用同一共享状态机（useUsageData）与
 * 纯计算（summarizeOverviewUsage），叠加 StorageOverview，不复制首页组件职责。
 */

import { useCallback, useMemo } from 'react';
import { Activity, BarChart3, RefreshCw, HardDrive, LayoutGrid } from 'lucide-react';
import { t, useLocale, type Locale } from '@/i18n';
import { useUsageData } from '@/hooks/useUsageData';
import { summarizeOverviewUsage } from '@/lib/personal-overview-data';
import { fmtCount } from '@/lib/format';
import { ErrorState, EmptyState } from '@/components/ui/EmptyState';
import StorageOverview from '@/components/settings/StorageOverview';
import HomeWorkspacePage from '@/components/home/HomeWorkspacePage';
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

function MetricCard({ icon, label, value, hint }: {
  icon: React.ReactNode;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] p-4">
      <div className="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
        {icon}
        <span>{label}</span>
      </div>
      <div className="mt-1.5 text-lg font-semibold text-[var(--text)]">{value}</div>
      {hint ? <div className="mt-0.5 text-[0.6875rem] text-[var(--text-disabled)]">{hint}</div> : null}
    </div>
  );
}

export default function PersonalOverviewSummary() {
  const locale = useLocale();

  // 摘要视图固定：当前时区、30d、无项目过滤（与 MenubarOverview 冻结契约一致）。
  const buildViewRequest = useCallback(
    (): UsageViewRequest => ({
      preset: '30d',
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      projectPath: null,
    }),
    [],
  );

  const { state, errorMsg, isSyncing, sync, clearError } = useUsageData(buildViewRequest);
  const usage = state.kind === 'ready' ? state.data : null;
  const loading = state.kind === 'reading-cache' && !usage;
  const cacheMissing = state.kind === 'missing-cache';
  const summary = useMemo(
    () => (usage ? summarizeOverviewUsage(usage, localDateKey()) : null),
    [usage],
  );

  const usableSources = (usage?.sources ?? []).filter((s) => s.state === 'ok' || s.state === 'partial').length;

  const handleSync = useCallback(async () => {
    await sync();
  }, [sync]);

  let body: React.ReactNode;
  if (errorMsg && !usage) {
    body = <ErrorState message={errorMsg} onRetry={() => void clearError()} />;
  } else if (loading) {
    body = (
      <div className="grid grid-cols-2 gap-3">
        {Array.from({ length: 4 }, (_, index) => (
          <div key={index} className="h-20 animate-pulse rounded-2xl bg-[var(--surface-hover)]" />
        ))}
      </div>
    );
  } else if (cacheMissing || !usage || !summary) {
    body = (
      <EmptyState
        icon={<BarChart3 size={24} />}
        title={t(locale, 'settings.overviewNoUsageTitle')}
        description={t(locale, 'settings.overviewNoUsage')}
        action={{ label: t(locale, 'settings.overviewSync'), onClick: handleSync }}
      />
    );
  } else {
    body = (
      <div className="grid grid-cols-2 gap-3">
        <MetricCard
          icon={<Activity size={13} />}
          label={t(locale, 'settings.overviewTodayTokens')}
          value={valueOrUnavailable(summary.todayTokens, locale)}
          hint={t(locale, 'settings.overviewTodayHint')}
        />
        <MetricCard
          icon={<BarChart3 size={13} />}
          label={t(locale, 'settings.overviewThirtyDayTokens')}
          value={valueOrUnavailable(summary.totalTokens, locale)}
        />
        <MetricCard
          icon={<Activity size={13} />}
          label={t(locale, 'settings.overviewInputOutput')}
          value={
            summary.inputTokens === null && summary.outputTokens === null
              ? t(locale, 'settings.overviewUnavailable')
              : `${valueOrUnavailable(summary.inputTokens, locale)} / ${valueOrUnavailable(summary.outputTokens, locale)}`
          }
        />
        <MetricCard
          icon={<Activity size={13} />}
          label={t(locale, 'settings.overviewSessions')}
          value={fmtCount(summary.sessions, locale)}
        />
        <MetricCard
          icon={<BarChart3 size={13} />}
          label={t(locale, 'settings.overviewSources', { count: usableSources })}
          value={fmtCount(usableSources, locale)}
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div className="text-sm text-[var(--text-secondary)]">{t(locale, 'settings.overviewDesc')}</div>
        <button
          type="button"
          onClick={handleSync}
          disabled={isSyncing}
          className="inline-flex items-center gap-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)] px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:text-[var(--text)] disabled:opacity-60"
        >
          <RefreshCw size={13} className={isSyncing ? 'animate-spin' : undefined} />
          {isSyncing ? t(locale, 'settings.overviewSyncing') : t(locale, 'settings.overviewSync')}
        </button>
      </div>

      {body}

      <section className="flex flex-col rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] p-4 min-h-[580px]">
        <div className="mb-2 flex items-center justify-between">
          <div className="flex items-center gap-2 text-sm font-semibold text-[var(--text)]">
            <LayoutGrid size={15} className="text-[var(--primary)]" />
            <span>{t(locale, 'settings.tabPersonalOverview')}</span>
          </div>
        </div>
        <div className="flex-1 min-h-[520px]">
          <HomeWorkspacePage />
        </div>
      </section>

      <section className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] p-4">
        <div className="mb-3 flex items-center gap-2 text-sm font-semibold text-[var(--text)]">
          <HardDrive size={15} className="text-[var(--primary)]" />
          {t(locale, 'settings.overviewStorage')}
        </div>
        <StorageOverview />
      </section>
    </div>
  );
}
