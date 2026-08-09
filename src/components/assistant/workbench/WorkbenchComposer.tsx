'use client';

import { Suspense, lazy, useMemo, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import {
  selectComposerDraft,
  selectPendingInteractions,
  useAssistantDispatch,
  useAssistantGateway,
  useAssistantStore,
} from '@/lib/assistant-workspace';
import {
  canInterject,
  canSelectRunCapabilities,
  hasMethod,
} from '@/lib/assistant-workspace/capability-gate';
import { parsePlanApprovalRequest, type PlanApproval } from '../plan-approval';
import { COMPOSER_COLUMN_CLASS } from '../InteractionPromptShell';
import MessageInput, { type ComposerSubagent } from '@/components/ui/conversation/MessageInput';
import PromptQueuePanel from '../PromptQueuePanel';
import PermissionRequestCard from '../PermissionRequestCard';
import AskUserPromptCard from '../AskUserPromptCard';
// ADR-0016 capability picker — lazy so it stays out of the initial bundle (R-P7).
const LazyCapabilityPickerPopover = lazy(() => import('@/components/ui/capability/CapabilityPickerPopover'));
// R-P7: the plan checklist is a low-frequency surface. The predicate that
// decides whether to show it is eager (plain module above); only the renderer
// is split out.
const PlanApprovalCard = lazy(() => import('../PlanApprovalCard'));
import { useAssistantWorkbenchComposer } from '@/hooks/useAssistantWorkbenchComposer';
import {
  normalizePermissionProfile,
  type AssistantPermissionProfile,
} from '@/lib/assistant-composer';
import {
  createTempConversationShell,
  createTempSession,
  isTempConversationId,
} from '@/lib/assistant-temp-conversation';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import type { Conversation, PromptQueueItem } from '@/lib/assistant-protocol';
import type { ConversationChangeSummary } from '@/lib/assistant-timeline';
import type { ModelSelection, ProviderReadiness } from '@/lib/provider-model-selection';
import type { ProviderWithModels } from '@/components/ui/conversation/ModelSelectorDropdown';
import type { AssistantNavigationSnapshot } from '../AssistantWorkspaceContext';

export interface WorkbenchComposerProps {
  locale: Locale;
  activeId: string | null;
  activeConversation: Conversation | null;
  activeProjectPath: string | null;
  providers: ProviderWithModels[];
  providerReadiness: ProviderReadiness;
  registeredProjects: Array<{
    id: string;
    path: string;
    lastOpenedAt?: string | null;
    label?: string;
    exists?: boolean;
  }>;
  /** Root conversation id for interaction lookups (batch waiters are parent-scoped). */
  rootConversationId: string | null;
  modelSelection: ModelSelection | null;
  ensureRunSubscription: (runId: string | null | undefined) => void;
  publishNavigation: (
    updater:
      | AssistantNavigationSnapshot
      | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
  ) => void;
  setSelectedRootConversationId: Dispatch<SetStateAction<string | null>>;
  setSelectedChildConversationId: Dispatch<SetStateAction<string | null>>;
  isGoalMode: boolean;
  isStreaming: boolean;
  promptQueue: PromptQueueItem[];
  stoppingRunId: string | null;
  activeRunId: string | undefined;
  handlePermission: (requestId: string, approved: boolean, scope?: string) => Promise<void>;
  onStop: () => void;
  conversationChangeSummary: ConversationChangeSummary;
  composerSubagents: ComposerSubagent[];
  activeComposerSubagent: ComposerSubagent | null;
  onSelectSubagent: (id: string) => void;
}

/**
 * Composer pane for the workbench: prompt queue, permission/ask-user overlays,
 * and the message input with the ADR-0016 capability picker.
 *
 * Owns `useAssistantWorkbenchComposer` (send path + picker state) so the shell
 * stays a pure orchestrator; interaction lookups and the picker gate are read
 * from the store here.
 */
export function WorkbenchComposer({
  locale,
  activeId,
  activeConversation,
  activeProjectPath,
  providers,
  providerReadiness,
  registeredProjects,
  rootConversationId,
  modelSelection,
  ensureRunSubscription,
  publishNavigation,
  setSelectedRootConversationId,
  setSelectedChildConversationId,
  isGoalMode,
  isStreaming,
  promptQueue,
  stoppingRunId,
  activeRunId,
  handlePermission,
  onStop,
  conversationChangeSummary,
  composerSubagents,
  activeComposerSubagent,
  onSelectSubagent,
}: WorkbenchComposerProps) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const { toast } = useToast();

  const stateRef = useRef(state);
  stateRef.current = state;

  const {
    capabilityPickerOpen,
    setCapabilityPickerOpen,
    handleSend,
    handleCapabilitySelectionChange,
  } = useAssistantWorkbenchComposer({
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
  });

  // Permissions / assignments for the root conversation (batch waiters are parent-scoped).
  const interactions = useMemo(
    () => selectPendingInteractions(state, rootConversationId),
    [state, rootConversationId],
  );
  const permission = interactions.find((i) => i.kind === 'permission');
  /**
   * A submitted plan reaches the GUI on the permission channel (`tool_name =
   * exit_plan_mode`), so it is the same pending interaction as any tool prompt —
   * only the payload tells it apart. Parsed here so the render below is a plain
   * branch.
   */
  const pendingPlanApproval: PlanApproval | null =
    permission && permission.kind === 'permission'
      ? parsePlanApprovalRequest(permission.toolName, permission.input)
      : null;
  const askUser = interactions.find((i) => i.kind === 'ask_user');

  // Composer-blocking interactions: hide MessageInput so the user cannot type/stop/send.
  const composerBlockedByInteraction = Boolean(
    (permission && permission.kind === 'permission') ||
      (askUser && askUser.kind === 'ask_user'),
  );

  const permissionProfile = normalizePermissionProfile(
    activeConversation?.permissionProfileId ?? 'ask',
  );
  // ADR-0016 composer capability picker (honest gate on conversation.updateCapabilities).
  const capabilityPickerEnabled = canSelectRunCapabilities(state.capabilities);
  const activeCapabilitySelection = activeId
    ? state.capabilitySelectionByConversation[activeId] ?? null
    : null;
  const capabilityCount = activeCapabilitySelection
    ? (activeCapabilitySelection.mcp_servers?.length ?? 0) +
      (activeCapabilitySelection.expert_id ? 1 : 0) +
      (activeCapabilitySelection.team_id ? 1 : 0)
    : 0;

  return (
    <>
      {/* Prompt queue is for in-flight multi-send, not goal chrome. */}
      <PromptQueuePanel
        items={isGoalMode ? [] : promptQueue}
        locale={locale}
        onEdit={(id, content) =>
          void gateway
            .request('promptQueue.update', {
              id,
              conversation_id: activeId,
              content,
            })
            .then(() =>
              gateway
                .request('promptQueue.list', { conversation_id: activeId })
                .then((items) =>
                  dispatch({
                    type: 'promptQueue/set',
                    conversationId: activeId!,
                    items: items as typeof promptQueue,
                  }),
                ),
            )
        }
        onRemove={(id) =>
          void gateway.request('promptQueue.remove', { id }).then(() =>
            dispatch({
              type: 'promptQueue/set',
              conversationId: activeId!,
              items: promptQueue.filter((i) => i.id !== id),
            }),
          )
        }
        onSendNow={(id) => void gateway.request('promptQueue.sendNow', { id })}
        onReorder={(ids) =>
          void gateway.request('promptQueue.reorder', {
            conversation_id: activeId,
            ids,
          })
        }
      />

      {composerBlockedByInteraction ? (
        <div className={`${COMPOSER_COLUMN_CLASS} pb-5 pt-2`} data-composer-interaction-overlay>
          {permission && permission.kind === 'permission' ? (
            // The plan checklist is a second rendering of the same pending
            // permission, so it is gated on the RPC it would answer with.
            // When `permission.respond` is not advertised the generic card
            // still renders: the composer is already hidden behind this
            // overlay, and showing nothing would wedge the run with no way
            // for the user to answer at all.
            pendingPlanApproval && hasMethod(state.capabilities, 'permission.respond') ? (
              <Suspense
                fallback={
                  <div
                    className="rounded-[var(--radius-lg,14px)] border border-[var(--border-subtle)] bg-[var(--surface)] px-4 py-3 text-xs text-[var(--text-secondary)]"
                    data-plan-approval-loading
                  >
                    {t(locale, 'assistant.permission.processing')}
                  </div>
                }
              >
                <PlanApprovalCard
                  requestId={permission.id}
                  approval={pendingPlanApproval}
                  locale={locale}
                  // Must return the Promise so the card can await + recover on failure.
                  onApprove={(id, scope) => handlePermission(id, true, scope)}
                  onReject={(id) => handlePermission(id, false)}
                />
              </Suspense>
            ) : (
              <PermissionRequestCard
                request={{
                  id: permission.id,
                  toolName: permission.toolName,
                  reason: permission.reason,
                  input: permission.input,
                  status: 'pending',
                  createdAt: permission.createdAt,
                }}
                locale={locale}
                onApprove={(id, scope) => {
                  // Must return the Promise so the card can await + recover on failure.
                  return handlePermission(id, true, scope);
                }}
                onReject={(id) => handlePermission(id, false)}
              />
            )
          ) : null}
          {askUser && askUser.kind === 'ask_user' ? (
            <AskUserPromptCard
              interaction={askUser}
              locale={locale}
              onAnswer={async (id, answer) => {
                await gateway.request('interaction.respond', {
                  id,
                  answer,
                  run_id: askUser.runId,
                });
                dispatch({ type: 'interaction/remove', id });
              }}
              onCancel={async (id) => {
                await gateway.request('interaction.respond', {
                  id,
                  cancelled: true,
                  run_id: askUser.runId,
                });
                dispatch({ type: 'interaction/remove', id });
              }}
            />
          ) : null}
        </div>
      ) : (
        <MessageInput
          locale={locale}
          onSend={(draft) => handleSend(draft, false)}
          onForceSend={(draft) => handleSend(draft, true)}
          onInterject={
            canInterject(state.capabilities) && isStreaming && activeId && !isTempConversationId(activeId)
              ? async (content) => {
                  try {
                    await gateway.request('promptQueue.interject', {
                      conversation_id: activeId,
                      content,
                    });
                    return true;
                  } catch (err) {
                    toast(classifyError(err).userMessage, 'error');
                    return false;
                  }
                }
              : undefined
          }
          onStop={onStop}
          isStopping={stoppingRunId === activeRunId}
          isStreaming={isStreaming}
          allowQueueWhileStreaming
          inputDisabledReason={
            providerReadiness === 'no_provider'
              ? 'no_provider'
              : providerReadiness === 'no_model'
                ? 'no_model'
                : null
          }
          permissionProfile={permissionProfile}
          onPermissionChange={async (profile: AssistantPermissionProfile) => {
            // Always update local conversation state so the picker reflects the choice.
            if (activeConversation) {
              dispatch({
                type: 'conversations/upsert',
                conversation: { ...activeConversation, permissionProfileId: profile },
              });
            } else if (activeId) {
              // Temp conversation shell without full object — create minimal patch via store.
              const existing = stateRef.current.conversations[activeId];
              if (existing) {
                dispatch({
                  type: 'conversations/upsert',
                  conversation: { ...existing, permissionProfileId: profile },
                });
              }
            }
            // Persist only when the conversation is real on the host.
            if (!activeId || isTempConversationId(activeId)) return;
            try {
              await gateway.request('conversation.update_permission', {
                id: activeId,
                permission_profile_id: profile,
              });
            } catch (err) {
              toast(classifyError(err).userMessage, 'error');
            }
          }}
          providers={providers}
          selectedProviderId={modelSelection?.providerId ?? providers[0]?.id ?? ''}
          selectedModel={modelSelection?.modelId}
          onSelectModel={(providerId, modelId) => {
            const now = new Date().toISOString();
            // Always write selection into store — even without an active conversation —
            // so the picker echoes immediately and temp shells stay editable.
            if (activeConversation) {
              dispatch({
                type: 'conversations/upsert',
                conversation: {
                  ...activeConversation,
                  providerId,
                  modelId,
                  updatedAt: now,
                },
              });
            } else if (activeId) {
              const existing = stateRef.current.conversations[activeId];
              if (existing) {
                dispatch({
                  type: 'conversations/upsert',
                  conversation: { ...existing, providerId, modelId, updatedAt: now },
                });
              } else {
                const shell = createTempConversationShell({
                  id: activeId,
                  projectId: activeProjectPath,
                  title: t(locale, 'assistant.newConversation'),
                  providerId,
                  modelId,
                  now,
                });
                dispatch({ type: 'conversations/upsert', conversation: shell });
              }
            } else {
              // No conversation yet: create a temp shell so selection has a home.
              const session = createTempSession({
                projectId: activeProjectPath,
                title: t(locale, 'assistant.newConversation'),
                providerId,
                modelId,
                now,
              });
              dispatch({ type: 'conversations/upsert', conversation: session.conversation });
              dispatch({ type: 'conversations/setActive', id: session.conversation.id });
              publishNavigation((prev) => ({
                ...prev,
                selectedId: session.conversation.id,
                tempSession: session,
              }));
            }
            if (activeId && !isTempConversationId(activeId)) {
              void gateway
                .request('conversation.update_model', {
                  id: activeId,
                  provider_id: providerId,
                  model_id: modelId,
                })
                .catch((err) => toast(classifyError(err).userMessage, 'error'));
            }
          }}
          draftText={selectComposerDraft(state, activeId).text}
          draftKey={activeId}
          onDraftChange={(text, conversationId) => {
            // Prefer the id captured at keystroke time so a switch mid-debounce
            // still writes the previous conversation's draft.
            const id = conversationId ?? activeId;
            if (!id) return;
            dispatch({ type: 'composer/set', conversationId: id, draft: { text } });
            // Temp shell remount restore (settings round-trip). Debounced by
            // MessageInput; publishNavigation bails without re-rendering the
            // shell tree when only tempSession.draft.text changes.
            if (isTempConversationId(id)) {
              publishNavigation((prev) => {
                if (!prev.tempSession || prev.tempSession.conversation.id !== id) {
                  return prev;
                }
                if (prev.tempSession.draft.text === text) return prev;
                return {
                  ...prev,
                  tempSession: {
                    ...prev.tempSession,
                    draft: {
                      ...prev.tempSession.draft,
                      text,
                      updatedAt: new Date().toISOString(),
                    },
                  },
                };
              });
            }
          }}
          projectPath={activeProjectPath}
          subagents={composerSubagents}
          activeSubagent={activeComposerSubagent}
          onSelectSubagent={(id) => void onSelectSubagent(id)}
          changeSummary={{
            fileCount: conversationChangeSummary.files.length,
            additions: conversationChangeSummary.additions,
            deletions: conversationChangeSummary.deletions,
          }}
          onToggleCapabilities={
            capabilityPickerEnabled && activeId
              ? () => setCapabilityPickerOpen((open) => !open)
              : undefined
          }
          capabilityCount={capabilityCount}
          capabilityPickerSlot={
            capabilityPickerOpen && capabilityPickerEnabled && activeId ? (
              <Suspense fallback={null}>
                <LazyCapabilityPickerPopover
                  locale={locale}
                  gateway={gateway}
                  selection={activeCapabilitySelection}
                  onChange={(selection) => void handleCapabilitySelectionChange(selection)}
                  onClose={() => setCapabilityPickerOpen(false)}
                  showSkills={false}
                />
              </Suspense>
            ) : null
          }
        />
      )}
    </>
  );
}

export default WorkbenchComposer;
