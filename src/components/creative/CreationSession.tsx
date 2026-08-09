'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import ConversationTimeline from '@/components/ui/conversation/ConversationTimeline';
import MessageInput from '@/components/ui/conversation/MessageInput';
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
import { readActiveProject } from '@/lib/active-project';
import { mapWireProviders } from '@/lib/provider-model-selection';
import type { Conversation } from '@/lib/assistant-protocol';
import type { AssistantDraft } from '@/lib/assistant-composer';
import type { ProviderWithModels } from '@/components/ui/conversation/ModelSelectorDropdown';
import type { CreativeDraft } from '@/lib/tauri-adapter';

/** Must match CREATIVE_DRAFT_AGENT_KIND in the daemon's production.rs. */
const CREATIVE_DRAFT_SURFACE = 'creative-draft';

interface CreationSessionProps {
  draft: CreativeDraft;
  locale: Locale;
  /** Called after every model turn so the preview can pick up a new revision. */
  onDraftMayHaveChanged: () => void;
  /** 生成中状态上报：draft.state 从未被引擎驱动，预览的「生成中」以此为真源。 */
  onGeneratingChange?: (generating: boolean) => void;
  /** 预览侧软失败（渲染报错等）的回填文案；随下一条消息带给模型后消费。 */
  pendingFeedback?: string | null;
  onFeedbackConsumed?: () => void;
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
  onGeneratingChange,
  pendingFeedback,
  onFeedbackConsumed,
}: CreationSessionProps) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const { ensureRunSubscription, stop, retry, abortAllSubscriptions } = useAssistantRun();

  const [conversationId, setConversationId] = useState<string | null>(draft.conversationId ?? null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [providers, setProviders] = useState<ProviderWithModels[]>([]);
  // 用户可选模型：此前 onSelectModel 是空函数，下拉是画出来的假控件
  const [selection, setSelection] = useState<{ providerId: string; modelId: string } | null>(null);
  // 权限档同理：此前 onPermissionChange 为空 async，选择无任何效果
  const [permissionProfile, setPermissionProfile] = useState<'readonly' | 'ask' | 'full_access'>('ask');
  const stateRef = useRef(state);
  stateRef.current = state;

  const provider = (selection && providers.find((p) => p.id === selection.providerId)) ?? providers[0];
  const modelId = selection?.modelId ?? provider?.models?.[0]?.id ?? '';

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
    const projectId = await readActiveProject(window.nativesAPI);
    if (!projectId) throw new Error(locale === 'zh' ? '请先选择项目文件夹' : 'Select a project directory first');

    setCreating(true);
    try {
      const raw = await gateway.request<Record<string, unknown> | Conversation>(
        'conversation.create',
        {
          mode: 'agent',
          title: draft.name,
          provider_id: provider.id,
          model_id: modelId,
          project_id: projectId,
          permission_profile_id: permissionProfile,
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
  }, [conversationId, provider, modelId, gateway, dispatch, draft.name, draft.draftId, locale, permissionProfile]);

  const send = useCallback(
    async (composed: AssistantDraft): Promise<boolean> => {
      setError(null);
      try {
        const id = await ensureConversation();
        // The model cannot see the draft it is editing unless we say which one.
        // P0 carries it in the message; a dedicated agent binding is the eventual
        // home for this, but it must not block the first working loop.
        const feedback = pendingFeedback ? `\n[preview-error] ${pendingFeedback}` : '';
        const content = `[draft:${draft.draftId}]${feedback}\n${composed.content}`;
        if (pendingFeedback) onFeedbackConsumed?.();
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
    [ensureConversation, gateway, dispatch, draft.draftId, provider, modelId, ensureRunSubscription, pendingFeedback, onFeedbackConsumed],
  );

  // A finished turn is the only moment a new revision can exist on disk.
  const wasStreaming = useRef(isStreaming);
  useEffect(() => {
    if (wasStreaming.current && !isStreaming) onDraftMayHaveChanged();
    wasStreaming.current = isStreaming;
  }, [isStreaming, onDraftMayHaveChanged]);

  // 生成中状态上报给父级（预览 loader 的真源；draft.state 无人驱动）
  useEffect(() => {
    onGeneratingChange?.(isStreaming);
    return () => onGeneratingChange?.(false);
  }, [isStreaming, onGeneratingChange]);

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
        permissionProfile={permissionProfile}
        onPermissionChange={async (profile) => {
          setPermissionProfile(profile);
          // 会话已存在时同步到引擎（与 AssistantWorkbench 相同的持久化通道）
          if (conversationId) {
            await gateway
              .request('conversation.update_permission', {
                id: conversationId,
                permission_profile_id: profile,
              })
              .catch(() => undefined);
          }
        }}
        providers={providers}
        selectedProviderId={provider?.id ?? ''}
        selectedModel={modelId}
        onSelectModel={(providerId, model) => setSelection({ providerId, modelId: model })}
      />
    </div>
  );
}
