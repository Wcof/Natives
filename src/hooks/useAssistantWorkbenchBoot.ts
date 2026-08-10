'use client';

/**
 * Boot effect for the assistant workbench.
 *
 * On mount: connects the workspace gateway, loads conversations, reads the
 * active project, lists registered projects, restores pinned-conversation
 * preferences, and fetches the provider list. On unmount: aborts all run
 * subscriptions and disconnects the gateway.
 *
 * The orchestrator retains single state authority — setters are passed in,
 * not recreated here.
 */

import { useEffect } from 'react';
import { useAssistantDispatch, useAssistantGateway } from '@/lib/assistant-workspace';
import { connectWorkspace, loadConversations } from '@/lib/assistant-workspace/controller';
import { classifyError } from '@/lib/error-classifier';
import {
  classifyProviderReadiness,
  mapWireProviders,
  toProviderInfo,
  type ProviderReadiness,
} from '@/lib/provider-model-selection';
import { readActiveProject } from '@/lib/active-project';
import type { ProviderWithModels } from '@/lib/assistant-ui-types';

export interface UseAssistantWorkbenchBootOptions {
  setProviders: (providers: ProviderWithModels[]) => void;
  setProviderReadiness: (readiness: ProviderReadiness) => void;
  setActiveProjectPath: (path: string | null) => void;
  setRegisteredProjects: (
    projects: Array<{ id: string; path: string; lastOpenedAt?: string | null; label?: string; exists?: boolean }>,
  ) => void;
  setPinnedConversationIds: (ids: Set<string>) => void;
  setLoadingConversations: (loading: boolean) => void;
  abortAllSubscriptions: () => void;
  toast: (message: string, kind: 'error' | 'success' | 'info') => void;
}

export function useAssistantWorkbenchBoot({
  setProviders,
  setProviderReadiness,
  setActiveProjectPath,
  setRegisteredProjects,
  setPinnedConversationIds,
  setLoadingConversations,
  abortAllSubscriptions,
  toast,
}: UseAssistantWorkbenchBootOptions) {
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await connectWorkspace(gateway, dispatch);
        if (cancelled) return;
        await loadConversations(gateway, dispatch);
        if (cancelled) return;

        try {
          const path = await readActiveProject(window.nativesAPI);
          if (!cancelled) setActiveProjectPath(path);
        } catch {
          /* browser */
        }
        try {
          const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
          if (!cancelled) setRegisteredProjects(projects);
        } catch {
          /* */
        }
        try {
          const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
          if (!cancelled && raw) {
            const parsed = JSON.parse(String(raw)) as Record<string, string[]>;
            const ids = new Set<string>();
            for (const list of Object.values(parsed ?? {})) {
              for (const id of list ?? []) ids.add(id);
            }
            setPinnedConversationIds(ids);
          }
        } catch {
          /* */
        }

        try {
          // Production returns `{ providers: [...] }`; fixtures may return a bare array.
          const list = await gateway.request<unknown>('provider.list', {});
          const mapped: ProviderWithModels[] = mapWireProviders(list);
          if (!cancelled) {
            setProviders(mapped);
            setProviderReadiness(classifyProviderReadiness(toProviderInfo(mapped)));
          }
        } catch (err) {
          if (!cancelled) {
            setProviders([]);
            setProviderReadiness('no_provider');
            toast(classifyError(err).userMessage, 'error');
          }
        }
      } catch (err) {
        if (!cancelled) toast(classifyError(err).userMessage, 'error');
      } finally {
        if (!cancelled) setLoadingConversations(false);
      }
    })();
    return () => {
      cancelled = true;
      abortAllSubscriptions();
      void gateway.disconnect();
    };
  }, [gateway, dispatch, toast]);
}
