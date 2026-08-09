'use client';

import { useCallback, useState } from 'react';
import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from '@/lib/assistant-workspace';
import { buildDiagnosticsText } from '@/lib/assistant-workspace/capability-gate';
import {
  cancelRun,
  connectWorkspace,
  respondPermission,
  retryRun,
} from '@/lib/assistant-workspace/controller';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { copyToClipboard } from '@/lib/clipboard';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import type { Run } from '@/lib/assistant-protocol';

export interface UseAssistantRunLifecycleOptions {
  /** Id of the run currently shown in the timeline (undefined when idle). */
  activeRunId: string | undefined;
  /** Root run of the selected root conversation. */
  rootRun: Run | null;
  /** Run actually displayed (surface run when a child is focused). */
  activeRun: Run | null;
  /** Keeps a fresh run subscribed after retry / permission routing. */
  ensureRunSubscription: (runId: string | null | undefined) => void;
  locale: Locale;
}

/**
 * Run control lifecycle for the workbench.
 *
 * Owns the single class of "what is happening to the current run right now":
 * the stopping flag and every user-facing run action (stop / retry / permission
 * response / file rollback / diagnostics / reconnect). The shell only passes the
 * run identities; connection diagnostics are read from the store here.
 */
export function useAssistantRunLifecycle({
  activeRunId,
  rootRun,
  activeRun,
  ensureRunSubscription,
  locale,
}: UseAssistantRunLifecycleOptions) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const { toast } = useToast();

  const [stoppingRunId, setStoppingRunId] = useState<string | null>(null);

  const handleStop = useCallback(async () => {
    if (!activeRunId || stoppingRunId === activeRunId) return;
    setStoppingRunId(activeRunId);
    try {
      await cancelRun(gateway, dispatch, activeRunId);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setStoppingRunId((current) => current === activeRunId ? null : current);
    }
  }, [
    // React Compiler cannot prove selector results immutable; callback dependencies are intentional.

    activeRunId, gateway, dispatch, toast, stoppingRunId,
  ]);

  const handleRetry = useCallback(async () => {
    if (!activeRunId) return;
    try {
      const newId = await retryRun(gateway, dispatch, activeRunId);
      ensureRunSubscription(newId);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [

    activeRunId, gateway, dispatch, toast, ensureRunSubscription,
  ]);

  const handlePermission = useCallback(
    async (requestId: string, approved: boolean, scope?: string) => {
      // Throw so PermissionRequestCard can unlock + show in-card error.
      await respondPermission(
        gateway,
        dispatch,
        requestId,
        approved,
        scope ?? 'once',
        activeRunId,
      );
    },
    [
      gateway, dispatch,

      activeRunId,
    ],
  );

  const handleRollbackChanges = useCallback(
    async (changes: Array<{ path: string; runId?: string }>): Promise<boolean> => {
      const byRun = new Map<string, string[]>();
      for (const change of changes) {
        const runId = change.runId ?? (rootRun ?? activeRun)?.id;
        if (!runId || !change.path) continue;
        const paths = byRun.get(runId) ?? [];
        if (!paths.includes(change.path)) paths.push(change.path);
        byRun.set(runId, paths);
      }
      if (byRun.size === 0) {
        toast(t(locale, 'assistant.noReversibleChanges'), 'error');
        return false;
      }
      try {
        const previews = await Promise.all(
          [...byRun.entries()].map(async ([runId, paths]) => {
            const preview = await gateway.request<Record<string, unknown>>('workspace.restorePreview', {
              run_id: runId,
              paths,
            });
            const checkpointId = String(preview?.checkpoint_id ?? preview?.checkpointId ?? '');
            const conflicts = Array.isArray(preview?.conflicts) ? preview.conflicts : [];
            if (!checkpointId || conflicts.length > 0) {
              throw new Error(t(locale, 'assistant.undoRefused'));
            }
            return { runId, paths, checkpointId };
          }),
        );
        for (const preview of previews) {
          await gateway.request('workspace.restore', {
            run_id: preview.runId,
            checkpoint_id: preview.checkpointId,
            paths: preview.paths,
            conflict_policy: 'fail',
          });
        }
        toast(t(locale, 'assistant.undoDone'), 'success');
        return true;
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
        return false;
      }
    },
    [rootRun, activeRun, gateway, toast, locale],
  );

  const handleCopyDiagnostics = useCallback(() => {
    const text = buildDiagnosticsText({
      connection: state.connection,
      connectionError: state.connectionError,
      protocolVersion: state.capabilities?.protocolVersion ?? null,
      methodsCount: state.capabilities?.methods?.length ?? 0,
      reconnectAttempts: state.reconnectAttempts,
    });
    void copyToClipboard(text).then((ok) => {
      if (ok) toast(t(locale, 'assistant.diagnosticsCopied'), 'success');
      else toast(t(locale, 'assistant.copyFailed'), 'error');
    });
  }, [
    state.connection,
    state.connectionError,
    state.capabilities,
    state.reconnectAttempts,
    toast,
    locale,
  ]);

  const handleRetryConnection = useCallback(() => {
    void connectWorkspace(gateway, dispatch).catch((err) => {
      toast(classifyError(err).userMessage, 'error');
    });
  }, [gateway, dispatch, toast]);

  return {
    stoppingRunId,
    handleStop,
    handleRetry,
    handlePermission,
    handleRollbackChanges,
    handleCopyDiagnostics,
    handleRetryConnection,
  };
}
