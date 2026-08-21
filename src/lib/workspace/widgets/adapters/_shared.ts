// ── Adapter 共享工具 ──
// 迁移期：部分 Widget 迁移源只暴露 React hook（useRecentFiles / useUsageData），
// 没有可直接调用的 imperative facade。这里提供一个防御性的「同名动态解析」，
// 优先命中 domain 模块已有的 imperative 函数；找不到时降级为空数据。
// 均记录 Deferred verification —— 由 A/C 确认真实函数名后替换候选表。

/** 从动态 import 的模块对象里挑出第一个可调用函数（按候选名顺序）。 */
export function pickImperative(
  mod: unknown,
  candidates: string[],
): ((...args: unknown[]) => Promise<unknown> | unknown) | null {
  if (!mod || typeof mod !== 'object') return null;
  const record = mod as Record<string, unknown>;
  for (const name of candidates) {
    const fn = record[name];
    if (typeof fn === 'function') return fn as (...args: unknown[]) => Promise<unknown> | unknown;
  }
  return null;
}

/** 解析 recent-files 返回值 → string[]。 */
export function normalizePaths(result: unknown): string[] {
  if (Array.isArray(result)) {
    if (result.every((p) => typeof p === 'string')) return result as string[];
    // 数组元素可能是 { path } 对象
    const mapped = result.map((p) => (p && typeof p === 'object' && 'path' in p ? (p as { path: unknown }).path : null));
    if (mapped.every((p): p is string => typeof p === 'string')) return mapped;
    return [];
  }
  if (result && typeof result === 'object') {
    const record = result as Record<string, unknown>;
    const paths = record.paths;
    if (Array.isArray(paths)) {
      if (paths.every((p) => typeof p === 'string')) return paths as string[];
      return [];
    }
    if (Array.isArray(record.files)) {
      const files = record.files;
      if (files.every((p) => typeof p === 'string')) return files as string[];
    }
  }
  return [];
}
