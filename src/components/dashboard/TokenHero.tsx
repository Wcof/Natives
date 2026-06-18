'use client';

import { useEffect, useState, useMemo } from 'react';
import { Activity, ArrowDownToLine, ArrowUpFromLine, Database, Sparkles, Loader2, Info } from 'lucide-react';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';
import { fmtCount } from '@/lib/format';
import type { ClaudeUsage } from '@/types/agent';
import { useTranslation } from 'react-i18next';

/**
 * TokenHero — Token usage hero card
 *
 * Layout:
 * ┌─────────────────────────────────────────────────────────┐
 * │  [Icon] Real Tokens Used    │ Requests │ Total Cost    │
 * │           12,345,678         │  1,234   │   $45.67      │
 * ├─────────────────────────────────────────────────────────┤
 * │ [Input] [Output] [Cache Write] [Cache Read] [Hit Rate] │
 * └─────────────────────────────────────────────────────────┘
 *
 * Data source: window.nativesAPI.usage (existing API)
 * Design: Follows vibe-* glassmorphic tokens (R-U1, R-U2)
 */

interface TokenHeroProps {
  usage?: ClaudeUsage | null;
  isLoading?: boolean;
}

interface TokenBreakdown {
  realTotal: number;
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
  hitRate: number;
  requests: number;
  cost: number | null;
}

function computeBreakdown(usage: ClaudeUsage | null): TokenBreakdown {
  if (!usage) {
    return {
      realTotal: 0,
      input: 0,
      output: 0,
      cacheWrite: 0,
      cacheRead: 0,
      hitRate: 0,
      requests: 0,
      cost: null,
    };
  }

  const input = usage.localTokens?.input ?? 0;
  const output = usage.localTokens?.output ?? 0;
  const cacheWrite = usage.localTokens?.cacheCreation ?? 0;
  const cacheRead = usage.localTokens?.cacheRead ?? 0;
  const realTotal = input + output + cacheWrite + cacheRead;

  // Cache hit rate = cacheRead / (input + cacheWrite + cacheRead)
  const cacheableInput = input + cacheWrite + cacheRead;
  const hitRate = cacheableInput > 0 ? cacheRead / cacheableInput : 0;

  const requests = usage.totalRequests ?? 0;
  const cost = usage.totalCost ?? null;

  return { realTotal, input, output, cacheWrite, cacheRead, hitRate, requests, cost };
}

