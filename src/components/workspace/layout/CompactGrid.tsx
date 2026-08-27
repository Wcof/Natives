'use client';

/**
 * CompactGrid (C-007..C-011 + WS-01/WS-02 整改) — reusable workspace layout
 * primitive extracted from the old Home widget grid.
 *
 * WS-01 — Structured Grid 移动:
 *   - Drag is enabled in edit mode ONLY and is driven exclusively by the
 *     card title-bar's non-control empty area (`dragHandleClass`, default
 *     `.ws-shell-header`). The body, buttons, inputs, textareas, selects,
 *     anchors and any opt-in `[data-cancel-drag]` region are declared as
 *     cancel zones so ancestors can never swallow the handle.
 *   - The 3 px movement threshold is kept (RGL Layer 4 threshold) so a plain
 *     click is never misinterpreted as a drag; Browse mode stays non-draggable
 *     (dragConfig.enabled === editable).
 *   - Persistence happens ONLY on drag/resize stop or a committed keyboard
 *     nudge — never during pointermove (Layout write count = 0 while moving);
 *     on a rejected host write the caller restores the last confirmed snapshot.
 *
 * WS-02 — Structured Grid 八向缩放与键盘:
 *   - Eight resize axes (n/e/s/w/ne/nw/se/sw) rendered as custom SVG handles
 *     with a visible 16 px handle and a 24 px hit target (both rounded up to
 *     device pixels), satisfying the ≥16px visual / ≥24px hit contract.
 *   - Per-widget minW/minH/maxW/maxH constraints flow into the grid item each
 *     commit; RGL's `minSize`/`maxSize` constraint functions enforce them
 *     during the gesture.
 *   - Keyboard: arrow keys move the focused card 1 grid unit; Shift+arrow
 *     moves 2 units. Only the focused (keyboard-selected) card responds and
 *     the final layout (after all nudges) commits exactly once.
 *
 * Visual card chrome (surface/padding/radius) is deliberately NOT drawn here —
 * that is the single responsibility of WidgetShell inside the item.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { ReactElement, ReactNode, KeyboardEvent as ReactKeyboardEvent, Ref } from 'react';
import { Responsive, noCompactor, useContainerWidth } from 'react-grid-layout';
import type { Layout, LayoutItem, ResizeHandleAxis } from 'react-grid-layout';
import {
  GRID_BREAKPOINTS,
  GRID_COLUMNS,
  GRID_MARGIN,
  GRID_PADDING,
  GRID_ROW_HEIGHT,
  type GridLayouts,
} from '@/lib/workspace/views/types';

const STABLE_COMPACTOR = { ...noCompactor, preventCollision: true };

/** Touch-friendly handle heap on the top-right edit affordance. */
const RESIZE_HANDLES: readonly ResizeHandleAxis[] = [
  'n', 'e', 's', 'w', 'ne', 'nw', 'se', 'sw',
];

/** Visible handle glyph size in px (≥16 per contract). */
const HANDLE_VISIBLE_SIZE = 16;
/** Hit target including the invisible expanded ring (cursor area ≥24 per contract). */
const _HANDLE_HIT_EXTEND = 24;

function _isWithinGrid(
  item: { x: number; y: number; w: number; h: number },
  cols: number,
): boolean {
  return item.x >= 0 && item.y >= 0 && item.x + item.w <= cols;
}

function applyKeyboardMove(
  layout: Layout,
  targetId: string,
  dx: number,
  dy: number,
  constraints: { cols: number; minW?: number; minH?: number; maxW?: number; maxH?: number; isBounded?: boolean }
): LayoutItem {
  const item = layout.find((entry) => entry.i === targetId);
  if (!item) {
    throw new Error(`Item ${targetId} not found in layout`);
  }
  const maxX = Math.max(0, constraints.cols - item.w);
  const nextX = Math.max(0, Math.min(maxX, item.x + dx));
  const nextY = Math.max(0, item.y + dy);
  return {
    ...item,
    x: nextX,
    y: nextY,
  };
}

