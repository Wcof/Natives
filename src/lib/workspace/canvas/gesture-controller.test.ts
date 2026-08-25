import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  beginDrag,
  beginMarquee,
  beginResize,
  applyMove,
  applyMarquee,
  commitOverride,
  MIN_NODE_SIZE,
  nudgeDelta,
  nudgeNodes,
  screenDeltaToWorld,
  type GestureDraft,
  type Point,
} from './gesture-controller';
import { createCanvasNode, type CanvasCamera, type CanvasNode } from './types';

const CAM: CanvasCamera = { x: 0, y: 0, zoom: 1 };

function makeNode(id: string, x: number, y: number, z = 1, locked = false): CanvasNode {
  return createCanvasNode({ id, kind: 'card', label: id, x, y, z, w: 100, h: 60, locked });
}

describe('Free Canvas gesture controller (WS-03)', () => {
  it('computes world deltas from screen deltas honoring zoom at start', () => {
    const draft: GestureDraft = {
      ...beginDrag({ x: 0, y: 0 }, CAM, 'a', ['a']),
      startCamera: { x: 0, y: 0, zoom: 2 },
    };
    const delta = screenDeltaToWorld(draft, { x: 40, y: 20 });
    assert.equal(delta.x, 20); // 40 screen px / 2 zoom
    assert.equal(delta.y, 10);
  });

  it('snaps dragged nodes to the 8px grid', () => {
    const nodes = [makeNode('a', 100, 100)];
    const draft = beginDrag({ x: 0, y: 0 }, CAM, 'a', ['a']);
    const moved = applyMove(draft, { x: 7, y: 7 }, nodes);
    // 100 + 7 = 107 → snap to 104 (8px grid: 13 * 8).
    assert.equal(moved['a']!.x, 104);
    assert.equal(moved['a']!.y, 104);
  });

  it('moves group members together with the group (Y uses dy, not dx)', () => {
    const member = makeNode('m1', 100, 200);
    const group = (() => {
      const g = createCanvasNode({ id: 'g1', kind: 'group', label: 'g1', x: 90, y: 180, w: 120, h: 80, z: 1, members: ['m1'] });
      return g;
    })();
    const nodes = [member, group];
    const draft = beginDrag({ x: 0, y: 0 }, CAM, 'g1', ['g1']);
    const moved = applyMove(draft, { x: 32, y: 48 }, nodes);
    // member x: 100 + 32 = 132 → snap 136; y: 200 + 48 = 248 → snap 248.
    assert.equal(moved['m1']!.x, 136);
    assert.equal(moved['m1']!.y, 248);
    // group: x 90 + 32 = 122 → snap 120; y 180 + 48 = 228 → snap 232.
    assert.equal(moved['g1']!.x, 120);
    assert.equal(moved['g1']!.y, 232);
  });

  it('rolls back an empty draft (nothing effectively moved)', () => {
    // beginDrag on a locked node yields an empty draft → commit returns nodes.
    const locked = { ...makeNode('l1', 100, 100), locked: true };
    const draft = beginDrag({ x: 0, y: 0 }, CAM, 'l1', ['l1']);
    const moved = applyMove(draft, { x: 32, y: 0 }, [locked]);
    assert.deepEqual(Object.keys(moved), []);
    assert.deepEqual(commitOverride([locked], moved), [locked]);
  });

  it('resizes from the east handle with grid-snapped width', () => {
    const node = makeNode('n1', 100, 100);
    const draft = beginResize({ x: 0, y: 0 }, CAM, node, 'e');
    const moved = applyMove(draft, { x: 10, y: 0 }, [node]);
    // 100 + 10 = 110 → snap 112.
    assert.equal(moved['n1']!.w, 112);
    assert.equal(moved['n1']!.x, 100); // East handle keeps x fixed.
  });

  it('resizes from the west handle moving x and shrinking width', () => {
    const node = makeNode('a1', 100, 100);
    const draft = beginResize({ x: 0, y: 0 }, CAM, node, 'w');
    const moved = applyMove(draft, { x: 8, y: 0 }, [node]);
    // x = 100 + 8 = 108 → snap 112; w = 100 - 8 = 92 → snap 96.
    assert.equal(moved['a1']!.x, 112);
    assert.equal(moved['a1']!.w, 96);
  });

  it('enforces a minimum node size on resize', () => {
    const node = makeNode('a1', 100, 100);
    const draft = beginResize({ x: 0, y: 0 }, CAM, node, 'nw');
    const moved = applyMove(draft, { x: -200, y: -200 }, [node]);
    assert.ok(moved['a1']!.w >= MIN_NODE_SIZE);
    assert.ok(moved['a1']!.h >= MIN_NODE_SIZE);
  });

  it('commits a draft over the canonical list', () => {
    const nodes = [makeNode('a', 100, 100)];
    assert.deepEqual(commitOverride(nodes, {}), nodes);
    const draft = { a: { ...nodes[0]!, x: 128 } };
    const committed = commitOverride(nodes, draft);
    assert.equal(committed[0]!.x, 128);
  });

  it('nudges with arrow keys on the grid and fine 1px with Shift', () => {
    assert.deepEqual(nudgeDelta('ArrowRight', false), { x: 8, y: 0 } as Point);
    assert.deepEqual(nudgeDelta('ArrowDown', true), { x: 0, y: 1 } as Point);
    assert.equal(nudgeDelta('Home', false), null);
  });

  it('nudges selected nodes only and skips locked', () => {
    const a = makeNode('a', 100, 100);
    const locked = makeNode('b', 200, 200, 1, true);
    const next = nudgeNodes([a, locked], ['a', 'b'], 8, 0);
    assert.equal(next[0]!.x, 112);
    assert.equal(next[1]!.x, 200); // locked stays
  });

  it('marquee selects intersecting nodes topmost first', () => {
    const nodes = [makeNode('n1', 50, 50, 1), makeNode('n2', 500, 500, 2)];
    const draft = beginMarquee({ x: 0, y: 0 }, CAM);
    const result = applyMarquee(draft, { x: 300, y: 300 }, nodes);
    assert.deepEqual(result.ids, ['n1']);
    assert.deepEqual(result.rect, { x: 0, y: 0, w: 300, h: 300 });
  });
});