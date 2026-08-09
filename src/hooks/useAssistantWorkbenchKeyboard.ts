'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from '@/lib/assistant-workspace';
import {
  ASSISTANT_LOCATE_EVENT,
  type AssistantLocateTarget,
} from '@/lib/assistant-notifications';
import {
  collectTempConversationIds,
  createTempSession,
  isTempConversationId,
} from '@/lib/assistant-temp-conversation';
import { selectAssistantModel, toProviderInfo } from '@/lib/provider-model-selection';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import type { Conversation } from '@/lib/assistant-protocol';
// W4: shared types from lib — hooks never depend on component internals.
import type { ProviderWithModels, AssistantCommand, AssistantNavigationSnapshot } from '@/lib/assistant-ui-types';

export interface UseAssistantWorkbenchKeyboardOptions {
  activeId: string | null;
  activeRunId: string | undefined;
  activeRunStatus: string | undefined;
  isStreaming: boolean;
  providers: ProviderWithModels[];
  activeProjectPath: string | null;
  locale: Locale;
  handleStop: () => void;
  handleRetry: () => void;
  ensureRunSubscription: (runId: string | null | undefined) => void;
  selectConversation: (id: string) => void;
  publishNavigation: (
    updater:
      | AssistantNavigationSnapshot
      | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
  ) => void;
  setRightPanelOpen: (open: boolean) => void;
}

/**
 * Keyboard / command palette ownership for the workbench.
 *
 * Owns the single class of "how the user drives the workbench by keyboard":
 * the palette-open flag, the global keydown shortcuts (incl. the notification
 * deep-link listener), and the command palette entries.
 */
export function useAssistantWorkbenchKeyboard({
  activeId,
  activeRunId,
  activeRunStatus,
  isStreaming,
  providers,
  activeProjectPath,
  locale,
  handleStop,
  handleRetry,
  ensureRunSubscription,
  selectConversation,
  publishNavigation,
  setRightPanelOpen,
}: UseAssistantWorkbenchKeyboardOptions) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();

  const [paletteOpen, setPaletteOpen] = useState(false);

  // Read-through ref so command handlers always see the latest store state.
  const stateRef = useRef(state);
  stateRef.current = state;

  // Keyboard shortcuts + notification deep-link
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;
      if (meta && e.key === '.') {
        e.preventDefault();
        void handleStop();
      }
      if (meta && e.shiftKey && e.key.toLowerCase() === 'p') {
        e.preventDefault();
        setPaletteOpen(true);
      }
      if (meta && e.shiftKey && e.key.toLowerCase() === 'i') {
        e.preventDefault();
        setRightPanelOpen(true);
        dispatch({ type: 'view/patch', patch: { rightCollapsed: false, inspectorTab: 'run' } });
      }
      if (meta && e.shiftKey && e.key.toLowerCase() === 't') {
        e.preventDefault();
        setRightPanelOpen(true);
        dispatch({ type: 'view/patch', patch: { rightCollapsed: false, inspectorTab: 'tasks' } });
      }
      if (e.key === 'Escape') {
        // close popovers only — never cancel run
        setPaletteOpen(false);
      }
    };
    const onLocate = (ev: Event) => {
      const detail = (ev as CustomEvent<AssistantLocateTarget>).detail;
      if (!detail?.conversationId) return;
      void selectConversation(detail.conversationId);
      if (detail.interactionId) {
        // focus interaction by leaving it in queue — card already binds run
      }
      setRightPanelOpen(true);
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener(ASSISTANT_LOCATE_EVENT, onLocate as EventListener);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener(ASSISTANT_LOCATE_EVENT, onLocate as EventListener);
    };
  }, [handleStop, dispatch, selectConversation, setRightPanelOpen]);

  const commands: AssistantCommand[] = useMemo(() => {
    const busy = isStreaming;
    return [
      {
        id: 'new',
        label: t(locale, 'assistant.commandNewConversation'),
        run: () => {
          for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
            dispatch({ type: 'conversations/remove', id: oldId });
            dispatch({ type: 'composer/clear', conversationId: oldId });
          }
          const pick = selectAssistantModel(toProviderInfo(providers));
          const session = createTempSession({
            projectId: activeProjectPath,
            title: t(locale, 'assistant.newConversation'),
            providerId: pick?.providerId,
            modelId: pick?.modelId,
          });
          dispatch({ type: 'conversations/upsert', conversation: session.conversation });
          dispatch({ type: 'conversations/setActive', id: session.conversation.id });
          publishNavigation((prev) => ({
            ...prev,
            selectedId: session.conversation.id,
            tempSession: session,
            pendingCreateProjectPath: undefined,
          }));
        },
      },      {
        id: 'stop',
        label: t(locale, 'assistant.stopRun'),
        shortcut: '⌘.',
        disabledReason: busy ? undefined : t(locale, 'assistant.noRunningTask'),
        run: () => void handleStop(),
      },
      {
        id: 'retry',
        label: t(locale, 'assistant.retry'),
        disabledReason: activeRunStatus === 'failed' || activeRunStatus === 'interrupted'
          ? undefined
          : t(locale, 'assistant.retryOnlyFailed'),
        run: () => void handleRetry(),
      },
      {
        id: 'inspector',
        label: t(locale, 'assistant.openInspector'),
        shortcut: '⌘⇧I',
        run: () => {
          setRightPanelOpen(true);
          dispatch({ type: 'view/patch', patch: { rightCollapsed: false } });
        },
      },
      {
        id: 'tasks',
        label: t(locale, 'assistant.tasksPanel'),
        shortcut: '⌘⇧T',
        run: () => {
          setRightPanelOpen(true);
          dispatch({ type: 'view/patch', patch: { inspectorTab: 'tasks', rightCollapsed: false } });
        },
      },
      {
        id: 'background',
        label: t(locale, 'assistant.backgroundRun'),
        disabledReason: busy ? undefined : t(locale, 'assistant.noActiveRun'),
        run: () => {
          // Background mode is represented by the live subscription/event stream;
          // it is not a separate engine RPC.
          if (activeRunId) {
            ensureRunSubscription(activeRunId);
          }
        },
      },
      {
        id: 'fork',
        label: t(locale, 'assistant.forkConversation'),
        disabledReason: activeId && !isTempConversationId(activeId) ? undefined : t(locale, 'assistant.noConversation'),
        run: () => {
          if (!activeId || isTempConversationId(activeId)) return;
          void gateway.request('conversation.fork', { conversation_id: activeId }).then((forked) => {
            const c = forked as Conversation;
            if (c?.id) {
              dispatch({ type: 'conversations/upsert', conversation: c });
              void selectConversation(c.id);
            }
          });
        },
      },
    ];
  }, [
    isStreaming,
    // Selector-derived primitives are immutable for this render.

    activeRunId,

    activeRunStatus,
    handleStop,
    handleRetry,
    dispatch,
    gateway,
    activeId,
    selectConversation,
    providers,
    locale,
    activeProjectPath,
    publishNavigation,
    ensureRunSubscription,
  ]);

  return {
    paletteOpen,
    setPaletteOpen,
    commands,
  };
}
