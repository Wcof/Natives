import type { Layout, LayoutItem, ResponsiveLayouts } from 'react-grid-layout';

export const BREAKPOINTS = { lg: 1000, md: 720, sm: 0 } as const;
export const COLUMNS = { lg: 12, md: 8, sm: 4 } as const;
export const FIXTURE_COUNTS = [5, 20, 40] as const;

export type HomeBreakpoint = keyof typeof BREAKPOINTS;
export type FixtureCount = (typeof FIXTURE_COUNTS)[number];

const ITEM_HEIGHT = 4;
const MIN_HEIGHT = 2;
const MAX_HEIGHT = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function finiteInteger(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.floor(value)
    : fallback;
}

function itemWidth(breakpoint: HomeBreakpoint): number {
  if (breakpoint === 'lg') return 3;
  if (breakpoint === 'md') return 4;
  return 4;
}

function intersects(left: LayoutItem, right: LayoutItem): boolean {
  return !(
    left.x + left.w <= right.x ||
    right.x + right.w <= left.x ||
    left.y + left.h <= right.y ||
    right.y + right.h <= left.y
  );
}

function firstFreePosition(
  placed: readonly LayoutItem[],
  cols: number,
  width: number,
  height: number,
): { x: number; y: number } {
  for (let y = 0; ; y += 1) {
    for (let x = 0; x <= cols - width; x += 1) {
      const candidate: LayoutItem = { i: '__candidate__', x, y, w: width, h: height };
      if (!placed.some((item) => intersects(item, candidate))) return { x, y };
    }
  }
}

function normalizeItem(
  id: string,
  candidate: Partial<LayoutItem> | undefined,
  breakpoint: HomeBreakpoint,
  placed: readonly LayoutItem[],
): LayoutItem {
  const cols = COLUMNS[breakpoint];
  const defaultWidth = itemWidth(breakpoint);
  const width = clamp(finiteInteger(candidate?.w, defaultWidth), 2, cols);
  const height = clamp(finiteInteger(candidate?.h, ITEM_HEIGHT), MIN_HEIGHT, MAX_HEIGHT);
  const requested = {
    i: id,
    x: clamp(finiteInteger(candidate?.x, 0), 0, cols - width),
    y: Math.max(0, finiteInteger(candidate?.y, 0)),
    w: width,
    h: height,
  } satisfies LayoutItem;
  const position = placed.some((item) => intersects(item, requested))
    ? firstFreePosition(placed, cols, width, height)
    : { x: requested.x, y: requested.y };

  return {
    ...requested,
    ...position,
    minW: 2,
    minH: MIN_HEIGHT,
    maxW: cols,
    maxH: MAX_HEIGHT,
    isBounded: true,
  };
}

export function fixtureWidgetId(index: number): string {
  return `widget-${String(index + 1).padStart(2, '0')}`;
}

export function normalizeLayout(
  input: Layout | undefined,
  breakpoint: HomeBreakpoint,
  count: FixtureCount,
): Layout {
  const byId = new Map<string, LayoutItem>();
  for (const item of input ?? []) {
    if (!byId.has(item.i)) byId.set(item.i, item);
  }

  const placed: LayoutItem[] = [];
  for (let index = 0; index < count; index += 1) {
    const id = fixtureWidgetId(index);
    placed.push(normalizeItem(id, byId.get(id), breakpoint, placed));
  }
  return placed;
}

export function normalizeResponsiveLayouts(
  input: ResponsiveLayouts<HomeBreakpoint> | undefined,
  count: FixtureCount,
): ResponsiveLayouts<HomeBreakpoint> {
  return {
    lg: normalizeLayout(input?.lg, 'lg', count),
    md: normalizeLayout(input?.md, 'md', count),
    sm: normalizeLayout(input?.sm, 'sm', count),
  };
}

export function createFixtureLayouts(count: FixtureCount): ResponsiveLayouts<HomeBreakpoint> {
  return normalizeResponsiveLayouts(undefined, count);
}

export function resolveBreakpoint(containerWidth: number): HomeBreakpoint {
  if (containerWidth >= BREAKPOINTS.lg) return 'lg';
  if (containerWidth >= BREAKPOINTS.md) return 'md';
  return 'sm';
}

export function hasOverlap(layout: Layout): boolean {
  return layout.some((item, index) =>
    layout.slice(index + 1).some((other) => intersects(item, other)),
  );
}

export function isBounded(layout: Layout, breakpoint: HomeBreakpoint): boolean {
  const cols = COLUMNS[breakpoint];
  return layout.every((item) =>
    item.x >= 0 && item.y >= 0 && item.w >= 1 && item.h >= 1 && item.x + item.w <= cols,
  );
}
