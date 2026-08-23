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
  WorkspaceTemplate,
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
  layoutMode: 'structured' | 'free',
  breakpoint: string,
  layout: unknown,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceLayout | null> {
  return call('workspace_layout_save', { workspaceId, layoutMode, breakpoint, layoutVersion: 1, layoutJson: JSON.stringify(layout), expectedRevision });
}

// ──────────────────────────────────────────────
// View states
// ──────────────────────────────────────────────

export function saveViewState(
  workspaceId: string,
  viewKey: string,
  state: Record<string, unknown>,
  expectedRevision?: ExpectedRevision,
): Promise<WorkspaceViewState | null> {
  return call('workspace_view_state_save', { workspaceId, viewKey, stateVersion: 1, stateJson: JSON.stringify(state), expectedRevision });
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

export function closeSession(workspaceId: string): Promise<WorkspaceSessionSnapshot> {
  return call('workspace_session_close', { workspaceId });
}

export function getSessionSnapshot(): Promise<WorkspaceSessionSnapshot> {
  return call('workspace_session_snapshot');
}

export function reorderSessions(orderedWorkspaceIds: string[]): Promise<WorkspaceSessionSnapshot> {
  return call('workspace_session_reorder', { orderedWorkspaceIds });
}

export function listTemplates(): Promise<WorkspaceTemplate[]> {
  return call('workspace_template_list');
}

export function savePersonalTemplate(workspaceId: string, name: string, id?: string): Promise<WorkspaceTemplate | null> {
  return call('workspace_template_save', { workspaceId, req: { name, id } });
}

export function deletePersonalTemplate(templateId: string): Promise<boolean> {
  return call('workspace_template_delete', { templateId });
}

export function restoreTemplate(workspaceId: string, templateId: string, expectedRevision?: number): Promise<WorkspaceSnapshot | null> {
  return call('workspace_restore_template', { workspaceId, templateId, expectedRevision });
}

export function resetWidget(workspaceId: string, widgetId: string, templateId: string | undefined, expectedRevision?: number): Promise<WorkspaceSnapshot | null> {
  return call('workspace_widget_reset', { workspaceId, widgetId, templateId, expectedRevision });
}
