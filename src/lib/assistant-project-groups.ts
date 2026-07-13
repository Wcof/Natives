export interface AssistantProjectConversation {
  id: string;
  projectId: string | null;
  title: string;
  updatedAt: string;
  mode?: 'chat' | 'agent';
}

export interface AssistantProjectGroup {
  id: string;
  path: string | null;
  label: string;
  conversations: AssistantProjectConversation[];
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

export function groupAssistantConversations(
  conversations: AssistantProjectConversation[],
  registeredProjects: string[] = [],
  unassignedLabel = 'Unassigned',
): AssistantProjectGroup[] {
  const byProject = new Map<string, AssistantProjectConversation[]>();
  const unassigned: AssistantProjectConversation[] = [];

  // Seed with all registered projects so empty-project groups appear
  for (const projPath of registeredProjects) {
    if (!byProject.has(projPath)) {
      byProject.set(projPath, []);
    }
  }

  for (const conversation of conversations) {
    const path = conversation.projectId?.trim() ?? '';
    if (!path) {
      unassigned.push(conversation);
      continue;
    }
    const group = byProject.get(path) ?? [];
    group.push(conversation);
    byProject.set(path, group);
  }

  const sortConversations = (items: AssistantProjectConversation[]) =>
    [...items].sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));

  const groups: AssistantProjectGroup[] = [...byProject.entries()].map(([path, items]) => ({
    id: path,
    path,
    label: displayProjectName(path),
    conversations: sortConversations(items),
  }));

  if (unassigned.length > 0) {
    groups.push({
      id: '__unassigned__',
      path: null,
      label: unassignedLabel,
      conversations: sortConversations(unassigned),
    });
  }

  return groups.sort((left, right) => {
    // Unassigned always goes last
    if (left.path === null && right.path !== null) return 1;
    if (left.path !== null && right.path === null) return -1;
    // Otherwise sort by latest conversation or project name
    const leftDate = left.conversations[0]?.updatedAt ?? '';
    const rightDate = right.conversations[0]?.updatedAt ?? '';
    if (leftDate !== rightDate) return rightDate.localeCompare(leftDate);
    return left.label.localeCompare(right.label);
  });
}
