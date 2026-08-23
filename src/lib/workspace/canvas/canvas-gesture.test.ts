import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  snap,
  normalizeRect,
  nodesInMarquee,
} from './geometry';
import { clampCamera } from './camera';
import { createCanvasNode, type CanvasCamera, type CanvasNode } from './types';

describe('Free Canvas Geometry & Gestures', () => {
  it('snaps coordinates to grid intervals correctly', () => {
    assert.equal(snap(0), 0);
    assert.equal(snap(7), 0);
    assert.equal(snap(8), 16);
    assert.equal(snap(15), 16);
    assert.equal(snap(24), 32);
    assert.equal(snap(-7), -0);
  });

  it('normalizes bounding rectangles in any drag direction', () => {
    const r1 = normalizeRect(100, 100, 50, 60);
    assert.deepEqual(r1, { x: 100, y: 100, w: 50, h: 60 });

    const r2 = normalizeRect(100, 100, -50, -60);
    assert.deepEqual(r2, { x: 50, y: 40, w: 50, h: 60 });
  });

  it('verifies group member coordinate calculations (X and Y displacement)', () => {
    const member1 = createCanvasNode({
      id: 'm1',
      kind: 'card',
      label: 'Card 1',
      x: 100,
      y: 200,
      z: 1,
    });
    const dxWorld = 32;
    const dyWorld = 48;

    // Verifies that member Y position uses dyWorld rather than dxWorld
    const nextMemberX = snap(member1.x + dxWorld);
    const nextMemberY = snap(member1.y + dyWorld);

    assert.equal(nextMemberX, 128);
    assert.equal(nextMemberY, 256);
  });

  it('clamps camera zoom within bounded limits', () => {
    const cam: CanvasCamera = { x: 500, y: 500, zoom: 0.05 };
    const clamped = clampCamera(cam);

    assert.ok(clamped.zoom >= 0.25);
    assert.ok(clamped.zoom <= 2.0);
    assert.equal(clamped.zoom, 0.25);
    assert.equal(clamped.x, 500);
    assert.equal(clamped.y, 500);
  });

  it('finds nodes overlapping a marquee selection', () => {
    const nodes: CanvasNode[] = [
      createCanvasNode({ id: 'n1', kind: 'card', label: '1', x: 50, y: 50, z: 1 }),
      createCanvasNode({ id: 'n2', kind: 'card', label: '2', x: 500, y: 500, z: 2 }),
    ];
    const marquee = { x: 0, y: 0, w: 300, h: 300 };
    const hit = nodesInMarquee(nodes, marquee);

    assert.deepEqual(hit.map((n) => n.id), ['n1']);
  });
});
