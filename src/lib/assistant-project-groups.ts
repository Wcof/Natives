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
  const normalized = path.trim().replace(/\/+$/, '');
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
 * Project order (caller may further pin-sort):
 * 1. Registered projects keep the order of `registeredProjects` (backend last_opened_at DESC).
 * 2. Any conversation-only projects not in registered list are appended by latest conversation.
 * 3. Unassigned is always last.
 *
 * Conversation order within a project:
 * 1. Pinned conversations first (stable by updatedAt among pins).
 * 2. Remaining by updatedAt DESC.
 *
 * Product decision 1: `hiddenProjectPaths` are soft-deleted projects. Their
 * daemon sessions still carry `project_id`, but the sidebar must NOT re-invent
 * the project node (extras) nor list those sessions — the project is hidden
 * until the user re-adds the path via `project.register`.
 */
export function groupAssistantConversations(
  conversations: AssistantProjectConversation[],
  registeredProjects: Array<string | RegisteredProjectMeta> = [],
  unassignedLabel = 'Unassigned',
  hiddenProjectPaths: Iterable<string> = [],
): AssistantProjectGroup[] {
  const registered: RegisteredProjectMeta[] = registeredProjects.map((item) =>
    typeof item === 'string' ? { path: item } : item,
  );
  const hiddenPaths = new Set(Array.from(hiddenProjectPaths, (p) => p.trim()).filter(Boolean));

  const byProject = new Map<string, AssistantProjectConversation[]>();
  const unassigned: AssistantProjectConversation[] = [];
  const metaByPath = new Map<string, RegisteredProjectMeta>();
  const missingProjectPaths = new Set<string>();

  for (const proj of registered) {
    const path = proj.path.trim();
    if (!path || hiddenPaths.has(path)) continue;
    metaByPath.set(path, proj);
    if (proj.exists === false) {
      missingProjectPaths.add(path);
      continue;
    }
    if (!byProject.has(path)) byProject.set(path, []);
  }

  for (const conversation of conversations) {
    if (conversation.parentConversationId?.trim()) continue;
    const path = conversation.projectId?.trim() ?? '';
    // Soft-deleted project: hide the session entirely, never move it to
    // unassigned or re-invent the project from its project_id.
    if (hiddenPaths.has(path)) continue;
    if (!path || missingProjectPaths.has(path)) {
      unassigned.push(conversation);
      continue;
    }
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
    const path = proj.path.trim();
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

  // Conversation-only projects not in the registered list (should be rare after unassigned policy).
  const extras: AssistantProjectGroup[] = [];
  for (const [path, items] of byProject.entries()) {
    if (seen.has(path)) continue;
    extras.push({
      id: path,
      path,
      label: displayProjectName(path),
      conversations: sortConversations(items),
      lastOpenedAt: metaByPath.get(path)?.lastOpenedAt ?? null,
    });
  }
  extras.sort((left, right) => {
    const leftDate = left.conversations[0]?.updatedAt ?? '';
    const rightDate = right.conversations[0]?.updatedAt ?? '';
    if (leftDate !== rightDate) return rightDate.localeCompare(leftDate);
    return left.label.localeCompare(right.label);
  });
  groups.push(...extras);

  if (unassigned.length > 0) {
    groups.push({
      id: '__unassigned__',
      path: null,
      label: unassignedLabel,
      conversations: sortConversations(unassigned),
      lastOpenedAt: null,
    });
  }

  // Unassigned always last; do not re-sort registered projects by session time.
  return groups.sort((left, right) => {
    if (left.path === null && right.path !== null) return 1;
    if (left.path !== null && right.path === null) return -1;
    return 0;
  });
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
