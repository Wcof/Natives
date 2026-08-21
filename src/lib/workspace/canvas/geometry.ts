/**
 * Free Canvas geometry helpers (pure functions).
 */

import { CANVAS_GRID } from './types';
import type { CanvasNode, CanvasRect } from './types';

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** Snap a coordinate to the canvas grid. */
export function snap(value: number, grid = CANVAS_GRID): number {
  return Math.round(value / grid) * grid;
}

export function normalizeRect(x: number, y: number, w: number, h: number): CanvasRect {
  return { x: w < 0 ? x + w : x, y: h < 0 ? y + h : y, w: Math.abs(w), h: Math.abs(h) };
}

/** Hit test: is point inside rect (already normalized). */
export function rectContainsPoint(rect: CanvasRect, px: number, py: number): boolean {
  return px >= rect.x && px <= rect.x + rect.w && py >= rect.y && py <= rect.y + rect.h;
}

/** Does a rect intersect another rect (with optional padding)? */
export function rectsIntersect(a: CanvasRect, b: CanvasRect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

/** Top-most node containing point (highest z wins; frames tested after cards). */
export function hitTestNodes(nodes: CanvasNode[], px: number, py: number): CanvasNode | null {
  let hit: CanvasNode | null = null;
  for (const node of nodes) {
    if (node.kind === 'group') continue;
    if (rectContainsPoint(node, px, py)) {
      if (!hit || node.z > hit.z) hit = node;
    }
  }
  // Frames are selectable via their header band only, so nodes stay above them.
  for (const node of nodes) {
    if (node.kind !== 'frame') continue;
    const header = { x: node.x, y: node.y, w: node.w, h: 24 };
    if (rectContainsPoint(header, px, py)) {
      if (!hit || node.z >= hit.z) hit = node;
    }
  }
  return hit;
}

/** Nodes intersecting the marquee rect (topmost first). */
export function nodesInMarquee(nodes: CanvasNode[], marquee: CanvasRect): CanvasNode[] {
  return nodes
    .filter((node) => node.kind !== 'group' && rectsIntersect(node, marquee))
    .sort((a, b) => b.z - a.z);
}

/** Clamp a rect so it stays at least partially on the world grid. */
export function clampNodeOnWorld(rect: CanvasRect, worldW: number, worldH: number): CanvasRect {
  const minX = -rect.w + 80;
  const minY = -rect.h + 40;
  return {
    x: clamp(rect.x, minX, worldW - 40),
    y: clamp(rect.y, minY, worldH - 40),
    w: rect.w,
    h: rect.h,
  };
}

/** Next z index (top). */
export function nextZ(nodes: CanvasNode[]): number {
  return nodes.reduce((max, node) => Math.max(max, node.z), 0) + 1;
}

/** Move the given ids above every non-selected node, preserving their order. */
export function bringToFront(nodes: CanvasNode[], ids: string[]): CanvasNode[] {
  const set = new Set(ids);
  const top = nextZ(nodes);
  const selected = nodes
    .filter((node) => set.has(node.id))
    .sort((a, b) => a.z - b.z);
  const zById = new Map<string, number>();
  selected.forEach((node, index) => zById.set(node.id, top + index + 1));
  return nodes.map((node) => (set.has(node.id) ? { ...node, z: zById.get(node.id)! } : node));
}

/** Move the given ids below every non-selected node, preserving their order. */
export function sendToBack(nodes: CanvasNode[], ids: string[]): CanvasNode[] {
  const set = new Set(ids);
  const min = nodes.reduce((lo, node) => Math.min(lo, set.has(node.id) ? Infinity : node.z), Infinity);
  const base = min === Infinity ? 0 : Math.min(min - 1, 0);
  const selected = nodes
    .filter((node) => set.has(node.id))
    .sort((a, b) => a.z - b.z);
  const zById = new Map<string, number>();
  selected.forEach((node, index) => zById.set(node.id, base + index));
  return nodes.map((node) => (set.has(node.id) ? { ...node, z: zById.get(node.id)! } : node));
}

/** Canvas is 2400x1600 world units (framed, larger than viewport). */
export const CANVAS_WORLD = { w: 2400, h: 1600 };
