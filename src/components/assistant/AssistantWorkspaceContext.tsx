'use client';

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react';
import {
  groupAssistantConversations,
  type AssistantProjectCreationState,
  type AssistantProjectGroup,
} from '@/lib/assistant-project-groups';
import type { AssistantFileChange, AssistantRunEvent } from '@/lib/assistant-types';
import { readActiveProject, writeActiveProject } from '@/lib/active-project';

export interface AssistantNavigationSnapshot {
  groups: AssistantProjectGroup[];
  selectedId: string | null;
  activeProjectPath: string | null;
  loading: boolean;
  creationState: AssistantProjectCreationState;
  isCreatingConversation: boolean;
  pendingCreateProjectPath?: string | null;
}

export interface AssistantRuntimeSnapshot {
  conversationId: string | null;
  conversationTitle: string | null;
  conversationMode: 'chat' | 'agent' | 'goal';
  providerId: string;
  modelId: string;
  runId: string | null;
  runStatus: string;
  runStartedAt: string | null;
  runFinishedAt: string | null;
  events: AssistantRunEvent[];
  fileChanges: AssistantFileChange[];
  artifacts: Array<{ id: string; path: string; label?: string; size: number; kind: string }>;
  usage: { inputTokens: number | null; outputTokens: number | null; reasoningTokens: number | null };
}

export interface AssistantWorkspaceActions {
  selectConversation(id: string): void;
  selectProject(path: string | null): void;
  addProjectFolder(): void;
  createConversation(): void;
  createConversationInProject(path: string): void;
  removeProject(path: string): void;
  renameConversation(id: string, title: string): void;
  archiveConversation(id: string): void;
  deleteConversation(id: string): Promise<boolean>;
  retryRun(): void;
  respondPermission(requestId: string, approved: boolean): void;
}

interface AssistantWorkspaceContextValue {
  navigation: AssistantNavigationSnapshot;
  runtime: AssistantRuntimeSnapshot;
  actions: AssistantWorkspaceActions | null;
  publishNavigation: (
    snapshot:
      | AssistantNavigationSnapshot
      | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
  ) => void;
  publishRuntime: (snapshot: AssistantRuntimeSnapshot) => void;
  registerActions: (actions: AssistantWorkspaceActions | null) => void;
}

const emptyNavigation: AssistantNavigationSnapshot = {
  groups: [],
  selectedId: null,
  activeProjectPath: null,
  loading: false,
  creationState: 'engine_unavailable',
  isCreatingConversation: false,
  pendingCreateProjectPath: undefined,
};

const emptyRuntime: AssistantRuntimeSnapshot = {
  conversationId: null,
  conversationTitle: null,
  conversationMode: 'chat',
  providerId: '',
  modelId: '',
  runId: null,
  runStatus: 'idle',
  runStartedAt: null,
  runFinishedAt: null,
  events: [],
  fileChanges: [],
  artifacts: [],
  usage: { inputTokens: null, outputTokens: null, reasoningTokens: null },
};

const AssistantWorkspaceContext = createContext<AssistantWorkspaceContextValue | null>(null);

