'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import { BarChart2, RefreshCw } from 'lucide-react';
import { EmptyState } from '@/components/ui/EmptyState';
import type { RtkUsage, RtkCommandStat } from '@/types/agent';
import { useAsyncData } from '@/hooks/useAsyncData';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

export default function RtkPanel() {
  const { data: usage, loading, error, reload: fetchUsage } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (!api?.usage?.refresh) return null;
    const result = await api.usage.refresh();
    return (result?.rtk ?? null) as RtkUsage | null;
  }, []);
  const [paused, setPaused] = useState(false);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  const topCommands: RtkCommandStat[] = usage?.topCommands ?? [];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', padding: 'var(--space-sm)' }}>
      {/* Header */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        marginBottom: SPACING.md,
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.rtkUsage')}
        </div>
        <div style={{ display: 'flex', gap: SPACING.xs }}>
          <button
            className="btn-ghost"
            onClick={() => setPaused(!paused)}
            style={{
              fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
              color: paused ? 'var(--warning)' : 'var(--text-faint)',
            }}
            title={paused ? t(locale, 'aiWorkbench.rtk.resumeTracking') : t(locale, 'aiWorkbench.rtk.pauseTracking')}
          >
            {paused ? '▶' : '⏸'}
          </button>
          <button
            className="btn-ghost"
            onClick={fetchUsage}
            disabled={loading}
            style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm }}
          >
            {loading ? '...' : <RefreshCw size={12} />}
          </button>
        </div>
      </div>

      {/* Stats cards */}
      <div style={{ display: 'flex', gap: 'var(--space-sm)', marginBottom: SPACING.md }}>
        <div style={{
          flex: 1, padding: 10, borderRadius: BORDER_RADIUS.md,
          background: 'var(--bg-2)', border: '1px solid var(--border)',
        }}>
          <div style={{ fontSize: 18, fontWeight: 700, color: 'var(--accent)' }}>
            {usage?.totalSaved ?? 0}
          </div>
          <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', marginTop: 2 }}>
            {t(locale, 'aiWorkbench.tokensSaved')}
          </div>
        </div>
        <div style={{
          flex: 1, padding: 10, borderRadius: BORDER_RADIUS.md,
          background: 'var(--bg-2)', border: '1px solid var(--border)',
        }}>
          <div style={{ fontSize: 18, fontWeight: 700, color: 'var(--text)' }}>
            {usage?.totalCommands ?? 0}
          </div>
          <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', marginTop: 2 }}>
            {t(locale, 'aiWorkbench.commands')}
          </div>
        </div>
      </div>

      {/* Top commands */}
      <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 6, textTransform: 'uppercase', letterSpacing: 0.5 }}>
        {t(locale, 'aiWorkbench.rtk.topCommands')}
      </div>
      <div style={{ flex: 1, overflow: 'auto' }}>
        {topCommands.length === 0 ? (
          <EmptyState icon={<BarChart2 size={32} />} title={t(locale, 'aiWorkbench.rtk.noData')} />
        ) : (
          topCommands.map((cmd, i) => (
            <div key={cmd.command} style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              padding: '6px 8px', marginBottom: 3, borderRadius: BORDER_RADIUS.sm,
              background: 'var(--bg-2)', fontSize: FONT_SIZE.sm,
            }}>
              <span style={{ color: 'var(--text-dim)', fontFamily: 'var(--font-mono)' }}>
                {cmd.command}
              </span>
              <span style={{ color: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}>
                {cmd.count}x · {cmd.totalSaved} {t(locale, 'aiWorkbench.tokensSaved')}
              </span>
            </div>
          ))
        )}
      </div>

      {/* Status */}
      {paused && (
        <div style={{
          fontSize: FONT_SIZE.xs, color: 'var(--warning)', textAlign: 'center', padding: 4,
          marginTop: 8, borderTop: '1px solid var(--border)',
        }}>
          ⏸ {t(locale, 'aiWorkbench.rtk.paused')}
        </div>
      )}
    </div>
  );
}
