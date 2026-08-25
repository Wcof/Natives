/**
 * Free Canvas gesture controller (WS-03) — the Stage's single pointer owner.
 *
 * The Stage is the ONLY element that captures the pointer. Child nodes and
 * resize handles never capture; they only record WHICH gesture should start
 * (drag / resize / pan / marquee). The Stage owns every `pointermove`,
 * `pointerup` and `pointercancel`.
 *
 * Lifecycle: `idle → draft(drag|resize|pan|marquee) → commit | rollback`.
 *
 * Performance contract (R-P11 / ADR-0022 §3): during `move` we mutate ONLY an
 * in-memory draft keyed by node id. No IPC, no SQLite write. The caller
 * commits once on a valid `pointerup` (or keyboard stop) and rolls back to the
 * canonical snapshot on `pointercancel` / `Escape`.
 *
 * `applyMove` / `applyMarquee` / `nudgeNodes` are pure so the same geometry is
 * unit-testable without a DOM and reusable by keyboard nudges.
 */

import { normalizeRect, rectsIntersect, snap } from './geometry';
import type { CanvasCamera, CanvasNode, CanvasRect } from './types';
import { CANVAS_GRID } from './types';

export type HandleKey = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

export const HANDLES: readonly HandleKey[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

export const HANDLE_CURSOR: Record<HandleKey, string> = {
  nw: 'nwse-resize',
  se: 'nwse-resize',
  ne: 'nesw-resize',
  sw: 'nesw-resize',
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
};

export interface Point {
  x: number;
  y: number;
}

export type GestureMode = 'pan' | 'drag' | 'resize' | 'marquee';

export interface GestureDraft {
  mode: GestureMode;
  /** Client-space start (for camera-relative deltas). */
  startScreen: Point;
  /** World coordinates at gesture start. */
  startWorld: Point;
  startCamera: CanvasCamera;
  /** drag / resize target. */
  nodeId?: string;
  handle?: HandleKey;
  /** marquee / resize anchor (world / local rect at start). */
  marqueeStart?: Point;
  startRect?: CanvasRect;
  /** Selection ids captured at start. */
  snapshotIds: string[];
}

/** World-space delta for a pointer move, honoring the camera zoom at start. */
export function screenDeltaToWorld(draft: GestureDraft, next: Point): Point {
  return {
    x: (next.x - draft.startScreen.x) / draft.startCamera.zoom,
    y: (next.y - draft.startScreen.y) / draft.startCamera.zoom,
  };
}

/** Begin a gesture from a pointer-down. Pure; returns the draft to store. */
export function beginDrag(at: Point, camera: CanvasCamera, nodeId: string, ids: string[]): GestureDraft {
  return {
    mode: 'drag',
    startScreen: at,
    startWorld: at,
    startCamera: camera,
    snapshotIds: ids,
    nodeId,
  };
}

export function beginPan(at: Point, camera: CanvasCamera): GestureDraft {
  return { mode: 'pan', startScreen: at, startWorld: at, startCamera: camera, snapshotIds: [] };
}

export function beginMarquee(at: Point, camera: CanvasCamera): GestureDraft {
  return {
    mode: 'marquee',
    startScreen: at,
    startWorld: at,
    startCamera: camera,
    snapshotIds: [],
    marqueeStart: at,
  };
}

export function beginResize(at: Point, camera: CanvasCamera, node: CanvasNode, handle: HandleKey): GestureDraft {
  return {
    mode: 'resize',
    startScreen: at,
    startWorld: at,
    startCamera: camera,
    snapshotIds: [node.id],
    nodeId: node.id,
    handle,
    startRect: { x: node.x, y: node.y, w: node.w, h: node.h },
  };
}

/** Resize helper — pure geometry over a start rect and world-space delta. */
export function resizeFromHandle(
  start: CanvasRect,
  handle: HandleKey,
  dx: number,
  dy: number,
  min: number,
): CanvasRect {
  let { x, y, w, h } = start;
  if (handle.includes('e')) w = start.w + dx;
  if (handle.includes('s')) h = start.h + dy;
  if (handle.includes('w')) {
    w = start.w - dx;
    x = start.x + dx;
  }
  if (handle.includes('n')) {
    h = start.h - dy;
    y = start.y + dy;
  }
  if (w < min) {
    if (handle.includes('w')) x = start.x + start.w - min;
    w = min;
  }
  if (h < min) {
    if (handle.includes('n')) y = start.y + start.h - min;
    h = min;
  }
  return { x, y, w, h };
}

/** Handle centre (local rect space). */
export function handlePosition(rect: CanvasRect, handle: HandleKey): Point {
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const x = handle.includes('e') ? rect.x + rect.w : handle.includes('w') ? rect.x : cx;
  const y = handle.includes('s') ? rect.y + rect.h : handle.includes('n') ? rect.y : cy;
  return { x, y };
}

/** Drag a single non-locked node by a world delta, snapping to the grid. */
function offsetDrag(node: CanvasNode, dx: number, dy: number): { x: number; y: number } | null {
  if (node.locked) return null;
  return { x: snap(node.x + dx), y: snap(node.y + dy) };
}

/**
 * Apply a pointer-move to the canonical node list and produce a draft map.
 * PURE — the caller stores the returned map and keeps `nodes` canonical until
 * commit. Group members translate together; locked nodes are skipped.
 * Returns an EMPTY map when nothing is draggable / the gesture is not a
 * world-space move.
 */
export function applyMove(
  draft: GestureDraft,
  current: Point,
  nodes: CanvasNode[],
): Record<string, CanvasNode> {
  if (draft.mode === 'drag') {
    const delta = screenDeltaToWorld(draft, current);
    const out: Record<string, CanvasNode> = {};
    for (const id of draft.snapshotIds) {
      const node = nodes.find((item) => item.id === id);
      if (!node) continue;
      const offset = offsetDrag(node, delta.x, delta.y);
      if (!offset) continue;
      out[id] = { ...node, ...offset };
      if (node.kind === 'group' && node.members) {
        for (const memberId of node.members) {
          const member = nodes.find((item) => item.id === memberId);
          if (member) {
            const memberOffset = offsetDrag(member, delta.x, delta.y);
            if (memberOffset) out[memberId] = { ...member, ...memberOffset };
          }
        }
      }
    }
    return out;
  }

  if (draft.mode === 'resize' && draft.nodeId && draft.handle) {
    const node = nodes.find((item) => item.id === draft.nodeId);
    if (!node || node.locked) return {};
    const delta = screenDeltaToWorld(draft, current);
    const min = MIN_NODE_SIZE;
    const start = draft.startRect ?? { x: node.x, y: node.y, w: node.w, h: node.h };
    const next = resizeFromHandle(start, draft.handle, delta.x, delta.y, min);
    return {
      [node.id]: {
        ...node,
        x: draft.handle.includes('w') ? snap(next.x) : start.x,
        y: draft.handle.includes('n') ? snap(next.y) : start.y,
        w: snap(next.w),
        h: snap(next.h),
      },
    };
  }

  return {};
}

/** Apply a marquee move, returning the rect and the ids it intersects. */
export function applyMarquee(
  draft: GestureDraft,
  current: Point,
  nodes: CanvasNode[],
): { rect: CanvasRect; ids: string[] } {
  const anchor = draft.marqueeStart ?? draft.startWorld;
  const w = current.x - anchor.x;
  const h = current.y - anchor.y;
  const rect = normalizeRect(anchor.x, anchor.y, w, h);
  return {
    rect,
    ids: nodes
      .filter((node) => node.kind !== 'group' && rectsIntersect(node, rect))
      .sort((a, b) => b.z - a.z)
      .map((node) => node.id),
  };
}

/** Keyboard nudge step: plain arrow = grid (8px), Shift+arrow = 1px (WS-04). */
export function nudgeDelta(key: string, shift?: boolean): Point | null {
  const step = shift ? 1 : CANVAS_GRID;
  switch (key) {
    case 'ArrowLeft': return { x: -step, y: 0 };
    case 'ArrowRight': return { x: step, y: 0 };
    case 'ArrowUp': return { x: 0, y: -step };
    case 'ArrowDown': return { x: 0, y: step };
    default: return null;
  }
}

/** Nudge selected non-locked nodes, snapping the result (keyboard commit path). */
export function nudgeNodes(nodes: CanvasNode[], ids: string[], dx: number, dy: number): CanvasNode[] {
  return nodes.map((node) =>
    ids.includes(node.id) && !node.locked
      ? { ...node, x: snap(node.x + dx), y: snap(node.y + dy) }
      : node,
  );
}

/**
 * Merge a draft map over the canonical list. THE single write point for a
 * gesture (pointer-up or keyboard stop). Returns the canonical list unchanged
 * when the draft is empty (nothing was effectively moved).
 */
export function commitOverride(
  nodes: CanvasNode[],
  draft: Record<string, CanvasNode>,
): CanvasNode[] {
  const keys = Object.keys(draft);
  if (keys.length === 0) return nodes;
  return nodes.map((node) => draft[node.id] ?? node);
}

/** Min node size in world px. */
export const MIN_NODE_SIZE = 48;