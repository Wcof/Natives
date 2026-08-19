import { normalizeAssistantProjectPath } from './assistant-project-path';

export interface AssistantProjectConversation {
  id: string;
  projectId: string | null;
  title: string;
  updatedAt: string;
  mode?: 'chat' | 'agent' | 'goal';
  parentConversationId?: string | null;
  /** Whether this conversation is pinned within its project (UI preference). */
  pinned?: boolean;
  /** W8: low-frequency activity projection — most recent run status. */
  lastRunStatus?: string | null;
}

export interface AssistantProjectGroup {
  id: string;
  path: string | null;
  label: string;
  conversations: AssistantProjectConversation[];
  /** Host last_opened_at when available; used for unpinned project order. */
  lastOpenedAt?: string | null;
}

export type AssistantProjectCreationState =
  | 'engine_unavailable'
  | 'provider_needed'
  | 'model_needed'
  | 'ready';

export type AssistantSurfaceState =
  | 'renderer_only'
  | 'connecting_engine'
  | 'engine_unavailable'
  | 'provider_needed'
  | 'model_needed'
  | 'ready';

export function classifyAssistantSurface(input: {
  bridge: boolean;
  engine?: 'connecting' | 'failed' | 'ready';
  provider?: 'no_provider' | 'no_model' | 'ready';
}): AssistantSurfaceState {
  if (!input.bridge) return 'renderer_only';
  if (input.engine === 'connecting' || !input.engine) return 'connecting_engine';
  if (input.engine === 'failed') return 'engine_unavailable';
  if (input.provider === 'no_provider') return 'provider_needed';
  if (input.provider === 'no_model') return 'model_needed';
  return 'ready';
}

export function projectCreationState(input: {
  engine: 'ready' | 'connecting' | 'unavailable';
  providerReadiness: 'no_provider' | 'no_model' | 'ready';
}): AssistantProjectCreationState {
  if (input.engine !== 'ready') return 'engine_unavailable';
  if (input.providerReadiness === 'no_provider') return 'provider_needed';
  if (input.providerReadiness === 'no_model') return 'model_needed';
  return 'ready';
}

export function displayProjectName(path: string): string {
  const normalized = normalizeAssistantProjectPath(path);
  const parts = normalized.split('/').filter(Boolean);
  return parts.at(-1) ?? normalized;
}

export interface RegisteredProjectMeta {
  path: string;
  lastOpenedAt?: string | null;
  label?: string | null;
  /** Host filesystem check; false means the recorded project directory is gone. */
  exists?: boolean;
}

/**
 * Group conversations by project.
 *
 * Project order (caller may further pin-sort): registered projects keep the
 * order of `registeredProjects` (backend last_opened_at DESC).
 *
 * Conversation order within a project:
 * 1. Pinned conversations first (stable by updatedAt among pins).
 * 2. Remaining by updatedAt DESC.
 *
 * Product decision 1: `hiddenProjectPaths` are soft-deleted projects. Their
 * daemon sessions still carry `project_id`, but the sidebar must NOT re-invent
 * the project node (extras) nor list those sessions — the project is hidden
 * until the user re-adds the path via `project.register`.
 *
 * The sidebar only projects registered projects. Any session whose `project_id`
 * references a project that is soft-deleted, physically deleted (missing
 * directory), unregistered (a legacy UUID / orphaned path), or has no project
 * at all is hidden entirely — there is no "unassigned" bucket and no invented
 * project node.
 */
export function groupAssistantConversations(
  conversations: AssistantProjectConversation[],
  registeredProjects: Array<string | RegisteredProjectMeta> = [],
  hiddenProjectPaths: Iterable<string> = [],
): AssistantProjectGroup[] {
  const registered: RegisteredProjectMeta[] = registeredProjects.map((item) =>
    typeof item === 'string' ? { path: item } : item,
  );
  const hiddenPaths = new Set(
    Array.from(hiddenProjectPaths, normalizeAssistantProjectPath).filter(Boolean),
  );

  const byProject = new Map<string, AssistantProjectConversation[]>();
  const missingProjectPaths = new Set<string>();

  for (const proj of registered) {
    const path = normalizeAssistantProjectPath(proj.path);
    if (!path || hiddenPaths.has(path)) continue;
    if (proj.exists === false) {
      missingProjectPaths.add(path);
      continue;
    }
    if (!byProject.has(path)) byProject.set(path, []);
  }

  for (const conversation of conversations) {
    if (conversation.parentConversationId?.trim()) continue;
    const path = conversation.projectId
      ? normalizeAssistantProjectPath(conversation.projectId)
      : '';
    // Hide any session that does not belong to a registered, existing project:
    // soft-deleted, physically deleted (missing directory), orphaned legacy
    // UUID/path, or no project at all.
    if (hiddenPaths.has(path)) continue;
    if (!path || !byProject.has(path)) continue;
    const group = byProject.get(path) ?? [];
    group.push(conversation);
    byProject.set(path, group);
  }

  const sortConversations = (items: AssistantProjectConversation[]) =>
    [...items].sort((left, right) => {
      const pinDelta = Number(Boolean(right.pinned)) - Number(Boolean(left.pinned));
      if (pinDelta !== 0) return pinDelta;
      return right.updatedAt.localeCompare(left.updatedAt);
    });

  // Preserve registered project order from backend (last_opened_at DESC).
  const groups: AssistantProjectGroup[] = [];
  const seen = new Set<string>();
  for (const proj of registered) {
    const path = normalizeAssistantProjectPath(proj.path);
    if (!path || missingProjectPaths.has(path) || seen.has(path)) continue;
    seen.add(path);
    const items = byProject.get(path) ?? [];
    groups.push({
      id: path,
      path,
      label: proj.label?.trim() || displayProjectName(path),
      conversations: sortConversations(items),
      lastOpenedAt: proj.lastOpenedAt ?? null,
    });
  }

  return groups;
}

/** Pin overlay: pinned projects first, preserving relative order within each bucket. */
export function orderGroupsWithPins(
  groups: AssistantProjectGroup[],
  pinnedProjectIds: Iterable<string>,
): AssistantProjectGroup[] {
  const pinned = new Set(pinnedProjectIds);
  return groups
    .map((group, index) => ({ group, index }))
    .sort((left, right) => {
      const pinDelta =
        Number(pinned.has(right.group.id)) - Number(pinned.has(left.group.id));
      if (pinDelta !== 0) return pinDelta;
      return left.index - right.index;
    })
    .map((item) => item.group);
}
