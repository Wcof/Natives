// T30 · Virtual file view core (pure helpers)
//
// Bounded virtualization 纯函数层（R-P4）：>200 项必须窗口化，DOM 与 viewport
// 近似 O(viewport)，滚动后不累积。Grid 按容器宽度分组为虚拟行（每行 N 卡），
// List 按固定行高窗口化。T30 实现 VirtualFileViewHandle 接缝（C0 冻结），
// T31 拥有真实 scroll container 并调用 scrollToIndex/getColumnCount。

export interface VirtualRange {
  start: number;
  end: number; // exclusive
}

export const VIRTUAL_ROW_HEIGHT = 48; // FileRow 固定/近固定行高
export const VIRTUAL_OVERSCAN = 4;

/** 按固定行高计算可见窗口（index 区间，含 overscan；滚动超过内容时钳制到空窗口） */
export function computeRowRange(
  scrollTop: number,
  viewportHeight: number,
  totalRows: number,
  rowHeight = VIRTUAL_ROW_HEIGHT,
  overscan = VIRTUAL_OVERSCAN,
): VirtualRange {
  if (totalRows <= 0 || viewportHeight <= 0 || rowHeight <= 0) return { start: 0, end: 0 };
  const total = Math.max(0, totalRows);
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const last = Math.min(total, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan);
  const start = Math.min(first, total);
  const end = Math.max(start, Math.min(last, total));
  return { start, end };
}

/** 按容器宽度与卡片最小宽计算 Grid 列数（getColumnCount 语义） */
export function computeGridColumnCount(
  containerWidth: number,
  minCardWidth: number,
  gap: number,
): number {
  if (containerWidth <= 0 || minCardWidth <= 0) return 1;
  return Math.max(1, Math.floor((containerWidth + gap) / (minCardWidth + gap)));
}

/** 把条目数组按列数分组为虚拟行（grid 行语义：每行 N 卡，最后一行可不满） */
export function groupIntoRows<T>(entries: T[], columnCount: number): T[][] {
  if (columnCount <= 0) return [];
  const rows: T[][] = [];
  for (let i = 0; i < entries.length; i += columnCount) {
    rows.push(entries.slice(i, i + columnCount));
  }
  return rows;
}

/** grid 行窗口：行级别窗口化，再展开为条目索引区间 */
export function gridIndexRange(
  scrollTop: number,
  viewportHeight: number,
  totalEntries: number,
  columnCount: number,
  rowHeight: number,
): VirtualRange {
  const totalRows = Math.ceil(totalEntries / Math.max(1, columnCount));
  const rowRange = computeRowRange(scrollTop, viewportHeight, totalRows, rowHeight);
  return {
    start: rowRange.start * Math.max(1, columnCount),
    end: Math.min(totalEntries, rowRange.end * Math.max(1, columnCount)),
  };
}

/** 条目 index → scrollTop（scrollToIndex 用；index 必须已存在） */
export function rowScrollTop(index: number, columnCount: number, rowHeight: number): number {
  const row = Math.floor(index / Math.max(1, columnCount));
  return Math.max(0, row * rowHeight);
}

export interface VirtualScrollState {
  scrollTop: number;
  viewportHeight: number;
}
