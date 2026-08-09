'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from '@/lib/assistant-workspace';
import { resolveModelSelection, type ProviderReadiness } from '@/lib/provider-model-selection';
import { updateConversationCapabilities } from '@/lib/assistant-workspace/capability-admin';
import { canSelectRunCapabilities } from '@/lib/assistant-workspace/capability-gate';
import { sendOrQueue } from '@/lib/assistant-workspace/controller';
// W4: useToast from lib — hooks never depend on component internals.
import { useToast } from '@/lib/toast-context';
import { classifyError } from '@/lib/error-classifier';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import { isTempConversationId } from '@/lib/assistant-temp-conversation';
import { mapWireConversation } from '@/lib/assistant-protocol';
import type { CapabilitySelection, Conversation } from '@/lib/assistant-protocol';
import type { AssistantDraft } from '@/lib/assistant-composer';
// W4: shared types from lib — hooks never depend on component internals.
import type { ProviderWithModels, AssistantNavigationSnapshot } from '@/lib/assistant-ui-types';

export interface UseAssistantWorkbenchComposerOptions {
  /** Surface conversation id (child when a subagent is focused). */
  activeId: string | null;
  activeConversation: Conversation | null;
  providers: ProviderWithModels[];
  providerReadiness: ProviderReadiness;
  registeredProjects: Array<{
    id: string;
    path: string;
    lastOpenedAt?: string | null;
    label?: string;
    exists?: boolean;
  }>;
  activeProjectPath: string | null;
  locale: Locale;
  ensureRunSubscription: (runId: string | null | undefined) => void;
  publishNavigation: (
    updater:
      | AssistantNavigationSnapshot
      | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
  ) => void;
  setSelectedRootConversationId: Dispatch<SetStateAction<string | null>>;
  setSelectedChildConversationId: Dispatch<SetStateAction<string | null>>;
}

/**
 * Composer ownership for the workbench.
 *
 * Owns the single class of "what the user is typing/selecting in the composer":
 * the send path (temp → persisted promotion, capability carry-over, queueing),
 * the ADR-0016 capability picker (visibility + selection persistence), and the
 * picker auto-close on conversation switch.
 */
