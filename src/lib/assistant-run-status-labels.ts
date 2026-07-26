// i18n-pending: run 状态标签暂以硬编码双语表维护（原分散在 RunStatusBar /
// GoalStatusBar / assistant-notifications），后续统一收敛到 src/i18n。
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

export interface RunStatusLabel {
  readonly zh: string;
  readonly en: string;
}

/** Single source of truth for run status → bilingual label. */
export const RUN_STATUS_LABELS: Readonly<Record<DisplayRunStatus, RunStatusLabel>> = {
  created: { zh: '已创建', en: 'Created' },
  connecting: { zh: '连接中', en: 'Connecting' },
  queued: { zh: '排队中', en: 'Queued' },
  preparing: { zh: '准备中', en: 'Preparing' },
  reasoning: { zh: '思考中', en: 'Reasoning' },
  generating: { zh: '生成中', en: 'Generating' },
  running: { zh: '运行中', en: 'Running' },
  running_tool: { zh: '执行工具', en: 'Running tool' },
  waiting_permission: { zh: '等待权限', en: 'Permission needed' },
  waiting_user: { zh: '等待你的回答', en: 'Waiting for your answer' },
  waiting_subagent: { zh: '等待子任务', en: 'Waiting subagent' },
  compacting: { zh: '压缩上下文', en: 'Compacting' },
  reconnecting: { zh: '重连中', en: 'Reconnecting' },
  recovering: { zh: '恢复中', en: 'Recovering' },
  cancelling: { zh: '正在停止', en: 'Cancelling' },
  completed: { zh: '已完成', en: 'Completed' },
  failed: { zh: '失败', en: 'Failed' },
  cancelled: { zh: '已取消', en: 'Cancelled' },
  interrupted: { zh: '已中断', en: 'Interrupted' },
  background_watching: { zh: '后台监视', en: 'Background' },
};

/**
 * Resolve a run status to its display label.
 * - `status` null/undefined → 待命 / Idle.
 * - Unknown statuses fall through verbatim rather than crashing.
 * - `overrides` lets a surface keep context-specific wording
 *   (e.g. goal mode presents `interrupted` as 已暂停 / Paused).
 */
export function runStatusLabel(
  status: string | null | undefined,
  zh: boolean,
  overrides?: Partial<Record<DisplayRunStatus, RunStatusLabel>>,
): string {
  if (!status) return zh ? '待命' : 'Idle';
  const label =
    overrides?.[status as DisplayRunStatus] ?? RUN_STATUS_LABELS[status as DisplayRunStatus];
  if (!label) return status;
  return zh ? label.zh : label.en;
}
