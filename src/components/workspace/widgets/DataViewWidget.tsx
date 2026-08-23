'use client';

import { z } from 'zod';
import { CalendarDays, Columns3, List, Table2 } from 'lucide-react';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import { loadRecentFiles, recentFilesAdapterKey, type RecentFilesData } from '@/lib/workspace/widgets/adapters/recent-files';

type DataViewMode = 'list' | 'table' | 'board' | 'calendar';
interface DataViewSettings extends Record<string, unknown> { mode: DataViewMode }

function DataView({ data, config }: WidgetProps<RecentFilesData, DataViewSettings>) {
  const paths = data?.paths ?? [];
  const mode = config.settings?.mode ?? 'list';
  if (!paths.length) return <div className="ws-shell-state text-xs text-[var(--text-tertiary)]">No recent files</div>;
  if (mode === 'board') return <div className="grid h-full grid-cols-2 gap-2 overflow-auto p-2">{paths.map((path) => <div key={path} className="rounded-lg bg-[var(--surface-hover)] p-2 text-xs"><Columns3 size={13} className="mb-2 text-[var(--text-disabled)]" /><span className="line-clamp-2">{path.split('/').pop()}</span></div>)}</div>;
  if (mode === 'calendar') return <div className="grid h-full grid-cols-7 content-start gap-1 overflow-auto p-2">{paths.map((path, index) => <div key={path} className="min-h-16 rounded-md bg-[var(--surface-hover)] p-1 text-[0.625rem]"><CalendarDays size={11} /><span className="mt-1 line-clamp-2">{path.split('/').pop()}</span><span className="block text-[var(--text-disabled)]">{index + 1}</span></div>)}</div>;
  if (mode === 'table') return <div className="h-full overflow-auto p-2"><table className="w-full text-left text-xs"><thead className="text-[var(--text-disabled)]"><tr><th className="pb-2 font-medium">Name</th><th className="pb-2 font-medium">Path</th></tr></thead><tbody>{paths.map((path) => <tr key={path} className="border-t border-[var(--border-subtle)]"><td className="max-w-36 truncate py-2"><Table2 size={12} className="mr-1 inline" />{path.split('/').pop()}</td><td className="max-w-52 truncate py-2 text-[var(--text-secondary)]">{path}</td></tr>)}</tbody></table></div>;
  return <ul className="h-full overflow-auto p-2">{paths.map((path) => <li key={path} className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-[var(--surface-hover)]"><List size={12} /><span className="truncate">{path.split('/').pop()}</span></li>)}</ul>;
}

export const dataViewWidgetDefinition: WidgetDefinition<RecentFilesData, DataViewSettings> = {
  type: 'data_view', titleKey: 'workspace.dataView', descriptionKey: 'workspace.dataView',
  configVersion: 1, defaultConfig: { mode: 'list' },
  configSchema: z.object({ mode: z.enum(['list','table','board','calendar']) }),
  size: 'large', surfacePolicy: { surfaces: ['plain','material'], allowBlur: false, allowGlow: false },
  adapterKeyBuilder: recentFilesAdapterKey, load: loadRecentFiles, Component: DataView,
};

export default dataViewWidgetDefinition;
