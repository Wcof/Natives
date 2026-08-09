'use client';

import React from 'react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import type { UsageMetrics } from '@/types/usage';
import { fmtCount, fmtDuration } from '@/lib/format';
import { Coins, Cpu, MessageSquare, Clock, Activity, Download, Upload, Database, Timer, User } from 'lucide-react';

interface Props {
  metrics: UsageMetrics | null;
  prevMetrics: UsageMetrics | null;
  totalSessions: number;
  prevTotalSessions: number;
  lastSyncTime?: number | null;
}

interface CardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  exactValue?: string;
  delta: React.ReactNode;
  subLabel?: string;
  subValue?: string;
  exactSubValue?: string;
}

function MetricCard({ icon, label, value, exactValue, delta, subLabel, subValue, exactSubValue }: CardProps) {
  return (
    <div style={{
      padding: `${SPACING.sm}px ${SPACING.md}px`,
      borderRadius: 12,
      background: 'var(--bg-2)',
      border: '0.0625rem solid var(--border)',
      display: 'flex', flexDirection: 'column', gap: 4,
      justifyContent: 'space-between',
      minHeight: '84px',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: FONT_SIZE.xs, color: 'var(--text-dim)' }}>
        {icon}
        <span className="truncate">{label}</span>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', margin: '4px 0' }}>
        <span title={exactValue} style={{ fontSize: '18px', fontWeight: 700, color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>{value}</span>
        {delta}
      </div>
      {subLabel && (
        <div style={{ fontSize: '11px', color: 'var(--text-faint)', display: 'flex', justifyContent: 'space-between' }}>
          <span>{subLabel}</span>
          <span title={exactSubValue} style={{ fontFamily: 'var(--font-mono)' }}>{subValue}</span>
        </div>
      )}
    </div>
  );
}

function renderDelta(current: number | null, previous: number | null, isLowerBetter: boolean, newLabel: string) {
  if (current === null || previous === null) {
    return <span style={{ color: 'var(--text-faint)', fontSize: '11px', marginLeft: '6px' }}>—</span>;
  }
  if (previous === 0) {
    if (current === 0) {
      return <span style={{ color: 'var(--text-faint)', fontSize: '11px', marginLeft: '6px' }}>0%</span>;
    }
    return (
      <span style={{
        fontSize: '10px',
        padding: '2px 6px',
        borderRadius: '4px',
        background: 'var(--success-soft)',
        color: 'var(--success)',
        fontWeight: 600,
        marginLeft: '6px'
      }}>
        {newLabel}
      </span>
    );
  }
  const pct = ((current - previous) / previous) * 100;
  if (Math.abs(pct) < 0.5) {
    return <span style={{ color: 'var(--text-faint)', fontSize: '11px', marginLeft: '6px' }}>0%</span>;
  }
  
  const isUp = pct > 0;
  // If lower is better (e.g. cost/tokens), up is bad (danger/red), down is good (success/green).
  // If higher is better, up is good, down is bad.
  const isGood = isLowerBetter ? !isUp : isUp;
  
  const color = isGood ? 'var(--success)' : 'var(--danger)';
  const bg = isGood ? 'var(--success-soft)' : 'var(--danger-soft)';
  const prefix = isUp ? '+' : '';
  
  return (
    <span style={{
      fontSize: '10px',
      padding: '2px 6px',
      borderRadius: '4px',
      background: bg,
      color: color,
      fontWeight: 600,
      marginLeft: '6px'
    }}>
      {prefix}{pct.toFixed(0)}%
    </span>
  );
}

export function UsageMetricGrid({ metrics, prevMetrics, totalSessions, prevTotalSessions, lastSyncTime }: Props) {
  const locale = useLocale();

  // Cost card values
  const costStr = metrics?.estimatedCost != null ? `$${metrics.estimatedCost.toFixed(4)}` : '—';
  
  // Tokens card values
  const tokensStr = metrics?.totalTokens != null ? fmtCount(metrics.totalTokens, locale) : '—';
  const inputStr = metrics?.totalInputTokens != null ? fmtCount(metrics.totalInputTokens, locale) : '—';
  const outputStr = metrics?.totalOutputTokens != null ? fmtCount(metrics.totalOutputTokens, locale) : '—';
  const cacheReadStr = metrics?.totalCacheReadTokens != null ? fmtCount(metrics.totalCacheReadTokens, locale) : '—';
  const cacheCreationStr = metrics?.totalCacheCreationTokens != null ? fmtCount(metrics.totalCacheCreationTokens, locale) : '—';
  const exact = (value: number | null | undefined) => value == null ? undefined : value.toLocaleString(locale.startsWith('zh') ? 'zh-CN' : 'en-US');

  // Duration card values
  const durationStr = metrics?.estimatedActiveSeconds != null ? fmtDuration(metrics.estimatedActiveSeconds, locale) : '—';
  const spanStr = metrics?.sessionSpanMs != null ? fmtDuration(Math.round(metrics.sessionSpanMs / 1000), locale) : '—';

  // Messages values
  const totalMsg = metrics ? metrics.totalUserMessages + metrics.totalAssistantMessages : null;
  const prevTotalMsg = prevMetrics ? prevMetrics.totalUserMessages + prevMetrics.totalAssistantMessages : null;

  return (
    <div style={{ marginBottom: SPACING.md }}>
      <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.sm, display: 'flex', alignItems: 'center', gap: 6, justifyContent: 'space-between', width: '100%' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <Activity size={14} /> {t(locale, 'usage.metrics')}
        </div>
        {lastSyncTime && (
          <span style={{ fontSize: '11px', fontWeight: 400, color: 'var(--text-disabled)' }}>
            {t(locale, 'usage.dataAsOf')} {new Date(lastSyncTime).toLocaleString(locale.startsWith('zh') ? 'zh-CN' : 'en-US')}
          </span>
        )}
      </div>
      
      {/* 第一排看板 */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-2.5 mb-2.5">
        <MetricCard
          icon={<Coins size={12} />}
          label={t(locale, 'usage.estimatedCost')}
          value={costStr}
          delta={renderDelta(metrics?.estimatedCost ?? null, prevMetrics?.estimatedCost ?? null, true, t(locale, 'usage.newData'))}
          subLabel={metrics?.costCoverage != null ? t(locale, 'usage.costCoverage') : undefined}
          subValue={metrics?.costCoverage != null ? `${Math.round(metrics.costCoverage * 100)}%` : undefined}
        />
        <MetricCard
          icon={<Cpu size={12} />}
          label={t(locale, 'usage.totalTokens')}
          value={tokensStr}
          exactValue={exact(metrics?.totalTokens)}
          delta={renderDelta(metrics?.totalTokens ?? null, prevMetrics?.totalTokens ?? null, true, t(locale, 'usage.newData'))}
        />
        <MetricCard
          icon={<Download size={12} />}
          label={t(locale, 'usage.inputTokens')}
          value={inputStr}
          exactValue={exact(metrics?.totalInputTokens)}
          delta={renderDelta(metrics?.totalInputTokens ?? null, prevMetrics?.totalInputTokens ?? null, true, t(locale, 'usage.newData'))}
        />
        <MetricCard
          icon={<Upload size={12} />}
          label={t(locale, 'usage.outputTokens')}
          value={outputStr}
          exactValue={exact(metrics?.totalOutputTokens)}
          delta={renderDelta(metrics?.totalOutputTokens ?? null, prevMetrics?.totalOutputTokens ?? null, true, t(locale, 'usage.newData'))}
        />
        <MetricCard
          icon={<Database size={12} />}
          label={t(locale, 'usage.cacheRead')}
          value={cacheReadStr}
          exactValue={exact(metrics?.totalCacheReadTokens)}
          delta={renderDelta(metrics?.totalCacheReadTokens ?? null, prevMetrics?.totalCacheReadTokens ?? null, true, t(locale, 'usage.newData'))}
          subLabel={t(locale, 'usage.cacheCreation')}
          subValue={cacheCreationStr}
          exactSubValue={exact(metrics?.totalCacheCreationTokens)}
        />
      </div>

      {/* 第二排看板 */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-2.5">
        <MetricCard
          icon={<Clock size={12} />}
          label={t(locale, 'usage.estimatedActiveDuration')}
          value={durationStr}
          delta={renderDelta(metrics?.estimatedActiveSeconds ?? null, prevMetrics?.estimatedActiveSeconds ?? null, false, t(locale, 'usage.newData'))}
        />
        <MetricCard
          icon={<Timer size={12} />}
          label={t(locale, 'usage.totalDuration')}
          value={spanStr}
          delta={renderDelta(metrics?.sessionSpanMs ?? null, prevMetrics?.sessionSpanMs ?? null, false, t(locale, 'usage.newData'))}
        />
        <MetricCard
          icon={<MessageSquare size={12} />}
          label={t(locale, 'usage.sessions')}
          value={fmtCount(totalSessions, locale)}
          exactValue={exact(totalSessions)}
          delta={renderDelta(totalSessions, prevTotalSessions, false, t(locale, 'usage.newData'))}
        />
        <MetricCard
          icon={<MessageSquare size={12} />}
          label={t(locale, 'usage.totalMessages')}
          value={totalMsg != null ? fmtCount(totalMsg, locale) : '—'}
          exactValue={exact(totalMsg)}
          delta={renderDelta(totalMsg, prevTotalMsg, false, t(locale, 'usage.newData'))}
        />
        <MetricCard
          icon={<User size={12} />}
          label={t(locale, 'usage.userMessages')}
          value={metrics?.totalUserMessages != null ? fmtCount(metrics.totalUserMessages, locale) : '—'}
          exactValue={exact(metrics?.totalUserMessages)}
          delta={renderDelta(metrics?.totalUserMessages ?? null, prevMetrics?.totalUserMessages ?? null, false, t(locale, 'usage.newData'))}
        />
      </div>
    </div>
  );
}
