/**
 * Pure view-model helpers for ActivityInspector 《任务》/《产物》 tabs.
 * Prefer optional props from Workbench; fall back to events when absent.
 */

import type { Artifact, FileChange, RunEvent } from '@/lib/assistant-protocol';

export type TodoStatus = 'pending' | 'in_progress' | 'completed';

export interface ActivityTodo {
  id: string;
  content: string;
  status: TodoStatus;
}

export type ArtifactBucket = 'used' | 'created' | 'modified';

/** Shared empty todos — effect / useMemo deps stay referentially stable. */
const EMPTY_TODOS: ActivityTodo[] = [];

export interface ArtifactFileItem {
  path: string;
  changeType: ArtifactBucket;
  at?: string;
  runId?: string;
}

export interface FileEventInput {
  path: string;
  changeType: 'created' | 'modified' | string;
  at?: string;
  runId?: string;
}

export type SubagentUiStatusKey =
  | 'pending_assignment'
  | 'in_progress'
  | 'completed'
  | 'closed';

export interface SubagentUiStatus {
  key: SubagentUiStatusKey;
  zh: string;
  en: string;
}

/** Normalize a full path for de-duplication (absolute/relative both OK). */
export function normalizeArtifactPath(path: string): string {
  if (!path) return '';
  let p = path.trim().replace(/\\/g, '/');
  // Collapse repeated slashes (keep leading // for UNC-like if present → reduce to /)
  p = p.replace(/\/{2,}/g, '/');
  // Drop trailing slash except root
  if (p.length > 1 && p.endsWith('/')) p = p.slice(0, -1);
  return p;
}

function isTodoWriteName(name: string): boolean {
  const n = name.trim().toLowerCase().replace(/[-_]/g, '');
  return n === 'todowrite';
}

function normalizeTodoStatus(raw: unknown): TodoStatus {
  const s = String(raw ?? '')
    .trim()
    .toLowerCase()
    .replace(/[-\s]/g, '_');
  if (
    s === 'completed' ||
    s === 'complete' ||
    s === 'done' ||
    s === 'finished' ||
    s === 'success'
  ) {
    return 'completed';
  }
  if (
    s === 'in_progress' ||
    s === 'inprogress' ||
    s === 'running' ||
    s === 'active' ||
    s === 'doing' ||
    s === 'working'
  ) {
    return 'in_progress';
  }
  return 'pending';
}

function parseTodoList(raw: unknown): ActivityTodo[] | null {
  if (!Array.isArray(raw)) return null;
  const out: ActivityTodo[] = [];
  for (let i = 0; i < raw.length; i += 1) {
    const item = raw[i];
    if (!item || typeof item !== 'object') continue;
    const rec = item as Record<string, unknown>;
    const content = String(rec.content ?? rec.text ?? rec.title ?? rec.task ?? '').trim();
    if (!content) continue;
    const id = String(rec.id ?? rec.todo_id ?? rec.todoId ?? `todo-${i}`);
    out.push({
      id,
      content,
      status: normalizeTodoStatus(rec.status ?? rec.state),
    });
  }
  return out.length > 0 ? out : [];
}

function todosFromPayload(payload: Record<string, unknown>): ActivityTodo[] | null {
  const input = (payload.input ?? payload.args ?? payload.tool_input ?? payload.toolInput) as
    | Record<string, unknown>
    | unknown;
  if (input && typeof input === 'object' && !Array.isArray(input)) {
    const fromInput = parseTodoList((input as Record<string, unknown>).todos);
    if (fromInput) return fromInput;
  }

  const output = payload.output ?? payload.result ?? payload.tool_output ?? payload.toolOutput;
  if (output && typeof output === 'object' && !Array.isArray(output)) {
    const rec = output as Record<string, unknown>;
    const nested = (rec.result ?? rec.output) as Record<string, unknown> | unknown;
    const fromOut = parseTodoList(rec.todos);
    if (fromOut) return fromOut;
    if (nested && typeof nested === 'object' && !Array.isArray(nested)) {
      const fromNested = parseTodoList((nested as Record<string, unknown>).todos);
      if (fromNested) return fromNested;
    }
  }

  // Some adapters flatten todos onto the event payload itself.
  const direct = parseTodoList(payload.todos);
  if (direct) return direct;
  return null;
}

/**
 * Recover the latest Todo list from the most recent todo_write tool event.
 * Walks events in reverse so the last write wins.
 */
