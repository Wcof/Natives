/**
//! tauri/apps — Apps Domain API 契约（APP-049 / 06 IPC 契约）。
//!
//! 应用中心前端唯一 IPC 入口，强类型 AppKind / RegistrationOrigin / AppCapabilities / AppView。
*/

import { cmd } from './core';

export type AppKind = 'local_project' | 'system_application' | 'web_application';

export type RegistrationOrigin =
  | 'manual'
  | 'local_scan'
  | 'system_discovery'
  | 'legacy_internal'
  | 'legacy_github'
  | 'migration';

export type AppRuntimeState =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'failed'
  | 'orphaned'
  | 'hibernated';

export interface AppCapabilities {
  canStart: boolean;
  canStop: boolean;
  canRestart: boolean;
  canOpen: boolean;
  canEdit: boolean;
  canRemove: boolean;
  canSidebar: boolean;
  riskLevel: number;
}

export interface AppView {
  appId: string;
  title: string;
  kind: AppKind;
  registrationOrigin: RegistrationOrigin;
  description?: string;
  showInSidebar: boolean;
  sidebarOrder?: number;
  capabilities: AppCapabilities;
  runtimeState: AppRuntimeState;
}

export interface App {
  id: string;
  source: string;
  sourceId: string;
  title: string;
  description?: string;
  icon?: string;
  version: string;
  createdAt: string;
  updatedAt: string;
}

export interface RuntimeInstance {
  id: string;
  applicationId: string;
  planId?: string;
  status: string;
  cleanupStatus?: string;
  ownerKind: string;
  pgid?: number;
  currentPort?: number;
  pid?: number;
  ownershipMode?: string;
  externalIdentity?: string;
  failure?: string;
  createdAt: string;
  updatedAt: string;
}

export interface Surface {
  id: string;
  applicationId: string;
  kind: string;
  label: string;
  title?: string;
  url?: string;
  boundsJson?: string;
  createdAt: string;
  updatedAt: string;
}

export interface SystemAppCandidate {
  bundleId?: string;
  path: string;
  displayName: string;
  iconPath?: string;
}

export interface LocalProjectScanResult {
  projectRoot: string;
  projectKind: string;
  entryFile?: string;
  scripts: string[];
  candidatePlans: unknown[];
  rulePlan?: unknown;
  aiSuggestedPlan?: unknown;
}

export interface RegisterLocalProjectInput {
  title: string;
  projectRoot: string;
  description?: string;
  icon?: string;
}

export interface RegisterSystemApplicationInput {
  title: string;
  applicationPath: string;
  bundleIdentifier?: string;
  platform: string;
  launchPolicy?: string;
}

export interface RegisterWebApplicationInput {
  title: string;
  url: string;
  approvedOrigins: string[];
  openBehavior?: string;
  keepAlive?: boolean;
}

export interface UpdateAppMetadataInput {
  appId: string;
  title?: string;
  description?: string;
  icon?: string;
  showInSidebar?: boolean;
  sidebarOrder?: number;
}

export interface UpdateSystemApplicationSpecInput {
  appId: string;
  applicationPath?: string;
  bundleIdentifier?: string;
  platform?: string;
  launchPolicy?: string;
}

export interface UpdateWebApplicationSpecInput {
  appId: string;
  url?: string;
  approvedOrigins?: string[];
  openBehavior?: string;
  keepAlive?: boolean;
}

export interface AppHealthResult {
  healthy: boolean;
  status: string;
  port: number | null;
}

export const appsApi = {
  // ── Query ───────────────────────────────────────────────────────────────
  list: () => cmd<App[]>('apps_list'),
  listViews: () => cmd<AppView[]>('apps_list_views'),
  get: (id: string) => cmd<App | null>('apps_get', { id }),
  getView: (id: string) => cmd<AppView | null>('apps_get_view', { id }),
  listInstances: (applicationId: string) => cmd<RuntimeInstance[]>('apps_list_instances', { applicationId }),
  listSurfaces: (applicationId: string) => cmd<Surface[]>('apps_list_surfaces', { applicationId }),
  activeSpec: (applicationId: string) => cmd<unknown | null>('apps_active_spec', { applicationId }),
  systemDiscover: () => cmd<SystemAppCandidate[]>('apps_system_discover'),
  localInspect: (projectRoot: string) => cmd<LocalProjectScanResult>('apps_local_inspect', { projectRoot }),
  localLogs: (id: string, limit?: number) =>
    cmd<Array<{ seq: number; tsMs: number; stream: string; text: string }>>('apps_local_logs', { id, limit }),

  // ── Register ────────────────────────────────────────────────────────────
  registerLocal: (input: RegisterLocalProjectInput) => cmd<AppView>('apps_register_local', { input }),
  registerSystem: (input: RegisterSystemApplicationInput) => cmd<AppView>('apps_register_system', { input }),
  registerWeb: (input: RegisterWebApplicationInput) => cmd<AppView>('apps_register_web', { input }),

  // ── Mutate ──────────────────────────────────────────────────────────────
  updateMetadata: (input: UpdateAppMetadataInput) => cmd<AppView>('apps_update_metadata', { input }),
  updateSystemSpec: (input: UpdateSystemApplicationSpecInput) => cmd<AppView>('apps_update_system_spec', { input }),
  updateWebSpec: (input: UpdateWebApplicationSpecInput) => cmd<AppView>('apps_update_web_spec', { input }),
  setSidebarVisibility: (id: string, show: boolean) => cmd<AppView>('apps_set_sidebar_visibility', { id, show }),
  setSidebarOrder: (id: string, order: number | null) => cmd<AppView>('apps_set_sidebar_order', { id, order }),
  remove: (id: string, riskLevel?: number) => cmd<boolean>('apps_remove', { id, riskLevel }),

  // ── Lifecycle ───────────────────────────────────────────────────────────
  open: (id: string) => cmd<boolean>('apps_open', { id }),
  start: (id: string) => cmd<boolean>('apps_start', { id }),
  stop: (id: string, riskLevel?: number) => cmd<boolean>('apps_stop', { id, riskLevel }),
  restart: (id: string) => cmd<boolean>('apps_restart', { id }),
  forceStop: (id: string) => cmd<boolean>('apps_force_stop', { id }),
  resolveOrphan: (id: string, action: 'stop' | 'restart') => cmd<boolean>('apps_resolve_orphan', { id, action }),

  // ── Web Surface ─────────────────────────────────────────────────────────
  webClose: (id: string) => cmd<boolean>('apps_web_close', { id }),
  webHide: (id: string) => cmd<boolean>('apps_web_hide', { id }),
  webReload: (id: string) => cmd<boolean>('apps_web_reload', { id }),
  webBack: (id: string) => cmd<boolean>('apps_web_back', { id }),
  webForward: (id: string) => cmd<boolean>('apps_web_forward', { id }),
  webClearData: (id: string) => cmd<boolean>('apps_web_clear_data', { id }),

  // ── Legacy Compat ───────────────────────────────────────────────────────
  create: (input: { title: string; source: string; sourceId: string; description?: string; icon?: string }) =>
    cmd<App>('apps_create', { input }),
  delete: (id: string) => cmd<boolean>('apps_delete', { id }),
  kill: (id: string) => cmd<boolean>('apps_kill', { id }),
  health: (applicationId: string) => cmd<AppHealthResult>('apps_health', { applicationId }),
};
