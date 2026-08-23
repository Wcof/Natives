/**
 * Free Canvas z-order manipulation helpers (C-035).
 */

import type { CanvasNode } from './types';
import { bringToFront, sendToBack, nextZ } from './geometry';

export { bringToFront, sendToBack, nextZ };

/** Step selected nodes one layer forward (+1 z). */
export function stepForward(nodes: CanvasNode[], ids: string[]): CanvasNode[] {
  const set = new Set(ids);
  return nodes.map((node) => (set.has(node.id) ? { ...node, z: node.z + 1 } : node));
}

/** Step selected nodes one layer backward (-1 z). */
export function stepBackward(nodes: CanvasNode[], ids: string[]): CanvasNode[] {
  const set = new Set(ids);
  return nodes.map((node) => (set.has(node.id) ? { ...node, z: Math.max(0, node.z - 1) } : node));
}
