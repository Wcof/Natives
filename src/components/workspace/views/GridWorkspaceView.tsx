'use client';

/**
 * GridWorkspaceView — a workspace tab whose content is a CompactGrid of small
 * built-in widgets (C-007..C-011). The layout lives in the snapshot view config
 * and is only persisted on drag/resize stop.
 */

import { useMemo } from 'react';
import { GripVertical, Sparkles, StickyNote, Timer } from 'lucide-react';
import CompactGrid, { type CompactGridItem } from '../layout/CompactGrid';
import type { GridLayouts } from '@/lib/workspace/views/types';

export interface GridWorkspaceViewProps {
  viewId: string;
  layouts: GridLayouts;
  editable: boolean;
  onLayoutChange: (layouts: GridLayouts, breakpoint: 'lg' | 'md' | 'sm') => void;
  onBreakpointChange?: (breakpoint: 'lg' | 'md' | 'sm') => void;
  onRemoveItem?: (id: string) => void;
  onActivateItem?: (id: string) => void;
}

const WIDGET_META: Record<string, { title: string; icon: React.ReactNode }> = {
  'note-welcome': { title: 'Welcome', icon: <Sparkles size={13} /> },
  'todo-focus': { title: 'Today', icon: <Timer size={13} /> },
  'note-snippets': { title: 'Snippets', icon: <StickyNote size={13} /> },
  'stats-quick': { title: 'Activity', icon: <StickyNote size={13} /> },
};

export default function GridWorkspaceView({
  viewId,
  layouts,
  editable,
  onLayoutChange,
  onBreakpointChange,
  onRemoveItem,
  onActivateItem,
}: GridWorkspaceViewProps) {
  const items = useMemo<CompactGridItem[]>(
    () =>
      Object.keys(WIDGET_META).map((id) => ({
        id,
        title: WIDGET_META[id]?.title ?? id,
        render: ({ editing, active }) => (
          <div className="flex h-full min-h-0 flex-col">
            {editing && (
              <div className="grid-drag-handle flex h-7 shrink-0 cursor-move items-center gap-1 border-b border-[var(--border-subtle)] bg-[var(--surface-hover)] px-2 text-[var(--text-disabled)]">
                <GripVertical size={12} />
                <span className="text-[0.625rem]">{WIDGET_META[id]?.title ?? id}</span>
              </div>
            )}
            <div className="grid-content min-h-0 flex-1 overflow-hidden p-3">
              <WidgetBody id={id} active={active} />
            </div>
          </div>
        ),
      })),
    [],
  );

  return (
    <div className="h-full min-h-0 overflow-auto">
      <CompactGrid
        layouts={layouts}
        items={items}
        editable={editable}
        onLayoutChange={onLayoutChange}
        onBreakpointChange={onBreakpointChange}
        onRemoveItem={onRemoveItem}
        onActivateItem={onActivateItem}
        emptyText="This view has no widgets yet."
      />
    </div>
  );
}

function WidgetBody({ id, active }: { id: string; active: boolean }) {
  const meta = WIDGET_META[id];
  const title = meta?.title ?? id;
  return (
    <div
      data-widget-id={id}
      data-active={active ? 'true' : 'false'}
      className="flex h-full min-h-0 flex-col gap-2"
    >
      <div className="flex items-center gap-1.5 text-sm font-medium text-[var(--text)]">
        {meta?.icon}
        <span className="truncate">{title}</span>
      </div>
      {id === 'note-welcome' && (
        <p className="text-xs leading-relaxed text-[var(--text-secondary)]">
          This is a V2 workspace grid view. Switch to edit mode to rearrange
          widgets; layouts persist only when you stop dragging or resizing.
        </p>
      )}
      {id === 'todo-focus' && (
        <ul className="space-y-1 text-xs text-[var(--text-secondary)]">
          <li className="flex items-center gap-2"><span className="h-1.5 w-1.5 rounded-full bg-[var(--primary)]" /> Ship Wave1 workspace core</li>
          <li className="flex items-center gap-2"><span className="h-1.5 w-1.5 rounded-full bg-[var(--border)]" /> Migrate domain pages to V2 tokens</li>
          <li className="flex items-center gap-2"><span className="h-1.5 w-1.5 rounded-full bg-[var(--border)]" /> Wire inspector host</li>
        </ul>
      )}
      {id === 'note-snippets' && (
        <ul className="space-y-1 text-xs text-[var(--text-secondary)]">
          <li className="rounded-md border border-[var(--border-subtle)] px-2 py-1">⌘K → quick actions</li>
          <li className="rounded-md border border-[var(--border-subtle)] px-2 py-1">Space + drag → pan canvas</li>
          <li className="rounded-md border border-[var(--border-subtle)] px-2 py-1">⌘G groups canvas selection</li>
        </ul>
      )}
      {id === 'stats-quick' && (
        <div className="grid grid-cols-2 gap-2 text-xs">
          <Stat label="Views" value="3" />
          <Stat label="Canvas nodes" value="0" />
          <Stat label="Data rows" value="6" />
          <Stat label="Inspector" value="Ready" />
        </div>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)] px-2.5 py-2">
      <div className="text-base font-semibold tabular-nums text-[var(--text)]">{value}</div>
      <div className="text-[0.625rem] text-[var(--text-disabled)]">{label}</div>
    </div>
  );
}
