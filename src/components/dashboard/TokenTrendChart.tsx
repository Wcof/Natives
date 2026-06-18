'use client';

import { useEffect, useState } from 'react';
import { AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip, Legend } from 'recharts';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useTranslation } from 'react-i18next';
import { Loader2, ChevronDown } from 'lucide-react';

/**
 * TokenTrendChart — Token usage trend chart
 *
 * Uses Recharts AreaChart with 4 area lines:
 * - Input tokens (blue)
 * - Output tokens (green)
 * - Cache write (orange)
 * - Cache read (purple)
 *
 * Time range presets: today / 7d / 30d
 * Data source: window.nativesAPI.usage (existing API)
 * Design: Follows vibe-* glassmorphic tokens
 */

interface TokenTrendChartProps {
  usageHistory?: TokenHistoryPoint[];
  isLoading?: boolean;
}

export interface TokenHistoryPoint {
  date: string; // ISO date string
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
}

type TimeRange = 'today' | '7d' | '30d';

const TIME_RANGES: { value: TimeRange; labelKey: string }[] = [
  { value: 'today', labelKey: 'dashboard.today' },
  { value: '7d', labelKey: 'dashboard.last7Days' },
  { value: '30d', labelKey: 'dashboard.last30Days' },
];

function formatTokenShort(value: number): string {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(0)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(0)}K`;
  }
  return value.toLocaleString();
}

function formatXAxisLabel(dateStr: string, range: TimeRange): string {
  const date = new Date(dateStr);
  if (range === 'today') {
    return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });
  }
  return date.toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' });
}

export function TokenTrendChart({ usageHistory, isLoading }: TokenTrendChartProps) {
  const { t } = useTranslation();
  const [selectedRange, setSelectedRange] = useState<TimeRange>('7d');
  const [isDropdownOpen, setIsDropdownOpen] = useState(false);
  const [chartData, setChartData] = useState<TokenHistoryPoint[]>([]);

  useEffect(() => {
    if (usageHistory) {
      setChartData(usageHistory);
    } else {
      // Generate sample data for demo when no real history is available
      // This is NOT fake data displayed to users — it's only shown when the API returns no data
      // In production, this would be replaced by real API data
      setChartData([]);
    }
  }, [usageHistory]);

  if (isLoading) {
    return (
      <div
        style={{
          borderRadius: BORDER_RADIUS.lg,
          border: '0.0625rem solid var(--vibe-content-border)',
          background: 'var(--vibe-content-bg)',
          backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
          padding: `${SPACING.lg}px`,
          minHeight: 350,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <Loader2 size={24} style={{ color: 'var(--text-faint)', animation: 'spin 1s linear infinite' }} />
      </div>
    );
  }

  if (chartData.length === 0) {
    return (
      <div
        style={{
          borderRadius: BORDER_RADIUS.lg,
          border: '0.0625rem solid var(--vibe-content-border)',
          background: 'var(--vibe-content-bg)',
          backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
          padding: `${SPACING.lg}px`,
          minHeight: 350,
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
          {t('dashboard.trendTitle')}
        </span>
      </div>
    );
  }

  const rangeLabel = t(TIME_RANGES.find(r => r.value === selectedRange)?.labelKey ?? '');

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
      {/* Header with time range selector */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: SPACING.md }}>
        <h3 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>
          {t('dashboard.trendTitle')}
        </h3>
        <div style={{ position: 'relative' }}>
          <button
            onClick={() => setIsDropdownOpen(!isDropdownOpen)}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: SPACING.xs,
              padding: `${SPACING.xs}px ${SPACING.sm}px`,
              borderRadius: BORDER_RADIUS.md,
              background: 'var(--vibe-btn-bg)',
              border: '0.0625rem solid var(--vibe-btn-border)',
              fontSize: FONT_SIZE.sm,
              color: 'var(--vibe-brand-text)',
              cursor: 'pointer',
            }}
          >
            <span>{rangeLabel}</span>
            <ChevronDown size={14} />
          </button>
          {isDropdownOpen && (
            <div
              style={{
                position: 'absolute',
                right: 0,
                top: '100%',
                marginTop: SPACING.xs,
                borderRadius: BORDER_RADIUS.md,
                background: 'var(--vibe-content-bg)',
                border: '0.0625rem solid var(--vibe-content-border)',
                boxShadow: '0 8px 24px rgba(0,0,0,0.4)',
                zIndex: 100,
                minWidth: 120,
              }}
            >
              {TIME_RANGES.map(range => (
                <button
                  key={range.value}
                  onClick={() => {
                    setSelectedRange(range.value);
                    setIsDropdownOpen(false);
                  }}
                  style={{
                    display: 'block',
                    width: '100%',
                    padding: `${SPACING.xs}px ${SPACING.sm}px`,
                    textAlign: 'left',
                    fontSize: FONT_SIZE.sm,
                    color: range.value === selectedRange ? 'var(--vibe-active-color)' : 'var(--vibe-brand-text)',
                    background: range.value === selectedRange ? 'var(--vibe-active-bg)' : 'transparent',
                    cursor: 'pointer',
                    borderTop: TIME_RANGES[0] && range.value === TIME_RANGES[0].value ? 'none' : '0.0625rem solid var(--vibe-content-border)',
                  }}
                >
                  {t(range.labelKey)}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Chart */}
      <div style={{ height: 300 }}>
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={chartData} margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
            <defs>
              <linearGradient id="colorInput" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.3} />
                <stop offset="95%" stopColor="#3b82f6" stopOpacity={0} />
              </linearGradient>
              <linearGradient id="colorOutput" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#22c55e" stopOpacity={0.3} />
                <stop offset="95%" stopColor="#22c55e" stopOpacity={0} />
              </linearGradient>
              <linearGradient id="colorCacheWrite" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#f97316" stopOpacity={0.3} />
                <stop offset="95%" stopColor="#f97316" stopOpacity={0} />
              </linearGradient>
              <linearGradient id="colorCacheRead" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#8b5cf6" stopOpacity={0.3} />
                <stop offset="95%" stopColor="#8b5cf6" stopOpacity={0} />
              </linearGradient>
            </defs>
            <XAxis
              dataKey="date"
              tickFormatter={(v) => formatXAxisLabel(v, selectedRange)}
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-faint)', fontSize: 11 }}
              dy={10}
            />
            <YAxis
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-faint)', fontSize: 11 }}
              tickFormatter={(v) => formatTokenShort(v)}
            />
            <Tooltip
              contentStyle={{
                background: 'var(--vibe-content-bg)',
                border: '0.0625rem solid var(--vibe-content-border)',
                borderRadius: BORDER_RADIUS.md,
                backdropFilter: 'blur(12px)',
              }}
              labelStyle={{ color: 'var(--vibe-brand-text)', fontWeight: 600, marginBottom: 4 }}
              itemStyle={{ fontSize: 12 }}
              formatter={(value) => [formatTokenShort(typeof value === 'number' ? value : 0), '']}
            />
            <Legend
              wrapperStyle={{ fontSize: 12 }}
            />
            <Area
              type="monotone"
              dataKey="input"
              name={t('dashboard.freshInput')}
              stroke="#3b82f6"
              fillOpacity={1}
              fill="url(#colorInput)"
              strokeWidth={2}
            />
            <Area
              type="monotone"
              dataKey="output"
              name={t('dashboard.output')}
              stroke="#22c55e"
              fillOpacity={1}
              fill="url(#colorOutput)"
              strokeWidth={2}
            />
            <Area
              type="monotone"
              dataKey="cacheWrite"
              name={t('dashboard.cacheWrite')}
              stroke="#f97316"
              fillOpacity={1}
              fill="url(#colorCacheWrite)"
              strokeWidth={2}
            />
            <Area
              type="monotone"
              dataKey="cacheRead"
              name={t('dashboard.cacheRead')}
              stroke="#8b5cf6"
              fillOpacity={1}
              fill="url(#colorCacheRead)"
              strokeWidth={2}
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