export function extractTodosFromEvents(events: RunEvent[]): ActivityTodo[] {
  if (!events?.length) return EMPTY_TODOS;

  for (let i = events.length - 1; i >= 0; i -= 1) {
    const event = events[i]!;
    const type = String(event.type ?? '');
    const isToolEvent =
      type === 'tool_call_requested' ||
      type === 'tool_call_started' ||
      type === 'tool_call_delta' ||
      type === 'tool_call_completed' ||
      type === 'tool_started' ||
      type === 'tool_completed' ||
      type === 'tool_output_delta';
    if (!isToolEvent) continue;

    const p = (event.payload ?? {}) as Record<string, unknown>;
    const name = String(p.name ?? p.tool_name ?? p.toolName ?? '');
    if (!isTodoWriteName(name)) continue;

    const todos = todosFromPayload(p);
    if (todos) return todos;
  }
  return EMPTY_TODOS;
}

/** Aggregate status: any in_progress → in_progress; all completed → completed; else pending. */
export function summarizeTodoStatus(todos: ActivityTodo[]): TodoStatus {
  if (!todos.length) return 'pending';
  let allCompleted = true;
  for (const todo of todos) {
    if (todo.status === 'in_progress') return 'in_progress';
    if (todo.status !== 'completed') allCompleted = false;
  }
  return allCompleted ? 'completed' : 'pending';
}

function coerceChangeBucket(raw: string | undefined | null): ArtifactBucket {
  const s = String(raw ?? '')
    .trim()
    .toLowerCase();
  if (s === 'used' || s === 'reference' || s === 'attachment' || s === 'input') {
    return 'used';
  }
  if (s === 'created' || s === 'create' || s === 'added' || s === 'add' || s === 'new') {
    return 'created';
  }
  if (s === 'modified' || s === 'modify' || s === 'edited' || s === 'edit' || s === 'updated' || s === 'update') {
    return 'modified';
  }
  // Unknown file-like change: default to created (spec: non-file artifact with path → 新增)
  return 'created';
}

/**
 * created-wins-over-later-modified:
 * once a path has been seen as created, later modified events keep it in the created bucket
 * but update `at` / `runId` from the latest event.
 */
function applyFileEvent(
  map: Map<string, ArtifactFileItem>,
  pathRaw: string,
  changeTypeRaw: string,
  at?: string,
  runId?: string,
): void {
  const path = normalizeArtifactPath(pathRaw);
  if (!path) return;
  const bucket = coerceChangeBucket(changeTypeRaw);
  const existing = map.get(path);
  if (!existing) {
    map.set(path, { path, changeType: bucket, at, runId });
    return;
  }
  // Created wins over modified; used remains used unless explicitly created/modified
  const nextType: ArtifactBucket =
    existing.changeType === 'created' || bucket === 'created'
      ? 'created'
      : existing.changeType === 'modified' || bucket === 'modified'
        ? 'modified'
        : bucket;
  map.set(path, {
    path,
    changeType: nextType,
    // Latest event wins for metadata
    at: at ?? existing.at,
    runId: runId ?? existing.runId,
  });
}

/**
 * Aggregate artifact files into three buckets: used, created, modified.
 * Dedupes by normalized full path; created wins over later modified.
 */
