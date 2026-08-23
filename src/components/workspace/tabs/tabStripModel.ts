/**
 * Tab strip pure model (C-003 close/pin/reorder; Close != Delete).
 */

import { t, type Locale } from '@/i18n';
import type { WorkspaceTab, WorkspaceViewKind } from '@/lib/workspace/views/types';

/** Neighbour index for keyboard focus navigation (wraps around). */
export function nextTabIndex(
  tabs: WorkspaceTab[],
  currentIndex: number,
  direction: 1 | -1,
): number {
  if (tabs.length === 0) return -1;
  const delta = direction === 1 ? 1 : -1;
  return (currentIndex + delta + tabs.length) % tabs.length;
}

/** Move a tab to an absolute index (clamped; used by Ctrl+Arrow reorder). */
export function moveTabTo(tabs: WorkspaceTab[], tabId: string, toIndex: number): WorkspaceTab[] {
  const from = tabs.findIndex((tab) => tab.id === tabId);
  if (from < 0) return tabs;
  const to = Math.max(0, Math.min(tabs.length - 1, toIndex));
  if (from === to) return tabs;
  const next = [...tabs];
  const moved = next.splice(from, 1)[0];
  if (!moved) return tabs;
  next.splice(to, 0, moved);
  return next;
}

/** Human-readable kind label for menus/aria (temp English; i18n keys deferred). */
export function kindLabel(kind: WorkspaceViewKind, locale: Locale = 'zh'): string {
  switch (kind) {
    case 'grid':
      return t(locale, 'workspace.kindGrid');
    case 'canvas':
      return t(locale, 'workspace.kindCanvas');
    case 'data':
      return t(locale, 'workspace.kindData');
  }
}
