'use client';

/**
 * Overlay renderers for the assistant workbench.
 *
 * Renders the two modal overlays the workbench owns:
 * - SubagentAssignmentModal (subagent assignment interaction + key switch),
 * - ConfirmDialog (goal conversation deletion confirmation).
 *
 * Kept as a separate component so the orchestrator return stays a flat
 * composition of panes + overlays. All state and callbacks are passed in
 * from the orchestrator — this component owns no state itself.
 */

import { useCallback } from 'react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { SubagentAssignmentInteraction } from '@/lib/assistant-protocol';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import SubagentAssignmentModal, {
  type AssignmentKeyOption,
  type SubagentAssignmentConfirmPayload,
} from '../SubagentAssignmentModal';
import { useAssistantDispatch, useAssistantGateway } from '@/lib/assistant-workspace';

export interface WorkbenchOverlaysProps {
  locale: Locale;
  rootConversationId: string | null;
  subagentAssignment: SubagentAssignmentInteraction | undefined;
  switchKeySessionId: string | null;
  assignmentKeyOptions: AssignmentKeyOption[];
  onSwitchKeySessionId: (id: string | null) => void;
  onAssignmentConfirm: (payload: SubagentAssignmentConfirmPayload) => void | Promise<void>;
  confirmDeleteGoalId: string | null;
  setConfirmDeleteGoalId: (id: string | null) => void;
  stateRef: { current: { activeConversationId: string | null } };
  toast: (message: string, kind: 'error' | 'success' | 'info') => void;
}

export function WorkbenchOverlays({
  locale,
  rootConversationId,
  subagentAssignment,
  switchKeySessionId,
  assignmentKeyOptions,
  onSwitchKeySessionId,
  onAssignmentConfirm,
  confirmDeleteGoalId,
  setConfirmDeleteGoalId,
  stateRef,
  toast,
}: WorkbenchOverlaysProps) {
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();

  const handleCloseModal = useCallback(() => {
    if (switchKeySessionId) {
      onSwitchKeySessionId(null);
      return;
    }
    if (subagentAssignment) {
      void gateway
        .request('interaction.respond', {
          id: subagentAssignment.id,
          run_id: subagentAssignment.runId,
          response: {
            approved: false,
            cancelled: true,
            conversation_id:
              subagentAssignment.conversationId ?? rootConversationId ?? undefined,
          },
        })
        .then(() => dispatch({ type: 'interaction/remove', id: subagentAssignment.id }))
        .catch(() => {
          // Keep card/modal open on failure so user can retry or dismiss again.
        });
    }
  }, [switchKeySessionId, onSwitchKeySessionId, subagentAssignment, gateway, dispatch, rootConversationId]);

  const handleConfirmDelete = useCallback(() => {
    const id = confirmDeleteGoalId;
    setConfirmDeleteGoalId(null);
    if (!id) return;
    void gateway
      .request('conversation.delete', { id })
      .then(() => {
        dispatch({ type: 'conversations/remove', id });
        if (stateRef.current.activeConversationId === id) {
          dispatch({ type: 'conversations/setActive', id: null });
        }
      })
      .catch((err) => toast(classifyError(err).userMessage, 'error'));
  }, [confirmDeleteGoalId, setConfirmDeleteGoalId, gateway, dispatch, stateRef, toast]);

  return (
    <>
      <SubagentAssignmentModal
        open={Boolean(subagentAssignment) || Boolean(switchKeySessionId)}
        locale={locale}
        interaction={subagentAssignment ?? null}
        keys={assignmentKeyOptions}
        switchSessionId={switchKeySessionId}
        onClose={handleCloseModal}
        onConfirm={onAssignmentConfirm}
      />
      {/* T216/T302: unified confirm dialog for goal conversation deletion */}
      <ConfirmDialog
        open={confirmDeleteGoalId !== null}
        title={t(locale, 'assistant.goalDelete')}
        message={t(locale, 'assistant.goalDeleteConfirm')}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={handleConfirmDelete}
        onCancel={() => setConfirmDeleteGoalId(null)}
      />
    </>
  );
}