function formatTokenShort(value: number): string {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}K`;
  }
  return value.toLocaleString();
}

export function TokenHero({ usage, isLoading }: TokenHeroProps) {
  const { t, i18n } = useTranslation();
  const [breakdown, setBreakdown] = useState<TokenBreakdown | null>(null);

  useEffect(() => {
    if (isLoading) {
      setBreakdown(null);
      return;
    }
    setBreakdown(computeBreakdown(usage ?? null));
  }, [usage, isLoading]);

  if (isLoading) {
    return (
      <div
        style={{
          borderRadius: BORDER_RADIUS.lg,
          border: '0.0625rem solid var(--vibe-content-border)',
          background: 'var(--vibe-content-bg)',
          backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
          padding: `${SPACING.lg}px`,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          minHeight: 140,
        }}
      >
        <Loader2 size={24} style={{ color: 'var(--text-faint)', animation: 'spin 1s linear infinite' }} />
      </div>
    );
  }

  if (!breakdown || breakdown.realTotal === 0) {
    return (
      <div
        style={{
          borderRadius: BORDER_RADIUS.lg,
          border: '0.0625rem solid var(--vibe-content-border)',
          background: 'var(--vibe-content-bg)',
          backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
          padding: `${SPACING.lg}px`,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: `${SPACING.sm}px` }}>
          <Info size={14} style={{ color: 'var(--text-faint)' }} />
          <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)' }}>
            {t('dashboard.noData')}
          </span>
        </div>
      </div>
    );
  }

  const hitPercent = Math.max(0, Math.min(100, breakdown.hitRate * 100));
  const hitPercentLabel = hitPercent >= 99.95 ? hitPercent.toFixed(0) : hitPercent.toFixed(1);

  return (
    <div
      style={{
        borderRadius: BORDER_RADIUS.lg,
        border: '0.0625rem solid var(--vibe-content-border)',
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
        padding: `${SPACING.lg}px`,
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: `${SPACING.md}px` }}>
        {/* Top row: Main token count + requests + cost */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: SPACING.sm }}>
            <span style={{ fontSize: FONT_SIZE.xl, fontWeight: 700, color: 'var(--vibe-brand-text)', letterSpacing: '-0.02em' }}>
              {fmtCount(breakdown.realTotal)}
            </span>
            <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)' }}>
              {t('dashboard.realTotalTokens')}
            </span>
          </div>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: `${SPACING.xl}px`,
              padding: `${SPACING.sm}px ${SPACING.md}px`,
              borderRadius: BORDER_RADIUS.md,
              background: 'var(--vibe-btn-bg)',
              border: '0.0625rem solid var(--vibe-btn-border)',
            }}
          >
            <div style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <span style={{ fontSize: '10px', color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
                {t('dashboard.totalRequests')}
              </span>
              <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)', fontFamily: 'var(--font-mono)' }}>
                {fmtCount(breakdown.requests)}
              </span>
            </div>
            <div style={{ width: '0.0625rem', height: 24, background: 'var(--vibe-content-border)' }} />
            <div style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <span style={{ fontSize: '10px', color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
                {t('dashboard.totalCost')}
              </span>
              <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)', fontFamily: 'var(--font-mono)' }}>
                {breakdown.cost !== null ? `$${breakdown.cost.toFixed(2)}` : '--'}
              </span>
            </div>
          </div>
        </div>

        {/* Bottom row: Breakdown mini stats */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(5, 1fr)', gap: SPACING.sm }}>
          <MiniStat
            icon={<ArrowDownToLine size={14} style={{ color: '#3b82f6' }} />}
            label={t('dashboard.freshInput')}
            value={formatTokenShort(breakdown.input)}
            accentColor="#3b82f6"
          />
          <MiniStat
            icon={<ArrowUpFromLine size={14} style={{ color: '#22c55e' }} />}
            label={t('dashboard.output')}
            value={formatTokenShort(breakdown.output)}
            accentColor="#22c55e"
          />
          <MiniStat
            icon={<Database size={14} style={{ color: '#f97316' }} />}
            label={t('dashboard.cacheWrite')}
            value={formatTokenShort(breakdown.cacheWrite)}
            accentColor="#f97316"
          />
          <MiniStat
            icon={<Sparkles size={14} style={{ color: '#10b981' }} />}
            label={t('dashboard.cacheRead')}
            value={formatTokenShort(breakdown.cacheRead)}
            accentColor="#10b981"
          />
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: '4px',
              padding: `${SPACING.xs}px ${SPACING.sm}px`,
              borderRadius: BORDER_RADIUS.md,
              background: 'var(--vibe-btn-bg)',
              border: '0.0625rem solid var(--vibe-btn-border)',
              justifyContent: 'center',
            }}
          >
            <span style={{ fontSize: '10px', color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
              {t('dashboard.cacheHitRate')}
            </span>
            <div style={{ display: 'flex', alignItems: 'baseline', gap: '4px' }}>
              <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: '#10b981', fontFamily: 'var(--font-mono)' }}>
                {hitPercentLabel}%
              </span>
            </div>
            <div style={{ height: 3, borderRadius: '999px', background: 'var(--vibe-btn-bg)', overflow: 'hidden' }}>
              <div
                style={{
                  height: '100%',
                  borderRadius: '999px',
                  background: 'linear-gradient(to right, #10b981, #3b82f6)',
                  width: `${hitPercent}%`,
                }}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function MiniStat({
  icon,
  label,
  value,
  accentColor,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  accentColor: string;
}) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: '4px',
        padding: `${SPACING.xs}px ${SPACING.sm}px`,
        borderRadius: BORDER_RADIUS.md,
        background: 'var(--vibe-btn-bg)',
        border: '0.0625rem solid var(--vibe-btn-border)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
        {icon}
        <span style={{ fontSize: '10px', color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
          {label}
        </span>
      </div>
      <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)', fontFamily: 'var(--font-mono)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {value}
      </span>
    </div>
  );
}
