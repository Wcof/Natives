/** Shared file-browser sort helpers (pure, unit-testable). */

export type FileSortBy = 'name' | 'mtime' | 'size';
export type FileSortDir = 'asc' | 'desc';

/**
 * Resolve the next sort state after the user picks a sort field.
 * - Same field → toggle direction (so "按名称排序" is never a no-op).
 * - New field → natural default (name asc; mtime/size desc).
 */
export function nextSortForField(
  currentBy: FileSortBy,
  currentDir: FileSortDir,
  nextBy: FileSortBy,
): { sortBy: FileSortBy; sortDir: FileSortDir } {
  if (nextBy === currentBy) {
    return { sortBy: currentBy, sortDir: currentDir === 'asc' ? 'desc' : 'asc' };
  }
  return { sortBy: nextBy, sortDir: nextBy === 'name' ? 'asc' : 'desc' };
}

/**
 * Resolve an explicit direction action.
 * Accepts 'asc' | 'desc', or toggles when value is omitted/unknown.
 */
export function nextSortDir(
  currentDir: FileSortDir,
  explicit?: unknown,
): FileSortDir {
  if (explicit === 'asc' || explicit === 'desc') return explicit;
  return currentDir === 'asc' ? 'desc' : 'asc';
}
