/**
 * Free Canvas snap and alignment engine (C-019).
 * Snaps moving / resizing nodes to the canvas grid and adjacent nodes' edges.
 */

import { CANVAS_GRID, type CanvasNode, type CanvasRect } from './types';
import { snap as snapGrid } from './geometry';

export interface SnapGuide {
  axis: 'x' | 'y';
  position: number;
}

export interface SnapResult {
  x: number;
  y: number;
  guides: SnapGuide[];
}

const EDGE_SNAP_TOLERANCE = 8;

/**
 * Snap a moving candidate rect against other nodes and the grid.
 */
export function calculateSnap(
  candidate: CanvasRect,
  otherNodes: CanvasNode[],
  gridSize = CANVAS_GRID,
): SnapResult {
  let snappedX = snapGrid(candidate.x, gridSize);
  let snappedY = snapGrid(candidate.y, gridSize);
  const guides: SnapGuide[] = [];

  const candLeft = candidate.x;
  const candRight = candidate.x + candidate.w;
  const candTop = candidate.y;
  const candBottom = candidate.y + candidate.h;

  for (const node of otherNodes) {
    const nodeLeft = node.x;
    const nodeRight = node.x + node.w;
    const nodeTop = node.y;
    const nodeBottom = node.y + node.h;

    // X alignment
    if (Math.abs(candLeft - nodeLeft) <= EDGE_SNAP_TOLERANCE) {
      snappedX = nodeLeft;
      guides.push({ axis: 'x', position: nodeLeft });
    } else if (Math.abs(candRight - nodeRight) <= EDGE_SNAP_TOLERANCE) {
      snappedX = nodeRight - candidate.w;
      guides.push({ axis: 'x', position: nodeRight });
    }

    // Y alignment
    if (Math.abs(candTop - nodeTop) <= EDGE_SNAP_TOLERANCE) {
      snappedY = nodeTop;
      guides.push({ axis: 'y', position: nodeTop });
    } else if (Math.abs(candBottom - nodeBottom) <= EDGE_SNAP_TOLERANCE) {
      snappedY = nodeBottom - candidate.h;
      guides.push({ axis: 'y', position: nodeBottom });
    }
  }

  return {
    x: snappedX,
    y: snappedY,
    guides,
  };
}
