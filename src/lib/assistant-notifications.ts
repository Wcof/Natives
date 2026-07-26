import {
  RUN_STATUS_LABELS,
  type RunStatusLabel,
} from '@/lib/assistant-run-status-labels';

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

export function notificationTitle(kind: NotificationKind, zh: boolean): string {
  // waiting_* kinds mirror run statuses; their wording comes from the shared table.
  const map: Record<NotificationKind, RunStatusLabel> = {
    run_completed: { zh: '运行完成', en: 'Run completed' },
    run_failed: { zh: '运行失败', en: 'Run failed' },
    waiting_permission: RUN_STATUS_LABELS.waiting_permission,
    waiting_user: RUN_STATUS_LABELS.waiting_user,
    subagent_input: { zh: '子任务需要输入', en: 'Subagent needs input' },
    scheduler_failed: { zh: '计划任务失败', en: 'Scheduler failed' },
    daemon_fatal: { zh: '引擎无法恢复', en: 'Engine cannot recover' },
  };
  const label = map[kind];
  return zh ? label.zh : label.en;
}
