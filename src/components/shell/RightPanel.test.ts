import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  RIGHT_PANEL_DEFAULT_WIDTH,
  RIGHT_PANEL_MAX_WIDTH,
  RIGHT_PANEL_MIN_WIDTH,
  clampRightPanelWidth,
} from './RightPanel';

const source = readFileSync(new URL('./RightPanel.tsx', import.meta.url), 'utf8');
const css = readFileSync(new URL('../../app/globals.css', import.meta.url), 'utf8');

test('clampRightPanelWidth enforces min / max / viewport floor', () => {
  assert.equal(clampRightPanelWidth(100, 1600), RIGHT_PANEL_MIN_WIDTH);
  assert.equal(clampRightPanelWidth(900, 1600), RIGHT_PANEL_MAX_WIDTH);
  assert.equal(clampRightPanelWidth(320, 1600), 320);
  // Narrow viewport: max becomes viewport - main floor (420)
  assert.equal(clampRightPanelWidth(500, 700), 280);
  assert.equal(clampRightPanelWidth(200, 700), RIGHT_PANEL_MIN_WIDTH);
});

test('clampRightPanelWidth rounds to integer pixels', () => {
  assert.equal(clampRightPanelWidth(333.7, 1600), 334);
});

test('RightPanel implements left-edge drag resize', () => {
  assert.match(source, /right-panel-drag-handle/);
  assert.match(source, /col-resize/);
  assert.match(source, /startX - ev\.clientX/);
  assert.match(source, /onDoubleClick=\{handleDragDoubleClick\}/);
  assert.match(source, /RIGHT_PANEL_DEFAULT_WIDTH/);
  assert.match(source, /data-density/);
  assert.match(source, /role="separator"/);
});

test('right-panel CSS supports elastic content + drag handle', () => {
  assert.match(css, /\.right-panel-drag-handle/);
  assert.match(css, /\.right-panel\.is-resizing/);
  assert.match(css, /data-density='compact'/);
  assert.match(css, /\.right-panel-content\s*\{[\s\S]*min-width:\s*0/);
});

test('exported width constants stay coherent', () => {
  assert.ok(RIGHT_PANEL_MIN_WIDTH < RIGHT_PANEL_DEFAULT_WIDTH);
  assert.ok(RIGHT_PANEL_DEFAULT_WIDTH < RIGHT_PANEL_MAX_WIDTH);
  assert.equal(RIGHT_PANEL_DEFAULT_WIDTH, 320);
});
