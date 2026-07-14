'use client';

import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import type { UsageDashboardResponse, UsageDashboardRequest, UsageMetrics } from '@/types/usage';
import { filterUsageRecords, aggregateUsageMetrics, uniqueSessionCount } from '@/lib/usage-dashboard';
import {
  serializeUsageCsv, serializeUsageBadgeSvg, serializeUsageMarkdown,
  hasShareableMetrics, defaultExportFilename,
} from '@/lib/usage-export';
import { UsageToolbar } from './UsageToolbar';
import { UsageMetricGrid } from './UsageMetricGrid';
import { UsageCharts } from './UsageCharts';
import { UsageSourcesPanel } from './UsageSourcesPanel';
import {
  RefreshCw, AlertCircle, Inbox,
} from 'lucide-react';
import { EmptyState } from '@/components/ui/EmptyState';
import { classifyError } from '@/lib/error-classifier';
import styles from './UsageDashboard.module.css';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';

// ── Date presets ──
const PRESETS: { key: string; labelKey: string; days: number }[] = [
  { key: 'today', labelKey: 'usage.dateToday', days: 1 },
  { key: '7d', labelKey: 'usage.date7d', days: 7 },
  { key: '30d', labelKey: 'usage.date30d', days: 30 },
  { key: '90d', labelKey: 'usage.date90d', days: 90 },
  { key: 'custom', labelKey: 'usage.dateCustom', days: 0 },
];

function getDateRange(days: number): { startMs: number; endMs: number } {
  const now = new Date();
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59, 59, 999);
  if (days === 1) {
    const start = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 0, 0, 0);
    return { startMs: start.getTime(), endMs: end.getTime() };
  }
  const start = new Date(end.getTime() - days * 86400000);
  return { startMs: start.getTime(), endMs: end.getTime() };
}

type LoadState = 'idle' | 'loading' | 'error' | 'empty' | 'partial' | 'loaded';

