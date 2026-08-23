/**
 * Free Canvas selection model (C-017).
 * Pure functions for single-select, multi-select toggle, marquee selection, and clear.
 */

import type { CanvasNode, CanvasRect, CanvasSelection } from './types';
import { nodesInMarquee } from './geometry';

export function createSelection(ids: string[] = []): CanvasSelection {
  return {
    ids,
    mode: ids.length === 0 ? 'none' : ids.length === 1 ? 'single' : 'marquee',
  };
}

export function selectNode(current: CanvasSelection, id: string, shift = false): CanvasSelection {
  if (!shift) {
    return { ids: [id], mode: 'single' };
  }
  const exists = current.ids.includes(id);
  const nextIds = exists ? current.ids.filter((i) => i !== id) : [...current.ids, id];
  return createSelection(nextIds);
}

export function selectByMarquee(nodes: CanvasNode[], marquee: CanvasRect): CanvasSelection {
  const inside = nodesInMarquee(nodes, marquee);
  return createSelection(inside.map((n) => n.id));
}

export function clearSelection(): CanvasSelection {
  return { ids: [], mode: 'none' };
}
