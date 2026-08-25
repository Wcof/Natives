/**
 * Free Canvas node model (C-012..C-022).
 *
 * Self-built lightweight DOM canvas — no Yjs / CRDT / BlockSuite / AFFiNE /
 * Plane / Twenty source is copied. The interaction model (pan/zoom/select/
 * marquee/snap/group/z-order) is an original minimal implementation that only
 * borrows *concepts* common to every infinite-canvas tool.
 */

export type CanvasNodeKind = 'card' | 'note' | 'frame' | 'group' | 'widget';

export interface CanvasNode {
  id: string;
  kind: CanvasNodeKind;
  label: string;
  x: number;
  y: number;
  w: number;
  h: number;
  /** z-order; higher renders on top. */
  z: number;
  /** Semantic token name (e.g. '--primary-soft'); never a raw hex value. */
  accent?: string;
  locked?: boolean;
  /** Group member ids (only meaningful when kind === 'group'). */
  members?: string[];
  /** Frame membership (top-level frame id) for grouping/z-order scoping. */
  frameId?: string;
  /** Widget type key from registry (e.g. 'notes', 'today_usage', 'token_metrics'). */
  widgetType?: string;
  widgetConfig?: Record<string, unknown>;
}

export interface CanvasCamera {
  x: number;
  y: number;
  zoom: number;
}

export interface CanvasRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type CanvasSelectionMode = 'none' | 'single' | 'marquee';

export interface CanvasSelection {
  ids: string[];
  mode: CanvasSelectionMode;
}

/** Free Canvas grid step (WS-04). ADR-0022 §3: 8px grid snap + arrow steps. */
export const CANVAS_GRID = 8;
export const CANVAS_MIN_ZOOM = 0.25;
export const CANVAS_MAX_ZOOM = 2.5;
export const CANVAS_ZOOM_STEP = 0.15;
export const CANVAS_DEFAULT_SIZE = { w: 240, h: 160 };

export function createCanvasNode(partial: Partial<CanvasNode> & { id: string }): CanvasNode {
  return {
    kind: 'card',
    label: 'New card',
    x: 40,
    y: 40,
    w: CANVAS_DEFAULT_SIZE.w,
    h: CANVAS_DEFAULT_SIZE.h,
    z: 0,
    accent: '--primary-soft',
    ...partial,
  };
}
