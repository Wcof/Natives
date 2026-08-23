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

import { cmd } from "@/lib/tauri/core";
import type {
  ExpectedRevision,
  WorkspaceContextItem,
  WorkspaceContextItemInput,
  WorkspaceContextItemPatch,
  WorkspaceCreateInput,
  WorkspaceLayout,
  WorkspaceMcpExposure,
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
  WorkspaceWidgetConfigPatch,
  WorkspaceWidgetInput,
} from './contracts';

type NativesAPI = {
  workspace?: Record<string, (...args: unknown[]) => Promise<unknown>>;
};

function nativesAPI(): NativesAPI {
  return (window as unknown as { nativesAPI?: NativesAPI }).nativesAPI ?? {};
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const ns = nativesAPI().workspace;
  if (ns && typeof ns[command] === "function") {
    return (await ns[command](args)) as T;
  }
  return cmd<T>(command, args);
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

/**
 * G-008: `expectedRevision` is the A-033 snapshot revision the caller last
 * saw (optional; omit to skip the stale-check). A stale value rejects the
 * command with a `Conflict` error before anything is written.
 */
export function updateWorkspace(
  workspaceId: string,
  patch: WorkspaceUpdateInput,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceSnapshot | null> {
  return call('workspace_update', { workspaceId, patch, expectedRevision });
}

export function deleteWorkspace(
  workspaceId: string,
  expectedRevision?: ExpectedRevision,
): Promise<boolean> {
  return call('workspace_delete', { workspaceId, expectedRevision });
}

export function setActiveWorkspace(workspaceId: string): Promise<WorkspaceSnapshot | null> {
  return call('workspace_set_active', { workspaceId });
}

/** A-022: duplicate a workspace (copy all child rows under fresh ids). */
export function duplicateWorkspace(
  workspaceId: string,
  name: string,
): Promise<WorkspaceSummary | null> {
  return call('workspace_duplicate', { workspaceId, name });
}

// ──────────────────────────────────────────────
// Tabs
// ──────────────────────────────────────────────

export function createTab(
  workspaceId: string,
  input: WorkspaceTabInput,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceTab | null> {
  return call('workspace_tab_create', { workspaceId, input, expectedRevision });
}

export function updateTab(
  tabId: string,
  patch: WorkspaceTabUpdateInput,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceTab | null> {
  return call('workspace_tab_update', { tabId, patch, expectedRevision });
}

export function closeTab(tabId: string, expectedRevision?: ExpectedRevision): Promise<boolean> {
  return call('workspace_tab_close', { tabId, expectedRevision });
}

export function reorderTabs(
  workspaceId: string,
  orderedIds: string[],
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceTab[]> {
  return call('workspace_tab_reorder', { workspaceId, orderedIds, expectedRevision });
}

// ──────────────────────────────────────────────
// Context items
// ──────────────────────────────────────────────

export function addContextItem(
  workspaceId: string,
  input: WorkspaceContextItemInput,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceContextItem | null> {
  return call('workspace_context_add', { workspaceId, input, expectedRevision });
}

export function removeContextItem(
  workspaceId: string,
  itemId: string,
  expectedRevision?: ExpectedRevision,
): Promise<boolean> {
  return call('workspace_context_remove', { workspaceId, itemId, expectedRevision });
}

export function batchUpdateContextItems(
  workspaceId: string,
  patches: WorkspaceContextItemPatch[],
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceContextItem[]> {
  return call('workspace_context_batch_update', { workspaceId, patches, expectedRevision });
}

export function reorderContextItems(
  workspaceId: string,
  orderedIds: string[],
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceContextItem[]> {
  return call('workspace_context_reorder', { workspaceId, orderedIds, expectedRevision });
}

export function getWorkspaceMcpExposure(
  workspaceId: string,
): Promise<WorkspaceMcpExposure | null> {
  return call('workspace_mcp_exposure', { workspaceId });
}

// ──────────────────────────────────────────────
// Widgets
// ──────────────────────────────────────────────

export function upsertWidget(
  workspaceId: string,
  input: WorkspaceWidgetInput,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceWidget | null> {
  return call('workspace_widget_upsert', { workspaceId, input, expectedRevision });
}

export function removeWidget(
  workspaceId: string,
  widgetId: string,
  expectedRevision?: ExpectedRevision,
): Promise<boolean> {
  return call('workspace_widget_remove', { workspaceId, widgetId, expectedRevision });
}

/** Contract `batch_update_widget_configs`: apply config patches in one transaction. */
export function batchUpdateWidgetConfigs(
  workspaceId: string,
  updates: WorkspaceWidgetConfigPatch[],
  expectedRevision?: ExpectedRevision,
): Promise<number> {
  return call('workspace_widget_batch_update', { workspaceId, updates, expectedRevision });
}

// ──────────────────────────────────────────────
// Layouts
// ──────────────────────────────────────────────

export function saveLayout(
  workspaceId: string,
  breakpoint: string,
  layoutJson: string,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceLayout | null> {
  return call('workspace_layout_save', { workspaceId, breakpoint, layoutJson, expectedRevision });
}

// ──────────────────────────────────────────────
// View states
// ──────────────────────────────────────────────

export function saveViewState(
  workspaceId: string,
  viewKey: string,
  stateJson: string,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceViewState | null> {
  return call('workspace_view_state_save', { workspaceId, viewKey, stateJson, expectedRevision });
}

// ──────────────────────────────────────────────
// Tool profiles
// ──────────────────────────────────────────────

export function bindToolProfile(
  workspaceId: string,
  profileId: string,
  toolKey: string | null,
  configJson: string,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceToolProfile | null> {
  return call('workspace_tool_profile_bind', {
    workspaceId,
    profileId,
    toolKey,
    configJson,
    expectedRevision,
  });
}

export function unbindToolProfile(
  workspaceId: string,
  profileId: string,
  expectedRevision?: ExpectedRevision,
): Promise<boolean> {
  return call('workspace_tool_profile_unbind', { workspaceId, profileId, expectedRevision });
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
