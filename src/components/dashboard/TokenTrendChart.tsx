'use client';

import { useState, useMemo } from 'react';
import { AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip, Legend, Dot } from 'recharts';
import { SPACING, FONT_SIZE, BORDER_RADIUS, SHADOW } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import { ChevronDown } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';

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
  minimal?: boolean;
}

export interface TokenHistoryPoint {
  date: string; // ISO date string
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
  skills?: number;
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

export function TokenTrendChart({ usageHistory, isLoading, minimal }: TokenTrendChartProps) {
  const locale = useLocale();
  const [selectedRange, setSelectedRange] = useState<TimeRange>('7d');
  const [isDropdownOpen, setIsDropdownOpen] = useState(false);

  const chartData = useMemo(() => {
    if (usageHistory) return usageHistory;
    return [];
  }, [usageHistory]);

  if (isLoading) {
    return (
      <div
        style={minimal ? {
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          minHeight: 150,
        } : {
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
        <MathCurveLoader size={36} />
      </div>
    );
  }

  if (chartData.length === 0) {
    return (
      <div
        style={minimal ? {
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: SPACING.sm,
          minHeight: 150,
          paddingTop: SPACING.md,
        } : {
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
          {t(locale, 'dashboard.noData')}
        </span>
        <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)' }}>
          {t(locale, 'dashboard.trendTitle')}
        </span>
      </div>
    );
  }

  const rangeLabel = t(locale, TIME_RANGES.find(r => r.value === selectedRange)?.labelKey ?? '');

  return (
    <div
      style={minimal ? {
        overflow: 'visible',
      } : {
        borderRadius: BORDER_RADIUS.lg,
        border: '0.0625rem solid var(--vibe-content-border)',
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
        padding: `${SPACING.lg}px`,
      }}
    >
      {/* Header with time range selector */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: minimal ? 'flex-end' : 'space-between',
        marginBottom: SPACING.md,
        paddingTop: minimal ? SPACING.md : 0,
      }}>
        {!minimal && (
          <h3 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>
            {t(locale, 'dashboard.trendTitle')}
          </h3>
        )}
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
                background: 'var(--bg-3)',
                border: '0.0625rem solid var(--border)',
                boxShadow: SHADOW.elevated,
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
                    color: range.value === selectedRange ? 'var(--vibe-active-color)' : 'var(--text)',
                    background: range.value === selectedRange ? 'var(--vibe-active-bg)' : 'transparent',
                    cursor: 'pointer',
                    border: 'none',
                    borderTop: TIME_RANGES[0] && range.value === TIME_RANGES[0].value ? 'none' : '0.0625rem solid var(--border)',
                  }}
                >
                  {t(locale, range.labelKey)}
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
              <linearGradient id="colorSkills" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#f43f5e" stopOpacity={0.3} />
                <stop offset="95%" stopColor="#f43f5e" stopOpacity={0} />
              </linearGradient>
            </defs>
            <XAxis
              dataKey="date"
              tickFormatter={(v) => formatXAxisLabel(v, selectedRange)}
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}
              dy={10}
            />
            <YAxis
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}
              tickFormatter={(v) => formatTokenShort(v)}
            />
            <YAxis
              yAxisId="skills"
              orientation="right"
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}
              tickFormatter={(v) => String(Math.round(v))}
              domain={[0, 'auto']}
            />
            <Tooltip
              contentStyle={{
                background: 'var(--bg-3)',
                border: '0.0625rem solid var(--border)',
                borderRadius: BORDER_RADIUS.md,
                backdropFilter: 'blur(12px)',
                color: 'var(--text)',
              }}
              labelStyle={{ color: 'var(--text)', fontWeight: 600, marginBottom: 4 }}
              itemStyle={{ fontSize: FONT_SIZE.sm }}
              formatter={(value) => [formatTokenShort(typeof value === 'number' ? value : 0), '']}
            />
            <Legend
              wrapperStyle={{ fontSize: FONT_SIZE.sm }}
            />
            <Area
              type="monotone"
              dataKey="input"
              name={t(locale, 'dashboard.freshInput')}
              stroke="#3b82f6"
              fillOpacity={1}
              fill="url(#colorInput)"
              strokeWidth={2}
            />
            <Area
              type="monotone"
              dataKey="output"
              name={t(locale, 'dashboard.output')}
              stroke="#22c55e"
              fillOpacity={1}
              fill="url(#colorOutput)"
              strokeWidth={2}
            />
            <Area
              type="monotone"
              dataKey="cacheWrite"
              name={t(locale, 'dashboard.cacheWrite')}
              stroke="#f97316"
              fillOpacity={1}
              fill="url(#colorCacheWrite)"
              strokeWidth={2}
            />
            <Area
              type="monotone"
              dataKey="cacheRead"
              name={t(locale, 'dashboard.cacheRead')}
              stroke="#8b5cf6"
              fillOpacity={1}
              fill="url(#colorCacheRead)"
              strokeWidth={2}
            />
            <Area
              type="monotone"
              yAxisId="skills"
              dataKey="skills"
              name={t(locale, 'dashboard.skillsCount')}
              stroke="#f43f5e"
              fillOpacity={0.15}
              fill="url(#colorSkills)"
              strokeWidth={1.5}
              dot={false}
              connectNulls
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
