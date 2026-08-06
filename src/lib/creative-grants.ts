/**
 * Pure helpers for Creative App grants UI (T08).
 * Kept free of React for node:test coverage.
 */

import type { AppGrant, GrantEvent } from '@/lib/tauri-adapter';

export const GRANT_KINDS = ['upload', 'download', 'clipboard', 'window_open'] as const;
export const GRANT_POLICIES = ['default_deny', 'one_time', 'persistent'] as const;

export type GrantKind = (typeof GRANT_KINDS)[number];
export type GrantPolicy = (typeof GRANT_POLICIES)[number];

export function isGrantKind(kind: string): kind is GrantKind {
  return (GRANT_KINDS as readonly string[]).includes(kind);
}

export function isGrantPolicy(policy: string): policy is GrantPolicy {
  return (GRANT_POLICIES as readonly string[]).includes(policy);
}

/** Whether the app currently holds an effective grant for a capability. */
export function grantHeld(grants: AppGrant[], kind: string): boolean {
  const g = grants.find((x) => x.kind === kind);
  return g?.policy === 'one_time' || g?.policy === 'persistent';
}

/** Effective policy for a capability (defaults to deny). */
export function grantPolicyFor(grants: AppGrant[], kind: string): GrantPolicy {
  const g = grants.find((x) => x.kind === kind);
  return g && isGrantPolicy(g.policy) ? g.policy : 'default_deny';
}

/** Stable order for the permission list. */
export function sortGrants(grants: AppGrant[]): AppGrant[] {
  const rank = (k: string) => {
    const idx = GRANT_KINDS.indexOf(k as GrantKind);
    return idx === -1 ? GRANT_KINDS.length : idx;
  };
  return [...grants].sort((a, b) => rank(a.kind) - rank(b.kind));
}

/** i18n key for a capability label. */
export function kindLabelKey(kind: string): string {
  return `creative.grantKind.${kind}`;
}

/** i18n key for a policy label. */
export function policyLabelKey(policy: string): string {
  return `creative.grantPolicy.${policy}`;
}

/** i18n key for a history event label. */
export function eventLabelKey(event: string): string {
  return `creative.grantEvent.${event}`;
}

/** Newest-first history projection for the panel. */
export function sortGrantEvents(events: GrantEvent[]): GrantEvent[] {
  return [...events].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
}

/**
 * Whether the Host can auto-retry the denied capability after a grant is set.
 * Uploads are OS-dialog driven; clipboard/window.open/downloads are event driven.
 */
export function canRetryAfterGrant(kind: string): boolean {
  return kind === 'download' || kind === 'window_open';
}

/** A sanitized human-readable target for the grant-requested toast (never raw secrets). */
export function grantTargetSummary(target: string, max = 120): string {
  const trimmed = target.trim();
  if (trimmed.length <= max) return trimmed;
  return `${trimmed.slice(0, max)}…`;
}
