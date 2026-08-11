//! find-replace — 只读 Preview 查找的纯逻辑（files Feature，无 UI 依赖）。
//!
//! 问题12：只读 Preview 提供查找、上/下一个、大小写、结果数和有界匹配，
//! 不提供替换。匹配上限防止超大文档一次性高亮 200+ 项（R-P4）。

export interface FindMatch {
  /** 匹配在全文中的起始偏移（UTF-16 code unit，与 DOM text 一致）。 */
  start: number;
  /** 匹配结束偏移（不含）。 */
  end: number;
}

export const FIND_MATCH_LIMIT = 200;

/** 全文中查找所有匹配（大小写可选），超过 FIND_MATCH_LIMIT 截断。 */
export function findMatches(
  text: string,
  query: string,
  options: { caseSensitive?: boolean; limit?: number } = {},
): FindMatch[] {
  const { caseSensitive = false, limit = FIND_MATCH_LIMIT } = options;
  const trimmed = query.trim();
  if (!trimmed || text.length === 0) return [];
  const needle = caseSensitive ? trimmed : trimmed.toLowerCase();
  const haystack = caseSensitive ? text : text.toLowerCase();
  const matches: FindMatch[] = [];
  let from = 0;
  for (;;) {
    const index = haystack.indexOf(needle, from);
    if (index < 0) break;
    matches.push({ start: index, end: index + trimmed.length });
    if (matches.length >= limit) break;
    from = index + trimmed.length;
  }
  return matches;
}

/** 循环导航：上一项/下一项（-1 表示暂无匹配）。 */
export function navigateMatch(
  current: number,
  count: number,
  direction: 'prev' | 'next',
): number {
  if (count <= 0) return -1;
  if (current < 0) return direction === 'next' ? 0 : count - 1;
  if (direction === 'next') return (current + 1) % count;
  return (current - 1 + count) % count;
}
