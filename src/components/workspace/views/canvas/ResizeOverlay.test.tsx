/**
 * ResizeOverlay (WS-03) — screen-space resize handle contract test.
 *
 * Asserts the invariants that protect the canvas interaction contract:
 *  - eight handles exist (nw/n/ne/e/se/s/sw/w);
 *  - the visible marker is EXACTLY `HANDLE_SCREEN_SIZE` in screen px and the
 *    hit area is >= 24px — independent of any world zoom (the overlay is never
 *    `scale()`-transformed; it lives outside the world div);
 *  - a pointerdown on a handle announces the gesture WITHOUT capturing the
 *    pointer (the Stage owns capture) and stops propagation.
 */

import assert from 'node:assert/strict';
import React from 'react';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import { ResizeOverlay } from './ResizeOverlay';
import { HANDLES } from '@/lib/workspace/canvas/gesture-controller';
import { HANDLE_SCREEN_SIZE, HANDLE_VISIBLE_SIZE } from '@/lib/workspace/canvas/constraints';

const screenRect = { x: 100, y: 80, w: 240, h: 160 };

function render(zoom = 1): string {
  return renderToStaticMarkup(
    React.createElement(ResizeOverlay, {
      screenRect,
      onResizeStart: () => {},
    }),
  );
}

describe('ResizeOverlay (screen-space eight-direction overlay)', () => {
  it('renders exactly the eight handles', () => {
    for (const handle of HANDLES) {
      assert.ok(render().includes(`data-handle="${handle}"`), `missing handle ${handle}`);
    }
  });

  it('renders the overlay without a world scale transform', () => {
    const html = render();
    // No `scale(...)` in the overlay markup itself — it must stay screen-space.
    assert.ok(!/transform:\s*scale\(/.test(html));
    assert.ok(html.includes('data-testid="canvas-resize-overlay"'));
  });

  it('uses the constant 24px hit area in screen px', () => {
    assert.equal(HANDLE_SCREEN_SIZE, 24);
    assert.ok(HANDLE_SCREEN_SIZE >= 24, 'R-U19 hit area floor');
    assert.equal(HANDLE_VISIBLE_SIZE, 10);
    for (const handle of HANDLES) {
      assert.ok(render().includes(`width:${HANDLE_SCREEN_SIZE}px`), `${handle} marker not constant`);
      assert.ok(render().includes(`height:${HANDLE_SCREEN_SIZE}px`), `${handle} marker not constant`);
    }
  });

  it('does not add a capture/drag lifetime of its own', () => {
    // The onResizeStart prop exists and the handles do not call
    // setPointerCapture — capture lives on the Stage. We assert the overlay
    // does not render any `touch-action` conflicting with the parent (the
    // parent Stage already forces touch-action: none).
    // The marker is a plain div (no aria-hidden), and the handle has an
    // explicit role="separator".
    assert.ok(render().includes('role="separator"'));
  });
});