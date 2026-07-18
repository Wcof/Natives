'use client';

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
} from 'react';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { createDefaultGateway } from '@/lib/assistant-gateway';
import { workspaceReducer } from './reducer';
import {
  createInitialWorkspaceState,
  type AssistantWorkspaceState,
  type WorkspaceAction,
} from './state';
import {
  loadPersistedDrafts,
  loadPersistedView,
  savePersistedDrafts,
  savePersistedView,
} from './persistence';

interface StoreContextValue {
  state: AssistantWorkspaceState;
  dispatch: React.Dispatch<WorkspaceAction>;
  gateway: AssistantGateway;
}

const StoreContext = createContext<StoreContextValue | null>(null);

export interface AssistantStoreProviderProps {
  children: React.ReactNode;
  gateway?: AssistantGateway;
  /** Prefer fixture adapter even if daemon is available (tests / browser mock). */
  preferFixture?: boolean;
  initialState?: AssistantWorkspaceState;
  /** Disable localStorage hydration (tests). */
  disablePersistence?: boolean;
}

function hydrateInitial(
  base: AssistantWorkspaceState,
  disablePersistence: boolean,
): AssistantWorkspaceState {
  if (disablePersistence) return base;
  const view = loadPersistedView();
  const drafts = loadPersistedDrafts();
  return {
    ...base,
    view: view ? { ...base.view, ...view } : base.view,
    composerByConversation: { ...base.composerByConversation, ...drafts },
  };
}

export function AssistantStoreProvider({
  children,
  gateway: gatewayProp,
  preferFixture = false,
  initialState,
  disablePersistence = false,
}: AssistantStoreProviderProps) {
  const [state, dispatch] = useReducer(
    workspaceReducer,
    undefined,
    () => hydrateInitial(initialState ?? createInitialWorkspaceState(), disablePersistence),
  );
  const gatewayRef = useRef<AssistantGateway>(
    gatewayProp ?? createDefaultGateway(preferFixture),
  );
  if (gatewayProp) gatewayRef.current = gatewayProp;

  // Persist layout / drafts (never execution state)
  useEffect(() => {
    if (disablePersistence) return;
    savePersistedView(state.view);
  }, [
    disablePersistence,
    state.view.leftCollapsed,
    state.view.rightCollapsed,
    state.view.leftWidth,
    state.view.rightWidth,
    state.view.inspectorTab,
  ]);

  useEffect(() => {
    if (disablePersistence) return;
    savePersistedDrafts(state.composerByConversation);
  }, [disablePersistence, state.composerByConversation]);

  const value = useMemo(
    () => ({
      state,
      dispatch,
      gateway: gatewayRef.current,
    }),
    [state],
  );

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}

export function useAssistantStore(): AssistantWorkspaceState {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error('useAssistantStore must be used within AssistantStoreProvider');
  return ctx.state;
}

export function useAssistantDispatch(): React.Dispatch<WorkspaceAction> {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error('useAssistantDispatch must be used within AssistantStoreProvider');
  return ctx.dispatch;
}

export function useAssistantGateway(): AssistantGateway {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error('useAssistantGateway must be used within AssistantStoreProvider');
  return ctx.gateway;
}

/** Helper hook: dispatch + gateway for controllers. */
export function useAssistantController() {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error('useAssistantController must be used within AssistantStoreProvider');
  const batchEvents = useCallback(
    (action: WorkspaceAction) => {
      ctx.dispatch(action);
    },
    [ctx],
  );
  return { state: ctx.state, dispatch: ctx.dispatch, gateway: ctx.gateway, batchEvents };
}
