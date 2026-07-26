'use client';

import React, { lazy, Suspense, useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { useLocale, t } from '@/i18n';
import type { UsageDashboardResponse, UsageCacheReadResult, UsageCacheMetadata, UsageMetrics, UsageViewRequest, DashboardState } from '@/types/usage';
import { filterUsageRecords, aggregateUsageMetrics, uniqueSessionCount, buildSourceDimensions } from '@/lib/usage-dashboard';
import { UsageToolbar } from './UsageToolbar';
import { UsageMetricGrid } from './UsageMetricGrid';
const UsageCharts = lazy(() => import('./UsageCharts').then((module) => ({ default: module.UsageCharts })));
import { UsageSourcesPanel } from './UsageSourcesPanel';
import { RefreshCw, AlertCircle, X } from 'lucide-react';
import { classifyError } from '@/lib/error-classifier';
import styles from './UsageDashboard.module.css';
import { useToast } from '@/components/ui/Toast';

export function UsageDashboard() {
  const locale = useLocale();
  const { toast } = useToast();
  const requestIdRef = useRef(0);

  const [preset, setPreset] = useState('30d');
  const [customStart, setCustomStart] = useState('');
  const [customEnd, setCustomEnd] = useState('');
  const [sourceFilter, setSourceFilter] = useState<string[] | null>(null);
  const [modelFilter, setModelFilter] = useState<string[] | null>(null);
  const [projectFilter, setProjectFilter] = useState<string[] | null>(null);
  const [state, setState] = useState<DashboardState>({ kind: 'reading-cache' });
  const [isSyncing, setIsSyncing] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [lastSyncTime, setLastSyncTime] = useState<number | null>(null);
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

  const buildViewRequest = useCallback((timeZone: string): UsageViewRequest => ({
    preset: preset as UsageViewRequest['preset'],
    timeZone,
    projectPath: projectFilter?.[0] ?? null,
    customStartMs: preset === 'custom' && customStart ? new Date(`${customStart}T00:00:00`).getTime() : undefined,
    // The end date is inclusive in the UI, but backend ranges are exclusive.
    customEndMs: preset === 'custom' && customEnd ? new Date(`${customEnd}T23:59:59.999`).getTime() + 1 : undefined,
  }), [preset, customStart, customEnd, projectFilter]);

  // Sync data is user initiated. Cache reads never start a scan by themselves.
  const handleSync = useCallback(async () => {
    // 同步也参与请求竞态守卫：否则同步中切换预设后，旧预设的同步结果会
    // 覆盖新预设视图；反向的 stale loadCached 也会覆盖新同步数据
    const rid = ++requestIdRef.current;
    setIsSyncing(true);
    setErrorMsg(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.sync) throw new Error('usage API not available');
      const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
      const result = (await api.usage.sync({
        timeZone,
        currentView: buildViewRequest(timeZone),
      })) as { metadata: UsageCacheMetadata; response: UsageDashboardResponse };
      if (rid !== requestIdRef.current) return;
      setState({ kind: 'ready', data: result.response, metadata: result.metadata });
      setLastSyncTime(result.metadata.generatedAtMs);
      toast(t(locale, 'usage.syncedSuccess'), 'success');
    } catch (err: any) {
      if (rid !== requestIdRef.current) return;
      const classified = classifyError(err);
      setErrorMsg(classified.userMessage);
      toast(classified.userMessage, 'error');
      // Keep old data on failure
    } finally {
      setIsSyncing(false);
    }
  }, [buildViewRequest, locale, toast]);

  // Fetch cached data on mount / preset change
  const loadCached = useCallback(async () => {
    const rid = ++requestIdRef.current;
    setErrorMsg(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.getCached) throw new Error('usage API not available');
      const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
      const query = buildViewRequest(timeZone);
      const result = (await api.usage.getCached(query)) as UsageCacheReadResult;
      // Ignore stale responses
      if (rid !== requestIdRef.current) return;
      if (result.state === 'ready') {
        setState({ kind: 'ready', data: result.response, metadata: result.metadata });
        setLastSyncTime(result.metadata.generatedAtMs);
      } else {
        setState({ kind: 'missing-cache' });
      }
    } catch (err: any) {
      if (rid !== requestIdRef.current) return;
      const classified = classifyError(err);
      setErrorMsg(classified.userMessage);
      setState({ kind: 'missing-cache' });
    }
  }, [buildViewRequest]);

  // Load cache on mount and preset change
  useEffect(() => {
    setState({ kind: 'reading-cache' });
    loadCached();
  }, [loadCached]);

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
            onClick={handleSync}
            disabled={isSyncing}
            className={styles.primaryAction}
          >
            {isSyncing ? t(locale, 'usage.syncing') : t(locale, 'usage.syncData')}
          </button>
          {errorMsg && (
            <p className={styles.cacheError}>{errorMsg}</p>
          )}
        </div>
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
            onClick={handleSync}
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
            onClick={() => setErrorMsg(null)}
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

      {data && (
        <div className={styles.sourcesSection}>
          <UsageSourcesPanel sources={data.sources} warnings={data.warnings} lastRefresh={lastSyncTime} rtk={data.rtk} />
        </div>
      )}
    </div>
  );
}
