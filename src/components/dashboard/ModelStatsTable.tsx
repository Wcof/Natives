'use client';

import { useEffect, useState } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useTranslation } from 'react-i18next';
import { Loader2, TrendingUp } from 'lucide-react';

/**
 * ModelStatsTable — Model-level token statistics table
 *
 * Layout:
 * ┌─────────────────────────────────────────────────────┐
 * │  按模型统计                                          │
 * │  模型名              请求数    Tokens    成本       │
 * │  claude-sonnet-4-6    856    8.2M     $32.45       │
 * │  claude-opus-4-7      234    3.1M     $12.80       │
 * └─────────────────────────────────────────────────────┘
 *
 * Data source: window.nativesAPI.usage (existing API)
 * Design: Follows vibe-* glassmorphic tokens
 */

interface ModelStatsTableProps {
  modelStats?: ModelStat[];
  isLoading?: boolean;
}

export interface ModelStat {
  model: string;
  requestCount: number;
  totalTokens: number;
  totalCost: number;
  avgCostPerRequest: number;
}

function formatTokenShort(value: number): string {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(0)}K`;
  }
  return value.toLocaleString();
}

export function ModelStatsTable({ modelStats, isLoading }: ModelStatsTableProps) {
  const { t } = useTranslation();
  const [stats, setStats] = useState<ModelStat[]>([]);

  useEffect(() => {
    if (modelStats) {
      setStats(modelStats);
    } else {
      setStats([]);
    }
  }, [modelStats]);

  if (isLoading) {
    return (
      <div
        style={{
          borderRadius: BORDER_RADIUS.lg,
          border: '0.0625rem solid var(--vibe-content-border)',
          background: 'var(--vibe-content-bg)',
          backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
          padding: `${SPACING.lg}px`,
          minHeight: 200,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <Loader2 size={24} style={{ color: 'var(--text-faint)', animation: 'spin 1s linear infinite' }} />
      </div>
    );
  }

  if (stats.length === 0) {
    return (
      <div
        style={{
          borderRadius: BORDER_RADIUS.lg,
          border: '0.0625rem solid var(--vibe-content-border)',
          background: 'var(--vibe-content-bg)',
          backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
          padding: `${SPACING.lg}px`,
          minHeight: 200,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: SPACING.sm,
        }}
      >
        <span style={{ fontSize: FONT_SIZE.md, color: 'var(--text-faint)' }}>
          {t('dashboard.noData')}
        </span>
        <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)' }}>
          {t('dashboard.modelStats')}
        </span>
      </div>
    );
  }

  return (
    <div
      style={{
        borderRadius: BORDER_RADIUS.lg,
        border: '0.0625rem solid var(--vibe-content-border)',
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
        padding: `${SPACING.lg}px`,
        overflow: 'hidden',
      }}
    >
      {/* Header */}
      <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
        <TrendingUp size={16} style={{ color: 'var(--vibe-accent-color)' }} />
        <h3 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>
          {t('dashboard.modelStats')}
        </h3>
      </div>

      {/* Table */}
      <div style={{ overflowX: 'auto' }}>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: FONT_SIZE.sm }}>
          <thead>
            <tr style={{ borderBottom: '0.0625rem solid var(--vibe-content-border)' }}>
              <th
                style={{
                  textAlign: 'left',
                  padding: `${SPACING.sm}px ${SPACING.sm}px`,
                  fontSize: '10px',
                  color: 'var(--text-faint)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                  fontWeight: 600,
                }}
              >
                {t('dashboard.model')}
              </th>
              <th
                style={{
                  textAlign: 'right',
                  padding: `${SPACING.sm}px ${SPACING.sm}px`,
                  fontSize: '10px',
                  color: 'var(--text-faint)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                  fontWeight: 600,
                }}
              >
                {t('dashboard.requests')}
              </th>
              <th
                style={{
                  textAlign: 'right',
                  padding: `${SPACING.sm}px ${SPACING.sm}px`,
                  fontSize: '10px',
                  color: 'var(--text-faint)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                  fontWeight: 600,
                }}
              >
                {t('dashboard.tokens')}
              </th>
              <th
                style={{
                  textAlign: 'right',
                  padding: `${SPACING.sm}px ${SPACING.sm}px`,
                  fontSize: '10px',
                  color: 'var(--text-faint)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                  fontWeight: 600,
                }}
              >
                {t('dashboard.cost')}
              </th>
              <th
                style={{
                  textAlign: 'right',
                  padding: `${SPACING.sm}px ${SPACING.sm}px`,
                  fontSize: '10px',
                  color: 'var(--text-faint)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                  fontWeight: 600,
                }}
              >
                {t('dashboard.avgCost')}
              </th>
            </tr>
          </thead>
          <tbody>
            {stats.map((stat) => (
              <tr
                key={stat.model}
                style={{
                  borderBottom: '0.0625rem solid var(--vibe-content-border)',
                }}
              >
                <td style={{ padding: `${SPACING.sm}px ${SPACING.sm}px`, fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.sm, color: 'var(--vibe-brand-text)' }}>
                  {stat.model}
                </td>
                <td style={{ padding: `${SPACING.sm}px ${SPACING.sm}px`, textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.sm, color: 'var(--vibe-brand-text)' }}>
                  {stat.requestCount.toLocaleString()}
                </td>
                <td style={{ padding: `${SPACING.sm}px ${SPACING.sm}px`, textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.sm, color: 'var(--vibe-brand-text)' }}>
                  {formatTokenShort(stat.totalTokens)}
                </td>
                <td style={{ padding: `${SPACING.sm}px ${SPACING.sm}px`, textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.sm, color: 'var(--vibe-brand-text)' }}>
                  ${stat.totalCost.toFixed(2)}
                </td>
                <td style={{ padding: `${SPACING.sm}px ${SPACING.sm}px`, textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: '10px', color: 'var(--text-dim)' }}>
                  ${stat.avgCostPerRequest.toFixed(4)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