export function UsageDashboard() {
  const locale = useLocale();
  const { toast } = useToast();
  
  const [preset, setPreset] = useState('30d');
  const [customStart, setCustomStart] = useState('');
  const [customEnd, setCustomEnd] = useState('');
  const [sourceFilter, setSourceFilter] = useState<string[] | null>(null);
  const [modelFilter, setModelFilter] = useState<string[] | null>(null);
  const [projectFilter, setProjectFilter] = useState<string[] | null>(null);
  const [terminalFilter, setTerminalFilter] = useState<string[] | null>(null);
  const [data, setData] = useState<UsageDashboardResponse | null>(null);
  const [lastRefresh, setLastRefresh] = useState<number | null>(null);
  const [loadState, setLoadState] = useState<LoadState>('idle');
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [errorRetryable, setErrorRetryable] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);

  // Export state
  const [exportType, setExportType] = useState<'csv' | 'badge' | null>(null);
  const [exportFilename, setExportFilename] = useState('');
  const [isExporting, setIsExporting] = useState(false);

  // Compute date range
  const dateRange = useMemo(() => {
    if (preset === 'custom') {
      if (!customStart || !customEnd) return null;
      const start = new Date(customStart + 'T00:00:00');
      const endDate = new Date(customEnd + 'T23:59:59.999');
      if (start > endDate) return null;
      return { startMs: start.getTime(), endMs: endDate.getTime() };
    }
    const p = PRESETS.find((pr) => pr.key === preset);
    return p ? getDateRange(p.days) : getDateRange(30);
  }, [preset, customStart, customEnd]);

  // Fetch data
  const fetchData = useCallback(async (force = false) => {
    if (!dateRange) return;
    if (force) setIsRefreshing(true);
    else setLoadState('loading');
    setErrorMsg(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.refresh) throw new Error('usage API not available');
      const request: UsageDashboardRequest = {
        startMs: dateRange.startMs,
        endMs: dateRange.endMs,
        force,
        includeComparison: true,
        timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      };
      const response = (await api.usage.refresh(request)) as UsageDashboardResponse;
      setData(response);
      setLastRefresh(Date.now());
      if (response.daily.length === 0 && response.sessions.length === 0) {
        setLoadState('empty');
      } else if (response.warnings.some((w: any) => w.sourceId !== null && !w.code.startsWith('CLI'))) {
        setLoadState('partial');
      } else {
        setLoadState('loaded');
      }
    } catch (err: any) {
      const classified = classifyError(err);
      setErrorMsg(classified.userMessage);
      setErrorRetryable(classified.retryable);
      setLoadState('error');
    } finally {
      setIsRefreshing(false);
    }
  }, [dateRange]);

  // Fetch on date range change
  useEffect(() => {
    fetchData(false);
  }, [fetchData]);

  // Filtered data
  const filtered = useMemo(() => {
    if (!data) return null;
    return filterUsageRecords(data, sourceFilter, modelFilter, projectFilter, terminalFilter);
  }, [data, sourceFilter, modelFilter, projectFilter, terminalFilter]);

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
      projectFilter,
      terminalFilter,
    );
    return aggregateUsageMetrics(filteredComp.daily, filteredComp.sessions, data.sources);
  }, [data?.comparison, data?.sources, sourceFilter, modelFilter, projectFilter, terminalFilter]);

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
      projectFilter,
      terminalFilter,
    );
    return uniqueSessionCount(filteredComp.sessions);
  }, [data?.comparison, sourceFilter, modelFilter, projectFilter, terminalFilter]);

  const shareable = useMemo(() => hasShareableMetrics(metrics), [metrics]);

  // Set default filename when opening export dialog
  const openExportDialog = (type: 'csv' | 'badge') => {
    const ext = type === 'csv' ? '.csv' : '.svg';
    const base = defaultExportFilename('natives-usage');
    setExportFilename(base + ext);
    setExportType(type);
  };

  // Asynchronous export execution (non-blocking UI)
  const handleDoExport = async () => {
    if (!filtered || !exportType || !exportFilename) return;
    setIsExporting(true);
    try {
      const api = window.nativesAPI;
      if (exportType === 'csv') {
        const csv = serializeUsageCsv(filtered.daily);
        if (api?.fs?.writeFileAtomic) {
          await api.fs.writeFileAtomic(exportFilename, csv);
          toast(t(locale, 'usage.exportedSuccess'), 'success');
        } else {
          throw new Error('writeFileAtomic not available');
        }
      } else if (exportType === 'badge') {
        if (!metrics) return;
        const period = `${new Date(data!.range.startMs).toISOString().slice(0, 10)} – ${new Date(data!.range.endMs).toISOString().slice(0, 10)}`;
        const svg = serializeUsageBadgeSvg({
          period,
          tokens: metrics.totalTokens,
          cost: metrics.estimatedCost,
          sessions: metrics.totalSessions,
        });
        if (api?.fs?.writeFileAtomic) {
          await api.fs.writeFileAtomic(exportFilename, svg);
          toast(t(locale, 'usage.badgeSaved'), 'success');
        } else {
          throw new Error('writeFileAtomic not available');
        }
      }
      setExportType(null);
    } catch (err: any) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setIsExporting(false);
    }
  };

  const handleCopyMarkdown = useCallback(async () => {
    if (!filtered || !data || !metrics) return;
    const period = `${new Date(data.range.startMs).toISOString().slice(0, 10)} – ${new Date(data.range.endMs).toISOString().slice(0, 10)}`;
    const md = serializeUsageMarkdown(period, metrics, totalSessions);
    try {
      await navigator.clipboard.writeText(md);
      toast(t(locale, 'usage.copiedToClipboard'), 'success');
    } catch (err: any) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [filtered, data, metrics, totalSessions, locale, toast]);

  if (loadState === 'idle' || loadState === 'loading') {
    return (
      <div className={styles.loadingContainer}>
        <div className={styles.loadingInner}>
          <RefreshCw size={24} style={{ animation: 'spin 0.8s linear infinite', color: 'var(--text-dim)' }} />
          <span className={styles.loadingText}>{t(locale, 'usage.loading')}</span>
        </div>
      </div>
    );
  }

  if (loadState === 'error') {
    return (
      <div className={styles.errorContainer}>
        <div className={styles.errorCard}>
          <AlertCircle size={24} style={{ color: 'var(--danger)' }} />
          <p className={styles.errorText}>{errorMsg}</p>
          {errorRetryable && (
            <button onClick={() => fetchData(true)} className={styles.retryButton}>
              {t(locale, 'usage.retry')}
            </button>
          )}
        </div>
      </div>
    );
  }

  if (loadState === 'empty') {
    return (
      <div className={styles.emptyContainer}>
        <div className={styles.toolbarSection}>
          <UsageToolbar
            preset={preset}
            onPresetChange={setPreset}
            customStart={customStart}
            customEnd={customEnd}
            onCustomStartChange={setCustomStart}
            onCustomEndChange={setCustomEnd}
            sources={data?.dimensions.sources ?? []}
            models={data?.dimensions.models ?? []}
            projects={data?.dimensions.projects ?? []}
            terminals={data?.dimensions.terminals ?? []}
            sourceFilter={sourceFilter}
            modelFilter={modelFilter}
            projectFilter={projectFilter}
            terminalFilter={terminalFilter}
            onSourceFilterChange={setSourceFilter}
            onModelFilterChange={setModelFilter}
            onProjectFilterChange={setProjectFilter}
            onTerminalFilterChange={setTerminalFilter}
          />
        </div>
        <EmptyState
          icon={<Inbox size={32} />}
          title={t(locale, 'usage.emptyTitle')}
          description={t(locale, 'usage.emptyHint')}
        />
      </div>
    );
  }

  return (
    <div className={styles.container}>
      <div style={{
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center',
        gap: 16,
        marginBottom: SPACING.md,
        flexWrap: 'wrap',
      }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <UsageToolbar
            preset={preset}
            onPresetChange={setPreset}
            customStart={customStart}
            customEnd={customEnd}
            onCustomStartChange={setCustomStart}
            onCustomEndChange={setCustomEnd}
            sources={data?.dimensions.sources ?? []}
            models={data?.dimensions.models ?? []}
            projects={data?.dimensions.projects ?? []}
            terminals={data?.dimensions.terminals ?? []}
            sourceFilter={sourceFilter}
            modelFilter={modelFilter}
            projectFilter={projectFilter}
            terminalFilter={terminalFilter}
            onSourceFilterChange={setSourceFilter}
            onModelFilterChange={setModelFilter}
            onProjectFilterChange={setProjectFilter}
            onTerminalFilterChange={setTerminalFilter}
            style={{ marginBottom: 0 }}
          />
        </div>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <button
            onClick={handleCopyMarkdown}
            disabled={!shareable}
            style={{
              padding: '6px 12px',
              borderRadius: '20px',
              border: '1px solid var(--border)',
              background: 'var(--bg-2)',
              color: 'var(--text)',
              fontSize: '11px',
              fontWeight: 500,
              cursor: 'pointer',
              opacity: shareable ? 1 : 0.5,
            }}
          >
            {locale === 'zh' ? '分享' : 'Share'}
          </button>
          <button
            onClick={() => fetchData(true)}
            disabled={isRefreshing}
            style={{
              padding: '6px 12px',
              borderRadius: '20px',
              border: '1px solid var(--border)',
              background: 'var(--bg-2)',
              color: 'var(--text)',
              fontSize: '11px',
              fontWeight: 500,
              cursor: 'pointer',
              opacity: isRefreshing ? 0.6 : 1,
            }}
          >
            {locale === 'zh' ? '同步数据' : 'Sync Data'}
          </button>
          <button
            onClick={() => openExportDialog('badge')}
            disabled={!shareable}
            style={{
              padding: '6px 12px',
              borderRadius: '20px',
              border: '1px solid var(--border)',
              background: 'var(--bg-2)',
              color: 'var(--text)',
              fontSize: '11px',
              fontWeight: 500,
              cursor: 'pointer',
              opacity: shareable ? 1 : 0.5,
            }}
          >
            {locale === 'zh' ? '使用量 Badge' : 'Usage Badge'}
          </button>
        </div>
      </div>

      {loadState === 'partial' && data && (
        <div className={styles.partialBanner}>
          <AlertCircle size={14} style={{ color: 'var(--warning)' }} />
          <span>
            {t(locale, 'usage.partialData')}: {data.warnings.filter((w) => w.sourceId !== null).length} {t(locale, 'usage.warningsCount')}
          </span>
        </div>
      )}

      <UsageMetricGrid
        metrics={metrics}
        prevMetrics={prevMetrics}
        totalSessions={totalSessions}
        prevTotalSessions={prevTotalSessions}
      />

      <UsageCharts
        daily={filtered?.daily ?? []}
        activity={filtered?.activity ?? []}
        sessions={filtered?.sessions ?? []}
        sources={data?.sources ?? []}
        metrics={metrics}
        lastRefresh={lastRefresh}
      />

      {data && (
        <div className={styles.sourcesSection}>
          <UsageSourcesPanel sources={data.sources} warnings={data.warnings} lastRefresh={lastRefresh} rtk={data.rtk} />
        </div>
      )}

      {/* Export Dialog Modal */}
      {exportType && (
        <Modal
          isOpen={true}
          onClose={() => setExportType(null)}
          title={t(locale, exportType === 'csv' ? 'usage.exportCsv' : 'usage.saveBadge')}
          width={400}
        >
          <div style={{ padding: '8px 0', display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              <label style={{ fontSize: '11px', color: 'var(--text-dim)', fontWeight: 500 }}>
                {locale === 'zh' ? '导出文件路径 (包含文件名)' : 'Export Path (including filename)'}
              </label>
              <input
                type="text"
                value={exportFilename}
                onChange={(e) => setExportFilename(e.target.value)}
                style={{
                  padding: '8px 10px',
                  borderRadius: 6,
                  border: '1px solid var(--border)',
                  background: 'var(--bg-2)',
                  color: 'var(--text)',
                  fontSize: '12px',
                  fontFamily: 'var(--font-mono)',
                  width: '100%',
                }}
              />
            </div>
            
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 12 }}>
              <button
                onClick={() => setExportType(null)}
                style={{
                  padding: '6px 12px',
                  borderRadius: 6,
                  border: '1px solid var(--border)',
                  background: 'transparent',
                  color: 'var(--text)',
                  fontSize: '12px',
                  cursor: 'pointer',
                }}
              >
                {t(locale, 'tools.cancel')}
              </button>
              <button
                onClick={handleDoExport}
                disabled={isExporting || !exportFilename}
                style={{
                  padding: '6px 12px',
                  borderRadius: 6,
                  border: 'none',
                  background: 'var(--text)',
                  color: 'var(--bg)',
                  fontSize: '12px',
                  fontWeight: 600,
                  cursor: 'pointer',
                  opacity: isExporting || !exportFilename ? 0.6 : 1,
                }}
              >
                {isExporting ? '...' : t(locale, 'tools.save')}
              </button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}