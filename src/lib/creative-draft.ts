/**
 * Pure helpers for the Creative Draft state machine / action availability.
 * Kept free of React for node:test coverage.
 *
 * Authority: docs/architecture/creative-app-creator-workbench.md section 3.3 / section 4
 */

/** Draft lifecycle, mirrors the SQLite CHECK constraint on `creative_drafts.state`. */
export type CreativeDraftState =
  | 'drafting'
  | 'generating'
  | 'ready'
  | 'publishing'
  | 'published'
  | 'archived';

/** Action availability projected onto the draft toolbar. */
export interface CreativeDraftActions {
  canGenerate: boolean;
  canUndo: boolean;
  canPublish: boolean;
  canDelete: boolean;
}

/** 单草稿修订上限；超出丢弃最旧修订，但始终保留 rev-1。 */
export const MAX_DRAFT_REVISIONS = 50;

/**
 * Legal transitions of section 3.3
 * `generating → ready` covers both lint ok and lint fail — 失败时回落上一可用修订，
 * 状态出口相同，差异只在 `current_revision` 是否前进。
 */
const TRANSITIONS: Readonly<Record<CreativeDraftState, readonly CreativeDraftState[]>> = {
  drafting: ['generating'],
  generating: ['ready'],
  ready: ['generating', 'publishing'],
  // 成功 → published；失败 → ready（草稿完整保留，错误可见）。
  publishing: ['published', 'ready'],
  published: ['archived'],
  archived: [],
};

/** Whether the state machine allows `from → to`. 其余一律非法（含自迁移）。 */
export function canTransition(from: CreativeDraftState, to: CreativeDraftState): boolean {
  return TRANSITIONS[from].includes(to);
}

/** In-flight states: 只允许删除，其余动作一律禁用。 */
export function isDraftBusy(state: CreativeDraftState): boolean {
  return state === 'generating' || state === 'publishing';
}

/**
 * Action availability for the current state + revision pointer.
 * `revision` 是 `current_revision`（0 = 尚无修订）。
 */
export function draftActions(
  state: CreativeDraftState,
  revision: number,
): CreativeDraftActions {
  const busy = isDraftBusy(state);
  if (busy) {
    return { canGenerate: false, canUndo: false, canPublish: false, canDelete: false };
  }
  // 注意：引擎从不把 state 推进到 ready（版本写入发生在 daemon 侧、state 列
  // 恒为 drafting），活跃态下能力必须由 revision 推导，否则 Undo/发布永远不可用。
  const active = state === 'drafting' || state === 'ready';
  return {
    canGenerate: active,
    // 撤销只移动指针，rev-1 已是下限。
    canUndo: active && revision > 1,
    canPublish: active && revision >= 1,
    canDelete: true,
  };
}

/**
 * Move the revision pointer, clamped to [1, max].
 * 非法入参（NaN / max < 1）收敛到 1，避免把越界指针写进 DB。
 */
export function clampRevision(current: number, delta: number, max: number): number {
  const upper = Number.isFinite(max) ? Math.floor(max) : 1;
  if (upper < 1) return 1;
  const next = Math.floor(current) + Math.floor(delta);
  if (!Number.isFinite(next)) return 1;
  if (next < 1) return 1;
  if (next > upper) return upper;
  return next;
}

/** Revision chip label — `rev-3`，当前修订带 `•` 标记。 */
export function revisionLabel(revision: number, current: number): string {
  const label = `rev-${revision}`;
  return revision === current ? `${label} •` : label;
}

/** 修订数超过上限时需要裁剪最旧修订。 */
export function shouldPruneRevisions(count: number): boolean {
  return count > MAX_DRAFT_REVISIONS;
}

/**
 * Which revisions to drop when over the cap.
 * 约定：始终保留 rev-1 与最新修订，从第二旧开始丢弃。
 * `revisions` 传入已存在的修订号（无需有序）。
 */
export function revisionsToPrune(revisions: number[]): number[] {
  if (!shouldPruneRevisions(revisions.length)) return [];
  const sorted = [...revisions].sort((a, b) => a - b);
  const overflow = sorted.length - MAX_DRAFT_REVISIONS;
  const oldest = sorted[0];
  const newest = sorted[sorted.length - 1];
  const dropped: number[] = [];
  for (const rev of sorted) {
    if (dropped.length >= overflow) break;
    if (rev === oldest || rev === newest) continue;
    dropped.push(rev);
  }
  return dropped;
}
