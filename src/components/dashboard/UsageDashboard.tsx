'use client';

import React, { lazy, Suspense, useState, useEffect, useCallback, useMemo } from 'react';
import { useLocale, t } from '@/i18n';
import type { UsageMetrics, UsageViewRequest } from '@/types/usage';
import { filterUsageRecords, aggregateUsageMetrics, uniqueSessionCount, buildSourceDimensions } from '@/lib/usage-dashboard';
import { useUsageData } from '@/hooks/useUsageData';
import { UsageToolbar } from './UsageToolbar';
import { UsageMetricGrid } from './UsageMetricGrid';
const UsageCharts = lazy(() => import('./UsageCharts').then((module) => ({ default: module.UsageCharts })));
import { UsageSourcesPanel } from './UsageSourcesPanel';
import { RefreshCw, AlertCircle, X } from 'lucide-react';
import { classifyError } from '@/lib/error-classifier';
import styles from './UsageDashboard.module.css';
import { useToast } from '@/components/ui/Toast';

interface UsageDashboardProps {
  children?: React.ReactNode;
}

export function UsageDashboard({ children }: UsageDashboardProps = {}) {
  const locale = useLocale();
  const { toast } = useToast();

  const [preset, setPreset] = useState('30d');
  const [customStart, setCustomStart] = useState('');
  const [customEnd, setCustomEnd] = useState('');
  const [sourceFilter, setSourceFilter] = useState<string[] | null>(null);
  const [modelFilter, setModelFilter] = useState<string[] | null>(null);
  const [projectFilter, setProjectFilter] = useState<string[] | null>(null);
  const [dismissedWarningKey, setDismissedWarningKey] = useState<string | null>(null);
  const [warningSeconds, setWarningSeconds] = useState(10);

  // Export state
  const [exportType, setExportType] = useState<'csv' | 'badge' | null>(null);
  const [exportFilename, setExportFilename] = useState('');
  const [isExporting, setIsExporting] = useState(false);

  // Handle directory selection via picker
  const handleSelectDir = useCallback(async () => {
    try {
      const api = window.nativesAPI;
      if (api?.dialog?.pickDirectory) {
        const dir = await api.dialog.pickDirectory();
        if (dir) {
          setProjectFilter([dir]);
        }
      }
    } catch (err) {
      // 选择器失败不得无声（R-E12）
      toast(classifyError(err).userMessage, 'error');
    }
  }, [toast]);

  const buildViewRequest = useCallback((): UsageViewRequest => {
    const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
    return {
      preset: preset as UsageViewRequest['preset'],
      timeZone,
      projectPath: projectFilter?.[0] ?? null,
      customStartMs: preset === 'custom' && customStart ? new Date(`${customStart}T00:00:00`).getTime() : undefined,
      // The end date is inclusive in the UI, but backend ranges are exclusive.
      customEndMs: preset === 'custom' && customEnd ? new Date(`${customEnd}T23:59:59.999`).getTime() + 1 : undefined,
    };
  }, [preset, customStart, customEnd, projectFilter]);

  // Cache read + user-initiated sync live in the shared usage-data hook so
  // Settings Personal Overview composes from the same state machine.
  const { state, errorMsg, isSyncing, lastSyncTime, clearError, sync } = useUsageData(buildViewRequest);

  const handleSync = useCallback(async () => {
    const outcome = await sync();
    if (outcome.ok) toast(t(locale, 'usage.syncedSuccess'), 'success');
    else if (outcome.message) toast(outcome.message, 'error');
  }, [sync, locale, toast]);

  // Filtered data
  const data = state.kind === 'ready' ? state.data : null;
  const metadata = state.kind === 'ready' ? state.metadata : null;
  const warningKey = data?.warnings.map((warning) => `${warning.sourceId ?? 'system'}:${warning.code}`).sort().join('|') ?? '';
  const showWarning = warningKey.length > 0 && dismissedWarningKey !== warningKey;

  useEffect(() => {
    if (!showWarning) return;
    setWarningSeconds(10);
    const timer = window.setInterval(() => {
      // 只递减；归零后的关闭动作放在独立 effect（updater 内做副作用会被
      // StrictMode 双调用提前触发）
      setWarningSeconds((seconds) => Math.max(0, seconds - 1));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [showWarning, warningKey]);

  useEffect(() => {
    if (showWarning && warningSeconds === 0) {
      setDismissedWarningKey(warningKey);
    }
  }, [showWarning, warningSeconds, warningKey]);

  // 维度变化后清掉已不存在于选项中的过滤值——否则受控 select 显示空白
  // 而过滤仍然生效，用户面对「全为零的仪表盘」却看不到原因
  useEffect(() => {
    if (state.kind !== 'ready') return;
    const models = new Set(state.data.dimensions.models.map((m) => m.id));
    setModelFilter((prev) => (prev && prev.some((m) => !models.has(m)) ? null : prev));
    const sources = new Set(state.data.sources.map((s) => s.id));
    setSourceFilter((prev) => (prev && prev.some((s) => !sources.has(s)) ? null : prev));
  }, [state]);

  const filtered = useMemo(() => {
    if (!data) return null;
    // Directory selection is applied while slicing the cache on the backend. Do not
    // filter it again here, otherwise child projects are incorrectly excluded.
    // terminal 维度后端恒为空（usage/mod.rs dimensions.terminals: vec![]），
    // 对应过滤管线已从 UI 移除。
    return filterUsageRecords(data, sourceFilter, modelFilter, null, null);
  }, [data, sourceFilter, modelFilter]);

  // Aggregated metrics
  const metrics: UsageMetrics | null = useMemo(() => {
    if (!filtered) return null;
    return aggregateUsageMetrics(filtered.daily, filtered.sessions, data?.sources ?? []);
  }, [filtered, data?.sources]);

  const totalSessions = useMemo(() => {
    if (!filtered) return 0;
    return uniqueSessionCount(filtered.sessions);
  }, [filtered]);

  // Comparison period metrics
  const prevMetrics = useMemo(() => {
    if (!data?.comparison) return null;
    const filteredComp = filterUsageRecords(
      {
        ...data,
        daily: data.comparison.daily,
        activity: data.comparison.activity,
        sessions: data.comparison.sessions,
      },
      sourceFilter,
      modelFilter,
      null,
      null,
    );
    return aggregateUsageMetrics(filteredComp.daily, filteredComp.sessions, data.sources);
  }, [data?.comparison, data?.sources, sourceFilter, modelFilter]);

  const prevTotalSessions = useMemo(() => {
    if (!data?.comparison) return 0;
    const filteredComp = filterUsageRecords(
      {
        ...data,
        daily: data.comparison.daily,
        activity: data.comparison.activity,
        sessions: data.comparison.sessions,
      },
      sourceFilter,
      modelFilter,
      null,
      null,
    );
    return uniqueSessionCount(filteredComp.sessions);
  }, [data?.comparison, sourceFilter, modelFilter]);

  if (state.kind === 'reading-cache') {
    return (
      <div className={styles.container}>
        <div className={styles.toolbarSection}>
          <UsageToolbar
            preset={preset}
            onPresetChange={setPreset}
            customStart={customStart}
            customEnd={customEnd}
            onCustomStartChange={setCustomStart}
            onCustomEndChange={setCustomEnd}
            sources={[]}
            models={[]}
            projects={[]}
            sourceFilter={null}
            modelFilter={null}
            projectFilter={null}
            onSourceFilterChange={() => {}}
            onModelFilterChange={() => {}}
            onProjectFilterChange={() => {}}
            onSelectDir={handleSelectDir}
          />
        </div>
        {/* Metrics show static —, no loading text */}
        <UsageMetricGrid
          metrics={null}
          prevMetrics={null}
          totalSessions={0}
          prevTotalSessions={0}
          lastSyncTime={lastSyncTime}
        />
        {children}
      </div>
    );
  }

  if (state.kind === 'missing-cache') {
    return (
      <div className={styles.container}>
        <div className={styles.toolbarSection}>
          <UsageToolbar
            preset={preset}
            onPresetChange={setPreset}
            customStart={customStart}
            customEnd={customEnd}
            onCustomStartChange={setCustomStart}
            onCustomEndChange={setCustomEnd}
            sources={[]}
            models={[]}
            projects={[]}
            sourceFilter={null}
            modelFilter={null}
            projectFilter={null}
            onSourceFilterChange={() => {}}
            onModelFilterChange={() => {}}
            onProjectFilterChange={() => {}}
            onSelectDir={handleSelectDir}
          />
        </div>
        <div className={styles.cacheEmptyState}>
          <div className={styles.cacheEmptyText}>
            {t(locale, 'usage.noCache')}
          </div>
          <button
            onClick={() => void handleSync()}
            disabled={isSyncing}
            className={styles.primaryAction}
          >
            {isSyncing ? t(locale, 'usage.syncing') : t(locale, 'usage.syncData')}
          </button>
          {errorMsg && (
            <p className={styles.cacheError}>{errorMsg}</p>
          )}
        </div>
        {children}
      </div>
    );
  }

  return (
    <div className={styles.container}>
      {/* Toolbar: date presets + filters + unified sync row */}
      <UsageToolbar
        preset={preset}
        onPresetChange={setPreset}
        customStart={customStart}
        customEnd={customEnd}
        onCustomStartChange={setCustomStart}
        onCustomEndChange={setCustomEnd}
        sources={buildSourceDimensions(data?.sources ?? [])}
        models={data?.dimensions.models ?? []}
        projects={data?.dimensions.projects ?? []}
        sourceFilter={sourceFilter}
        modelFilter={modelFilter}
        projectFilter={projectFilter}
        onSourceFilterChange={setSourceFilter}
        onModelFilterChange={setModelFilter}
        onProjectFilterChange={setProjectFilter}
        onSelectDir={handleSelectDir}
        style={{ marginBottom: '16px' }}
      >
        <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 6 }}>
          <button
            onClick={() => void handleSync()}
            disabled={isSyncing}
            aria-busy={isSyncing}
            className={styles.secondaryAction}
          >
            <RefreshCw size={12} style={{ animation: isSyncing ? 'spin 0.8s linear infinite' : undefined }} />
            {isSyncing ? t(locale, 'usage.syncing') : t(locale, 'usage.syncData')}
          </button>
        </div>
      </UsageToolbar>

      {/* 已有数据时的同步失败需要常驻错误条（此前只有一闪而过的 toast） */}
      {errorMsg && (
        <div className={styles.partialBanner} role="alert">
          <AlertCircle size={14} style={{ color: 'var(--danger)' }} />
          <span>{errorMsg}</span>
          <button
            type="button"
            className={styles.warningClose}
            onClick={() => clearError()}
            aria-label={t(locale, 'common.close')}
            title={t(locale, 'common.close')}
          >
            <X size={14} />
          </button>
        </div>
      )}

      {data && showWarning && (
        <div className={styles.partialBanner}>
          <AlertCircle size={14} style={{ color: 'var(--warning)' }} />
          <span>
            {t(locale, 'usage.partialData')}: {data.warnings.filter((w) => w.sourceId !== null).length} {t(locale, 'usage.warningsCount')}
          </span>
          <span className={styles.warningCountdown}>{t(locale, 'usage.warningAutoClose', { seconds: warningSeconds })}</span>
          <button
            type="button"
            className={styles.warningClose}
            onClick={() => setDismissedWarningKey(warningKey)}
            aria-label={t(locale, 'usage.closeWarning')}
            title={t(locale, 'usage.closeWarning')}
          >
            <X size={14} />
          </button>
        </div>
      )}

      {metrics && (<UsageMetricGrid
        metrics={metrics}
        prevMetrics={prevMetrics}
        totalSessions={totalSessions}
        prevTotalSessions={prevTotalSessions}
        lastSyncTime={lastSyncTime}
      />)}

      {filtered && (<Suspense fallback={<div className={styles.chartPanel}>{t(locale, 'usage.loadingCharts')}</div>}><UsageCharts
        daily={filtered.daily}
        activity={filtered.activity}
        sources={data?.sources ?? []}
      /></Suspense>)}

      {children}

      {data && (
        <div className={styles.sourcesSection}>
          <UsageSourcesPanel sources={data.sources} warnings={data.warnings} lastRefresh={lastSyncTime} rtk={data.rtk} />
        </div>
      )}
    </div>
  );
}
