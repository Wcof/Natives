'use client';

/**
 * GridWorkspaceView — a workspace tab whose content is a CompactGrid of the
 * REAL widget instances owned by the host workspace (Slice 13 data chain).
 *
 *  - The widget list is read through the snapshot-store (populated by
 *    WorkspaceSessionProvider from the host via typed IPC) and stays in sync
 *    via subscribeSnapshot — this component never fabricates widget content.
 *  - Each widget row's `widgetType` is resolved through the widget registry
 *    (getWidget); rendering is delegated to WidgetRenderer (its real
 *    loading/error/ready state machine + WidgetShell chrome).
 *  - Layout (Slice 14 pointer rule): during drag/resize POINTER MOVE react
 *    grid layout keeps the new layout in its own in-memory state — this view
 *    receives NO per-move callback and makes NO network call. Persistence
 *    happens ONLY on the terminal event (RGL onDragStop/onResizeStop), which
 *    CompactGrid funnels into onLayoutChange(layouts, breakpoint) — the only
 *    path into the provider's host saveLayout.
 *  - Add/remove widget (edit mode only): the "Add widget" control is driven
 *    by the registry (AddWidgetMenu); per-card remove is delegated to
 *    WidgetShell's edit chrome via onRemoveWidget (and the grid's keyboard
 *    Delete/Backspace handler, which funnels into the same callback).
 *  - When the stored layout references no real widget id
 *    (e.g. a pre-migration default layout), a default flow layout is derived
 *    from the widget list so nothing renders into dead slots.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { t, useLocale } from '@/i18n';
import { Empty } from '@/components/ui/design-system';
import CompactGrid, { type CompactGridItem } from '../layout/CompactGrid';
import {
  DEFAULT_WIDGET_MIN_SIZE,
  createDefaultConfig,
  getWidget,
  normalizeWidgetConfig,
} from '@/lib/workspace/widgets';
import type { WidgetConfig, WidgetDefinition } from '@/lib/workspace/widgets';
import { WidgetRenderer } from '@/components/workspace/widgets';
import {
  getSnapshot,
  subscribeSnapshot,
} from '@/lib/workspace/snapshot-store';
import type { WorkspaceWidget } from '@/lib/workspace/contracts';
import type { GridLayouts } from '@/lib/workspace/views/types';

export interface GridWorkspaceViewProps {
  /** Host workspace id that owns the widget rows (from the session context). */
  workspaceId: string | null;
  viewId: string;
  layouts: GridLayouts;
  editable: boolean;
  selectedId?: string | null;
  /**
   * Terminal-event callback (RGL drag/resize STOP only — never pointer move).
   * Must perform the host persistence (provider saveLayout); a rejected
   * Promise is surfaced to the host via onWriteError.
   */
  onLayoutChange: (
    layouts: GridLayouts,
    breakpoint: 'lg' | 'md' | 'sm',
  ) => Promise<void>;
  onBreakpointChange?: (breakpoint: 'lg' | 'md' | 'sm') => void;
  /**
   * Host remove callback (client.removeWidget via the provider); wired to the
   * per-card remove chrome in edit mode (keyboard Delete/Backspace too).
   * Rejections surface via onWriteError.
   */
  onRemoveWidget?: (widgetId: string) => Promise<void>;
  onActivateItem?: (id: string | null) => void;
  /** Host add callback (client.upsertWidget via the provider). */
  onAddWidget?: (widgetType: string) => Promise<void>;
  /** Real error text from a failed host write (never silently dropped). */
  onWriteError?: (message: string) => void;
}

