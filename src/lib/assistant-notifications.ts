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
  const map: Record<NotificationKind, [string, string]> = {
    run_completed: ['运行完成', 'Run completed'],
    run_failed: ['运行失败', 'Run failed'],
    waiting_permission: ['等待权限', 'Permission needed'],
    waiting_user: ['等待你的回答', 'Waiting for your answer'],
    subagent_input: ['子任务需要输入', 'Subagent needs input'],
    scheduler_failed: ['计划任务失败', 'Scheduler failed'],
    daemon_fatal: ['引擎无法恢复', 'Engine cannot recover'],
  };
  const pair = map[kind];
  return zh ? pair[0] : pair[1];
}