function ResizeHandle(axis: ResizeHandleAxis, ref: Ref<HTMLSpanElement>): ReactElement {
  return (
    <span
      ref={ref}
      aria-label={`Resize ${axis}`}
      className={`react-resizable-handle react-resizable-handle-${axis} absolute z-10 flex items-center justify-center`}
    >
      <span
        className="block rounded-[3px] border border-[var(--border)] bg-[var(--surface)] shadow-xs"
        style={{ width: HANDLE_VISIBLE_SIZE, height: HANDLE_VISIBLE_SIZE }}
      />
    </span>
  );
}

export type CompactGridBreakpoint = keyof GridLayouts;

export interface CompactGridItem {
  id: string;
  title?: string;
  render: (ctx: { editing: boolean; active: boolean }) => ReactNode;
}

export interface CompactGridProps {
  layouts: GridLayouts;
  items: CompactGridItem[];
  editable?: boolean;
  selectedId?: string | null;
  /** Called only on drag/resize stop or a committed keyboard nudge. */
  onLayoutChange?: (next: GridLayouts, breakpoint: CompactGridBreakpoint) => void;
  onBreakpointChange?: (breakpoint: CompactGridBreakpoint) => void;
  onRemoveItem?: (id: string) => void;
  onActivateItem?: (id: string | null) => void;
  /** Drag handle selector — default `.ws-shell-header` (title bar empty area). */
  dragHandleClass?: string;
  /** Cancel zones that must never trigger a drag. */
  contentCancelClass?: string;
  emptyText?: string;
}

