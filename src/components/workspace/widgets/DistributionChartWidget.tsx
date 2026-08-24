'use client';

/**
 * Analytics / Distribution Chart Widget (B-032, 真实现).
 *
 * - 数据源：Host usage facade → `loadUsageTrend`（真实日趋势，非 MOCK）。
 * - 7d / 30d / 90d 范围切换（本地切片 + broker key 联动）。
 * - Area / Bar 视图切换。
 * - 自定义 Hover 探针（值 + 日期）。
 * - 双主题自适应：消费 chart 语义 token
 *   （--chart-token-line / --chart-token-fill 等，由 design-tokens 注入）。
 */

import { useMemo, useState } from 'react';
import { z } from 'zod';
import { useLocale, t } from '@/i18n';
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { ChartAreaGradient } from '@/components/ui/design-system/ChartAreaGradient';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadUsageTrend,
  usageTrendAdapterKey,
  type UsageTrendData,
  type UsageTrendRange,
} from '@/lib/workspace/widgets/adapters/usage-trend';

type AnalyticsSettings = {
  range?: UsageTrendRange;
  view?: 'area' | 'bar';
};

const RANGES: UsageTrendRange[] = ['7d', '30d', '90d'];

/** 按范围切片：adapter 取最宽 90d，本地再切 7/30/90。 */
function slicePoints(points: UsageTrendData['points'], range: UsageTrendRange) {
  const days = range === '7d' ? 7 : range === '30d' ? 30 : 90;
  return points.slice(-days);
}

interface TrendTooltipEntry {
  dataKey?: string | number;
  value?: number;
}

function TrendTooltip({
  active,
  payload,
  label,
}: {
  active?: boolean;
  payload?: TrendTooltipEntry[];
  label?: string | number;
}) {
  const locale = useLocale();
  if (!active || !Array.isArray(payload) || payload.length === 0) return null;
  const tokens = payload.find((p) => p.dataKey === 'tokens')?.value;
  return (
    <div className="rounded-lg border border-[var(--border)] bg-[var(--surface)] px-2.5 py-2 text-xs shadow-md">
      <p className="mb-1 font-medium text-[var(--text)]">{String(label ?? '')}</p>
      {typeof tokens === 'number' && (
        <p className="text-[var(--text-secondary)]">
          {t(locale, 'workspace.trendTokens')}:{' '}
          <span className="font-semibold text-[var(--text)]">
            {tokens.toLocaleString()}
          </span>
        </p>
      )}
    </div>
  );
}

function DistributionChartView({ data, config }: WidgetProps<UsageTrendData, AnalyticsSettings>) {
  const locale = useLocale();
  const range = (config.settings?.range ?? '30d') as UsageTrendRange;
  const view = (config.settings?.view ?? 'area') as 'area' | 'bar';

  const [localRange, setLocalRange] = useState<UsageTrendRange>(range);
  const [localView, setLocalView] = useState<'area' | 'bar'>(view);

  const activeRange = localRange;
  const activeView = localView;
  const activeSeries = useMemo(() => {
    const sliced = slicePoints(data?.points ?? [], activeRange);
    return sliced.map((p) => ({ date: p.date, tokens: p.totalTokens ?? 0 }));
  }, [data, activeRange]);

  const unavailable = !data?.available || activeSeries.length === 0;

  return (
    <div className="flex h-full w-full flex-col">
      {/* 范围 + 视图切换 */}
      <div className="mb-1.5 flex items-center justify-between">
        <div className="text-xs font-medium text-[var(--text-secondary)]">
          {t(locale, 'workspace.weeklyTokens')}
        </div>
        <div className="flex items-center gap-1">
          <div className="flex items-center rounded-md bg-[var(--surface-hover)] p-0.5">
            {RANGES.map((r) => (
              <button
                key={r}
                type="button"
                onClick={() => setLocalRange(r)}
                className={`rounded px-1.5 py-0.5 text-xs font-medium transition-colors ${
                  activeRange === r
                    ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
                }`}
              >
                {r}
              </button>
            ))}
          </div>
          <div className="flex items-center rounded-md bg-[var(--surface-hover)] p-0.5">
            <button
              type="button"
              onClick={() => setLocalView('area')}
              className={`rounded px-1.5 py-0.5 text-xs font-medium transition-colors ${
                activeView === 'area'
                  ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                  : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
              }`}
            >
              {t(locale, 'workspace.chartViewArea')}
            </button>
            <button
              type="button"
              onClick={() => setLocalView('bar')}
              className={`rounded px-1.5 py-0.5 text-xs font-medium transition-colors ${
                activeView === 'bar'
                  ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                  : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
              }`}
            >
              {t(locale, 'workspace.chartViewBar')}
            </button>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 w-full">
        {unavailable ? (
          <div className="flex h-full flex-col items-center justify-center gap-1.5 text-center">
            <p className="text-xs text-[var(--text-disabled)]">
              {t(locale, 'workspace.trendUnavailable')}
            </p>
          </div>
        ) : activeView === 'bar' ? (
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={activeSeries} margin={{ top: 4, right: 4, left: -20, bottom: 0 }}>
              <CartesianGrid stroke="var(--chart-grid)" vertical={false} />
              <XAxis dataKey="date" stroke="var(--text-disabled)" fontSize={9} tickLine={false} axisLine={false} />
              <YAxis stroke="var(--text-disabled)" fontSize={9} tickLine={false} axisLine={false} />
              <Tooltip content={<TrendTooltip />} cursor={{ fill: 'var(--surface-hover)' }} />
              <Bar dataKey="tokens" fill="var(--chart-token-line)" radius={[3, 3, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={activeSeries} margin={{ top: 4, right: 4, left: -20, bottom: 0 }}>
              <defs>
                <ChartAreaGradient id="widget-trend-fill" colorVar="var(--chart-token-line)" />
              </defs>
              <CartesianGrid stroke="var(--chart-grid)" vertical={false} />
              <XAxis dataKey="date" stroke="var(--text-disabled)" fontSize={9} tickLine={false} axisLine={false} />
              <YAxis stroke="var(--text-disabled)" fontSize={9} tickLine={false} axisLine={false} />
              <Tooltip content={<TrendTooltip />} />
              <Area
                type="monotone"
                dataKey="tokens"
                stroke="var(--chart-token-line)"
                strokeWidth={1.5}
                fill="url(#widget-trend-fill)"
                activeDot={{ r: 3, fill: 'var(--chart-token-line)', stroke: 'var(--surface)' }}
              />
            </AreaChart>
          </ResponsiveContainer>
        )}
      </div>
    </div>
  );
}

export const distributionChartWidgetDefinition: WidgetDefinition<UsageTrendData, AnalyticsSettings> = {
  type: 'distribution_chart',
  titleKey: 'home.todayTokens',
  descriptionKey: 'home.todayTokens',
  configVersion: 1,
  defaultConfig: { range: '30d', view: 'area' },
  configSchema: z.object({
    range: z.enum(['7d', '30d', '90d']).optional(),
    view: z.enum(['area', 'bar']).optional(),
  }),
  size: 'medium',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: usageTrendAdapterKey,
  load: loadUsageTrend,
  Component: DistributionChartView,
};

export default distributionChartWidgetDefinition;
