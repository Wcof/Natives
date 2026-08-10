/**
 * Shared assistant UI types (W4): provider/model options, command palette,
 * and workspace navigation snapshot. Moved out of component files so hooks
 * never depend on component internals. Components re-export these for
 * backwards compatibility.
 */
import type { AssistantProjectGroup, AssistantProjectCreationState } from './assistant-project-groups';
import type { TempConversationSession } from './assistant-temp-conversation';
import type { AssistantFileChange, AssistantRunEvent } from './assistant-types';

/** Discovered model metadata (kept in the provider option). */
export interface ModelInfo {
  id: string;
  displayName?: string;
  contextWindow?: number;
  maxOutput?: number;
  capabilities?: {
    streaming?: boolean;
    toolCalling?: boolean;
    imageInput?: boolean;
    reasoning?: boolean;
  };
  source?: 'api_discovery' | 'cache' | 'preset' | 'manual';
  discoveredAt?: string;
}

/** A provider with its keys and optional discovered models (model picker input). */
export interface ProviderWithModels {
  id: string;
  name: string;
  presetName: string;
  baseUrl: string;
  keys: Array<{
    id: string;
    label: string;
    maskedKey: string;
    isActive?: boolean;
    status?: 'untested' | 'valid' | 'invalid' | 'rate_limited' | 'unavailable' | string;
  }>;
  models?: ModelInfo[];
  defaultModel?: string | null;
}

/** One command palette entry. */
export interface AssistantCommand {
  id: string;
  label: string;
  description?: string;
  shortcut?: string;
  disabledReason?: string;
  run: () => void;
}

/**
 * Navigation snapshot published by the workspace provider: project groups,
 * selection, active project, and the local temp-session bookkeeping.
 */
export interface AssistantNavigationSnapshot {
  groups: AssistantProjectGroup[];
  selectedId: string | null;
  activeProjectPath: string | null;
  loading: boolean;
  /**
   * @deprecated Write-only legacy field: published by AssistantWorkbench but never
   * read anywhere (engine/provider readiness is surfaced elsewhere). Kept only so
   * AssistantWorkbench.tsx keeps type-checking this round; removal is scheduled
   * for the next pass together with its Workbench write site. Use `loadError` for
   * navigation fetch failures instead.
   */
  creationState: AssistantProjectCreationState;
  /**
   * Localized message when the last host navigation refresh failed. While set,
   * `groups` keeps the previous (possibly stale) data instead of being replaced
   * by an empty list — an engine outage must never look like "history deleted".
   * Optional so the Workbench (frozen this round) can keep publishing full
   * snapshots without the field; absent means "no known failure".
   */
  loadError?: string | null;
  isCreatingConversation: boolean;
  pendingCreateProjectPath?: string | null;
  /**
   * Local-only blank session created after project pick / "new conversation".
   * Never listed in `groups`, never written to the host DB until first send.
   */
  tempSession: TempConversationSession | null;
}

/**
 * Runtime snapshot published by the workspace provider: the currently visible
 * conversation/run/artifacts/usage for the assistant pane. Moved out of the
 * component file (W5) so hooks never depend on component internals.
 */
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

/**
 * The single action bag the workspace shell drives. Registered by the Workbench
 * while mounted; `registerActions(null)` on unmount. Moved out of the component
 * file (W5) so hooks never depend on component internals.
 */
export interface AssistantWorkspaceActions {
  selectConversation(id: string): void;
  selectProject(path: string | null): void;
  addProjectFolder(): void;
  createConversation(): void;
  createConversationInProject(path: string): void;
  removeProject(path: string): Promise<boolean>;
  renameProject(path: string, label: string): Promise<boolean>;
  renameConversation(id: string, title: string): void;
  archiveConversation(id: string): void;
  deleteConversation(id: string): Promise<boolean>;
  pinConversation(id: string, projectId: string | null, pinned: boolean): void;
  retryRun(): void;
  respondPermission(requestId: string, approved: boolean): void;
}
