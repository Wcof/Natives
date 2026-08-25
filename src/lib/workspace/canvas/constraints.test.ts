import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  HANDLE_SCREEN_SIZE,
  HANDLE_VISIBLE_SIZE,
  canResize,
  handleScreenMetrics,
  nodeMaxSize,
  nodeMinSize,
} from './constraints';
import { createCanvasNode } from './types';

describe('Free Canvas constraints (WS-03 / WS-04)', () => {
  it('keeps handle screen size constant regardless of world zoom', () => {
    assert.equal(handleScreenMetrics(1).size, HANDLE_SCREEN_SIZE);
    assert.equal(handleScreenMetrics(0.25).size, HANDLE_SCREEN_SIZE);
    assert.equal(handleScreenMetrics(2.5).size, HANDLE_SCREEN_SIZE);
    // The visible marker is likewise constant in screen px.
    assert.equal(handleScreenMetrics(0.25).visible, HANDLE_VISIBLE_SIZE);
    assert.ok(HANDLE_SCREEN_SIZE >= 24, 'hit area must be >= 24px (R-U19)');
  });

  it('computes widget min size by widget type', () => {
    assert.deepEqual(nodeMinSize(createCanvasNode({ id: 'w', kind: 'widget', widgetType: 'large' })), { w: 360, h: 240 });
    assert.deepEqual(nodeMinSize(createCanvasNode({ id: 'w', kind: 'widget', widgetType: 'small' })), { w: 220, h: 140 });
  });

  it('computes frame and plain-card min sizes', () => {
    assert.deepEqual(nodeMinSize(createCanvasNode({ id: 'f', kind: 'frame' })), { w: 160, h: 120 });
    assert.deepEqual(nodeMinSize(createCanvasNode({ id: 'c', kind: 'card' })), { w: 120, h: 80 });
    assert.deepEqual(nodeMinSize(createCanvasNode({ id: 'n', kind: 'note' })), { w: 120, h: 80 });
  });

  it('exposes max sizes', () => {
    assert.ok(nodeMaxSize(createCanvasNode({ id: 'c', kind: 'card' })).w > 0);
  });

  it('locked nodes and groups cannot be resized', () => {
    assert.equal(canResize(createCanvasNode({ id: 'c', kind: 'card' })), true);
    assert.equal(canResize(createCanvasNode({ id: 'l', kind: 'card', locked: true })), false);
    assert.equal(canResize(createCanvasNode({ id: 'g', kind: 'group' })), false);
  });
});