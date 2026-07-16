'use client';

import React, { createContext, useContext, useEffect, useMemo, useState } from 'react';
import {
  groupAssistantConversations,
  type AssistantProjectCreationState,
  type AssistantProjectGroup,
} from '@/lib/assistant-project-groups';
import type { AssistantFileChange, AssistantRunEvent } from '@/lib/assistant-types';
import { readActiveProject } from '@/lib/active-project';
import { type Locale } from '@/i18n';

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
  conversationMode: 'chat' | 'agent';
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
  publishNavigation: (snapshot: AssistantNavigationSnapshot) => void;
  publishRuntime: (snapshot: AssistantRuntimeSnapshot) => void;
  registerActions: (actions: AssistantWorkspaceActions | null) => void;
}

// loading defaults to false: AssistantWorkbench is lazy-mounted only on the
// assistant view, while AssistantSidebarSection always reads this snapshot.
// Starting at true left the sidebar spinner stuck until the user opened Assistant.
const emptyNavigation: AssistantNavigationSnapshot = {
  groups: [], selectedId: null, activeProjectPath: null, loading: false,
  creationState: 'engine_unavailable', isCreatingConversation: false,
  pendingCreateProjectPath: undefined,
};

const emptyRuntime: AssistantRuntimeSnapshot = {
  conversationId: null, conversationTitle: null, conversationMode: 'chat', providerId: '', modelId: '',
  runId: null, runStatus: 'idle', runStartedAt: null, runFinishedAt: null, events: [], fileChanges: [], artifacts: [],
  usage: { inputTokens: null, outputTokens: null, reasoningTokens: null },
};

const AssistantWorkspaceContext = createContext<AssistantWorkspaceContextValue | null>(null);

export function AssistantWorkspaceProvider({ children }: { children: React.ReactNode }) {
  const [navigation, publishNavigation] = useState(emptyNavigation);
  const [runtime, publishRuntime] = useState(emptyRuntime);
  const [actions, registerActions] = useState<AssistantWorkspaceActions | null>(null);

  useEffect(() => {
    let cancelled = false;

    const loadInitialData = async () => {
      if (typeof window === 'undefined') return;
      const api = window.nativesAPI;
      if (!api) return;

      try {
        // 1. Get active project path
        let activeProjectPath: string | null = null;
        try {
          activeProjectPath = await readActiveProject(api);
        } catch (e) {
          console.error('Failed to read active project:', e);
        }
        if (cancelled) return;

        // 2. Get registered projects list
        let registeredProjects: Array<{ id: string; path: string }> = [];
        try {
          registeredProjects = (await api.project.list()) ?? [];
        } catch (e) {
          console.error('Failed to list projects:', e);
        }
        if (cancelled) return;

        // 3. Get conversations list
        let conversations: any[] = [];
        try {
          if (api.assistantV2) {
            const list = await api.assistantV2.request('conversation.list', { include_archived: false });
            if (Array.isArray(list)) {
              conversations = list.filter((c: any) => !c.archived_at);
            }
          }
        } catch (e) {
          console.error('Failed to list conversations:', e);
        }
        if (cancelled) return;

        // 4. Get saved locale
        let savedLocale = 'zh';
        try {
          savedLocale = (await api.getLocale()) || 'zh';
        } catch (e) {
          console.error('Failed to get locale:', e);
        }
        if (cancelled) return;

        // 5. Group conversations under projects
        const groups = groupAssistantConversations(
          conversations.map(conversation => ({
            id: conversation.id,
            title: conversation.title,
            mode: conversation.mode,
            projectId: conversation.project_id ?? '',
            updatedAt: conversation.updated_at,
          })),
          registeredProjects.map(project => project.path),
          savedLocale === 'zh' ? '未关联项目' : 'Unassigned',
        );

        if (cancelled) return;

        // 6. Publish the initial navigation
        publishNavigation({
          groups,
          selectedId: null,
          activeProjectPath,
          loading: false,
          creationState: 'ready',
          isCreatingConversation: false,
        });
      } catch (err) {
        console.error('Failed to load initial assistant data:', err);
      }
    };

    // Delay slightly to let window.nativesAPI initialize
    const timer = setTimeout(() => {
      void loadInitialData();
    }, 150);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, []);

  const value = useMemo(() => ({ navigation, runtime, actions, publishNavigation, publishRuntime, registerActions }), [navigation, runtime, actions]);
  return <AssistantWorkspaceContext.Provider value={value}>{children}</AssistantWorkspaceContext.Provider>;
}

export function useAssistantWorkspace(): AssistantWorkspaceContextValue {
  const value = useContext(AssistantWorkspaceContext);
  if (!value) throw new Error('useAssistantWorkspace must be used inside AssistantWorkspaceProvider');
  return value;
}
