'use client';

/**
 * Home 布局归一化（复用 HOME-P0-GRID-SPIKE 纯函数逻辑，生产化）。
 *
 * 职责：把持久化布局恢复为合法网格（钳制/去重叠/出屏修复），保证
 * 重启恢复、响应式断点切换与拖动/缩放停止后都满足：
 * - 不重叠（preventCollision + 兜底 firstFreePosition）；
 * - 不出屏（bounded）；
 * - 幂等（相同输入 → 相同输出）。
 */

import type { Layout, LayoutItem, ResponsiveLayouts } from 'react-grid-layout';
import { BREAKPOINTS, COLUMNS, type HomeBreakpoint } from './model';

const MIN_W = 2;
const MIN_H = 2;
const MAX_H = 8;
const DEFAULT_H = 4;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function finiteInteger(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.floor(value)
    : fallback;
}

function defaultWidth(breakpoint: HomeBreakpoint): number {
  if (breakpoint === 'lg') return 4;
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
  const width = clamp(finiteInteger(candidate?.w, defaultWidth(breakpoint)), MIN_W, cols);
  const height = clamp(finiteInteger(candidate?.h, DEFAULT_H), MIN_H, MAX_H);
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
    minW: MIN_W,
    minH: MIN_H,
    maxW: cols,
    maxH: MAX_H,
    isBounded: true,
  };
}

/** 把持久化布局恢复为合法布局（幂等；实例 id 缺失的项被丢弃）。 */
export function normalizeLayout(
  input: Layout | undefined,
  breakpoint: HomeBreakpoint,
  instanceIds: readonly string[],
): Layout {
  const byId = new Map<string, LayoutItem>();
  for (const item of input ?? []) {
    if (!byId.has(item.i)) byId.set(item.i, item);
  }

  const placed: LayoutItem[] = [];
  for (const id of instanceIds) {
    placed.push(normalizeItem(id, byId.get(id), breakpoint, placed));
  }
  return placed;
}

export function normalizeResponsiveLayouts(
  input: ResponsiveLayouts<HomeBreakpoint> | undefined,
  instanceIds: readonly string[],
): ResponsiveLayouts<HomeBreakpoint> {
  return {
    lg: normalizeLayout(input?.lg, 'lg', instanceIds),
    md: normalizeLayout(input?.md, 'md', instanceIds),
    sm: normalizeLayout(input?.sm, 'sm', instanceIds),
  };
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

/** 为新增实例生成一个不与现有布局冲突的默认位置。 */
export function nextFreeSlot(layout: Layout, breakpoint: HomeBreakpoint, w: number, h: number): { x: number; y: number } {
  const cols = COLUMNS[breakpoint];
  const width = clamp(w, MIN_W, cols);
  const height = clamp(h, MIN_H, MAX_H);
  return firstFreePosition(layout, cols, width, height);
}
