'use client';

/**
 * Workspace sidebar actions registration for the assistant workbench.
 *
 * Builds the single `AssistantWorkspaceActions` bag the shell sidebar drives
 * (select conversation / project, add folder, create / rename / archive /
 * pin / delete conversation, remove / rename project, retry run, respond
 * permission) and registers it via the stable workspace API so the shell
 * always has an engine-backed handler while the workbench is mounted.
 *
 * The orchestrator retains single state authority — all setters and the
 * store ref are passed in, not recreated here.
 */

import { useEffect } from 'react';
import { useAssistantDispatch, useAssistantGateway } from '@/lib/assistant-workspace';
import { classifyError } from '@/lib/error-classifier';
import { selectAssistantModel, toProviderInfo } from '@/lib/provider-model-selection';
import { writeActiveProject } from '@/lib/active-project';
import { removeProjectFromHost } from '@/lib/assistant-project-remove';
import {
  collectTempConversationIds,
  createTempSession,
  isTempConversationId,
  resolveRegisteredProjectPath,
} from '@/lib/assistant-temp-conversation';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import type { Conversation } from '@/lib/assistant-protocol';
import type { ProviderWithModels } from '@/lib/assistant-ui-types';
import {
  type AssistantWorkspaceActions,
  type AssistantNavigationSnapshot,
} from '@/lib/assistant-ui-types';

export interface UseAssistantWorkbenchActionsOptions {
  providers: ProviderWithModels[];
  locale: Locale;
  activeProjectPath: string | null;
  selectConversation: (id: string) => void;
  handleRetry: () => void;
  handlePermission: (requestId: string, approved: boolean) => void;
  registerActions: (actions: AssistantWorkspaceActions | null) => void;
  publishNavigation: (
    updater:
      | AssistantNavigationSnapshot
      | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
  ) => void;
  setActiveProjectPath: (path: string | null) => void;
  setRegisteredProjects: (
    projects: Array<{ id: string; path: string; lastOpenedAt?: string | null; label?: string; exists?: boolean }>,
  ) => void;
  setHiddenProjectPaths: (paths: string[]) => void;
  setPinnedConversationIds: (
    updater: Set<string> | ((prev: Set<string>) => Set<string>),
  ) => void;
  stateRef: { current: { conversationOrder: string[]; activeConversationId: string | null; conversations: Record<string, Conversation> } };
  toast: (message: string, kind: 'error' | 'success' | 'info') => void;
}

