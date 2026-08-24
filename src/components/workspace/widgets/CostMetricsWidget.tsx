'use client';

/**
 * Cost & IO Breakdown Widget (B-030).
 * Displays input/output token proportions and estimated cost metrics.
 * Reuses loadUsageSummary from the usage adapter.
 */

import { z } from 'zod';
import { DollarSign, ArrowDownLeft, ArrowUpRight } from 'lucide-react';
import { t, useLocale } from '@/i18n';

import { fmtCount } from '@/lib/format';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadUsageSummary,
  usageAdapterKey,
  type UsageSummaryData,
} from '@/lib/workspace/widgets/adapters/usage';

type CostMetricsSettings = Record<string, unknown>;

function CostMetricsView({ data }: WidgetProps<UsageSummaryData, CostMetricsSettings>) {
  const locale = useLocale();
  const inputTokens = data?.inputTokens ?? null;
  const outputTokens = data?.outputTokens ?? null;
  const totalTokens = data?.totalTokens ?? 0;
  // Blended estimate approx $3 / 1M tokens
  const estimatedCost = totalTokens ? (totalTokens / 1_000_000) * 3 : 0;

  return (
    <div className="flex h-full flex-col justify-between">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
          <DollarSign size={15} className="text-[var(--primary)]" />
          <span>{t(locale, 'usage.estimatedCost')}</span>
        </div>
        <span className="text-sm font-semibold tabular-nums text-[var(--text)]">
          ${estimatedCost.toFixed(2)}
        </span>
      </div>

      <div className="grid grid-cols-2 gap-2 pt-2 border-t border-[var(--border-subtle)]">
        <div>
          <div className="flex items-center gap-1 text-xs text-[var(--text-secondary)]">
            <ArrowDownLeft size={12} className="text-[var(--primary)]" />
            <span>{t(locale, 'settings.overviewInput')}</span>
          </div>
          <div className="text-sm font-medium tabular-nums text-[var(--text)]">
            {inputTokens === null ? '—' : fmtCount(inputTokens, locale)}
          </div>
        </div>
        <div className="text-right">
          <div className="flex items-center justify-end gap-1 text-xs text-[var(--text-secondary)]">
            <span>{t(locale, 'settings.overviewOutput')}</span>
            <ArrowUpRight size={12} className="text-[var(--primary)]" />
          </div>
          <div className="text-sm font-medium tabular-nums text-[var(--text)]">
            {outputTokens === null ? '—' : fmtCount(outputTokens, locale)}
          </div>
        </div>
      </div>
    </div>
  );
}

export const costMetricsWidgetDefinition: WidgetDefinition<UsageSummaryData, CostMetricsSettings> = {
  type: 'cost_metrics',
  titleKey: 'usage.estimatedCost',
  descriptionKey: 'usage.estimatedCost',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  timeAware: true,
  adapterKeyBuilder: usageAdapterKey,
  load: loadUsageSummary,
  Component: CostMetricsView,
};

export default costMetricsWidgetDefinition;
