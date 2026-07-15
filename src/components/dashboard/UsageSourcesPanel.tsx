'use client';

import React from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import type { UsageSourceStatus, UsageWarning, RtkSummary } from '@/types/usage';
import { Layers, AlertCircle, Clock, Zap, CheckCircle, XCircle, MinusCircle } from 'lucide-react';

interface Props {
  sources: UsageSourceStatus[];
  warnings: UsageWarning[];
  lastRefresh: number | null;
  rtk: RtkSummary | null;
}

function StateIcon({ state }: { state: UsageSourceStatus['state'] }) {
  switch (state) {
    case 'ok': return <CheckCircle size={14} style={{ color: 'var(--semantic-green)' }} />;
    case 'partial': return <AlertCircle size={14} style={{ color: 'var(--warning)' }} />;
    case 'unavailable': return <XCircle size={14} style={{ color: 'var(--danger)' }} />;
  }
}

function CapabilityBadge({ label, available }: { label: string; available: boolean }) {
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 3,
      padding: '1px 6px', borderRadius: 4,
      fontSize: 10, fontWeight: 500,
      background: available ? 'var(--accent-soft)' : 'var(--bg-3)',
      color: available ? 'var(--accent)' : 'var(--text-faint)',
    }}>
      {available ? <CheckCircle size={10} /> : <MinusCircle size={10} />}
      {label}
    </span>
  );
}

export function UsageSourcesPanel({ sources, warnings, lastRefresh, rtk }: Props) {
  const locale = useLocale();

  const formatTime = (ms: number | null) => {
    if (!ms) return '—';
    return new Date(ms).toLocaleString(locale === 'en' ? 'en-US' : 'zh-CN');
  };

  return (
    <div style={{ padding: SPACING.md, borderRadius: BORDER_RADIUS.md, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
      <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)', marginBottom: SPACING.sm, display: 'flex', alignItems: 'center', gap: 6 }}>
        <Layers size={14} /> {t(locale, 'usage.dataSources')}
        {lastRefresh && (
          <span style={{ marginLeft: 'auto', fontWeight: 400, fontSize: 10, color: 'var(--text-faint)', display: 'flex', alignItems: 'center', gap: 3 }}>
            <Clock size={10} /> {t(locale, 'usage.lastRefresh')}: {formatTime(lastRefresh)}
          </span>
        )}
      </div>

      {/* RTK summary */}
      {rtk && (
        <div style={{ marginBottom: SPACING.sm, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--bg-3)', display: 'flex', alignItems: 'center', gap: SPACING.xs, fontSize: FONT_SIZE.xs, color: 'var(--text)' }}>
          <Zap size={12} style={{ color: 'var(--semantic-amber)' }} />
          {t(locale, 'usage.rtkSavings')}: {rtk.totalSavedTokens.toLocaleString()} {t(locale, 'usage.tokens')} · {rtk.totalCommands} {t(locale, 'usage.commands')}
        </div>
      )}

      {/* Source status grid */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: SPACING.xs }}>
        {sources.map((s) => (
          <div key={s.id} style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: 'var(--bg)', border: '0.0625rem solid var(--border)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
              <StateIcon state={s.state} />
              <span style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text)' }}>{s.label}</span>
              <span style={{ fontSize: 10, color: 'var(--text-faint)', marginLeft: 'auto' }}>
                {s.kind === 'natives' ? t(locale, 'usage.nativesSource') : t(locale, 'usage.externalSource')}
              </span>
            </div>
            {s.breadcrumbs.length > 0 && (
              <div style={{ fontSize: 10, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', marginBottom: 4, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {s.breadcrumbs.map((b, i) => (
                  <span key={i}>{i > 0 ? ' · ' : ''}{b.label}</span>
                ))}
              </div>
            )}
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3 }}>
              <CapabilityBadge label={t(locale, 'usage.tokens')} available={s.capabilities.totalTokens} />
              <CapabilityBadge label={t(locale, 'usage.inputOutput')} available={s.capabilities.tokenBreakdown} />
              <CapabilityBadge label={t(locale, 'usage.cache')} available={s.capabilities.cache} />
              <CapabilityBadge label={t(locale, 'usage.cost')} available={s.capabilities.cost} />
              <CapabilityBadge label={t(locale, 'usage.hourly')} available={s.capabilities.hourly} />
              <CapabilityBadge label={t(locale, 'usage.project')} available={s.capabilities.project} />
              <CapabilityBadge label={t(locale, 'usage.sessions')} available={s.capabilities.sessions} />
              <CapabilityBadge label={t(locale, 'usage.duration')} available={s.capabilities.duration} />
            </div>
          </div>
        ))}
      </div>

      {/* Warnings */}
      {warnings.length > 0 && (
        <div style={{ marginTop: SPACING.sm }}>
          <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 4, display: 'flex', alignItems: 'center', gap: 4 }}>
            <AlertCircle size={12} /> {t(locale, 'usage.warnings')} ({warnings.length})
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {warnings.map((w, i) => (
              <div key={i} style={{ fontSize: 10, color: 'var(--text-dim)', padding: '2px 8px', borderRadius: 4, background: 'var(--bg-3)' }}>
                <span style={{ color: 'var(--text-faint)' }}>{w.sourceId ?? 'system'}:</span> {w.code}
                {Object.keys(w.details).length > 0 && (
                  <span style={{ color: 'var(--text-faint)', marginLeft: 4 }}>
                    ({Object.entries(w.details).map(([k, v]) => `${k}=${v}`).join(', ')})
                  </span>
                )}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
