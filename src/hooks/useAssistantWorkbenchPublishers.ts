'use client';

/**
 * Navigation + runtime snapshot publishers for the assistant workbench.
 *
 * Publishes the navigation snapshot (sidebar groups, selected id, creation
 * state, temp session) and the runtime snapshot (active run status, events,
 * file changes, artifacts, usage) to the shell via the stable workspace API.
 *
 * The orchestrator retains single state authority — all values and setters
 * are passed in, not recreated here.
 */

import { useEffect } from 'react';
import { useAssistantStore } from '@/lib/assistant-workspace';
import {
  groupAssistantConversations,
  projectCreationState,
} from '@/lib/assistant-project-groups';
import { conversationsWithoutTemp, isTempConversationId } from '@/lib/assistant-temp-conversation';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import type { Conversation, RunEvent, Artifact, ContextUsage } from '@/lib/assistant-protocol';
import type { ProviderReadiness } from '@/lib/provider-model-selection';
import type { AssistantNavigationSnapshot, AssistantRuntimeSnapshot } from '@/lib/assistant-ui-types';

export interface UseAssistantWorkbenchPublishersOptions {
  locale: Locale;
  rootConversationId: string | null;
  activeId: string | null;
  activeProjectPath: string | null;
  rootConversation: Conversation | null;
  activeConversation: Conversation | null;
  activeRun: { id: string; status: string; startedAt?: string | null; finishedAt?: string | null } | null;
  events: RunEvent[];
  fileChanges: Array<{ path: string; changeType: string }>;
  artifacts: Artifact[];
  contextUsage: ContextUsage | null;
  registeredProjects: Array<{ id: string; path: string; lastOpenedAt?: string | null; label?: string; exists?: boolean }>;
  /** Soft-deleted (hidden) project paths — product decision 1. */
  hiddenProjectPaths: string[];
  pinnedConversationIds: Set<string>;
  loadingConversations: boolean;
  providerReadiness: ProviderReadiness;
  publishNavigation: (
    updater:
      | AssistantNavigationSnapshot
      | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
  ) => void;
  publishRuntime: (snapshot: AssistantRuntimeSnapshot) => void;
}

export function useAssistantWorkbenchPublishers({
  locale,
  rootConversationId,
  activeId,
  activeProjectPath,
  rootConversation,
  activeConversation,
  activeRun,
  events,
  fileChanges,
  artifacts,
  contextUsage,
  registeredProjects,
  hiddenProjectPaths,
  pinnedConversationIds,
  loadingConversations,
  providerReadiness,
  publishNavigation,
  publishRuntime,
}: UseAssistantWorkbenchPublishersOptions) {
  const state = useAssistantStore();

  // Publish navigation snapshot for shell sidebar
  useEffect(() => {
    const conversations = conversationsWithoutTemp(
      (state.conversationOrder
        .map((id) => state.conversations[id])
        .filter(Boolean) as Conversation[]),
    );
    const groups = groupAssistantConversations(
      conversations.map((c) => ({
        id: c.id,
        title: c.title,
        mode: c.mode,
        projectId: c.projectId ?? null,
        updatedAt: c.updatedAt,
        parentConversationId: c.parentConversationId ?? null,
        pinned: pinnedConversationIds.has(c.id),
      })),
      registeredProjects.map((p) => ({ path: p.path, lastOpenedAt: (p as { lastOpenedAt?: string | null; last_opened_at?: string | null }).lastOpenedAt ?? (p as { last_opened_at?: string | null }).last_opened_at ?? null, label: p.label, exists: p.exists })),
      t(locale, 'assistant.unassignedProjects'),
      hiddenProjectPaths,
    );
    // Merge with projects already seeded by AssistantWorkspaceProvider so a
    // late/empty workbench project.list cannot blank the sidebar on first paint.
    publishNavigation((prev) => {
      let nextGroups = groups;
      if (registeredProjects.length === 0 && prev.groups.length > 0) {
        const seedPaths = prev.groups
          .map((g) => g.path)
          .filter((p): p is string => Boolean(p));
        if (seedPaths.length > 0) {
          nextGroups = groupAssistantConversations(
            conversations.map((c) => ({
              id: c.id,
              title: c.title,
              mode: c.mode,
              projectId: c.projectId ?? null,
              updatedAt: c.updatedAt,
              parentConversationId: c.parentConversationId ?? null,
            })),
            seedPaths,
            t(locale, 'assistant.unassignedProjects'),
            hiddenProjectPaths,
          );
        }
      }
      // Prefer root-level temp shell (survives workbench remount) when store has none.
      const storeTempId = isTempConversationId(rootConversationId)
        ? rootConversationId
        : isTempConversationId(activeId)
          ? activeId
          : null;
      const rootTemp = prev.tempSession;
      const selectedId =
        storeTempId ??
        (rootTemp && rootConversationId === null ? rootTemp.conversation.id : rootConversationId) ??
        rootTemp?.conversation.id ??
        rootConversationId;
      return {
        groups: nextGroups,
        selectedId,
        activeProjectPath: activeProjectPath ?? prev.activeProjectPath,
        loading: loadingConversations,
        creationState: projectCreationState({
          engine:
            state.connection === 'connected'
              ? 'ready'
              : state.connection === 'connecting'
                ? 'connecting'
                : 'unavailable',
          providerReadiness,
        }),
        isCreatingConversation: false,
        pendingCreateProjectPath: prev.pendingCreateProjectPath,
        // Drop root temp once the store has a real active session (or a different temp).
        tempSession:
          storeTempId && rootTemp && rootTemp.conversation.id === storeTempId
            ? rootTemp
            : isTempConversationId(rootConversationId)
              ? rootTemp
              : rootConversationId
                ? null
                : rootTemp,
      };
    });
  }, [
    state.conversations,
    state.conversationOrder,
    state.connection,
    rootConversationId,
    activeId,
    activeProjectPath,
    registeredProjects,
    pinnedConversationIds,
    loadingConversations,
    providerReadiness,
    publishNavigation,
  ]);

  // Publish runtime for shell
  useEffect(() => {
    publishRuntime({
      conversationId: rootConversationId,
      conversationTitle: rootConversation?.title ?? activeConversation?.title ?? null,
      conversationMode: rootConversation?.mode ?? activeConversation?.mode ?? 'agent',
      providerId: activeConversation?.providerId ?? rootConversation?.providerId ?? '',
      modelId: activeConversation?.modelId ?? rootConversation?.modelId ?? '',
      runId: activeRun?.id ?? null,
      runStatus: activeRun?.status ?? 'idle',
      runStartedAt: activeRun?.startedAt ?? null,
      runFinishedAt: activeRun?.finishedAt ?? null,
      events: events.map((e) => ({
        runId: e.runId,
        sequence: e.sequence,
        timestamp: e.timestamp,
        type: String(e.type),
        payload: e.payload,
      })),
      fileChanges: fileChanges.map((f) => ({ path: f.path, change: f.changeType, changeType: f.changeType })),
      artifacts: artifacts.map((a) => ({
        id: a.id,
        path: a.path,
        label: a.label,
        size: a.size,
        kind: a.kind,
      })),
      usage: {
        inputTokens: contextUsage?.usedTokens ?? null,
        outputTokens: null,
        reasoningTokens: null,
      },
    });
  }, [
    rootConversationId,
    rootConversation,
    activeConversation,
    activeRun,
    events,
    fileChanges,
    artifacts,
    contextUsage,
    publishRuntime,
  ]);
}
