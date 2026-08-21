import assert from 'node:assert/strict';
import test from 'node:test';
import type { ResponsiveLayouts } from 'react-grid-layout';
import {
  COLUMNS,
  FIXTURE_COUNTS,
  createFixtureLayouts,
  fixtureWidgetId,
  hasOverlap,
  isBounded,
  normalizeResponsiveLayouts,
  resolveBreakpoint,
  type HomeBreakpoint,
} from './layoutModel';

const BREAKPOINT_NAMES = Object.keys(COLUMNS) as HomeBreakpoint[];

test('5, 20, and 40 widget fixtures are complete, bounded, and non-overlapping', () => {
  for (const count of FIXTURE_COUNTS) {
    const layouts = createFixtureLayouts(count);
    for (const breakpoint of BREAKPOINT_NAMES) {
      const layout = layouts[breakpoint];
      assert.ok(layout);
      assert.equal(layout.length, count);
      assert.equal(hasOverlap(layout), false);
      assert.equal(isBounded(layout, breakpoint), true);
    }
  }
});

test('layout restore is deterministic and does not mutate persisted input', () => {
  const persisted: ResponsiveLayouts<HomeBreakpoint> = {
    lg: [
      { i: fixtureWidgetId(0), x: 99, y: -4, w: 99, h: 99 },
      { i: fixtureWidgetId(1), x: 0, y: 0, w: 3, h: 4 },
      { i: fixtureWidgetId(1), x: 6, y: 6, w: 3, h: 4 },
    ],
  };
  const snapshot = JSON.stringify(persisted);
  const first = normalizeResponsiveLayouts(persisted, 5);
  const second = normalizeResponsiveLayouts(persisted, 5);

  assert.deepEqual(first, second);
  assert.equal(JSON.stringify(persisted), snapshot);
  for (const breakpoint of BREAKPOINT_NAMES) {
    const layout = first[breakpoint];
    assert.ok(layout);
    assert.equal(hasOverlap(layout), false);
    assert.equal(isBounded(layout, breakpoint), true);
  }
});

test('breakpoints are selected from container width at exact boundaries', () => {
  assert.equal(resolveBreakpoint(1000), 'lg');
  assert.equal(resolveBreakpoint(999), 'md');
  assert.equal(resolveBreakpoint(720), 'md');
  assert.equal(resolveBreakpoint(719), 'sm');
});

test('sidebar widths preserve the planned 1440/1280/1024 breakpoint behavior', () => {
  assert.equal(resolveBreakpoint(1440 - 248), 'lg');
  assert.equal(resolveBreakpoint(1440 - 64), 'lg');
  assert.equal(resolveBreakpoint(1280 - 248), 'lg');
  assert.equal(resolveBreakpoint(1280 - 64), 'lg');
  assert.equal(resolveBreakpoint(1024 - 248), 'md');
  assert.equal(resolveBreakpoint(1024 - 64), 'md');
});
