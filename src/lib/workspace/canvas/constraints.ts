/**
 * Free Canvas resize constraints (WS-03 / WS-04).
 *
 * Central rules for resize bounds and the screen-space resize overlay:
 *  - handles have a fixed SCREEN-pixel hit area (>= 24px) that NEVER shrinks
 *    with world zoom (ADR-0022 §3 / R-U19);
 *  - widget cards clamp to the widget's min size; plain cards / notes / frames
 *    have their own min and max bounds.
 */

import type { CanvasNode } from './types';

/** Screen-space handle hit area (px). Never scaled by world zoom. */
export const HANDLE_SCREEN_SIZE = 24;
/** Visible handle marker size (px). */
export const HANDLE_VISIBLE_SIZE = 10;

/** Max node size (world units) for cards/notes/frames. */
export const NODE_MAX_W = 960;
export const NODE_MAX_H = 720;
/** Default min node size (world units) for cards/notes/frames. */
export const NODE_MIN_W = 120;
export const NODE_MIN_H = 80;

/**
 * World-space min bound for a node's resize. Widgets keep a legible floor;
 * frames/plain cards their own floors.
 */
export function nodeMinSize(node: CanvasNode): { w: number; h: number } {
  if (node.kind === 'widget' && node.widgetType) return widgetMinSize(node.widgetType);
  if (node.kind === 'frame') return { w: 160, h: 120 };
  return { w: NODE_MIN_W, h: NODE_MIN_H };
}

/** World-space max bound for a node's resize. */
export function nodeMaxSize(_node: CanvasNode): { w: number; h: number } {
  return { w: NODE_MAX_W, h: NODE_MAX_H };
}

/** Whether the node may be resized at all (locked nodes and groups cannot). */
export function canResize(node: CanvasNode): boolean {
  if (node.locked) return false;
  return node.kind !== 'group';
}

/**
 * Compute the screen-space size of a handle at a given zoom.
 * The handle hit box and visible marker are both FIXED in screen px and do
 * not shrink when the world is zoomed out — `zoom` is kept only to document
 * the invariant (no scaling).
 */
export function handleScreenMetrics(_zoom: number): { size: number; visible: number } {
  return { size: HANDLE_SCREEN_SIZE, visible: HANDLE_VISIBLE_SIZE };
}

/** Widget min world size by widget type key (from registry WidgetSize). */
function widgetMinSize(widgetType: string): { w: number; h: number } {
  switch (widgetType) {
    case 'large':
      return { w: 360, h: 240 };
    case 'medium':
      return { w: 300, h: 180 };
    default:
      return { w: 220, h: 140 };
  }
}