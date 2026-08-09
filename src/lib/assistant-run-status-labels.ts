import { t } from '@/i18n';
import type { RunStatus } from '@/lib/assistant-protocol';

/**
 * Statuses the UI may display: the wire `RunStatus` union plus display-only
 * pseudo statuses emitted by run activity streams (never persisted).
 */
export type DisplayRunStatus =
  | RunStatus
  | 'connecting'
  | 'generating'
  | 'running_tool'
  | 'compacting'
  | 'reconnecting'
  | 'recovering';

/** Single source of truth for run status → i18n key. */
export const RUN_STATUS_KEYS: Readonly<Record<DisplayRunStatus, string>> = {
  created: 'runStatus.created',
  connecting: 'runStatus.connecting',
  queued: 'runStatus.queued',
  preparing: 'runStatus.preparing',
  reasoning: 'runStatus.reasoning',
  generating: 'runStatus.generating',
  running: 'runStatus.running',
  running_tool: 'runStatus.runningTool',
  waiting_permission: 'runStatus.waitingPermission',
  waiting_user: 'runStatus.waitingUser',
  waiting_subagent: 'runStatus.waitingSubagent',
  compacting: 'runStatus.compacting',
  reconnecting: 'runStatus.reconnecting',
  recovering: 'runStatus.recovering',
  cancelling: 'runStatus.cancelling',
  completed: 'runStatus.completed',
  failed: 'runStatus.failed',
  cancelled: 'runStatus.cancelled',
  interrupted: 'runStatus.interrupted',
  background_watching: 'runStatus.backgroundWatching',
};

/**
 * Resolve a run status to its display label.
 * - `status` null/undefined → 待命 / Idle.
 * - Unknown statuses fall through verbatim rather than crashing.
 * - `overrides` lets a surface keep context-specific wording
 *   (e.g. goal mode presents `interrupted` as 已暂停 / Paused).
 */
export function runStatusLabel(
  locale: string,
  status: string | null | undefined,
  overrides?: Partial<Record<DisplayRunStatus, string>>,
): string {
  if (!status) return t(locale, 'runStatus.idle');
  const key = overrides?.[status as DisplayRunStatus] ?? RUN_STATUS_KEYS[status as DisplayRunStatus];
  if (!key) return status;
  return t(locale, key);
}