/** Subscribe to the snapshot-store so the grid re-renders on host updates. */
function useHostWidgets(workspaceId: string | null): WorkspaceWidget[] {
  const [widgets, setWidgets] = useState<WorkspaceWidget[]>(() =>
    workspaceId ? (getSnapshot(workspaceId)?.widgets ?? []) : [],
  );

  useEffect(() => {
    if (!workspaceId) {
      setWidgets([]);
      return;
    }
    setWidgets(getSnapshot(workspaceId)?.widgets ?? []);
    return subscribeSnapshot(workspaceId, (snapshot) => {
      setWidgets(snapshot?.widgets ?? []);
    });
  }, [workspaceId]);

  return widgets;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

interface ResolvedGridWidget {
  id: string;
  title: string;
  def: WidgetDefinition;
  config: WidgetConfig;
  /** Default grid slot when the view layout has no entry for this id. */
  minSize: { w: number; h: number };
}

/**
 * Resolve host widget rows into registry instances:
 * getWidget(widgetType) + config pipeline (defaults ← row config).
 * Hidden rows and unregistered types are skipped (never rendered as demo).
 */
function resolveGridWidgets(locale: string, widgets: WorkspaceWidget[]): ResolvedGridWidget[] {
  return widgets.flatMap((widget, index) => {
    if (!widget.enabled) return [];
    const def = getWidget(widget.widgetType);
    if (!def) return [];
    const config = normalizeWidgetConfig(def, {
      ...createDefaultConfig(def),
      ...(isRecord(widget.config) ? widget.config : {}),
      order: index,
    });
    return [{
      id: widget.id,
      title: def.titleKey ? t(locale, def.titleKey) : widget.widgetType,
      def,
      config,
      minSize: DEFAULT_WIDGET_MIN_SIZE[def.size],
    }];
  });
}

export default function GridWorkspaceView({
  workspaceId,
  viewId: _viewId,
  layouts,
  editable,
  selectedId,
  onLayoutChange,
  onBreakpointChange,
  onActivateItem,
  onAddWidget: _onAddWidget,
  onRemoveWidget,
  onWriteError,
}: GridWorkspaceViewProps) {
  const locale = useLocale();
  const widgets = useHostWidgets(workspaceId);

  const resolved = useMemo(
    () => resolveGridWidgets(locale, widgets),
    [locale, widgets],
  );

  const items = useMemo<CompactGridItem[]>(
    () =>
      resolved.map((widget) => ({
        id: widget.id,
        title: widget.title,
        render: ({ editing, active: _active }) => (
          <WidgetRenderer
            instance={{ def: widget.def, config: widget.config }}
            editing={editing || editable}
            onRemove={
              editable && onRemoveWidget ? () => onRemoveWidget(widget.id) : undefined
            }
          />
        ),
      })),
    [resolved, editable, onRemoveWidget],
  );

  const effectiveLayouts = useMemo<GridLayouts>(() => {
    if (resolved.length === 0) return layouts;
    const result: GridLayouts = {
      lg: [...(layouts.lg ?? [])],
      md: [...(layouts.md ?? [])],
      sm: [...(layouts.sm ?? [])],
    };
    const breakpoints = ['lg', 'md', 'sm'] as const;
    for (const bp of breakpoints) {
      const existingIds = new Set(result[bp].map((item) => item.i));
      let nextY = result[bp].reduce((maxY, item) => Math.max(maxY, item.y + item.h), 0);
      for (const widget of resolved) {
        if (!existingIds.has(widget.id)) {
          const def = widget.def;
          const maxCols = bp === 'sm' ? 4 : bp === 'md' ? 8 : 12;
          const w = Math.min(def.minSize?.w ?? 4, maxCols);
          const h = def.minSize?.h ?? 4;
          result[bp].push({
            i: widget.id,
            x: 0,
            y: nextY,
            w,
            h,
            minW: def.minSize?.w,
            minH: def.minSize?.h,
            maxW: def.maxSize?.w,
            maxH: def.maxSize?.h,
          });
          nextY += h;
        }
      }
    }
    return result;
  }, [resolved, layouts]);

  /**
   * Terminal-event persistence wiring (Slice 14 pointer rule):
   * CompactGrid invokes this ONLY from RGL onDragStop/onResizeStop — never
   * during pointer move (RGL keeps the in-flight layout in its own
   * in-memory state; no onDrag/onResize callbacks are wired). This is the
   * ONLY call path from the grid into host persistence (provider saveLayout);
   * a failed write (e.g. G-008 Conflict) surfaces here as a real error.
   */
  const handleLayoutChange = useCallback(
    (next: GridLayouts, breakpoint: 'lg' | 'md' | 'sm') => {
      // THE ONLY persistence point: terminal stop event → host saveLayout.
      void onLayoutChange(next, breakpoint).catch((err: unknown) => {
        console.error('[workspace] layout save failed:', err);
        onWriteError?.(err instanceof Error ? err.message : String(err));
      });
    },
    [onLayoutChange, onWriteError],
  );

  const handleRemoveWidget = useCallback(
    (widgetId: string) => {
      void onRemoveWidget?.(widgetId).catch((err: unknown) => {
        console.error('[workspace] remove widget failed:', err);
        onWriteError?.(err instanceof Error ? err.message : String(err));
      });
    },
    [onRemoveWidget, onWriteError],
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-auto">
      <div className="min-h-0 flex-1">
        {items.length === 0 ? (
          <div className="flex h-full items-center justify-center overflow-auto">
            <Empty title={t(locale, 'workspace.gridEmpty')} />
          </div>
        ) : (
          <CompactGrid
            layouts={effectiveLayouts}
            items={items}
            editable={editable}
            selectedId={selectedId}
            onLayoutChange={handleLayoutChange}
            onBreakpointChange={onBreakpointChange}
            onRemoveItem={handleRemoveWidget}
            onActivateItem={onActivateItem}
            dragHandleClass=".ws-shell-header"
            contentCancelClass=".ws-shell-body, .ws-drag-cancel, button, input, textarea, select, a, [role='button']"
            emptyText={t(locale, 'workspace.gridEmpty')}
          />
        )}
      </div>
    </div>
  );
}