export function useAssistantWorkbenchComposer({
  activeId,
  activeConversation,
  providers,
  providerReadiness,
  registeredProjects,
  activeProjectPath,
  locale,
  ensureRunSubscription,
  publishNavigation,
  setSelectedRootConversationId,
  setSelectedChildConversationId,
}: UseAssistantWorkbenchComposerOptions) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const { toast } = useToast();

  const stateRef = useRef(state);
  stateRef.current = state;

  /** ADR-0016 composer capability picker visibility. */
  const [capabilityPickerOpen, setCapabilityPickerOpen] = useState(false);

  const handleSend = useCallback(
    async (draft: AssistantDraft, forceImmediate = false): Promise<boolean> => {
      // Sends always target the surface conversation (child when selected).
      let conversationId = activeId;
      /** Set on temp→real promotion; store update lands after this tick. */
      let promotedCapabilitySelection: CapabilitySelection | null = null;
      const pick = resolveModelSelection(providers, {
        providerId: activeConversation?.providerId,
        modelId: activeConversation?.modelId,
      });
      // Stale/deleted provider: block send and require explicit re-select (no ghost remap).
      if (
        activeConversation?.providerId &&
        !providers.some((p) => p.id === activeConversation.providerId)
      ) {
        toast(t(locale, 'assistant.providerInvalid'), 'error');
        return false;
      }
      const providerId = pick?.providerId ?? '';
      const modelId = pick?.modelId ?? '';
      if (!providerId || !modelId || providerReadiness !== 'ready') {
        toast(t(locale, 'assistant.configureProviderAndModel'), 'error');
        return false;
      }

      try {
        if (!conversationId || isTempConversationId(conversationId)) {
          const activeProject = registeredProjects.find((project) => project.path === activeProjectPath);
          if (!activeProjectPath || activeProject?.exists === false) {
            toast(
              t(locale, 'assistant.selectExistingProject'),
              'error',
            );
            return false;
          }
          const title = draft.content.trim().slice(0, 30) || t(locale, 'assistant.newConversation');
          const createdRaw = await gateway.request<Record<string, unknown> | Conversation>(
            'conversation.create',
            {
              mode: 'agent',
              title,
              provider_id: providerId,
              model_id: modelId,
              // Always use the normalized active project path (from project.register).
              project_id: activeProjectPath,
              permission_profile_id: activeConversation?.permissionProfileId ?? 'ask',
            },
          );
          // Host returns snake_case; map so providerId/modelId actually land in store.
          const created =
            createdRaw && typeof createdRaw === 'object' && 'providerId' in createdRaw
              ? (createdRaw as Conversation)
              : mapWireConversation((createdRaw ?? {}) as Record<string, unknown>);
          // Prefer the selection the user just confirmed if wire fields came back empty.
          const conversation: Conversation = {
            ...created,
            providerId: created.providerId || providerId,
            modelId: created.modelId || modelId,
            projectId: created.projectId ?? activeProjectPath,
            permissionProfileId:
              created.permissionProfileId ??
              activeConversation?.permissionProfileId ??
              'ask',
          };
          const previousTempId =
            activeId && isTempConversationId(activeId) ? activeId : null;
          dispatch({ type: 'conversations/upsert', conversation });
          dispatch({ type: 'conversations/setActive', id: conversation.id });
          // Atomic temp → persisted: drop the local shell so the session appears once.
          if (previousTempId && previousTempId !== conversation.id) {
            const tempDraft = stateRef.current.composerByConversation[previousTempId];
            if (tempDraft) {
              dispatch({
                type: 'composer/set',
                conversationId: conversation.id,
                draft: tempDraft,
              });
              dispatch({ type: 'composer/clear', conversationId: previousTempId });
            }
            // Carry the temp shell's capability selection to the real conversation
            // and persist it now that a daemon-side row exists (ADR-0016).
            const tempSelection =
              stateRef.current.capabilitySelectionByConversation[previousTempId] ?? null;
            promotedCapabilitySelection = tempSelection;
            if (tempSelection) {
              dispatch({
                type: 'capabilitySelection/set',
                conversationId: conversation.id,
                selection: tempSelection,
              });
              dispatch({
                type: 'capabilitySelection/set',
                conversationId: previousTempId,
                selection: null,
              });
              if (canSelectRunCapabilities(stateRef.current.capabilities)) {
                void updateConversationCapabilities(gateway, conversation.id, tempSelection).catch(
                  () => {
                    /* run.start still carries the selection */
                  },
                );
              }
            }
            dispatch({ type: 'conversations/remove', id: previousTempId });
          }
          // Clear root-level temp shell so sidebar/remount do not resurrect it.
          setSelectedRootConversationId(conversation.id);
          setSelectedChildConversationId(null);
          publishNavigation((prev) => ({
            ...prev,
            tempSession: null,
            selectedId: conversation.id,
            activeProjectPath: activeProjectPath ?? prev.activeProjectPath,
          }));
          conversationId = conversation.id;
        }

        const result = await sendOrQueue(gateway, dispatch, stateRef.current, {
          conversationId,
          content: draft.content,
          providerId,
          modelId,
          projectPath:
            activeProjectPath ??
            stateRef.current.conversations[conversationId]?.projectId ??
            null,
          attachments: draft.attachments.map((a) => ({
            path: a.path,
            name: a.name,
            mimeType: a.mimeType,
            size: a.size,
          })),
          forceImmediate,
          // MIG-001: 不再从 localStorage 静默读 runtimePref — 运行默认值由
          // Settings V2 defaultRuntime（或引擎自身决策）唯一权威。仅当用户
          // 本次 Run 显式选择 runtime 时才通过 controller 传 explicit override。
          // Promotion happened this tick — the store lookup would still miss it.
          ...(promotedCapabilitySelection
            ? { capabilitySelection: promotedCapabilitySelection }
            : {}),
        });

        if (!result.queued && result.runId) {
          ensureRunSubscription(result.runId);
        }
        return true;
      } catch (err) {
        // Create/send failure keeps the temp page and user input intact.
        toast(classifyError(err).userMessage, 'error');
        return false;
      }
    },
    [
      activeId,
      activeConversation,
      providers,
      providerReadiness,
      gateway,
      dispatch,
      toast,
      locale,
      activeProjectPath,
      ensureRunSubscription,
      publishNavigation,
    ],
  );

  /**
   * ADR-0016 conversation capability selection: store first (immediate echo),
   * then persist to the daemon when the conversation is real and the method is
   * advertised. Temp shells persist on promotion inside handleSend.
   */
  const handleCapabilitySelectionChange = useCallback(
    async (selection: CapabilitySelection | null) => {
      const id = activeId;
      if (!id) return;
      dispatch({ type: 'capabilitySelection/set', conversationId: id, selection });
      if (isTempConversationId(id)) return;
      if (!canSelectRunCapabilities(stateRef.current.capabilities)) return;
      try {
        await updateConversationCapabilities(gateway, id, selection);
      } catch (err) {
        toast(
          `${t(locale, 'capabilities.picker.saveFailed')}: ${classifyError(err).userMessage}`,
          'error',
        );
      }
    },

    [activeId, dispatch, gateway, toast, locale],
  );

  // Conversation switch closes the picker (selection is per conversation).
  useEffect(() => {
    setCapabilityPickerOpen(false);
  }, [activeId]);

  return {
    capabilityPickerOpen,
    setCapabilityPickerOpen,
    handleSend,
    handleCapabilitySelectionChange,
  };
}