export function AssistantWorkspaceProvider({ children }: { children: React.ReactNode }) {
  const [navigation, setNavigation] = useState(emptyNavigation);
  const [runtime, setRuntime] = useState(emptyRuntime);
  /** Engine-backed actions registered only while AssistantWorkbench is mounted. */
  const [workbenchActions, setWorkbenchActions] = useState<AssistantWorkspaceActions | null>(null);

  const publishNavigation = useCallback(
    (
      snapshot:
        | AssistantNavigationSnapshot
        | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
    ) => {
      setNavigation(snapshot);
    },
    [],
  );

  const publishRuntime = useCallback((snapshot: AssistantRuntimeSnapshot) => {
    setRuntime(snapshot);
  }, []);

  const registerActions = useCallback((next: AssistantWorkspaceActions | null) => {
    setWorkbenchActions(next);
  }, []);

  /**
   * Single root-level bootstrap: projects + conversations + active project in
   * parallel. Does not wait for Workbench mount or "new conversation" clicks.
   * Server result fully replaces navigation once ready (empty is honest empty).
   */
  const refreshNavigationFromHost = useCallback(async (): Promise<void> => {
    if (typeof window === 'undefined') return;
    const api = window.nativesAPI;
    if (!api?.project?.list) return;

    publishNavigation((prev) => ({ ...prev, loading: true }));

    let savedLocale = 'zh';
    try {
      savedLocale = (await api.getLocale?.()) || 'zh';
    } catch {
      /* keep zh */
    }
    const unassignedLabel = savedLocale.startsWith('zh') ? '未关联项目' : 'Unassigned';

    const [activeProjectPath, registeredProjects, conversations] = await Promise.all([
      readActiveProject(api).catch(() => null as string | null),
      api.project.list().then((p) => p ?? []).catch((e) => {
        console.error('Failed to list projects:', e);
        return [] as Array<{ id: string; path: string }>;
      }),
      (async () => {
        try {
          const raw = await api.assistantV2?.request?.('conversation.list', {
            include_archived: false,
          });
          const list = Array.isArray(raw)
            ? raw
            : Array.isArray((raw as { conversations?: unknown[] } | null)?.conversations)
              ? (raw as { conversations: unknown[] }).conversations
              : [];
          return list.map((item) => {
            const r = (item ?? {}) as Record<string, unknown>;
            // Accept both camelCase (already mapped) and snake_case host wire.
            if (r && typeof r === 'object' && 'providerId' in r) {
              return {
                id: String(r.id ?? ''),
                title: String(r.title ?? ''),
                mode: (String(r.mode ?? 'agent') as 'chat' | 'agent' | 'goal'),
                projectId: (r.projectId as string | null | undefined) ?? null,
                updatedAt: String(r.updatedAt ?? r.updated_at ?? ''),
              };
            }
            return {
              id: String(r.id ?? ''),
              title: String(r.title ?? ''),
              mode: (String(r.mode ?? 'agent') as 'chat' | 'agent' | 'goal'),
              projectId:
                (r.project_id as string | null | undefined) ??
                (r.projectId as string | null | undefined) ??
                null,
              updatedAt: String(r.updated_at ?? r.updatedAt ?? ''),
            };
          }).filter((c) => c.id.length > 0);
        } catch (e) {
          console.error('Failed to list conversations:', e);
          return [] as Array<{
            id: string;
            title: string;
            mode: 'chat' | 'agent' | 'goal';
            projectId: string | null;
            updatedAt: string;
          }>;
        }
      })(),
    ]);

    const projectPaths = registeredProjects.map((project) => project.path);
    // Sessions that reference unregistered / legacy project paths go to unassigned.
    // Do not invent historical project nodes from conversation.projectId alone.
    const groups = groupAssistantConversations(conversations, projectPaths, unassignedLabel);

    publishNavigation((prev) => ({
      ...prev,
      groups,
      activeProjectPath: activeProjectPath ?? prev.activeProjectPath,
      loading: false,
      creationState:
        prev.creationState === 'engine_unavailable' ? 'ready' : prev.creationState,
    }));
  }, [publishNavigation]);

  useEffect(() => {
    let cancelled = false;
    let attempts = 0;
    let timer: number | undefined;

    const loadInitialData = async (): Promise<void> => {
      if (typeof window === 'undefined' || cancelled) return;
      const api = window.nativesAPI;
      // Bridge may not be ready on first paint — retry instead of leaving the
      // sidebar empty until the user opens Assistant or clicks a project action.
      if (!api?.project?.list) {
        attempts += 1;
        if (!cancelled && attempts < 40) {
          timer = window.setTimeout(() => {
            void loadInitialData();
          }, 100);
        }
        return;
      }
      try {
        await refreshNavigationFromHost();
      } catch (err) {
        console.error('Failed to load initial assistant data:', err);
        if (!cancelled) {
          publishNavigation((prev) => ({ ...prev, loading: false }));
        }
      }
    };

    timer = window.setTimeout(() => {
      void loadInitialData();
    }, 0);

    return () => {
      cancelled = true;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [refreshNavigationFromHost, publishNavigation]);

  // Shell-owned actions: always available so project tree works before
  // lazy AssistantWorkbench mounts. Engine ops defer to workbench when present.
  const shellActions = useMemo<AssistantWorkspaceActions>(
    () => ({
      selectConversation: (id) => {
        if (workbenchActions) {
          workbenchActions.selectConversation(id);
          return;
        }
        publishNavigation((prev) => ({ ...prev, selectedId: id }));
      },
      selectProject: (path) => {
        if (workbenchActions) {
          workbenchActions.selectProject(path);
          return;
        }
        publishNavigation((prev) => ({ ...prev, activeProjectPath: path }));
        if (path) {
          void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
        }
      },
      addProjectFolder: () => {
        if (workbenchActions) {
          workbenchActions.addProjectFolder();
          return;
        }
        void window.nativesAPI?.dialog?.pickDirectory?.().then(async (path) => {
          if (!path) return;
          try {
            await window.nativesAPI?.project?.register?.(path);
            await refreshNavigationFromHost();
            publishNavigation((prev) => ({ ...prev, activeProjectPath: path }));
            void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
          } catch (e) {
            console.error('Failed to register project:', e);
          }
        });
      },
      createConversation: () => {
        if (workbenchActions) {
          workbenchActions.createConversation();
          return;
        }
        publishNavigation((prev) => ({
          ...prev,
          pendingCreateProjectPath: prev.activeProjectPath ?? null,
        }));
      },
      createConversationInProject: (path) => {
        if (workbenchActions) {
          workbenchActions.createConversationInProject(path);
          return;
        }
        publishNavigation((prev) => ({
          ...prev,
          activeProjectPath: path,
          pendingCreateProjectPath: path,
        }));
        void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
      },
      removeProject: (path) => {
        void (async () => {
          try {
            if (workbenchActions) {
              workbenchActions.removeProject(path);
            } else {
              const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
              const match = projects.find((p) => p.path === path || p.id === path);
              if (match?.id) {
                await window.nativesAPI?.project?.remove?.(match.id);
              } else if (path) {
                await window.nativesAPI?.project?.remove?.(path);
              }
            }
            // Re-bucket: host unassigns sessions; clear projectId in the published tree.
            publishNavigation((prev) => {
              const conversations = prev.groups.flatMap((g) =>
                g.conversations.map((c) => ({
                  id: c.id,
                  title: c.title,
                  mode: c.mode,
                  projectId: c.projectId === path ? null : (c.projectId ?? null),
                  updatedAt: c.updatedAt,
                })),
              );
              const projectPaths = prev.groups
                .map((g) => g.path)
                .filter((p): p is string => Boolean(p) && p !== path);
              const unassignedLabel =
                prev.groups.find((g) => !g.path)?.label ??
                (typeof navigator !== 'undefined' && navigator.language.startsWith('zh')
                  ? '未关联项目'
                  : 'Unassigned');
              return {
                ...prev,
                groups: groupAssistantConversations(conversations, projectPaths, unassignedLabel),
                activeProjectPath:
                  prev.activeProjectPath === path ? null : prev.activeProjectPath,
              };
            });
            // Also re-fetch registered projects so empty folders stay accurate.
            await refreshNavigationFromHost();
          } catch (e) {
            console.error('Failed to remove project:', e);
          }
        })();
      },
      renameConversation: (id, title) => {
        workbenchActions?.renameConversation(id, title);
      },
      archiveConversation: (id) => {
        workbenchActions?.archiveConversation(id);
      },
      deleteConversation: async (id) => {
        if (workbenchActions) return workbenchActions.deleteConversation(id);
        // Workbench may be unmounted (user only using sidebar) — still hit host DB.
        if (id.startsWith('temp-')) {
          publishNavigation((prev) => ({
            ...prev,
            groups: prev.groups.map((g) => ({
              ...g,
              conversations: g.conversations.filter((c) => c.id !== id),
            })),
            selectedId: prev.selectedId === id ? null : prev.selectedId,
          }));
          return true;
        }
        try {
          const api = window.nativesAPI?.assistantV2;
          if (!api?.request) return false;
          await api.request('conversation.delete', { id });
          publishNavigation((prev) => ({
            ...prev,
            groups: prev.groups.map((g) => ({
              ...g,
              conversations: g.conversations.filter((c) => c.id !== id),
            })),
            selectedId: prev.selectedId === id ? null : prev.selectedId,
          }));
          return true;
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          if (/not found|NOT_FOUND|conversation not found/i.test(message)) {
            publishNavigation((prev) => ({
              ...prev,
              groups: prev.groups.map((g) => ({
                ...g,
                conversations: g.conversations.filter((c) => c.id !== id),
              })),
              selectedId: prev.selectedId === id ? null : prev.selectedId,
            }));
            return true;
          }
          console.error('Failed to delete conversation:', e);
          return false;
        }
      },
      retryRun: () => {
        workbenchActions?.retryRun();
      },
      respondPermission: (requestId, approved) => {
        workbenchActions?.respondPermission(requestId, approved);
      },
    }),
    [workbenchActions, publishNavigation, refreshNavigationFromHost],
  );

  const value = useMemo(
    () => ({
      navigation,
      runtime,
      actions: shellActions,
      publishNavigation,
      publishRuntime,
      registerActions,
    }),
    [navigation, runtime, shellActions, publishNavigation, publishRuntime, registerActions],
  );

  return (
    <AssistantWorkspaceContext.Provider value={value}>{children}</AssistantWorkspaceContext.Provider>
  );
}

export function useAssistantWorkspace(): AssistantWorkspaceContextValue {
  const value = useContext(AssistantWorkspaceContext);
  if (!value) {
    throw new Error('useAssistantWorkspace must be used inside AssistantWorkspaceProvider');
  }
  return value;
}
