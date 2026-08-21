import test from 'node:test';
import assert from 'node:assert/strict';
import {
  normalizeLayout,
  normalizeResponsiveLayouts,
  resolveBreakpoint,
  hasOverlap,
  isBounded,
  nextFreeSlot,
} from './layoutModel';

test('normalizeLayout places items within grid bounds without overlaps', () => {
  const instanceIds = ['w1', 'w2', 'w3'];
  const corruptedInput = [
    { i: 'w1', x: -5, y: -2, w: 99, h: 99 },
    { i: 'w2', x: 0, y: 0, w: 6, h: 4 },
    { i: 'w3', x: 2, y: 1, w: 4, h: 3 },
  ];

  const normalizedLg = normalizeLayout(corruptedInput, 'lg', instanceIds);
  assert.equal(normalizedLg.length, 3);
  assert.equal(hasOverlap(normalizedLg), false);
  assert.equal(isBounded(normalizedLg, 'lg'), true);

  // Idempotency: normalizing twice gives exact same layout
  const secondPass = normalizeLayout(normalizedLg, 'lg', instanceIds);
  assert.deepEqual(secondPass, normalizedLg);
});

test('normalizeResponsiveLayouts repairs all breakpoints', () => {
  const instanceIds = ['a', 'b'];
  const layouts = normalizeResponsiveLayouts(undefined, instanceIds);

  assert.ok(layouts.lg);
  assert.ok(layouts.md);
  assert.ok(layouts.sm);

  const lg = layouts.lg!;
  const md = layouts.md!;
  const sm = layouts.sm!;

  assert.equal(lg.length, 2);
  assert.equal(md.length, 2);
  assert.equal(sm.length, 2);

  assert.equal(hasOverlap(lg), false);
  assert.equal(hasOverlap(md), false);
  assert.equal(hasOverlap(sm), false);

  assert.equal(isBounded(lg, 'lg'), true);
  assert.equal(isBounded(md, 'md'), true);
  assert.equal(isBounded(sm, 'sm'), true);
});

test('resolveBreakpoint maps width thresholds correctly', () => {
  assert.equal(resolveBreakpoint(1440), 'lg');
  assert.equal(resolveBreakpoint(1000), 'lg');
  assert.equal(resolveBreakpoint(999), 'md');
  assert.equal(resolveBreakpoint(720), 'md');
  assert.equal(resolveBreakpoint(719), 'sm');
  assert.equal(resolveBreakpoint(400), 'sm');
});

test('nextFreeSlot finds the first vacant bounding box', () => {
  const existing = [
    { i: 'w1', x: 0, y: 0, w: 6, h: 4 },
    { i: 'w2', x: 6, y: 0, w: 6, h: 4 },
  ];
  const slot = nextFreeSlot(existing, 'lg', 4, 4);
  assert.equal(slot.x, 0);
  assert.equal(slot.y, 4);
});
