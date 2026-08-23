/**
 * Free Canvas frame model (C-033).
 *
 * A frame is a LOGICAL CONTAINER, not a drawing / print frame engine:
 *  - it visually groups top-level nodes on the canvas,
 *  - nodes carry `frameId` (the frame they belong to),
 *  - moving / resizing a frame keeps its members inside it,
 *  - deleting a frame un-frames members (they stay on the canvas).
 *
 * All helpers are pure functions over `CanvasNode[]` (no React, no DOM, no
 * Yjs / CRDT / BlockSuite / AFFiNE / Plane / Twenty source).
 */

import type { CanvasNode, CanvasRect } from './types';
import { clamp, rectContainsPoint } from './geometry';

export const FRAME_HEADER_H = 24;

/** Frame header band (the only interactive part of a frame body). */
export function frameHeaderRect(node: CanvasNode): CanvasRect {
  return { x: node.x, y: node.y, w: node.w, h: FRAME_HEADER_H };
}

/** Is the point over a frame's header band? */
export function frameHeaderContainsPoint(node: CanvasNode, px: number, py: number): boolean {
  return rectContainsPoint(frameHeaderRect(node), px, py);
}

/** Create a frame node (logical container) at the given rect. */
export function createFrame(
  partial: { id: string; x?: number; y?: number; w?: number; h?: number; label?: string; z?: number },
): CanvasNode {
  return {
    kind: 'frame',
    label: partial.label ?? 'Frame',
    x: partial.x ?? 60,
    y: partial.y ?? 60,
    w: partial.w ?? 320,
    h: partial.h ?? 200,
    z: partial.z ?? 0,
    accent: '--surface-hover',
    frameId: undefined,
    ...partial,
  };
}

/** Union bounds of a set of nodes (null when empty). */
export function boundsOfNodes(nodes: CanvasNode[]): CanvasRect | null {
  if (nodes.length === 0) return null;
  const minX = Math.min(...nodes.map((n) => n.x));
  const minY = Math.min(...nodes.map((n) => n.y));
  const maxX = Math.max(...nodes.map((n) => n.x + n.w));
  const maxY = Math.max(...nodes.map((n) => n.y + n.h));
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}

/** Nodes (cards/notes/groups) fully contained inside a frame's bounds. */
export function nodesInFrame(nodes: CanvasNode[], frame: CanvasNode): CanvasNode[] {
  const minX = frame.x;
  const minY = frame.y;
  const maxX = frame.x + frame.w;
  const maxY = frame.y + frame.h;
  return nodes.filter(
    (node) =>
      node.id !== frame.id &&
      node.kind !== 'frame' &&
      node.x >= minX &&
      node.y >= minY &&
      node.x + node.w <= maxX &&
      node.y + node.h <= maxY,
  );
}

/**
 * Re-assign `frameId` for every node based on geometric containment.
 * A node belongs to the top-most frame that fully contains it. Nodes that are
 * frames themselves, and nodes nested inside groups, are left untouched.
 */
export function assignNodeFrames(nodes: CanvasNode[]): CanvasNode[] {
  const frames = nodes
    .filter((node) => node.kind === 'frame')
    .sort((a, b) => a.z - b.z);
  const byId = new Map(nodes.map((node) => [node.id, node]));
  return nodes.map((node) => {
    if (node.kind === 'frame') return node;
    if (node.kind === 'group') {
      // A group inherits the frame of its members when they share one.
      const memberFrames = new Set(
        (node.members ?? [])
          .map((id) => byId.get(id)?.frameId)
          .filter((id): id is string => Boolean(id)),
      );
      return memberFrames.size === 1
        ? { ...node, frameId: memberFrames.values().next().value as string }
        : { ...node, frameId: undefined };
    }
    const containing = frames.filter(
      (frame) =>
        node.x >= frame.x &&
        node.y >= frame.y &&
        node.x + node.w <= frame.x + frame.w &&
        node.y + node.h <= frame.y + frame.h,
    );
    const frame = containing[containing.length - 1];
    return frame ? { ...node, frameId: frame.id } : { ...node, frameId: undefined };
  });
}

/**
 * Translate a frame and every node that belongs to it (by frameId) by the same
 * delta. Members that were explicitly excluded stay put. Never touches nodes
 * that merely overlap the frame (they are not framed).
 */
export function translateFrame(
  nodes: CanvasNode[],
  frameId: string,
  dx: number,
  dy: number,
): CanvasNode[] {
  const frame = nodes.find((node) => node.id === frameId);
  if (!frame) return nodes;
  return nodes.map((node) => {
    if (node.id === frameId) return { ...node, x: node.x + dx, y: node.y + dy };
    if (node.frameId === frameId) return { ...node, x: node.x + dx, y: node.y + dy };
    return node;
  });
}

/**
 * Resize a frame. Nodes fully inside the new bounds stay framed; nodes that
 * fall outside are un-framed (kept on the canvas). This keeps the container
 * semantics honest — a frame never visually clips its members.
 */
export function resizeFrame(
  nodes: CanvasNode[],
  frameId: string,
  next: CanvasRect,
): CanvasNode[] {
  return nodes.map((node) => {
    if (node.id === frameId) {
      return { ...node, x: next.x, y: next.y, w: next.w, h: next.h };
    }
    if (node.frameId !== frameId) return node;
    const inside =
      node.x >= next.x &&
      node.y >= next.y &&
      node.x + node.w <= next.x + next.w &&
      node.y + node.h <= next.y + next.h;
    return inside ? node : { ...node, frameId: undefined };
  });
}

/** Delete a frame; its members are un-framed and stay on the canvas. */
export function deleteFrame(nodes: CanvasNode[], frameId: string): CanvasNode[] {
  return nodes
    .filter((node) => node.id !== frameId)
    .map((node) => (node.frameId === frameId ? { ...node, frameId: undefined } : node));
}

/** Clamp a frame so its header stays on the world and it keeps a min size. */
export function clampFrameRect(rect: CanvasRect): CanvasRect {
  return {
    x: clamp(rect.x, -rect.w + 80, 2400 - 60),
    y: clamp(rect.y, -rect.h + 40, 1600 - 40),
    w: Math.max(160, rect.w),
    h: Math.max(120, rect.h),
  };
}
