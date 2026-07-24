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
import {
  createTempSession,
  isTempConversationId,
  resolveRegisteredProjectPath,
  type TempConversationSession,
} from '@/lib/assistant-temp-conversation';

export interface AssistantNavigationSnapshot {
  groups: AssistantProjectGroup[];
  selectedId: string | null;
  activeProjectPath: string | null;
  loading: boolean;
  creationState: AssistantProjectCreationState;
  isCreatingConversation: boolean;
  pendingCreateProjectPath?: string | null;
  /**
   * Local-only blank session created after project pick / "new conversation".
   * Never listed in `groups`, never written to the host DB until first send.
   */
  tempSession: TempConversationSession | null;
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
  pinConversation(id: string, projectId: string | null, pinned: boolean): void;
  retryRun(): void;
  respondPermission(requestId: string, approved: boolean): void;
}

type PublishNavigation = (
  snapshot:
    | AssistantNavigationSnapshot
    | ((prev: AssistantNavigationSnapshot) => AssistantNavigationSnapshot),
) => void;

/** Combined value for callers that need everything (Workbench). Prefer the
 *  split hooks (`useAssistantNavigation` / `useAssistantRuntime` /
 *  `useAssistantActions`) so stream ticks do not re-render the shell sidebar. */
interface AssistantWorkspaceContextValue {
  navigation: AssistantNavigationSnapshot;
  runtime: AssistantRuntimeSnapshot;
  actions: AssistantWorkspaceActions | null;
  publishNavigation: PublishNavigation;
  publishRuntime: (snapshot: AssistantRuntimeSnapshot) => void;
  registerActions: (actions: AssistantWorkspaceActions | null) => void;
}

interface NavigationContextValue {
  navigation: AssistantNavigationSnapshot;
  publishNavigation: PublishNavigation;
}

interface RuntimeContextValue {
  runtime: AssistantRuntimeSnapshot;
}

interface ActionsContextValue {
  actions: AssistantWorkspaceActions | null;
}

/** Stable publishers/registrars — identity never changes after mount. */
interface WorkspaceApiContextValue {
  publishNavigation: PublishNavigation;
  publishRuntime: (snapshot: AssistantRuntimeSnapshot) => void;
  registerActions: (actions: AssistantWorkspaceActions | null) => void;
}

/** Content equality for project groups so publishNavigation can bail out. */
function navigationGroupsEqual(
  left: AssistantProjectGroup[],
  right: AssistantProjectGroup[],
): boolean {
  if (left === right) return true;
  if (left.length !== right.length) return false;
  for (let i = 0; i < left.length; i += 1) {
    const a = left[i]!;
    const b = right[i]!;
    if (
      a.id !== b.id ||
      a.path !== b.path ||
      a.label !== b.label ||
      a.lastOpenedAt !== b.lastOpenedAt
    ) {
      return false;
    }
    if (a.conversations.length !== b.conversations.length) return false;
    for (let j = 0; j < a.conversations.length; j += 1) {
      const ca = a.conversations[j]!;
      const cb = b.conversations[j]!;
      if (
        ca.id !== cb.id ||
        ca.title !== cb.title ||
        ca.mode !== cb.mode ||
        ca.projectId !== cb.projectId ||
        ca.updatedAt !== cb.updatedAt ||
        Boolean(ca.pinned) !== Boolean(cb.pinned)
      ) {
        return false;
      }
    }
  }
  return true;
}

/** Temp draft text-only changes must not re-render the shell sidebar tree. */
function tempSessionEqual(
  left: TempConversationSession | null,
  right: TempConversationSession | null,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  if (left.conversation.id !== right.conversation.id) return false;
  if (left.conversation.projectId !== right.conversation.projectId) return false;
  if (left.conversation.title !== right.conversation.title) return false;
  // Draft text is workbench-local restore data — not rendered by the tree.
  return true;
}

const emptyNavigation: AssistantNavigationSnapshot = {
  groups: [],
  selectedId: null,
  activeProjectPath: null,
  loading: false,
  creationState: 'engine_unavailable',
  isCreatingConversation: false,
  pendingCreateProjectPath: undefined,
  tempSession: null,
};

