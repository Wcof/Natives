'use client';

/**
 * ResizeOverlay (WS-03) — Screen-space eight-direction resize overlay.
 *
 * Rendered as the top-most layer over the world (NOT transformed with the
 * world's `scale(zoom)`), so handle hit areas are FIXED in screen pixels
 * (>= 24px) and never shrink when the camera zooms out (ADR-0022 §3 / R-U19).
 *
 * Pointer behaviour: the overlay does NOT capture the pointer. It only
 * announces which handle was grabbed (via onResizeStart); the Stage owns the
 * actual pointer capture and the whole move/up/cancel lifecycle. This keeps a
 * single pointer owner on the canvas.
 */

import { memo } from 'react';
import {
  HANDLES,
  HANDLE_CURSOR,
  type HandleKey,
} from '@/lib/workspace/canvas/gesture-controller';
import { HANDLE_SCREEN_SIZE, HANDLE_VISIBLE_SIZE } from '@/lib/workspace/canvas/constraints';

export interface ResizeOverlayProps {
  /** Screen-space bounds of the selected node (px, already multiplied by zoom). */
  screenRect: { x: number; y: number; w: number; h: number };
  onResizeStart: (handle: HandleKey, event: React.PointerEvent) => void;
}

function ResizeOverlayBase({ screenRect, onResizeStart }: ResizeOverlayProps) {
  // Screen-space overlay: not inside the scaled world div, so px values are
  // already in screen space.
  const { x, y, w, h } = screenRect;
  const positions: Record<HandleKey, { left: number; top: number }> = {
    nw: { left: 0, top: 0 },
    n: { left: w / 2, top: 0 },
    ne: { left: w, top: 0 },
    e: { left: w, top: h / 2 },
    se: { left: w, top: h },
    s: { left: w / 2, top: h },
    sw: { left: 0, top: h },
    w: { left: 0, top: h / 2 },
  };

  return (
    <div
      className="pointer-events-none absolute left-0 top-0"
      data-testid="canvas-resize-overlay"
      style={{ left: x - HANDLE_SCREEN_SIZE / 2, top: y - HANDLE_SCREEN_SIZE / 2, width: w + HANDLE_SCREEN_SIZE, height: h + HANDLE_SCREEN_SIZE }}
    >
      {HANDLES.map((handle) => {
        const pos = positions[handle];
        return (
          <div
            key={handle}
            role="separator"
            aria-label={handle}
            data-handle={handle}
            data-testid={`canvas-resize-handle-${handle}`}
            onPointerDown={(event) => {
              event.stopPropagation();
              onResizeStart(handle, event);
            }}
            className="absolute z-10 flex touch-none items-center justify-center"
            style={{
              left: pos.left,
              top: pos.top,
              width: HANDLE_SCREEN_SIZE,
              height: HANDLE_SCREEN_SIZE,
              transform: 'translate(-50%, -50%)',
              cursor: HANDLE_CURSOR[handle],
              // The hit area is constant (24px). The visible marker is a
              // smaller centered square so cursor/edge alignment stays exact.
            }}
          >
            <span
              className="block rounded-[4px] border border-[var(--border)] bg-[var(--surface)] shadow-sm"
              style={{ width: HANDLE_VISIBLE_SIZE, height: HANDLE_VISIBLE_SIZE }}
            />
          </div>
        );
      })}
    </div>
  );
}

/**
 * ResizeOverlay — eight resize handles in screen space.
 * Memoized: re-renders only when the node's screen rect changes.
 */
export const ResizeOverlay = memo(ResizeOverlayBase);