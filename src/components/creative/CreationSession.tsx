'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import ConversationTimeline from '@/components/assistant/ConversationTimeline';
import MessageInput from '@/components/assistant/MessageInput';
import {
  useAssistantDispatch,
  useAssistantGateway,
  useAssistantStore,
} from '@/lib/assistant-workspace';
import { connectWorkspace, openConversation, sendOrQueue } from '@/lib/assistant-workspace/controller';
import {
  selectConversationMessages,
  selectIsRunActive,
  selectRunEvents,
} from '@/lib/assistant-workspace/selectors';
import { useAssistantRun } from '@/lib/assistant-workspace/use-assistant-run';
import { mapWireConversation } from '@/lib/assistant-protocol';
import { mapWireProviders } from '@/lib/provider-model-selection';
import type { Conversation } from '@/lib/assistant-protocol';
import type { AssistantDraft } from '@/lib/assistant-composer';
import type { ProviderWithModels } from '@/components/assistant/ModelSelectorDropdown';
import type { CreativeDraft } from '@/lib/tauri-adapter';

/** Must match CREATIVE_DRAFT_AGENT_KIND in the daemon's production.rs. */
const CREATIVE_DRAFT_SURFACE = 'creative-draft';

interface CreationSessionProps {
  draft: CreativeDraft;
  locale: Locale;
  /** Called after every model turn so the preview can pick up a new revision. */
  onDraftMayHaveChanged: () => void;
}

/**
 * The conversation half of the creator workbench.
 *
 * All the AI surface here is the assistant module's — MessageInput,
 * ConversationTimeline and useAssistantRun are imported, never forked. What is
 * local is only *why* the run happens: it is bound to a draft, and the model is
 * given the draft id so its restricted tools know what to write.
 */
