'use client';

import React, { createContext, useContext, useMemo, useState } from 'react';
import type { AssistantProjectCreationState, AssistantProjectGroup } from '@/lib/assistant-project-groups';
import type { AssistantFileChange, AssistantRunEvent } from '@/lib/assistant-types';

export interface AssistantNavigationSnapshot {
  groups: AssistantProjectGroup[];
  selectedId: string | null;
  activeProjectPath: string | null;
  loading: boolean;
  creationState: AssistantProjectCreationState;
  /** New field: true while a conversation is being created */
  isCreatingConversation: boolean;
  /** @deprecated Use isCreatingConversation instead. Kept for AssistantSidebarSection compat. */
  creatingMode: 'chat' | 'agent' | null;
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
  /** @deprecated Use addProjectFolder instead. Kept for AssistantSidebarSection compat. */
  pickProject(): void;
  createConversation(): void;
  /** @deprecated createConversation no longer accepts a mode. Kept for AssistantSidebarSection compat. */
  createConversation(mode: 'chat' | 'agent'): void;
  renameConversation(id: string, title: string): void;
  archiveConversation(id: string): void;
  deleteConversation(id: string): void;
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

const emptyNavigation: AssistantNavigationSnapshot = {
  groups: [], selectedId: null, activeProjectPath: null, loading: true,
  creationState: 'engine_unavailable', isCreatingConversation: false,
  creatingMode: null,
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
  const value = useMemo(() => ({ navigation, runtime, actions, publishNavigation, publishRuntime, registerActions }), [navigation, runtime, actions]);
  return <AssistantWorkspaceContext.Provider value={value}>{children}</AssistantWorkspaceContext.Provider>;
}

export function useAssistantWorkspace(): AssistantWorkspaceContextValue {
  const value = useContext(AssistantWorkspaceContext);
  if (!value) throw new Error('useAssistantWorkspace must be used inside AssistantWorkspaceProvider');
  return value;
}
