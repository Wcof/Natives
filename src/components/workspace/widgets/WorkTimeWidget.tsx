'use client';

/**
 * AI Work Time & Sessions Widget (B-031).
 * Displays session activity, messages exchanged, and active projects count.
 */

import { z } from 'zod';
import { Clock, MessageSquare, FolderGit2 } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { fmtCount } from '@/lib/format';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadUsageSummary,
  usageAdapterKey,
  type UsageSummaryData,
} from '@/lib/workspace/widgets/adapters/usage';

type WorkTimeSettings = Record<string, unknown>;

function WorkTimeView({ data }: WidgetProps<UsageSummaryData, WorkTimeSettings>) {
  const locale = useLocale();
  const sessions = data?.sessions ?? 0;
  const messages = data?.messages ?? 0;
  const activeProjects = data?.activeProjects ?? 0;

  return (
    <div className="flex h-full flex-col justify-between p-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
          <Clock size={15} className="text-[var(--primary)]" />
          <span>{t(locale, 'settings.overviewSessions')}</span>
        </div>
        <span className="text-sm font-semibold tabular-nums text-[var(--text)]">
          {fmtCount(sessions, locale)}
        </span>
      </div>

      <div className="grid grid-cols-2 gap-2 pt-2 border-t border-[var(--border-subtle)]">
        <div>
          <div className="flex items-center gap-1 text-[0.6875rem] text-[var(--text-secondary)]">
            <MessageSquare size={12} />
            <span>{t(locale, 'settings.overviewMessages')}</span>
          </div>
          <div className="text-xs font-medium tabular-nums text-[var(--text)]">
            {fmtCount(messages, locale)}
          </div>
        </div>
        <div className="text-right">
          <div className="flex items-center justify-end gap-1 text-[0.6875rem] text-[var(--text-secondary)]">
            <span>{t(locale, 'settings.overviewActiveProjects')}</span>
            <FolderGit2 size={12} />
          </div>
          <div className="text-xs font-medium tabular-nums text-[var(--text)]">
            {fmtCount(activeProjects, locale)}
          </div>
        </div>
      </div>
    </div>
  );
}

export const workTimeWidgetDefinition: WidgetDefinition<UsageSummaryData, WorkTimeSettings> = {
  type: 'work_time',
  titleKey: 'settings.overviewSessions',
  descriptionKey: 'settings.overviewSessions',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  timeAware: true,
  adapterKeyBuilder: usageAdapterKey,
  load: loadUsageSummary,
  Component: WorkTimeView,
};

export default workTimeWidgetDefinition;
