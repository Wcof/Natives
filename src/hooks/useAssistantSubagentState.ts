'use client';

import { useCallback, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from '@/lib/assistant-workspace';
// W4: useToast from lib — hooks never depend on component internals.
import { useToast } from '@/lib/toast-context';
import { classifyError } from '@/lib/error-classifier';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import {
  isTempConversationId,
} from '@/lib/assistant-temp-conversation';
import type {
  Conversation,
  SubagentAssignmentInteraction,
  SubagentSession,
} from '@/lib/assistant-protocol';
// W4: shared types from lib — hooks never depend on component internals.
import type { ProviderWithModels } from '@/lib/assistant-ui-types';
import type {
  AssignmentKeyOption,
  SubagentAssignmentConfirmPayload,
} from '@/lib/subagent-assignment-types';
import type { ProviderKeySummary } from '@/lib/tauri-adapter';

/** Map a wire `subagent.list` payload to the typed session list used by the UI. */
export function mapSubagentSessions(raw: unknown): SubagentSession[] {
  const sessionsRaw =
    raw && typeof raw === 'object' && Array.isArray((raw as { sessions?: unknown }).sessions)
      ? (raw as { sessions: unknown[] }).sessions
      : Array.isArray(raw)
        ? raw
        : [];
  return sessionsRaw
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
    .map((r) => ({
      id: String(r.id ?? ''),
      parentConversationId: String(
        r.parent_conversation_id ?? r.parentConversationId ?? '',
      ),
      childConversationId: String(
        r.child_conversation_id ?? r.childConversationId ?? '',
      ),
      parentRunId:
        r.parent_run_id != null || r.parentRunId != null
          ? String(r.parent_run_id ?? r.parentRunId)
          : null,
      taskCallId:
        r.task_call_id != null || r.taskCallId != null
          ? String(r.task_call_id ?? r.taskCallId)
          : null,
      name: String(r.name ?? r.task ?? r.id ?? ''),
      task: String(r.task ?? ''),
      status: String(r.status ?? 'open'),
      providerId: String(r.provider_id ?? r.providerId ?? ''),
      keyId: String(r.key_id ?? r.keyId ?? ''),
      modelId: String(r.model_id ?? r.modelId ?? ''),
      lastActivityAt:
        r.last_activity_at != null || r.lastActivityAt != null
          ? String(r.last_activity_at ?? r.lastActivityAt)
          : undefined,
      closedAt:
        r.closed_at != null || r.closedAt != null
          ? String(r.closed_at ?? r.closedAt)
          : null,
      error: r.error != null ? String(r.error) : null,
      createdAt:
        r.created_at != null || r.createdAt != null
          ? String(r.created_at ?? r.createdAt)
          : undefined,
      updatedAt:
        r.updated_at != null || r.updatedAt != null
          ? String(r.updated_at ?? r.updatedAt)
          : undefined,
    }))
    .filter((s) => s.id.length > 0);
}

export interface UseAssistantSubagentStateOptions {
  rootConversationId: string | null;
  /** Pending subagent_assignment interaction (modal open driver), if any. */
  subagentAssignment: SubagentAssignmentInteraction | undefined;
  providers: ProviderWithModels[];
  activeConversation: Conversation | null;
  rootConversation: Conversation | null;
  locale: Locale;
  ensureRunSubscription: (runId: string | null | undefined) => void;
  setLoadingMessages: Dispatch<SetStateAction<boolean>>;
  setSelectedChildConversationId: Dispatch<SetStateAction<string | null>>;
}

/**
 * Subagent session / key-assignment state for the workbench.
 *
 * Owns the single class of "who is running as a subagent and on which key":
 * the session list, the switch-key modal state, the assignment key pool, and
 * the handlers that select / route / confirm subagent sessions.
 */
export function useAssistantSubagentState({
  rootConversationId,
  subagentAssignment,
  providers,
  activeConversation,
  rootConversation,
  locale,
  ensureRunSubscription,
  setLoadingMessages,
  setSelectedChildConversationId,
}: UseAssistantSubagentStateOptions) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const { toast } = useToast();

  const stateRef = useRef(state);
  stateRef.current = state;

  const [subagentSessions, setSubagentSessions] = useState<SubagentSession[]>([]);
  const [switchKeySessionId, setSwitchKeySessionId] = useState<string | null>(null);
  const [assignmentKeyOptions, setAssignmentKeyOptions] = useState<AssignmentKeyOption[]>([]);

  const refreshSubagentSessions = useCallback(
    async (parentConversationId: string | null | undefined) => {
      if (!parentConversationId || isTempConversationId(parentConversationId)) {
        setSubagentSessions([]);
        return;
      }
      try {
        const raw = await gateway.request<unknown>('subagent.list', {
          conversation_id: parentConversationId,
          include_closed: true,
        });
        setSubagentSessions(mapSubagentSessions(raw));
      } catch {
        // Method may be unavailable on older daemons — fall back to child runs.
      }
    },
    [gateway],
  );

  const loadAssignmentKeys = useCallback(async () => {
    const preferredProviderId = activeConversation?.providerId ?? rootConversation?.providerId ?? '';
    const preferredModelId = activeConversation?.modelId ?? rootConversation?.modelId ?? '';
    try {
      const list = await window.nativesAPI?.provider?.list?.();
      if (!Array.isArray(list)) {
        // Fall back to gateway provider list (has_active_key only; no status).
        const opts: AssignmentKeyOption[] = [];
        for (const p of providers) {
          if (!p.keys?.length) continue;
          for (const k of p.keys) {
            opts.push({
              providerId: p.id,
              providerName: p.name,
              keyId: k.id,
              keyLabel: k.label || k.maskedKey || k.id,
              modelId: p.id === preferredProviderId ? (preferredModelId || p.defaultModel || p.models?.[0]?.id || '') : (p.defaultModel || p.models?.[0]?.id || ''),
              models: (p.models ?? []).map((m) => ({
                id: m.id,
                displayName: m.displayName,
              })),
              isActive: true,
              status: 'valid',
            });
          }
        }
        opts.sort((left, right) => Number(right.providerId === preferredProviderId) - Number(left.providerId === preferredProviderId));
        setAssignmentKeyOptions(opts);
        return;
      }
      const opts: AssignmentKeyOption[] = [];
      for (const p of list) {
        const provider = p as {
          id: string;
          displayName?: string;
          name?: string;
          defaultModel?: string | null;
          models?: Array<{ id: string; displayName?: string | null }>;
          keys?: Array<
            Partial<ProviderKeySummary> & {
              id: string;
              label?: string;
              maskedKey?: string;
              isActive?: boolean;
              status?: ProviderKeySummary['status'] | string;
            }
          >;
        };
        const models = (provider.models ?? []).map((m) => ({
          id: m.id,
          displayName: m.displayName ?? undefined,
        }));
        for (const k of provider.keys ?? []) {
          // Only active + validated keys for random/custom; keep others out of the pool.
          const active = k.isActive !== false;
          if (!active) continue;
          if (k.status != null && k.status !== 'valid') continue;
          opts.push({
            providerId: provider.id,
            providerName: provider.displayName || provider.name || provider.id,
            keyId: k.id,
            keyLabel: k.label || k.maskedKey || k.id,
            modelId: provider.id === preferredProviderId ? (preferredModelId || provider.defaultModel || models[0]?.id || '') : (provider.defaultModel || models[0]?.id || ''),
            models,
            isActive: true,
            status: (k.status as AssignmentKeyOption['status']) ?? 'valid',
          });
        }
      }
      opts.sort((left, right) => Number(right.providerId === preferredProviderId) - Number(left.providerId === preferredProviderId));
      setAssignmentKeyOptions(opts);
    } catch {
      setAssignmentKeyOptions([]);
    }
  }, [providers, activeConversation?.providerId, activeConversation?.modelId, rootConversation?.providerId, rootConversation?.modelId]);

  const handleSelectSubagent = useCallback(
    async (id: string) => {
      const session = subagentSessions.find((s) => s.id === id);
      const childId = session?.childConversationId;
      if (!childId) {
        // Legacy child-run id without session row — still mark selection for tasks panel.
        setSelectedChildConversationId(null);
        return;
      }
      setSelectedChildConversationId(childId);
      setLoadingMessages(true);
      try {
        // Optional selective touch only when user focuses a specific subagent.
        void gateway
          .request('subagent.touch', { id: session.id })
          .catch(() => undefined);
        // Load hidden child conversation history without flipping sidebar root.
        const snapshot = await gateway.getSnapshot(childId);
        dispatch({ type: 'snapshot/apply', snapshot });
        ensureRunSubscription(stateRef.current.activeRunByConversation[childId]);
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
      } finally {
        setLoadingMessages(false);
      }
    },
    [subagentSessions, gateway, dispatch, toast, ensureRunSubscription, setLoadingMessages, setSelectedChildConversationId],
  );

  const handleBackToMain = useCallback(() => {
    setSelectedChildConversationId(null);
  }, [setSelectedChildConversationId]);

  const handleAssignmentConfirm = useCallback(
    async (payload: SubagentAssignmentConfirmPayload) => {
      const wireAssignments = payload.assignments.map((a) => ({
        call_id: a.callId,
        provider_id: a.providerId,
        key_id: a.keyId,
        model_id: a.modelId,
      }));
      const wirePool = payload.pool.map((b) => ({
        provider_id: b.providerId,
        key_id: b.keyId,
        model_id: b.modelId,
      }));
      const wireBindings = payload.bindings.map((b) => ({
        provider_id: b.providerId,
        key_id: b.keyId,
        model_id: b.modelId,
      }));

      if (payload.sessionId) {
        const result = (await gateway.request('subagent.switchRoute', {
          conversation_id: rootConversationId,
          session_id: payload.sessionId,
          mode: payload.mode,
          bindings: wireBindings,
          assignments: wireAssignments,
          pool: wirePool,
        })) as { restarted_run_id?: string; restartedRunId?: string } | null;
        setSwitchKeySessionId(null);
        const restarted =
          result?.restarted_run_id ?? result?.restartedRunId ?? null;
        if (restarted) {
          ensureRunSubscription(restarted);
        }
        void refreshSubagentSessions(rootConversationId);
        return;
      }
      if (!subagentAssignment) {
        throw new Error(t(locale, 'assistant.subagentAssignment.errorFallback'));
      }
      await gateway.request('interaction.respond', {
        id: subagentAssignment.id,
        run_id: subagentAssignment.runId,
        response: {
          approved: true,
          mode: payload.mode,
          assignments: wireAssignments,
          pool: wirePool,
          // Legacy flat bindings still accepted by older daemons.
          bindings: wireBindings,
          conversation_id:
            subagentAssignment.conversationId ?? rootConversationId ?? undefined,
        },
      });
      dispatch({ type: 'interaction/remove', id: subagentAssignment.id });
      void refreshSubagentSessions(rootConversationId);
    },
    [
      gateway,
      rootConversationId,
      subagentAssignment,
      dispatch,
      refreshSubagentSessions,
      locale,
      ensureRunSubscription,
    ],
  );

  return {
    subagentSessions,
    setSubagentSessions,
    switchKeySessionId,
    setSwitchKeySessionId,
    assignmentKeyOptions,
    setAssignmentKeyOptions,
    refreshSubagentSessions,
    loadAssignmentKeys,
    handleSelectSubagent,
    handleBackToMain,
    handleAssignmentConfirm,
  };
}
