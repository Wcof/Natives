'use client';

import { useState, useEffect, useMemo, useCallback } from 'react';
import { useLocale, t } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import type { UsageDashboardResponse, UsageCacheReadResult } from '@/types/usage';
import { filterUsageRecords, aggregateUsageMetrics } from '@/lib/usage-dashboard';
import { RefreshCw, AlertCircle, Database, Zap } from 'lucide-react';
import { classifyError } from '@/lib/error-classifier';
import { EmptyState, LoadingState } from '@/components/ui/EmptyState';

export default function UsagePanel() {
  const locale = useLocale();
  const [usageData, setUsageData] = useState<UsageDashboardResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadCached = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.getCached) throw new Error('usage API not available');
      const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
      const result = (await api.usage.getCached({
        preset: '30d',
        timeZone,
        projectPath: null,
      })) as UsageCacheReadResult;
      if (result.state === 'ready') {
        setUsageData(result.response);
      }
    } catch (err: any) {
      const classified = classifyError(err);
      setError(classified.userMessage);
    } finally {
      setLoading(false);
    }
  }, []);

  const handleSync = useCallback(async () => {
    setSyncing(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.sync) throw new Error('usage API not available');
      const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
      const result = (await api.usage.sync({
        timeZone,
        currentView: {
          preset: '30d',
          timeZone,
          projectPath: null,
        },
      })) as { metadata: unknown; response: UsageDashboardResponse };
      setUsageData(result.response);
    } catch (err: any) {
      const classified = classifyError(err);
      setError(classified.userMessage);
    } finally {
      setSyncing(false);
    }
  }, []);

  // Load on mount
  useEffect(() => { loadCached(); }, [loadCached]);

  // Filter and aggregate (no filters in compact view)
  const metrics = useMemo(() => {
    if (!usageData) return null;
    const { daily, sessions } = filterUsageRecords(usageData, null, null, null, null);
    return aggregateUsageMetrics(daily, sessions, usageData.sources);
  }, [usageData]);

  // 首载 loading 不得伪装成「无缓存」空态（R-E10 三态）
  if (loading && !usageData) {
    return <LoadingState message={t(locale, 'common.loading')} />;
  }

  if (error && !usageData) {
    return (
      <div style={{ padding: SPACING.md }}>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: SPACING.sm, padding: SPACING.md, textAlign: 'center' }}>
          <AlertCircle size={20} style={{ color: 'var(--danger)' }} />
          <p style={{ fontSize: FONT_SIZE.xs, color: 'var(--text)' }}>{error}</p>
          <button onClick={handleSync} style={{ padding: '4px 12px', borderRadius: BORDER_RADIUS.sm, background: 'var(--primary)', color: 'var(--primary-dark)', border: 'none', cursor: 'pointer', fontSize: FONT_SIZE.xs }}>
            {t(locale, 'usage.syncData')}
          </button>
        </div>
      </div>
    );
  }

  if (!usageData || usageData.daily.length === 0) {
    return (
      <div style={{ padding: SPACING.md }}>
        <EmptyState icon={<Database size={20} />} title={t(locale, 'usage.noCache')} />
        <button onClick={handleSync} disabled={syncing} style={{ marginTop: SPACING.sm, padding: '4px 12px', borderRadius: BORDER_RADIUS.sm, background: 'var(--primary)', color: 'var(--primary-dark)', border: 'none', cursor: 'pointer', fontSize: FONT_SIZE.xs, display: 'flex', alignItems: 'center', gap: 4 }}>
          <RefreshCw size={12} style={{ animation: syncing ? 'spin 0.8s linear infinite' : undefined }} /> {syncing ? t(locale, 'usage.syncing') : t(locale, 'usage.syncData')}
        </button>
      </div>
    );
  }

  return (
    <div style={{ padding: SPACING.sm, display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
      {/* Source summary */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: SPACING.xs, marginBottom: SPACING.xs }}>
        {usageData.sources.filter((s) => s.state === 'ok' || s.state === 'partial').map((s) => (
          <div key={s.id} style={{
            display: 'flex', alignItems: 'center', gap: 4,
            padding: '2px 8px', borderRadius: 4,
            background: 'var(--surface-hover)', fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)',
          }}>
            <Database size={10} /> {s.label}
          </div>
        ))}
      </div>

      {/* Metrics grid */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: SPACING.xs }}>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--surface)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-secondary)' }}>{t(locale, 'usage.estimatedCost')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.estimatedCost != null ? `$${metrics.estimatedCost.toFixed(4)}` : '—'}
          </div>
        </div>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--surface)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-secondary)' }}>{t(locale, 'usage.totalTokens')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.totalTokens != null ? metrics.totalTokens.toLocaleString() : '—'}
          </div>
        </div>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--surface)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-secondary)' }}>{t(locale, 'usage.sessions')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.totalSessions ?? 0}
          </div>
        </div>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--surface)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-secondary)' }}>{t(locale, 'usage.estimatedActiveDuration')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.estimatedActiveSeconds != null ? `${Math.round(metrics.estimatedActiveSeconds / 60)}m` : '—'}
          </div>
        </div>
      </div>

      {/* RTK savings */}
      {usageData.rtk && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--surface-hover)', fontSize: FONT_SIZE.xs, color: 'var(--text)' }}>
          <Zap size={12} style={{ color: 'var(--warning)' }} />
          {t(locale, 'usage.rtkSavings')}: {usageData.rtk.totalSavedTokens.toLocaleString()} {t(locale, 'usage.tokens')}
        </div>
      )}

      {/* 已有数据时的同步失败不再静默（原来 error 只在无数据分支渲染） */}
      {error && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--danger-soft)', fontSize: FONT_SIZE.xs, color: 'var(--danger)' }}>
          <AlertCircle size={10} />
          {error}
        </div>
      )}

      {/* Warnings */}
      {usageData.warnings.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--warning-soft)', border: '0.0625rem solid var(--warning)', fontSize: 10, color: 'var(--text-secondary)' }}>
          <AlertCircle size={10} style={{ color: 'var(--warning)' }} />
          {usageData.warnings.length} {t(locale, 'usage.warningsCount')}
        </div>
      )}

      {/* Sync button */}
      <button onClick={handleSync} disabled={syncing} style={{
        display: 'flex', alignItems: 'center', gap: 4,
        padding: '4px 12px', borderRadius: BORDER_RADIUS.sm,
        border: '0.0625rem solid var(--border)', background: 'transparent',
        color: 'var(--text)', fontSize: FONT_SIZE.xs, cursor: syncing ? 'default' : 'pointer',
        alignSelf: 'flex-start', opacity: syncing ? 0.6 : 1,
      }}>
        <RefreshCw size={12} style={{ animation: syncing ? 'spin 0.8s linear infinite' : undefined }} />
        {syncing ? t(locale, 'usage.syncing') : t(locale, 'usage.syncData')}
      </button>
    </div>
  );
}