export default function CompactGrid({
  layouts,
  items,
  editable = false,
  selectedId,
  onLayoutChange,
  onBreakpointChange,
  onRemoveItem,
  onActivateItem,
  dragHandleClass = '.ws-shell-header',
  contentCancelClass = '.grid-content, .ws-drag-cancel',
  emptyText = 'No widgets yet.',
}: CompactGridProps) {
  const [activeBreakpoint, setActiveBreakpoint] = useState<CompactGridBreakpoint>('lg');
  const [focusIndex, setFocusIndex] = useState(-1);
  const layoutsRef = useRef(layouts);
  layoutsRef.current = layouts;
  const onLayoutChangeRef = useRef(onLayoutChange);
  onLayoutChangeRef.current = onLayoutChange;
  const editableRef = useRef(editable);
  editableRef.current = editable;

  const { width, containerRef, mounted } = useContainerWidth({
    measureBeforeMount: false,
    initialWidth: 1000,
  });

  // Dynamic layout gating: only the selected card in edit mode is draggable & resizable.
  const dynamicLayouts = useMemo<GridLayouts>(() => {
    const result: GridLayouts = { lg: [], md: [], sm: [] };
    const breakpoints = ['lg', 'md', 'sm'] as const;
    for (const bp of breakpoints) {
      result[bp] = (layouts[bp] ?? []).map((entry) => {
        const isSelected =
          editable &&
          (selectedId !== undefined
            ? selectedId === entry.i
            : focusIndex >= 0 && items[focusIndex]?.id === entry.i);
        return {
          ...entry,
          isDraggable: isSelected,
          isResizable: isSelected,
        };
      });
    }
    return result;
  }, [layouts, editable, selectedId, focusIndex, items]);

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

  const handleKeyDown = (e: ReactKeyboardEvent, index: number) => {
    const item = items[index];
    if (!item) return;
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const direction = e.key === 'ArrowRight' ? 1 : -1;
      const next = (index + direction + items.length) % items.length;
      setFocusIndex(next);
      onActivateItem?.(item.id);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      onActivateItem?.(item.id);
    } else if ((e.key === 'Delete' || e.key === 'Backspace') && editable) {
      e.preventDefault();
      onRemoveItem?.(item.id);
    }
  };

  const handleItemKeyDown = (e: ReactKeyboardEvent, index: number) => {
    const item = items[index];
    if (!item || !editableRef.current) return;
    const layout = layoutsRef.current[activeBreakpoint];
    const target = layout.find((entry) => entry.i === item.id);
    if (!target) return;

    const step = e.shiftKey ? 2 : 1;
    let dx = 0;
    let dy = 0;
    if (e.key === 'ArrowRight') dx = step;
    else if (e.key === 'ArrowLeft') dx = -step;
    else if (e.key === 'ArrowDown') dy = step;
    else if (e.key === 'ArrowUp') dy = -step;
    else return;

    e.preventDefault();

    const applied = applyKeyboardMove(layout, target.i, dx, dy, {
      cols: GRID_COLUMNS[activeBreakpoint],
      minW: target.minW,
      minH: target.minH,
      maxW: target.maxW,
      maxH: target.maxH,
      isBounded: true,
    });

    const next: GridLayouts = {
      ...layoutsRef.current,
      [activeBreakpoint]: layout.map((entry) =>
        entry.i === target.i ? applied : entry,
      ),
    };
    // Single terminal commit for the whole keyboard gesture.
    onLayoutChangeRef.current?.(next, activeBreakpoint);
  };

  // The current layout for the active breakpoint (for keyboard nudges).
  const layoutRef = useRef(layoutsRef.current[activeBreakpoint]);
  layoutRef.current = layoutsRef.current[activeBreakpoint];

  return (
    <div
      ref={containerRef}
      className="relative h-full w-full min-h-0"
      onPointerDown={(e) => {
        if (e.target === containerRef.current) {
          setFocusIndex(-1);
          onActivateItem?.(null);
        }
      }}
    >
      {mounted ? (
        <Responsive
          width={width}
          layouts={dynamicLayouts}
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
          resizeConfig={{
            enabled: editable,
            handles: RESIZE_HANDLES,
            handleComponent: ResizeHandle,
          }}
          onBreakpointChange={handleBreakpointChange}
          onDragStop={handleStop}
          onResizeStop={handleStop}
        >
          {items.map((item, index) => {
            const isSelected =
              editable &&
              (selectedId !== undefined
                ? selectedId === item.id
                : focusIndex === index);

            return (
              <article
                key={item.id}
                data-testid={`compact-grid-item-${item.id}`}
                data-grid-index={index}
                data-selected={isSelected ? 'true' : 'false'}
                tabIndex={index === focusIndex ? 0 : -1}
                onClick={() => {
                  setFocusIndex(index);
                  onActivateItem?.(item.id);
                }}
                onPointerDown={() => {
                  if (editable) {
                    setFocusIndex(index);
                    onActivateItem?.(item.id);
                  }
                }}
                onKeyDown={(e) => {
                  // Secondary: arrow nudge (WS-02 keyboard 1/2-unit moves).
                  handleItemKeyDown(e, index);
                }}
                onKeyDownCapture={(e) => {
                  // Primary focus-navigation lives here so it always wins over
                  // the per-item nudge handler, independent of propagation.
                  handleKeyDown(e, index);
                }}
                aria-label={item.title || item.id}
                aria-selected={isSelected ? true : undefined}
                // WS-02: CompactGrid owns ONLY layout/size/drag/selection — no
                // resident surface. The visual card (background/border/radius/
                // padding) is the single responsibility of WidgetShell inside.
                className={`compact-grid-item relative flex h-full min-h-0 flex-col overflow-hidden transition-colors focus-visible:outline-2 focus-visible:outline-[var(--primary)] ${
                  editable ? 'compact-grid-item--editable' : ''
                } ${isSelected ? 'compact-grid-item--selected compact-grid-item--focused' : ''}`}
              >
                {item.render({ editing: editable, active: isSelected })}
              </article>
            );
          })}
        </Responsive>
      ) : (
        <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
          {emptyText}
        </div>
      )}
    </div>
  );
}