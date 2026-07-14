'use client';

import { useState, useEffect, useMemo, useCallback } from 'react';
import { useLocale, t } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import type { UsageDashboardResponse, UsageDashboardRequest } from '@/types/usage';
import { filterUsageRecords, aggregateUsageMetrics } from '@/lib/usage-dashboard';
import { RefreshCw, AlertCircle, Database, Coins, Zap } from 'lucide-react';
import { classifyError } from '@/lib/error-classifier';
import { EmptyState, ErrorState } from '@/components/ui/EmptyState';

export default function UsagePanel() {
  const locale = useLocale();
  const [usageData, setUsageData] = useState<UsageDashboardResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryable, setRetryable] = useState(false);

  const fetchUsage = useCallback(async (force = false) => {
    setLoading(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.refresh) throw new Error('usage API not available');
      const end = new Date();
      const start = new Date(end.getTime() - 30 * 86400000);
      const request: UsageDashboardRequest = {
        startMs: start.getTime(),
        endMs: end.getTime(),
        force,
        includeComparison: false,
        timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      };
      const result = await api.usage.refresh(request);
      setUsageData(result);
    } catch (err: any) {
      const classified = classifyError(err);
      setError(classified.userMessage);
      setRetryable(classified.retryable);
    } finally {
      setLoading(false);
    }
  }, []);

  // Load on mount
  useEffect(() => { fetchUsage(false); }, [fetchUsage]);

  // Filter and aggregate (no filters in compact view)
  const metrics = useMemo(() => {
    if (!usageData) return null;
    const { daily, sessions } = filterUsageRecords(usageData, null, null, null, null);
    return aggregateUsageMetrics(daily, sessions, usageData.sources);
  }, [usageData]);

  if (loading && !usageData) {
    return (
      <div style={{ padding: SPACING.md, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: SPACING.sm, minHeight: 100 }}>
        <RefreshCw size={16} style={{ animation: 'spin 0.8s linear infinite', color: 'var(--text-dim)' }} />
        <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)' }}>{t(locale, 'usage.loading')}</span>
      </div>
    );
  }

  if (error && !usageData) {
    return (
      <div style={{ padding: SPACING.md }}>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: SPACING.sm, padding: SPACING.md, textAlign: 'center' }}>
          <AlertCircle size={20} style={{ color: 'var(--danger)' }} />
          <p style={{ fontSize: FONT_SIZE.xs, color: 'var(--text)' }}>{error}</p>
          {retryable && (
            <button onClick={() => fetchUsage(true)} style={{ padding: '4px 12px', borderRadius: BORDER_RADIUS.sm, background: 'var(--accent)', color: 'var(--accent-ink)', border: 'none', cursor: 'pointer', fontSize: FONT_SIZE.xs }}>
              {t(locale, 'usage.retry')}
            </button>
          )}
        </div>
      </div>
    );
  }

  if (!usageData || usageData.daily.length === 0) {
    return (
      <div style={{ padding: SPACING.md }}>
        <EmptyState icon={<Database size={20} />} title={t(locale, 'usage.emptyTitle')} />
        <button onClick={() => fetchUsage(true)} style={{ marginTop: SPACING.sm, padding: '4px 12px', borderRadius: BORDER_RADIUS.sm, background: 'var(--accent)', color: 'var(--accent-ink)', border: 'none', cursor: 'pointer', fontSize: FONT_SIZE.xs, display: 'flex', alignItems: 'center', gap: 4 }}>
          <RefreshCw size={12} /> {t(locale, 'usage.refresh')}
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
            background: 'var(--bg-3)', fontSize: FONT_SIZE.xs, color: 'var(--text-dim)',
          }}>
            <Database size={10} /> {s.label}
          </div>
        ))}
      </div>

      {/* Metrics grid */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: SPACING.xs }}>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>{t(locale, 'usage.estimatedCost')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.estimatedCost != null ? `$${metrics.estimatedCost.toFixed(4)}` : '—'}
          </div>
        </div>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>{t(locale, 'usage.totalTokens')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.totalTokens != null ? metrics.totalTokens.toLocaleString() : '—'}
          </div>
        </div>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>{t(locale, 'usage.sessions')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.totalSessions ?? 0}
          </div>
        </div>
        <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>{t(locale, 'usage.estimatedActiveDuration')}</div>
          <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>
            {metrics?.estimatedActiveSeconds != null ? `${Math.round(metrics.estimatedActiveSeconds / 60)}m` : '—'}
          </div>
        </div>
      </div>

      {/* RTK savings */}
      {usageData.rtk && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--bg-3)', fontSize: FONT_SIZE.xs, color: 'var(--text)' }}>
          <Zap size={12} style={{ color: 'var(--semantic-amber)' }} />
          {t(locale, 'usage.rtkSavings')}: {usageData.rtk.totalSavedTokens.toLocaleString()} {t(locale, 'usage.tokens')}
        </div>
      )}

      {/* Warnings */}
      {usageData.warnings.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--warning-soft', border: '0.0625rem solid var(--warning)', fontSize: 10, color: 'var(--text-dim)' }}>
          <AlertCircle size={10} style={{ color: 'var(--warning)' }} />
          {usageData.warnings.length} {t(locale, 'usage.warningsCount')}
        </div>
      )}

      {/* Refresh button */}
      <button onClick={() => fetchUsage(true)} disabled={loading} style={{
        display: 'flex', alignItems: 'center', gap: 4,
        padding: '4px 12px', borderRadius: BORDER_RADIUS.sm,
        border: '0.0625rem solid var(--border)', background: 'transparent',
        color: 'var(--text)', fontSize: FONT_SIZE.xs, cursor: loading ? 'default' : 'pointer',
        alignSelf: 'flex-start', opacity: loading ? 0.6 : 1,
      }}>
        <RefreshCw size={12} style={{ animation: loading ? 'spin 0.8s linear infinite' : undefined }} />
        {t(locale, 'usage.refresh')}
      </button>
    </div>
  );
}
