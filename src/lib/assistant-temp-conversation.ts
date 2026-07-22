/**
 * Local-only temporary conversation shells.
 *
 * Created when the user picks a project directory (or starts a blank session)
 * so the workbench can show an empty composer immediately. They live only in
 * renderer memory — never call conversation.create, never appear in the
 * sidebar project groups, and must not be written to long-term draft storage.
 */

import type { Conversation } from '@/lib/assistant-protocol';

export const TEMP_CONVERSATION_ID_PREFIX = 'temp-';

export interface TempComposerDraft {
  text: string;
  attachments: Array<{ path: string; name: string; mimeType?: string; size?: number }>;
  updatedAt: string;
}

export interface TempConversationSession {
  conversation: Conversation;
  draft: TempComposerDraft;
}

export function isTempConversationId(id: string | null | undefined): boolean {
  return typeof id === 'string' && id.startsWith(TEMP_CONVERSATION_ID_PREFIX);
}

export function createTempConversationId(now = Date.now()): string {
  return `${TEMP_CONVERSATION_ID_PREFIX}${now}`;
}

export function emptyTempDraft(now = new Date().toISOString()): TempComposerDraft {
  return { text: '', attachments: [], updatedAt: now };
}

/**
 * Build a local-only conversation shell. Provider/model may be empty so the
 * user can land on a blank page before configuring them.
 */
export function createTempConversationShell(input: {
  projectId: string | null;
  title: string;
  providerId?: string;
  modelId?: string;
  permissionProfileId?: string;
  now?: string;
  id?: string;
}): Conversation {
  const now = input.now ?? new Date().toISOString();
  return {
    id: input.id ?? createTempConversationId(),
    mode: 'agent',
    title: input.title,
    providerId: input.providerId ?? '',
    modelId: input.modelId ?? '',
    projectId: input.projectId,
    permissionProfileId: input.permissionProfileId ?? 'ask',
    createdAt: now,
    updatedAt: now,
  };
}

export function createTempSession(input: {
  projectId: string | null;
  title: string;
  providerId?: string;
  modelId?: string;
  permissionProfileId?: string;
  now?: string;
  id?: string;
}): TempConversationSession {
  return {
    conversation: createTempConversationShell(input),
    draft: emptyTempDraft(input.now),
  };
}

/** Ids of all local temp shells (used to drop the previous one on re-pick). */
export function collectTempConversationIds(ids: Iterable<string>): string[] {
  return [...ids].filter((id) => isTempConversationId(id));
}

/** Drop temp-* keys from a draft / conversation map before long-term persist. */
export function omitTempKeys<T>(record: Record<string, T>): Record<string, T> {
  const next: Record<string, T> = {};
  for (const [key, value] of Object.entries(record)) {
    if (!isTempConversationId(key)) next[key] = value;
  }
  return next;
}

/** Conversations safe to publish into the sidebar project groups. */
export function conversationsWithoutTemp<T extends { id: string }>(
  conversations: T[],
): T[] {
  return conversations.filter((c) => !isTempConversationId(c.id));
}

/**
 * Prefer the normalized path returned by project.register; fall back to the
 * picker path only when the host omits path (should not happen in production).
 */
export function resolveRegisteredProjectPath(
  registered: { path?: string | null } | null | undefined,
  pickerPath: string,
): string {
  const normalized = registered?.path?.trim();
  if (normalized) return normalized;
  return pickerPath;
}
