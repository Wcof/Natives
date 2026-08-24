// QA-01 — pure helpers extracted from WorkspaceCompositionPage for unit testing.
// Keep these free of React/DOM and Date.now() in render so they can be tested
// with the node:test runner (renderToStaticMarkup / plain asserts).

/** Locale-aware relative-time label for a millisecond delta (no Date.now()). */
export function relativeTime(locale: string, ms: number): string {
  const sec = Math.max(0, Math.round(ms / 1000));
  if (sec < 60) return locale.startsWith('zh') ? `${sec} 秒` : `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return locale.startsWith('zh') ? `${min} 分钟` : `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return locale.startsWith('zh') ? `${hr} 小时` : `${hr}h`;
  const day = Math.floor(hr / 24);
  return locale.startsWith('zh') ? `${day} 天` : `${day}d`;
}

export type RenameError = 'required' | 'tooLong' | 'conflict' | null;

/**
 * Validate a workspace rename attempt against the Host contract
 * (src-tauri/src/workspace/store.rs MAX_WORKSPACE_NAME_CHARS = 80).
 * Returns a localized-ish error discriminator; the caller maps it to i18n keys.
 */
export function validateRename(next: string, current: string): RenameError {
  const trimmed = next.trim();
  if (trimmed.length === 0) return 'required';
  if (trimmed.length > 80) return 'tooLong';
  if (trimmed === current) return null; // no-op, not an error
  return null;
}