export function aggregateArtifactFiles(options: {
  fileChanges?: FileChange[];
  fileEvents?: FileEventInput[];
  artifacts?: Artifact[];
  events?: RunEvent[];
  usedFiles?: string[];
}): { used: ArtifactFileItem[]; created: ArtifactFileItem[]; modified: ArtifactFileItem[] } {
  const map = new Map<string, ArtifactFileItem>();

  // Explicit used files (attachments, external references)
  if (options.usedFiles?.length) {
    for (const uf of options.usedFiles) {
      applyFileEvent(map, uf, 'used');
    }
  }

  // Explicit chronological fileEvents
  if (options.fileEvents?.length) {
    for (const fe of options.fileEvents) {
      applyFileEvent(map, fe.path, fe.changeType, fe.at, fe.runId);
    }
  }

  // file_changed and file_reference events
  if (options.events?.length) {
    for (const event of options.events) {
      const type = String(event.type ?? '');
      if (type === 'file_changed') {
        const p = (event.payload ?? {}) as Record<string, unknown>;
        applyFileEvent(
          map,
          String(p.path ?? ''),
          String(p.change_type ?? p.changeType ?? 'modified'),
          event.timestamp,
          event.runId,
        );
      } else if (type === 'file_reference') {
        const p = (event.payload ?? {}) as Record<string, unknown>;
        applyFileEvent(
          map,
          String(p.path ?? p.filePath ?? p.file_path ?? ''),
          'used',
          event.timestamp,
          event.runId,
        );
      }
    }
  }

  // Workspace fileChanges
  if (options.fileChanges?.length) {
    for (const fc of options.fileChanges) {
      applyFileEvent(map, fc.path, fc.changeType, undefined, fc.runId);
    }
  }

  // Artifacts
  if (options.artifacts?.length) {
    for (const a of options.artifacts) {
      const path = normalizeArtifactPath(a.path);
      if (!path) continue;
      if (map.has(path)) {
        const existing = map.get(path)!;
        map.set(path, {
          ...existing,
          at: existing.at ?? a.createdAt,
          runId: existing.runId ?? a.runId,
        });
        continue;
      }
      const kind = String(a.kind ?? 'file').toLowerCase();
      const bucket: ArtifactBucket =
        kind === 'modified' || kind === 'edit' || kind === 'edited'
          ? 'modified'
          : kind === 'used' || kind === 'reference'
            ? 'used'
            : 'created';
      map.set(path, {
        path,
        changeType: bucket,
        at: a.createdAt,
        runId: a.runId,
      });
    }
  }

  const used: ArtifactFileItem[] = [];
  const created: ArtifactFileItem[] = [];
  const modified: ArtifactFileItem[] = [];
  for (const item of map.values()) {
    if (item.changeType === 'used') used.push(item);
    else if (item.changeType === 'created') created.push(item);
    else modified.push(item);
  }

  // Stable sort by path for deterministic UI
  const byPath = (a: ArtifactFileItem, b: ArtifactFileItem) => a.path.localeCompare(b.path);
  used.sort(byPath);
  created.sort(byPath);
  modified.sort(byPath);
  return { used, created, modified };
}

/**
 * Map wire/subagent status to UI key + labels.
 * pending_assignment → 待分配
 * open / queued / running / waiting* → 执行中
 * completed / idle → 已完成（idle 仅兼容）
 * failed / cancelled / interrupted / closed → 关闭
 */
export function mapSubagentUiStatus(status: string | null | undefined): SubagentUiStatus {
  const s = String(status ?? '')
    .trim()
    .toLowerCase();

  if (s === 'pending_assignment' || s === 'pending-assignment' || s === 'unassigned') {
    return { key: 'pending_assignment', zh: '待分配', en: 'Pending assignment' };
  }
  // completed and idle (compat) both surface as 已完成
  if (
    s === 'completed' ||
    s === 'complete' ||
    s === 'done' ||
    s === 'success' ||
    s === 'idle'
  ) {
    return { key: 'completed', zh: '已完成', en: 'Completed' };
  }
  if (
    s === 'failed' ||
    s === 'cancelled' ||
    s === 'canceled' ||
    s === 'interrupted' ||
    s === 'closed' ||
    s === 'aborted' ||
    s === 'error' ||
    s === 'rejected'
  ) {
    return { key: 'closed', zh: '关闭', en: 'Closed' };
  }
  // open / queued / running / waiting_* / in_progress / active / preparing / …
  if (
    !s ||
    s === 'open' ||
    s === 'queued' ||
    s === 'running' ||
    s === 'waiting' ||
    s.startsWith('waiting_') ||
    s === 'in_progress' ||
    s === 'active' ||
    s === 'preparing' ||
    s === 'pending' ||
    s === 'created' ||
    s === 'reasoning' ||
    s === 'cancelling'
  ) {
    return { key: 'in_progress', zh: '执行中', en: 'In progress' };
  }
  // Unknown non-terminal → treat as in progress; unknown terminal-ish already handled.
  return { key: 'in_progress', zh: '执行中', en: 'In progress' };
}

export function todoStatusLabel(status: TodoStatus, zh: boolean): string {
  // Labels kept for pure-helper fallbacks / tests; UI prefers i18n keys.
  if (status === 'completed') return zh ? '已完成' : 'Completed';
  if (status === 'in_progress') return zh ? '执行中' : 'In progress';
  return zh ? '待执行' : 'Pending';
}