export default function CreationSession({
  draft,
  locale,
  onDraftMayHaveChanged,
}: CreationSessionProps) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const { ensureRunSubscription, stop, retry, abortAllSubscriptions } = useAssistantRun();

  const [conversationId, setConversationId] = useState<string | null>(draft.conversationId ?? null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [providers, setProviders] = useState<ProviderWithModels[]>([]);
  const stateRef = useRef(state);
  stateRef.current = state;

  const provider = providers[0];
  const modelId = provider?.models?.[0]?.id ?? '';

  const storeMessages = conversationId ? selectConversationMessages(state, conversationId) : [];
  const messages = useMemo(
    () =>
      storeMessages.map((m) => ({
        id: m.id,
        role: m.role as 'system' | 'user' | 'assistant',
        contentBlocks: m.contentBlocks,
        status: m.status,
        createdAt: m.createdAt,
        runId: m.runId ?? null,
        inputTokens: m.inputTokens,
        outputTokens: m.outputTokens,
      })),
    [storeMessages],
  );
  const activeRunId = conversationId ? (state.activeRunByConversation?.[conversationId] ?? null) : null;
  const isStreaming = conversationId ? selectIsRunActive(state, conversationId) : false;
  const eventsByRun = activeRunId ? { [activeRunId]: selectRunEvents(state, activeRunId) } : {};

  // This surface owns its own store, so it must also own the connection to the
  // engine — nothing else will have connected this gateway.
  useEffect(() => {
    void connectWorkspace(gateway, dispatch).catch((err) => {
      setError(err instanceof Error ? err.message : String(err));
    });
    return () => {
      abortAllSubscriptions();
      void gateway.disconnect();
    };
  }, [gateway, dispatch, abortAllSubscriptions]);

  // Adopt the conversation the draft already points at, so reopening a draft
  // shows its history instead of starting a blank thread.
  useEffect(() => {
    if (!conversationId) return;
    void openConversation(gateway, dispatch, conversationId).catch(() => {
      // A conversation deleted out from under the draft should not wedge the
      // pane; the next message creates a fresh one.
      setConversationId(null);
    });
  }, [conversationId, gateway, dispatch]);

  useEffect(() => {
    ensureRunSubscription(activeRunId);
  }, [activeRunId, ensureRunSubscription]);

  // Model and credential choice stay the provider module's authority; this
  // surface only reads the list, it never persists a second copy of it.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await gateway.request<unknown>('provider.list', {});
        if (!cancelled) setProviders(mapWireProviders(list) as ProviderWithModels[]);
      } catch {
        if (!cancelled) setProviders([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [gateway]);

  const ensureConversation = useCallback(async (): Promise<string> => {
    if (conversationId) return conversationId;
    if (!provider || !modelId) throw new Error(t(locale, 'creative.session.noModel'));

    setCreating(true);
    try {
      const raw = await gateway.request<Record<string, unknown> | Conversation>(
        'conversation.create',
        {
          mode: 'agent',
          title: draft.name,
          provider_id: provider.id,
          model_id: modelId,
          permission_profile_id: 'ask',
        },
      );
      const created =
        raw && typeof raw === 'object' && 'providerId' in raw
          ? (raw as Conversation)
          : mapWireConversation((raw ?? {}) as Record<string, unknown>);
      dispatch({ type: 'conversations/upsert', conversation: created });
      // Persist the link so reopening this draft restores its history, and so
      // the draft tools' cross-session guard has an owner to compare against.
      // A failure here costs history, not the turn — do not block the send.
      await window.nativesAPI?.creativeDraft
        ?.bindConversation(draft.draftId, created.id)
        .catch(() => undefined);
      setConversationId(created.id);
      return created.id;
    } finally {
      setCreating(false);
    }
  }, [conversationId, provider, modelId, gateway, dispatch, draft.name, locale]);

  const send = useCallback(
    async (composed: AssistantDraft): Promise<boolean> => {
      setError(null);
      try {
        const id = await ensureConversation();
        // The model cannot see the draft it is editing unless we say which one.
        // P0 carries it in the message; a dedicated agent binding is the eventual
        // home for this, but it must not block the first working loop.
        const content = `[draft:${draft.draftId}]\n${composed.content}`;
        const result = await sendOrQueue(gateway, dispatch, stateRef.current, {
          conversationId: id,
          content,
          providerId: provider?.id ?? '',
          modelId,
          attachments: composed.attachments,
          // Selects the restricted draft tool surface in the daemon; without it
          // the model gets the general tools and cannot touch drafts at all.
          agentProfileId: CREATIVE_DRAFT_SURFACE,
        });
        if (result?.runId) ensureRunSubscription(result.runId);
        return true;
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        return false;
      }
    },
    [ensureConversation, gateway, dispatch, draft.draftId, provider, modelId, ensureRunSubscription],
  );

  // A finished turn is the only moment a new revision can exist on disk.
  const wasStreaming = useRef(isStreaming);
  useEffect(() => {
    if (wasStreaming.current && !isStreaming) onDraftMayHaveChanged();
    wasStreaming.current = isStreaming;
  }, [isStreaming, onDraftMayHaveChanged]);

  return (
    <div className="flex h-full flex-col">
      <ConversationTimeline
        messages={messages}
        loading={creating}
        locale={locale}
        eventsByRun={eventsByRun}
        onRetry={activeRunId ? () => void retry(activeRunId) : undefined}
      />
      {error && (
        <p className="px-4 py-2 text-sm text-red-600 dark:text-red-400" role="alert">
          {error}
        </p>
      )}
      <MessageInput
        locale={locale}
        onSend={send}
        onStop={() => {
          if (activeRunId) void stop(activeRunId);
        }}
        isStreaming={isStreaming}
        disabled={!provider}
        inputDisabledReason={!provider ? 'no_provider' : null}
        permissionProfile="ask"
        onPermissionChange={async () => {}}
        providers={providers}
        selectedProviderId={provider?.id ?? ''}
        selectedModel={modelId}
        onSelectModel={() => {
          // Model choice belongs to the provider module; the creator surface
          // follows the assistant's selection rather than keeping its own.
        }}
      />
    </div>
  );
}
