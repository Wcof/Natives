import assert from 'node:assert/strict';
import test from 'node:test';
import {
  clampResizableRightPanelWidth,
  rightPanelViewportMax,
  RESIZABLE_RIGHT_PANEL_DEFAULT_WIDTH,
  RESIZABLE_RIGHT_PANEL_MIN_WIDTH,
} from './ResizableRightPanel';

test('clampResizableRightPanelWidth respects min and viewport floor (no 640 cap)', () => {
  assert.equal(clampResizableRightPanelWidth(100, 2000), RESIZABLE_RIGHT_PANEL_MIN_WIDTH);
  // 问题12：不再有 640 产品上限 —— 大视口下宽度可以超过 640。
  assert.equal(clampResizableRightPanelWidth(900, 2000), 900);
  assert.equal(clampResizableRightPanelWidth(320, 2000), RESIZABLE_RIGHT_PANEL_DEFAULT_WIDTH);
  // viewport 700 → max = 700 - 420 = 280, but min is 260 so clamp 500 → 280
  assert.equal(clampResizableRightPanelWidth(500, 700), 280);
});

test('rightPanelViewportMax is the physical viewport boundary', () => {
  assert.equal(rightPanelViewportMax(2000), 2000 - 420);
  assert.equal(rightPanelViewportMax(700), 700 - 420);
});
