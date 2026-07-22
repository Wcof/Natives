import assert from 'node:assert/strict';
import test from 'node:test';
import {
  clampResizableRightPanelWidth,
  RESIZABLE_RIGHT_PANEL_DEFAULT_WIDTH,
  RESIZABLE_RIGHT_PANEL_MIN_WIDTH,
  RESIZABLE_RIGHT_PANEL_MAX_WIDTH,
} from './ResizableRightPanel';

test('clampResizableRightPanelWidth respects min/max and viewport floor', () => {
  assert.equal(clampResizableRightPanelWidth(100, 2000), RESIZABLE_RIGHT_PANEL_MIN_WIDTH);
  assert.equal(clampResizableRightPanelWidth(900, 2000), RESIZABLE_RIGHT_PANEL_MAX_WIDTH);
  assert.equal(clampResizableRightPanelWidth(320, 2000), RESIZABLE_RIGHT_PANEL_DEFAULT_WIDTH);
  // viewport 700 → max = 700 - 420 = 280, but min is 260 so clamp 500 → 280
  assert.equal(clampResizableRightPanelWidth(500, 700), 280);
});
