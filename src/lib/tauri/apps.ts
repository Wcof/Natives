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
  updatedAt?: string;
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

export interface SystemRunningState {
  installed: boolean;
  running: boolean;
  pid?: number;
  hidden: boolean;
  active: boolean;
  bundlePath?: string;
  bundleId?: string;
  unobservable: boolean;
}

export type DockStatus = 'docked' | 'permission_required' | 'unsupported' | 'failed';
export type DockCapability =
  | 'available'
  | 'permission_required'
  | 'no_standard_window'
  | 'not_movable'
  | 'not_resizable'
  | 'fullscreen_unsupported';
export interface DockResult {
  status: DockStatus;
  capability: DockCapability;
  message?: string;
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

/** APPV2-T03：内容区矩形（与 Rust `BrowserBounds` camelCase 对齐）。 */
export interface WebBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const appsApi = {
  // ── Query ───────────────────────────────────────────────────────────────
  listViews: () => cmd<AppView[]>('apps_list_views'),
  get: (id: string) => cmd<App | null>('apps_get', { id }),
  getView: (id: string) => cmd<AppView | null>('apps_get_view', { id }),
  listInstances: (applicationId: string) => cmd<RuntimeInstance[]>('apps_list_instances', { applicationId }),
  listSurfaces: (applicationId: string) => cmd<Surface[]>('apps_list_surfaces', { applicationId }),
  activeSpec: (applicationId: string) => cmd<unknown | null>('apps_active_spec', { applicationId }),
  systemDiscover: () => cmd<SystemAppCandidate[]>('apps_system_discover'),

  // ── Register ────────────────────────────────────────────────────────────
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
  open: (id: string, bounds?: WebBounds) => cmd<boolean>('apps_open', { id, bounds }),
  start: (id: string) => cmd<boolean>('apps_start', { id }),
  stop: (id: string, riskLevel?: number) => cmd<boolean>('apps_stop', { id, riskLevel }),
  restart: (id: string) => cmd<boolean>('apps_restart', { id }),
  systemObserve: (id: string) => cmd<SystemRunningState>('apps_system_observe', { id }),
  systemHide: (id: string) => cmd<boolean>('apps_system_hide', { id }),
  systemUnhide: (id: string) => cmd<boolean>('apps_system_unhide', { id }),
  systemDock: (id: string, bounds: WebBounds) => cmd<DockResult>('apps_system_dock', { id, bounds }),
  systemOpenAccessibilitySettings: () => cmd<boolean>('apps_system_open_accessibility_settings'),
  activateHost: () => cmd<boolean>('apps_activate_host'),

  // ── Web Surface ─────────────────────────────────────────────────────────
  webClose: (id: string) => cmd<boolean>('apps_web_close', { id }),
  webHide: (id: string) => cmd<boolean>('apps_web_hide', { id }),
  webReload: (id: string) => cmd<boolean>('apps_web_reload', { id }),
  webBack: (id: string) => cmd<boolean>('apps_web_back', { id }),
  webForward: (id: string) => cmd<boolean>('apps_web_forward', { id }),
  /** APPV2-T03：Web Surface 动态内容区 bounds（Host 验证/clamp 后返回实际生效值）。 */
  webSetBounds: (id: string, bounds: WebBounds) => cmd<WebBounds>('apps_web_set_bounds', { id, bounds }),
  webClearData: (id: string) => cmd<boolean>('apps_web_clear_data', { id }),

};
