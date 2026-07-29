/** Async home discovery is only a fallback; an explicit navigation always wins. */
export function shouldApplyHomeFallback(currentPath: string, hasNavigationIntent: boolean): boolean {
  return currentPath === '/' && !hasNavigationIntent;
}
