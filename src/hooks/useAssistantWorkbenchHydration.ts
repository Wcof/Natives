'use client';

/**
 * Temp-session hydration effects for the assistant workbench.
 *
 * Hydrates a root-level temp shell (created by the shell sidebar before the
 * lazy workbench mounts) into the workbench store, and handles the legacy
 * `pendingCreateProjectPath` deferred path. Both paths adopt the temp shell
 * without calling `conversation.create` and keep only the latest pick.
 *
 * The orchestrator retains single state authority — all setters and the store
 * ref are passed in, not recreated here.
 */

import { useEffect } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { useAssistantDispatch } from '@/lib/assistant-workspace';
import type { AssistantWorkspaceState } from '@/lib/assistant-workspace/state';
import { writeActiveProject } from '@/lib/active-project';
import {
  collectTempConversationIds,
  createTempSession,
  isTempConversationId,
} from '@/lib/assistant-temp-conversation';
import { selectAssistantModel, toProviderInfo } from '@/lib/provider-model-selection';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import type {
  Conversation,
} from '@/lib/assistant-protocol';
import type { ProviderWithModels } from '@/components/ui/conversation/ModelSelectorDropdown';
import type {
  AssistantNavigationSnapshot,
} from '@/components/assistant/AssistantWorkspaceContext';

export interface UseAssistantWorkbenchHydrationOptions {
  locale: Locale;
  providers: ProviderWithModels[];
  loadingConversations: boolean;
  navigation: AssistantNavigationSnapshot;
  stateRef: { current: AssistantWorkspaceState };
  setActiveProjectPath: Dispatch<SetStateAction<string | null>>;
  publishNavigation: (
    updater:
      | AssistantNavigationSnapshot
      | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
  ) => void;
}

export function useAssistantWorkbenchHydration({
  locale,
  providers,
  loadingConversations,
  navigation,
  stateRef,
  setActiveProjectPath,
  publishNavigation,
}: UseAssistantWorkbenchHydrationOptions) {
  const dispatch = useAssistantDispatch();

  /**
   * Hydrate a root-level temp shell into the workbench store.
   * Shell can create temp-* before this lazy workbench mounts; once mounted we
   * adopt it without calling conversation.create. Consecutive project picks
   * leave only the latest temp shell.
   *
   * Re-runs after loadConversations finishes because `conversations/replace`
   * wipes local-only shells — we re-upsert from the root-level tempSession.
   */
  useEffect(() => {
    const temp = navigation.tempSession;
    if (!temp) return;
    if (loadingConversations) return;
    const { conversation } = temp;
    if (!isTempConversationId(conversation.id)) return;

    if (conversation.projectId) {
      const projectId = conversation.projectId;
      setActiveProjectPath((prev) => (prev === projectId ? prev : projectId));
      void writeActiveProject(window.nativesAPI, projectId).catch(() => undefined);
    }

    // Drop any previous local temp shells (only keep the latest pick).
    for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
      if (oldId !== conversation.id) {
        dispatch({ type: 'conversations/remove', id: oldId });
        dispatch({ type: 'composer/clear', conversationId: oldId });
      }
    }

    // Prefer a model already chosen on the shell; otherwise fill from live providers
    // without blocking when none are configured (composer shows existing disabled state).
    const pick = selectAssistantModel(toProviderInfo(providers));
    const existing = stateRef.current.conversations[conversation.id];
    const shell: Conversation = {
      ...conversation,
      // Keep in-store edits (provider/model/permission) if hydrate re-runs after list load.
      providerId:
        existing?.providerId || conversation.providerId || pick?.providerId || '',
      modelId: existing?.modelId || conversation.modelId || pick?.modelId || '',
      permissionProfileId:
        existing?.permissionProfileId || conversation.permissionProfileId || 'ask',
    };
    // Skip upsert when the shell is already active and fields match — avoids
    // conversations map identity churn that re-fires the navigation publisher.
    const alreadyActive =
      stateRef.current.activeConversationId === shell.id &&
      existing &&
      existing.providerId === shell.providerId &&
      existing.modelId === shell.modelId &&
      existing.permissionProfileId === shell.permissionProfileId &&
      existing.projectId === shell.projectId &&
      existing.title === shell.title;
    if (!alreadyActive) {
      dispatch({ type: 'conversations/upsert', conversation: shell });
      dispatch({ type: 'conversations/setActive', id: shell.id });
    }
    const draftFromStore = stateRef.current.composerByConversation[shell.id];
    const draft = draftFromStore?.text || (draftFromStore?.attachments?.length ?? 0) > 0
      ? draftFromStore
      : temp.draft;
    if (draft && (draft.text || (draft.attachments?.length ?? 0) > 0)) {
      dispatch({
        type: 'composer/set',
        conversationId: shell.id,
        draft: {
          text: draft.text,
          attachments: draft.attachments,
          updatedAt: draft.updatedAt,
        },
      });
    }
  }, [navigation.tempSession?.conversation.id, loadingConversations]);

  // Legacy deferred path: shell set pendingCreateProjectPath before tempSession existed.
  useEffect(() => {
    const path = navigation.pendingCreateProjectPath;
    if (path === undefined) return;
    // Prefer the modern tempSession path when both are present.
    if (navigation.tempSession) {
      publishNavigation((prev) => ({ ...prev, pendingCreateProjectPath: undefined }));
      return;
    }
    if (path) {
      setActiveProjectPath(path);
      void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
    }
    const session = createTempSession({
      projectId: path,
      title: t(locale, 'assistant.newConversation'),
    });
    const pick = selectAssistantModel(toProviderInfo(providers));
    const conversation: Conversation = {
      ...session.conversation,
      providerId: pick?.providerId ?? '',
      modelId: pick?.modelId ?? '',
    };
    for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
      if (oldId !== conversation.id) {
        dispatch({ type: 'conversations/remove', id: oldId });
        dispatch({ type: 'composer/clear', conversationId: oldId });
      }
    }
    dispatch({ type: 'conversations/upsert', conversation });
    dispatch({ type: 'conversations/setActive', id: conversation.id });
    publishNavigation((prev) => ({
      ...prev,
      pendingCreateProjectPath: undefined,
      tempSession: { conversation, draft: session.draft },
      selectedId: conversation.id,
      activeProjectPath: path,
    }));
  }, [navigation.pendingCreateProjectPath]);
}
