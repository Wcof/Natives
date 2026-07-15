'use client';

import React, { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { useLocale, t } from '@/i18n';
import type { UsageDashboardResponse, UsageCacheReadResult, UsageCacheMetadata, UsageMetrics, UsageViewRequest, DashboardState } from '@/types/usage';
import { filterUsageRecords, aggregateUsageMetrics, uniqueSessionCount } from '@/lib/usage-dashboard';
import {
  serializeUsageCsv, serializeUsageBadgeSvg, serializeUsageMarkdown,
  hasShareableMetrics, defaultExportFilename,
} from '@/lib/usage-export';
import { UsageToolbar } from './UsageToolbar';
import { UsageMetricGrid } from './UsageMetricGrid';
import { UsageCharts } from './UsageCharts';
import { UsageSourcesPanel } from './UsageSourcesPanel';
import { RefreshCw, AlertCircle, X } from 'lucide-react';
import { classifyError } from '@/lib/error-classifier';
import styles from './UsageDashboard.module.css';
import Modal from '@/components/ui/Modal';
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
  const [terminalFilter, setTerminalFilter] = useState<string[] | null>(null);
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
    } catch { /* ignore */ }
  }, []);

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
      setState({ kind: 'ready', data: result.response, metadata: result.metadata });
      setLastSyncTime(result.metadata.generatedAtMs);
      toast(t(locale, 'usage.syncedSuccess'), 'success');
    } catch (err: any) {
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
      setWarningSeconds((seconds) => {
        if (seconds <= 1) {
          setDismissedWarningKey(warningKey);
          return 0;
        }
        return seconds - 1;
      });
    }, 1000);
    return () => window.clearInterval(timer);
  }, [showWarning, warningKey]);

  const filtered = useMemo(() => {
    if (!data) return null;
    // Directory selection is applied while slicing the cache on the backend. Do not
    // filter it again here, otherwise child projects are incorrectly excluded.
    return filterUsageRecords(data, sourceFilter, modelFilter, null, terminalFilter);
  }, [data, sourceFilter, modelFilter, terminalFilter]);

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
      terminalFilter,
    );
    return aggregateUsageMetrics(filteredComp.daily, filteredComp.sessions, data.sources);
  }, [data?.comparison, data?.sources, sourceFilter, modelFilter, terminalFilter]);

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
      terminalFilter,
    );
    return uniqueSessionCount(filteredComp.sessions);
  }, [data?.comparison, sourceFilter, modelFilter, terminalFilter]);

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
    if (!filtered || !exportType) return;
    setIsExporting(true);
    try {
      const api = window.nativesAPI;
      // Get save path via dialog
      let savePath = exportFilename;
      if (api?.dialog?.saveFile) {
        const result = await api.dialog.saveFile();
        if (!result) {
          setIsExporting(false);
          return; // User cancelled
        }
        savePath = result;
      } else if (!exportFilename) {
        throw new Error('Either dialog.saveFile or exportFilename is required');
      }

      if (exportType === 'csv') {
        const csv = serializeUsageCsv(filtered.daily);
        if (api?.fs?.writeFileAtomic) {
          await api.fs.writeFileAtomic(savePath, csv);
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
          await api.fs.writeFileAtomic(savePath, svg);
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
      const api = window.nativesAPI;
      if (api?.clipboard?.write) {
        await api.clipboard.write(md);
      } else {
        await navigator.clipboard.writeText(md);
      }
      toast(t(locale, 'usage.copiedToClipboard'), 'success');
    } catch (err: any) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [filtered, data, metrics, totalSessions, locale, toast]);

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
            terminals={[]}
            sourceFilter={null}
            modelFilter={null}
            projectFilter={null}
            terminalFilter={null}
            onSourceFilterChange={() => {}}
            onModelFilterChange={() => {}}
            onProjectFilterChange={() => {}}
            onTerminalFilterChange={() => {}}
            onSelectDir={handleSelectDir}
          />
        </div>
        {/* Metrics show static —, no loading text */}
        <UsageMetricGrid
          metrics={null}
          prevMetrics={null}
          totalSessions={0}
          prevTotalSessions={0}
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
            terminals={[]}
            sourceFilter={null}
            modelFilter={null}
            projectFilter={null}
            terminalFilter={null}
            onSourceFilterChange={() => {}}
            onModelFilterChange={() => {}}
            onProjectFilterChange={() => {}}
            onTerminalFilterChange={() => {}}
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
      <div className={styles.header}>
        <div className={styles.headerActions}>
          {/* Date range display */}
          {metadata && data && (
            <span className={styles.dateRange}>
              {new Date(data.range.startMs).toLocaleDateString()} – {new Date(data.range.endMs).toLocaleDateString()}
            </span>
          )}
          <button
            onClick={handleCopyMarkdown}
            disabled={!shareable}
            className={styles.secondaryAction}
          >
            {t(locale, 'usage.share')}
          </button>
          <button
            onClick={handleSync}
            disabled={isSyncing}
            aria-busy={isSyncing}
            className={styles.secondaryAction}
          >
            <RefreshCw size={12} style={{ animation: isSyncing ? 'spin 0.8s linear infinite' : undefined }} />
            {isSyncing ? t(locale, 'usage.syncing') : t(locale, 'usage.syncData')}
          </button>
          {lastSyncTime && (
            <span className={styles.dateRange}>
              {t(locale, 'usage.dataAsOf')} {new Date(lastSyncTime).toLocaleString()}
            </span>
          )}
          <button
            onClick={() => openExportDialog('badge')}
            disabled={!shareable}
            className={styles.secondaryAction}
          >
            {t(locale, 'usage.usageBadge')}
          </button>
        </div>
      </div>

      {/* Toolbar: date presets + filters */}
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
        onSelectDir={handleSelectDir}
        style={{ marginBottom: 0 }}
      />

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
      />)}

      {filtered && (<UsageCharts
        daily={filtered?.daily ?? []}
        activity={filtered?.activity ?? []}
        sessions={filtered?.sessions ?? []}
        sources={data?.sources ?? []}
        metrics={metrics}
        lastRefresh={lastSyncTime}
      />)}

      {data && (
        <div className={styles.sourcesSection}>
          <UsageSourcesPanel sources={data.sources} warnings={data.warnings} lastRefresh={lastSyncTime} rtk={data.rtk} />
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
                {t(locale, 'usage.exportPath')}
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
