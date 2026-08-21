/**
 * Activity Center model（WSP-004）。
 *
 * 确定性后台操作：每个 operation 有稳定的 id、类型、生命周期状态与结果，
 * 无隐式 timer/轮询。UI（NotificationPanel、状态栏）只消费本 model 的
 * 投影；后端（Host 命令 / usage 同步 / Apps 生命周期）产生 operation。
 *
 * 本 model 不实现 Widget Runtime / 任务系统——它只描述"确定性后台操作"的
 * 类型与状态机，供跨模块统一展示。
 */

/** 后台操作的生命周期状态（确定性状态机）。 */
export type OperationStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

/** 一次确定性后台操作。 */
export interface ActivityOperation {
  /** 稳定 id（同一操作重跑共享 id，用于去重/恢复）。 */
  id: string;
  /** 操作类型（如 usage-sync、app-start、proxy-route-apply）。 */
  kind: string;
  status: OperationStatus;
  /** 开始时间（ms epoch；queued 时为入队时间）。 */
  startedAt: number;
  /** 完成时间（succeeded/failed/cancelled 时非空）。 */
  finishedAt: number | null;
  /** 展示标题 i18n key 或纯文本。 */
  title: string;
  /** 可选错误摘要（不携带 Secret）。 */
  error: string | null;
}

/** Operation 集合的状态查询（供面板渲染）。 */
export interface ActivitySnapshot {
  operations: ActivityOperation[];
}

/** 判定状态机是否允许转移（确定性；非法转移静默拒绝）。 */
export function canTransition(
  from: OperationStatus,
  to: OperationStatus,
): boolean {
  const allowed: Record<OperationStatus, OperationStatus[]> = {
    queued: ['running', 'cancelled'],
    running: ['succeeded', 'failed', 'cancelled'],
    succeeded: [],
    failed: [],
    cancelled: [],
  };
  return allowed[from].includes(to);
}

/** 过滤出"活动中的操作"（queued/running）。 */
export function activeOperations(snapshot: ActivitySnapshot): ActivityOperation[] {
  return snapshot.operations.filter(
    (op) => op.status === 'queued' || op.status === 'running',
  );
}

/** 按 kind 统计（确定性聚合；供状态栏/概览）。 */
export function countByKind(
  snapshot: ActivitySnapshot,
  kinds: string[],
): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const kind of kinds) counts[kind] = 0;
  for (const op of snapshot.operations) {
    if (op.status === 'succeeded' || op.status === 'failed') continue;
    if (Object.prototype.hasOwnProperty.call(counts, op.kind)) {
      counts[op.kind] = (counts[op.kind] ?? 0) + 1;
    }
  }
  return counts;
}

export function createOperation(
  id: string,
  kind: string,
  title: string,
): ActivityOperation {
  return {
    id,
    kind,
    status: 'queued',
    startedAt: Date.now(),
    finishedAt: null,
    title,
    error: null,
  };
}

/** 生成规范化状态转移（非法转移返回原对象，确定性）。 */
export function withStatus(
  op: ActivityOperation,
  status: OperationStatus,
  error: string | null = null,
): ActivityOperation {
  if (!canTransition(op.status, status)) return op;
  return {
    ...op,
    status,
    finishedAt: status === 'succeeded' || status === 'failed' || status === 'cancelled'
      ? Date.now()
      : null,
    error: status === 'failed' ? error : null,
  };
}
