'use client';

/**
 * CompactGrid (C-007..C-011) — extracted from the old Home widget grid into a
 * reusable workspace layout primitive.
 *
 * - Continues react-grid-layout (no rewrite of the drag engine).
 * - Responsive columns: lg/md/sm → 12/8/4.
 * - Layout is persisted ONLY on drag/resize stop (never per-pointer).
 * - Includes a keyboard operation layer (focus + arrows + Enter + Delete).
 *
 * Inspired by the existing HomeWorkspacePage implementation; no third-party
 * source is copied.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { Responsive, noCompactor, useContainerWidth } from 'react-grid-layout';
import type { Layout } from 'react-grid-layout';
import {
  GRID_BREAKPOINTS,
  GRID_COLUMNS,
  GRID_MARGIN,
  GRID_PADDING,
  GRID_ROW_HEIGHT,
  type GridLayouts,
} from '@/lib/workspace/views/types';

const STABLE_COMPACTOR = { ...noCompactor, preventCollision: true };

export type CompactGridBreakpoint = keyof GridLayouts;

export interface CompactGridItem {
  id: string;
  title?: string;
  render: (ctx: { editing: boolean; active: boolean }) => React.ReactNode;
}

export interface CompactGridProps {
  layouts: GridLayouts;
  items: CompactGridItem[];
  editable?: boolean;
  /** Called only on drag/resize stop with the merged responsive layouts. */
  onLayoutChange?: (next: GridLayouts, breakpoint: CompactGridBreakpoint) => void;
  onBreakpointChange?: (breakpoint: CompactGridBreakpoint) => void;
  onRemoveItem?: (id: string) => void;
  onActivateItem?: (id: string) => void;
  dragHandleClass?: string;
  contentCancelClass?: string;
  emptyText?: string;
}

export default function CompactGrid({
  layouts,
  items,
  editable = false,
  onLayoutChange,
  onBreakpointChange,
  onRemoveItem,
  onActivateItem,
  dragHandleClass = '.grid-drag-handle',
  contentCancelClass = '.grid-content',
  emptyText = 'No widgets yet.',
}: CompactGridProps) {
  const [activeBreakpoint, setActiveBreakpoint] = useState<CompactGridBreakpoint>('lg');
  const [focusIndex, setFocusIndex] = useState(-1);
  const layoutsRef = useRef(layouts);
  layoutsRef.current = layouts;
  const onLayoutChangeRef = useRef(onLayoutChange);
  onLayoutChangeRef.current = onLayoutChange;

  const { width, containerRef, mounted } = useContainerWidth({
    measureBeforeMount: true,
    initialWidth: 1000,
  });

  // Keyboard focus follows the focused index (ref callbacks only fire on mount).
  useEffect(() => {
    if (focusIndex < 0) return;
    const el = containerRef.current?.querySelector<HTMLElement>(`[data-grid-index="${focusIndex}"]`);
    el?.focus();
  }, [focusIndex, containerRef]);

  const commitStopped = useCallback((layout: Layout, breakpoint: CompactGridBreakpoint) => {
    const instanceIds = items.map((item) => item.id);
    const next: GridLayouts = {
      ...layoutsRef.current,
      [breakpoint]: (layout ?? [])
        .filter((item) => instanceIds.includes(item.i))
        .map((item) => ({
          i: item.i,
          x: item.x,
          y: item.y,
          w: item.w,
          h: item.h,
          minW: item.minW,
          minH: item.minH,
          maxW: item.maxW,
          maxH: item.maxH,
          isBounded: item.isBounded,
        })),
    };
    onLayoutChangeRef.current?.(next, breakpoint);
  }, [items]);

  const handleStop = useCallback(
    (layout: Layout) => commitStopped(layout, activeBreakpoint),
    [commitStopped, activeBreakpoint],
  );

  const handleBreakpointChange = useCallback(
    (breakpoint: string) => {
      const bp = (breakpoint in GRID_COLUMNS ? breakpoint : 'lg') as CompactGridBreakpoint;
      setActiveBreakpoint(bp);
      onBreakpointChange?.(bp);
    },
    [onBreakpointChange],
  );

  const handleKeyDown = (e: React.KeyboardEvent, index: number) => {
    const item = items[index];
    if (!item) return;
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const direction = e.key === 'ArrowRight' ? 1 : -1;
      const next = (index + direction + items.length) % items.length;
      setFocusIndex(next);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      onActivateItem?.(item.id);
    } else if ((e.key === 'Delete' || e.key === 'Backspace') && editable) {
      e.preventDefault();
      onRemoveItem?.(item.id);
    }
  };

  return (
    <div ref={containerRef} className="relative h-full w-full min-h-0">
      {mounted ? (
        <Responsive
          width={width}
          layouts={layouts}
          breakpoints={GRID_BREAKPOINTS}
          cols={GRID_COLUMNS}
          rowHeight={GRID_ROW_HEIGHT}
          margin={GRID_MARGIN}
          containerPadding={GRID_PADDING}
          compactor={STABLE_COMPACTOR}
          dragConfig={{
            enabled: editable,
            bounded: true,
            handle: dragHandleClass,
            cancel: contentCancelClass,
            threshold: 3,
          }}
          resizeConfig={{ enabled: editable, handles: ['se'] }}
          onBreakpointChange={handleBreakpointChange}
          onDragStop={handleStop}
          onResizeStop={handleStop}
        >
          {items.map((item, index) => (
            <article
              key={item.id}
              data-testid={`compact-grid-item-${item.id}`}
              data-grid-index={index}
              tabIndex={index === focusIndex ? 0 : -1}
              onKeyDown={(e) => handleKeyDown(e, index)}
              aria-label={item.title || item.id}
              // WS-02: CompactGrid owns ONLY layout/size/drag/selection — no
              // resident surface. The visual card (background/border/radius/
              // padding) is the single responsibility of WidgetShell inside.
              className={`compact-grid-item relative flex h-full min-h-0 flex-col overflow-hidden transition-colors focus-visible:outline-2 focus-visible:outline-[var(--primary)] ${
                editable ? 'compact-grid-item--editable' : ''
              } ${index === focusIndex ? 'compact-grid-item--focused' : ''}`}
            >
              {item.render({ editing: editable, active: index === focusIndex })}
            </article>
          ))}
        </Responsive>
      ) : (
        <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
          {emptyText}
        </div>
      )}
    </div>
  );
}
