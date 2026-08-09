import { RUN_STATUS_KEYS } from '@/lib/assistant-run-status-labels';
import { t } from '@/i18n';

/**
 * Desktop / in-app notification targeting:
 * 项目 → 会话 → Run → Interaction/Block
 */
export interface AssistantLocateTarget {
  projectPath?: string | null;
  conversationId: string;
  runId?: string | null;
  interactionId?: string | null;
  blockId?: string | null;
  messageId?: string | null;
}

export const ASSISTANT_LOCATE_EVENT = 'natives:assistant-locate';

export function locateAssistantTarget(target: AssistantLocateTarget): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent(ASSISTANT_LOCATE_EVENT, { detail: target }));
}

export function shouldSuppressDesktopNotification(
  activeConversationId: string | null,
  targetConversationId: string,
): boolean {
  return Boolean(activeConversationId && activeConversationId === targetConversationId);
}

export type NotificationKind =
  | 'run_completed'
  | 'run_failed'
  | 'waiting_permission'
  | 'waiting_user'
  | 'subagent_input'
  | 'scheduler_failed'
  | 'daemon_fatal';

export function notificationTitle(locale: string, kind: NotificationKind): string {
  // waiting_* kinds mirror run statuses; their wording comes from the shared table.
  const map: Record<NotificationKind, string> = {
    run_completed: 'notificationLabels.runCompleted',
    run_failed: 'notificationLabels.runFailed',
    waiting_permission: RUN_STATUS_KEYS.waiting_permission,
    waiting_user: RUN_STATUS_KEYS.waiting_user,
    subagent_input: 'notificationLabels.subagentInput',
    scheduler_failed: 'notificationLabels.schedulerFailed',
    daemon_fatal: 'notificationLabels.daemonFatal',
  };
  return t(locale, map[kind]);
}
