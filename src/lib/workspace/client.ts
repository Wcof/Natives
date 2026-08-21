'use client';

/**
 * Workspace V2 — typed IPC client.
 *
 * Single access path from the renderer to the workspace domain. All calls go
 * through the Tauri bridge (never the DB). Resolution order:
 *
 * 1. `window.nativesAPI.workspace.<command>` — the repo's hand-rolled bridge,
 *    when the command surface is bound there.
 * 2. `window.__TAURI_INTERNALS__.invoke` — Tauri v2 core internals (used by
 *    `@tauri-apps/api/core`), when the nativesAPI namespace is not bound yet.
 *
 * Command names match `src-tauri/src/commands/workspace.rs` verbatim.
 */

import type {
  WorkspaceContextItem,
  WorkspaceContextItemInput,
  WorkspaceCreateInput,
  WorkspaceLayout,
  WorkspaceSessionSnapshot,
  WorkspaceSnapshot,
  WorkspaceSummary,
  WorkspaceTab,
  WorkspaceTabInput,
  WorkspaceTabUpdateInput,
  WorkspaceToolProfile,
  WorkspaceUpdateInput,
  WorkspaceViewState,
  WorkspaceWidget,
  WorkspaceWidgetInput,
} from './contracts';

type NativesAPI = {
  workspace?: Record<string, (...args: unknown[]) => Promise<unknown>>;
};

function nativesAPI(): NativesAPI {
  return (window as unknown as { nativesAPI?: NativesAPI }).nativesAPI ?? {};
}

function tauriInvoke(): ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>) | null {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> } })
    .__TAURI_INTERNALS__;
  return internals?.invoke ?? null;
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const ns = nativesAPI().workspace;
  if (ns && typeof ns[command] === 'function') {
    return (await ns[command](args)) as T;
  }
  const invoke = tauriInvoke();
  if (invoke) {
    return (await invoke(command, args)) as T;
  }
  throw new Error(`[workspace/client] IPC bridge unavailable for command "${command}"`);
}

// ──────────────────────────────────────────────
// Workspaces
// ──────────────────────────────────────────────

export function listWorkspaces(): Promise<WorkspaceSummary[]> {
  return call('workspace_list');
}

export function getWorkspace(workspaceId: string): Promise<WorkspaceSnapshot | null> {
  return call('workspace_get', { workspaceId });
}

export function createWorkspace(input: WorkspaceCreateInput): Promise<WorkspaceSnapshot> {
  return call('workspace_create', { req: input });
}

export function updateWorkspace(
  workspaceId: string,
  patch: WorkspaceUpdateInput,
): Promise<WorkspaceSnapshot | null> {
  return call('workspace_update', { workspaceId, patch });
}

export function deleteWorkspace(workspaceId: string): Promise<boolean> {
  return call('workspace_delete', { workspaceId });
}

export function setActiveWorkspace(workspaceId: string): Promise<WorkspaceSnapshot | null> {
  return call('workspace_set_active', { workspaceId });
}

// ──────────────────────────────────────────────
// Tabs
// ──────────────────────────────────────────────

export function createTab(
  workspaceId: string,
  input: WorkspaceTabInput,
): Promise<WorkspaceTab | null> {
  return call('workspace_tab_create', { workspaceId, input });
}

export function updateTab(
  tabId: string,
  patch: WorkspaceTabUpdateInput,
): Promise<WorkspaceTab | null> {
  return call('workspace_tab_update', { tabId, patch });
}

export function closeTab(tabId: string): Promise<boolean> {
  return call('workspace_tab_close', { tabId });
}

export function reorderTabs(workspaceId: string, orderedIds: string[]): Promise<WorkspaceTab[]> {
  return call('workspace_tab_reorder', { workspaceId, orderedIds });
}

// ──────────────────────────────────────────────
// Context items
// ──────────────────────────────────────────────

export function addContextItem(
  workspaceId: string,
  input: WorkspaceContextItemInput,
): Promise<WorkspaceContextItem | null> {
  return call('workspace_context_add', { workspaceId, input });
}

export function removeContextItem(workspaceId: string, itemId: string): Promise<boolean> {
  return call('workspace_context_remove', { workspaceId, itemId });
}

// ──────────────────────────────────────────────
// Widgets
// ──────────────────────────────────────────────

export function upsertWidget(
  workspaceId: string,
  input: WorkspaceWidgetInput,
): Promise<WorkspaceWidget | null> {
  return call('workspace_widget_upsert', { workspaceId, input });
}

export function removeWidget(workspaceId: string, widgetId: string): Promise<boolean> {
  return call('workspace_widget_remove', { workspaceId, widgetId });
}

// ──────────────────────────────────────────────
// Layouts
// ──────────────────────────────────────────────

export function saveLayout(
  workspaceId: string,
  breakpoint: string,
  layoutJson: string,
): Promise<WorkspaceLayout | null> {
  return call('workspace_layout_save', { workspaceId, breakpoint, layoutJson });
}

// ──────────────────────────────────────────────
// View states
// ──────────────────────────────────────────────

export function saveViewState(
  workspaceId: string,
  viewKey: string,
  stateJson: string,
): Promise<WorkspaceViewState | null> {
  return call('workspace_view_state_save', { workspaceId, viewKey, stateJson });
}

// ──────────────────────────────────────────────
// Tool profiles
// ──────────────────────────────────────────────

export function bindToolProfile(
  workspaceId: string,
  profileId: string,
  toolKey: string | null,
  configJson: string,
): Promise<WorkspaceToolProfile | null> {
  return call('workspace_tool_profile_bind', { workspaceId, profileId, toolKey, configJson });
}

export function unbindToolProfile(workspaceId: string, profileId: string): Promise<boolean> {
  return call('workspace_tool_profile_unbind', { workspaceId, profileId });
}

// ──────────────────────────────────────────────
// Sessions
// ──────────────────────────────────────────────

export function openSession(workspaceId: string): Promise<WorkspaceSessionSnapshot | null> {
  return call('workspace_session_open', { workspaceId });
}

export function closeSession(): Promise<void> {
  return call('workspace_session_close');
}

export function getSessionSnapshot(workspaceId: string): Promise<WorkspaceSessionSnapshot | null> {
  return call('workspace_session_snapshot', { workspaceId });
}