function newConversationTitle(): string {
  if (typeof navigator !== 'undefined' && navigator.language.startsWith('zh')) {
    return '新会话';
  }
  return 'New conversation';
}

/** Replace any prior temp shell with a fresh one for `projectPath` (null = unassigned). */
function withFreshTempSession(
  prev: AssistantNavigationSnapshot,
  projectPath: string | null,
): AssistantNavigationSnapshot {
  const session = createTempSession({
    projectId: projectPath,
    title: newConversationTitle(),
  });
  return {
    ...prev,
    activeProjectPath: projectPath,
    selectedId: session.conversation.id,
    tempSession: session,
    // Creating a local shell never blocks on engine/provider readiness.
    isCreatingConversation: false,
    pendingCreateProjectPath: undefined,
  };
}

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
/** Sidebar / shell tree: only re-renders when navigation content actually changes. */
const AssistantNavigationContext = createContext<NavigationContextValue | null>(null);
/** Runtime inspector consumers: stream ticks land here, not on the sidebar. */
const AssistantRuntimeContext = createContext<RuntimeContextValue | null>(null);
/** Shell action façade (select/create/delete). Stable unless workbench re-registers. */
const AssistantActionsContext = createContext<ActionsContextValue | null>(null);
/** publish/register only — callbacks are useCallback([]) so this value is mount-stable. */
const AssistantWorkspaceApiContext = createContext<WorkspaceApiContextValue | null>(null);

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
      setNavigation((prev) => {
        const next = typeof snapshot === 'function' ? snapshot(prev) : snapshot;
        // Structural bailout: workbench republishes on every store tick and
        // always allocates a fresh groups array. Compare groups by content so
        // reference churn alone does not re-render the shell sidebar.
        if (prev === next) return prev;
        if (
          prev.selectedId === next.selectedId &&
          prev.activeProjectPath === next.activeProjectPath &&
          prev.loading === next.loading &&
          prev.creationState === next.creationState &&
          prev.isCreatingConversation === next.isCreatingConversation &&
          prev.pendingCreateProjectPath === next.pendingCreateProjectPath &&
          tempSessionEqual(prev.tempSession, next.tempSession) &&
          (prev.groups === next.groups || navigationGroupsEqual(prev.groups, next.groups))
        ) {
          // Drop draft-only tempSession updates without re-rendering the tree.
          // Remount restore prefers composer store (composer/set) over temp.draft.
          return prev;
        }
        return next;
      });
    },
    [],
  );

  const publishRuntime = useCallback((snapshot: AssistantRuntimeSnapshot) => {
    setRuntime((prev) => {
      if (
        prev.conversationId === snapshot.conversationId &&
        prev.conversationTitle === snapshot.conversationTitle &&
        prev.conversationMode === snapshot.conversationMode &&
        prev.providerId === snapshot.providerId &&
        prev.modelId === snapshot.modelId &&
        prev.runId === snapshot.runId &&
        prev.runStatus === snapshot.runStatus &&
        prev.runStartedAt === snapshot.runStartedAt &&
        prev.runFinishedAt === snapshot.runFinishedAt &&
        prev.usage.inputTokens === snapshot.usage.inputTokens &&
        prev.usage.outputTokens === snapshot.usage.outputTokens &&
        prev.usage.reasoningTokens === snapshot.usage.reasoningTokens &&
        // Stream ticks allocate fresh arrays every publish; compare length + tail
        // so shell consumers do not re-render on pure reference churn.
        prev.events.length === snapshot.events.length &&
        (prev.events.length === 0 ||
          (prev.events[prev.events.length - 1]?.sequence ===
            snapshot.events[snapshot.events.length - 1]?.sequence &&
            prev.events[prev.events.length - 1]?.runId ===
              snapshot.events[snapshot.events.length - 1]?.runId &&
            prev.events[prev.events.length - 1]?.type ===
              snapshot.events[snapshot.events.length - 1]?.type)) &&
        prev.fileChanges.length === snapshot.fileChanges.length &&
        prev.artifacts.length === snapshot.artifacts.length &&
        (prev.fileChanges.length === 0 ||
          prev.fileChanges.every(
            (f, i) =>
              f.path === snapshot.fileChanges[i]?.path &&
              f.change === snapshot.fileChanges[i]?.change &&
              f.changeType === snapshot.fileChanges[i]?.changeType,
          )) &&
        (prev.artifacts.length === 0 ||
          prev.artifacts.every(
            (a, i) =>
              a.id === snapshot.artifacts[i]?.id &&
              a.path === snapshot.artifacts[i]?.path &&
              a.size === snapshot.artifacts[i]?.size,
          ))
      ) {
        return prev;
      }
      return snapshot;
    });
  }, []);

  const registerActions = useCallback((next: AssistantWorkspaceActions | null) => {
    setWorkbenchActions((prev) => (prev === next ? prev : next));
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
        return [] as Array<{ id: string; path: string; lastOpenedAt?: string | null }>;
      }),
      (async () => {
        try {
          const request = api.assistantV2?.request;
          if (!request) return [];
          const raw = await request('conversation.listPage', { limit: 100 })
            .catch(() => request('conversation.list', { include_archived: false }));
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

    // Keep host project.list order (last_opened_at DESC). Do not re-sort by session time.
    const projectMetas = registeredProjects.map((project) => {
      const rec = project as { path: string; lastOpenedAt?: string | null; last_opened_at?: string | null; label?: string };
      return {
        path: rec.path,
        lastOpenedAt: rec.lastOpenedAt ?? rec.last_opened_at ?? null,
        label: rec.label,
      };
    });
    // Sessions that reference unregistered / legacy project paths go to unassigned.
    // Do not invent historical project nodes from conversation.projectId alone.
    // Temp shells never come from the host list — keep them out of sidebar groups.
    const groups = groupAssistantConversations(
      conversations.filter((c) => !isTempConversationId(c.id)),
      projectMetas,
      unassignedLabel,
    );

    publishNavigation((prev) => ({
      ...prev,
      groups,
      activeProjectPath: activeProjectPath ?? prev.activeProjectPath,
      loading: false,
      creationState:
        prev.creationState === 'engine_unavailable' ? 'ready' : prev.creationState,
      // Host refresh must not drop an in-memory temp shell the user is composing.
      tempSession: prev.tempSession,
      selectedId: prev.selectedId,
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
        // Selecting a persisted session clears any local temp shell.
        publishNavigation((prev) => ({
          ...prev,
          selectedId: id,
          tempSession: isTempConversationId(id) ? prev.tempSession : null,
        }));
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
        void window.nativesAPI?.dialog?.pickDirectory?.().then(async (picked) => {
          if (!picked) return;
          try {
            const registered = await window.nativesAPI?.project?.register?.(picked);
            const path = resolveRegisteredProjectPath(registered, picked);
            await refreshNavigationFromHost();
            // Navigate + set active project + deselect prior persistent session +
            // create a local temp-* shell (no conversation.create).
            publishNavigation((prev) => withFreshTempSession(prev, path));
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
        // Shell-only path: local temp shell, no host create, no provider gate.
        publishNavigation((prev) =>
          withFreshTempSession(prev, prev.activeProjectPath ?? null),
        );
      },
      createConversationInProject: (path) => {
        if (workbenchActions) {
          workbenchActions.createConversationInProject(path);
          return;
        }
        publishNavigation((prev) => withFreshTempSession(prev, path));
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
            // Soft-delete: sessions keep their project_id, just refresh the project list.
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
      pinConversation: (id, projectId, pinned) => {
        workbenchActions?.pinConversation?.(id, projectId, pinned);
      },
      deleteConversation: async (id) => {
        if (workbenchActions) return workbenchActions.deleteConversation(id);
        // Workbench may be unmounted (user only using sidebar) — still hit host DB.
        const dropFromNav = () => {
          publishNavigation((prev) => ({
            ...prev,
            groups: prev.groups.map((g) => ({
              ...g,
              conversations: g.conversations.filter((c) => c.id !== id),
            })),
            selectedId: prev.selectedId === id ? null : prev.selectedId,
            tempSession:
              prev.tempSession?.conversation.id === id ? null : prev.tempSession,
          }));
        };

        if (isTempConversationId(id)) {
          dropFromNav();
          return true;
        }

        // Optimistic: sidebar must update even if RPC is slow/offline.
        dropFromNav();
        try {
          const api = window.nativesAPI?.assistantV2;
          if (!api?.request) {
            // UI already updated; host cleanup will retry when engine is back.
            console.warn('assistantV2 unavailable after optimistic conversation delete');
            return true;
          }
          await api.request('conversation.delete', { id });
          return true;
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          if (/not found|NOT_FOUND|conversation not found/i.test(message)) {
            return true;
          }
          console.error('Failed to delete conversation on host:', e);
          // Keep optimistic removal — user asked to delete; do not resurrect.
          return true;
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

  const navigationValue = useMemo<NavigationContextValue>(
    () => ({ navigation, publishNavigation }),
    [navigation, publishNavigation],
  );

  const runtimeValue = useMemo<RuntimeContextValue>(
    () => ({ runtime }),
    [runtime],
  );

  const actionsValue = useMemo<ActionsContextValue>(
    () => ({ actions: shellActions }),
    [shellActions],
  );

  // All three callbacks are useCallback([]) — stable for the provider lifetime.
  const apiValue = useMemo<WorkspaceApiContextValue>(
    () => ({ publishNavigation, publishRuntime, registerActions }),
    [publishNavigation, publishRuntime, registerActions],
  );

  // Combined bag kept for legacy callers. Nested providers ensure the shell
  // sidebar (Navigation + Actions only) does not re-render when `runtime`
  // thrash from stream ticks (events / usage / status).
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
    <AssistantWorkspaceContext.Provider value={value}>
      <AssistantWorkspaceApiContext.Provider value={apiValue}>
        <AssistantNavigationContext.Provider value={navigationValue}>
          <AssistantRuntimeContext.Provider value={runtimeValue}>
            <AssistantActionsContext.Provider value={actionsValue}>
              {children}
            </AssistantActionsContext.Provider>
          </AssistantRuntimeContext.Provider>
        </AssistantNavigationContext.Provider>
      </AssistantWorkspaceApiContext.Provider>
    </AssistantWorkspaceContext.Provider>
  );
}

export function useAssistantWorkspace(): AssistantWorkspaceContextValue {
  const value = useContext(AssistantWorkspaceContext);
  if (!value) {
    throw new Error('useAssistantWorkspace must be used inside AssistantWorkspaceProvider');
  }
  return value;
}

/** Project / conversation tree. Safe for the shell sidebar during streaming. */
export function useAssistantNavigation(): NavigationContextValue {
  const value = useContext(AssistantNavigationContext);
  if (!value) {
    throw new Error('useAssistantNavigation must be used inside AssistantWorkspaceProvider');
  }
  return value;
}

/** Run/events/artifacts snapshot for inspectors — high-churn during streams. */
export function useAssistantRuntime(): RuntimeContextValue {
  const value = useContext(AssistantRuntimeContext);
  if (!value) {
    throw new Error('useAssistantRuntime must be used inside AssistantWorkspaceProvider');
  }
  return value;
}

/** Select / create / delete / pin — shell buttons and the conversation tree. */
export function useAssistantActions(): ActionsContextValue {
  const value = useContext(AssistantActionsContext);
  if (!value) {
    throw new Error('useAssistantActions must be used inside AssistantWorkspaceProvider');
  }
  return value;
}

/**
 * Stable publish/register API. Workbench should use this for publishers so
 * stream-driven runtime state updates do not bounce Workbench through a
 * second context subscription (store already drives its re-renders).
 */
export function useAssistantWorkspaceApi(): WorkspaceApiContextValue {
  const value = useContext(AssistantWorkspaceApiContext);
  if (!value) {
    throw new Error('useAssistantWorkspaceApi must be used inside AssistantWorkspaceProvider');
  }
  return value;
}