export function useAssistantWorkbenchActions({
  providers,
  locale,
  activeProjectPath,
  selectConversation,
  handleRetry,
  handlePermission,
  registerActions,
  publishNavigation,
  setActiveProjectPath,
  setRegisteredProjects,
  setHiddenProjectPaths,
  setPinnedConversationIds,
  stateRef,
  toast,
}: UseAssistantWorkbenchActionsOptions) {
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();

  useEffect(() => {
    const actions: AssistantWorkspaceActions = {
      selectConversation: (id) => {
        // Switching to a persisted session: drop any local temp shell.
        // Do NOT cancel background runs on other conversations.
        if (!isTempConversationId(id)) {
          for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
            dispatch({ type: 'conversations/remove', id: oldId });
            dispatch({ type: 'composer/clear', conversationId: oldId });
          }
          publishNavigation((prev) => ({
            ...prev,
            tempSession: null,
            selectedId: id,
          }));
        }
        void selectConversation(id);
      },
      selectProject: (path) => {
        setActiveProjectPath(path);
        if (path) void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
      },
      addProjectFolder: () => {
        void window.nativesAPI?.dialog?.pickDirectory?.().then(async (picked) => {
          if (!picked) return;
          try {
            const registered = await window.nativesAPI?.project?.register?.(picked);
            const path = resolveRegisteredProjectPath(registered, picked);
            const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
            setRegisteredProjects(projects);
            setActiveProjectPath(path);
            void writeActiveProject(window.nativesAPI, path).catch(() => undefined);

            // Drop previous local temps; keep only this pick. Never cancel other runs.
            for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
              dispatch({ type: 'conversations/remove', id: oldId });
              dispatch({ type: 'composer/clear', conversationId: oldId });
            }

            const pick = selectAssistantModel(toProviderInfo(providers));
            const session = createTempSession({
              projectId: path,
              title: t(locale, 'assistant.newConversation'),
              providerId: pick?.providerId,
              modelId: pick?.modelId,
            });
            dispatch({ type: 'conversations/upsert', conversation: session.conversation });
            dispatch({ type: 'conversations/setActive', id: session.conversation.id });
            publishNavigation((prev) => ({
              ...prev,
              activeProjectPath: path,
              selectedId: session.conversation.id,
              tempSession: session,
              pendingCreateProjectPath: undefined,
            }));
          } catch (err) {
            toast(classifyError(err).userMessage, 'error');
          }
        });
      },
      createConversation: () => {
        // Local temp shell only — no conversation.create, no provider hard-gate.
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
      createConversationInProject: (path) => {
        setActiveProjectPath(path);
        void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
        for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
          dispatch({ type: 'conversations/remove', id: oldId });
          dispatch({ type: 'composer/clear', conversationId: oldId });
        }
        const pick = selectAssistantModel(toProviderInfo(providers));
        const session = createTempSession({
          projectId: path,
          title: t(locale, 'assistant.newConversation'),
          providerId: pick?.providerId,
          modelId: pick?.modelId,
        });
        dispatch({ type: 'conversations/upsert', conversation: session.conversation });
        dispatch({ type: 'conversations/setActive', id: session.conversation.id });
        publishNavigation((prev) => ({
          ...prev,
          activeProjectPath: path,
          selectedId: session.conversation.id,
          tempSession: session,
          pendingCreateProjectPath: undefined,
        }));
      },      removeProject: async (path) => {
        // 审计收口 #1：删除 = Host 可见性软删，唯一实现 removeProjectFromHost。
        // refresh visible + hidden + navigation projection 在一个接缝内完成；
        // 重新添加只走同一 Host 权威，禁止 Renderer 侧二次删会话或从 daemon
        // project_id 复活。Host 检查 affected rows；不存在/已删除不假成功。
        return removeProjectFromHost({
          api:
            window.nativesAPI?.project
              ? {
                  list: async () => (await window.nativesAPI?.project?.list?.()) ?? null,
                  listHidden: async () => (await window.nativesAPI?.project?.listHidden?.()) ?? null,
                  remove: async (id) => {
                    await window.nativesAPI?.project?.remove?.(id);
                  },
                }
              : null,
          path,
          activeProjectPath,
          setRegisteredProjects,
          setHiddenProjectPaths,
          setActiveProjectPath,
          publishNavigation: (updater) => {
            publishNavigation((prev) => {
              const partial = updater({
                groups: prev.groups,
                activeProjectPath: prev.activeProjectPath ?? null,
              });
              // removeProjectFromHost 只过滤导航组、不改变组结构，
              // 因此窄类型 groups 可以安全合并回完整快照。
              return {
                ...prev,
                groups: partial.groups as typeof prev.groups,
                activeProjectPath: partial.activeProjectPath,
              };
            });
          },
          writeActiveProject: async (nextPath) => {
            void writeActiveProject(window.nativesAPI, nextPath).catch(() => undefined);
          },
          onError: (message) => toast(classifyError(new Error(message)).userMessage, 'error'),
        });
      },
      renameProject: async (path, label) => {
        try {
          const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
          const match = projects.find((p) => p.path === path || p.id === path);
          await window.nativesAPI?.project?.rename?.(match?.id ?? path, label);
          setRegisteredProjects((await window.nativesAPI?.project?.list?.()) ?? []);
          return true;
        } catch (err) {
          toast(classifyError(err).userMessage, 'error');
          return false;
        }
      },
      renameConversation: (id, title) => {
        void gateway.request('conversation.rename', { id, title }).then(() => {
          const c = stateRef.current.conversations[id];
          if (c) dispatch({ type: 'conversations/upsert', conversation: { ...c, title } });
        });
      },
      archiveConversation: (id) => {
        void gateway.request('conversation.archive', { id }).then(() => {
          dispatch({ type: 'conversations/remove', id });
          // Drop pin preference for archived sessions.
          void (async () => {
            try {
              const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
              if (!raw) return;
              const map = JSON.parse(String(raw)) as Record<string, string[]>;
              let changed = false;
              for (const key of Object.keys(map)) {
                const next = (map[key] ?? []).filter((x) => x !== id);
                if (next.length !== (map[key] ?? []).length) {
                  map[key] = next;
                  changed = true;
                }
              }
              if (changed) {
                await window.nativesAPI?.db?.set('assistant:pinnedConversations', JSON.stringify(map));
                setPinnedConversationIds((prev) => {
                  const n = new Set(prev);
                  n.delete(id);
                  return n;
                });
              }
            } catch { /* ignore */ }
          })();
        });
      },
      pinConversation: (id, projectId, pinned) => {
        void (async () => {
          const key = projectId?.trim() || '__unassigned__';
          try {
            const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
            const map = raw ? (JSON.parse(String(raw)) as Record<string, string[]>) : {};
            const list = new Set(map[key] ?? []);
            if (pinned) list.add(id);
            else list.delete(id);
            map[key] = [...list];
            // Clean empty buckets
            if (map[key].length === 0) delete map[key];
            await window.nativesAPI?.db?.set('assistant:pinnedConversations', JSON.stringify(map));
            setPinnedConversationIds((prev) => {
              const n = new Set(prev);
              if (pinned) n.add(id);
              else n.delete(id);
              return n;
            });
          } catch (err) {
            toast(classifyError(err).userMessage, 'error');
          }
        })();
      },
      deleteConversation: async (id) => {
        // Always drop from UI first so the sidebar never looks like a no-op.
        // Host/daemon cleanup is best-effort after the optimistic remove.
        const clearPin = () => {
          setPinnedConversationIds((prev) => {
            if (!prev.has(id)) return prev;
            const n = new Set(prev);
            n.delete(id);
            return n;
          });
          void (async () => {
            try {
              const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
              if (!raw) return;
              const map = JSON.parse(String(raw)) as Record<string, string[]>;
              let changed = false;
              for (const key of Object.keys(map)) {
                const next = (map[key] ?? []).filter((x) => x !== id);
                if (next.length !== (map[key] ?? []).length) {
                  map[key] = next;
                  changed = true;
                }
              }
              if (changed) {
                await window.nativesAPI?.db?.set('assistant:pinnedConversations', JSON.stringify(map));
              }
            } catch { /* ignore */ }
          })();
        };
        const dropFromUi = () => {
          dispatch({ type: 'conversations/remove', id });
          clearPin();
          if (stateRef.current.activeConversationId === id) {
            dispatch({ type: 'conversations/setActive', id: null });
          }
          publishNavigation((prev) => ({
            ...prev,
            groups: prev.groups.map((g) => ({
              ...g,
              conversations: g.conversations.filter((c) => c.id !== id),
            })),
            tempSession:
              prev.tempSession?.conversation.id === id ? null : prev.tempSession,
            selectedId: prev.selectedId === id ? null : prev.selectedId,
          }));
        };

        if (isTempConversationId(id)) {
          dropFromUi();
          return true;
        }

        dropFromUi();
        try {
          await gateway.request('conversation.delete', { id });
          return true;
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          // Already gone on host — UI already updated.
          if (/not found|NOT_FOUND|conversation not found/i.test(message)) {
            return true;
          }
          // Keep UI deleted (idempotent user intent) but surface the host error.
          toast(classifyError(err).userMessage, 'error');
          return true;
        }
      },
      retryRun: () => void handleRetry(),
      respondPermission: (requestId, approved) => void handlePermission(requestId, approved),
    };
    registerActions(actions);
    return () => registerActions(null);
  }, [
    selectConversation,
    providers,
    locale,
    activeProjectPath,
    gateway,
    dispatch,
    handleRetry,
    handlePermission,
    registerActions,
    publishNavigation,
    toast,
  ]);
}
