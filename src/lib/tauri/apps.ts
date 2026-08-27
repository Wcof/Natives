/**
//! tauri/apps — Apps Domain API 契约（APP-049 / 06 IPC 契约）。
//!
//! 应用中心前端唯一 IPC 入口，强类型 AppKind / RegistrationOrigin / AppCapabilities / AppView。
//!
//! APP-01 / APP-02：`AppView` 等 DTO 统一使用 `@/types/generated`（ts-rs 从 Rust
//! struct 生成的单一来源），本模块不再手工定义同字段影子类型。生成契约只被
//! `appsApi.*` 的 adapter 边界消费；adapter 在边界做 trust-boundary 校验：
//!   1. `appId` 必须非空字符串（身份健全性红线，禁止静默 fallback 到 undefined）；
//!   2. 列表投影必须 appId 唯一（重复 id 视为契约损坏，抛出结构化错误）。
//! 校验失败抛出 [`AppContractError`]，由上层错误分类器展示（R-F5），不吞错降级。
//! 注意：ts-rs 生成的 `kind`/`registrationOrigin` 为 `string`；本模块用
//! `AppKind`/`RegistrationOrigin` 联合类型收窄，避免组件侧丢失字符串字面量推断。
*/

import { cmd } from './core';

// ── 统一类型（ts-rs 生成，单一来源）────────────────────────────────────────────────
// AppView / AppKind / RegistrationOrigin / AppRuntimeState / AppCapabilities 由
// ts-rs 从 Rust struct 生成（见 src/types/generated/）。本模块 re-export 并只做
// 字面量收窄与 trust-boundary 校验，不重复定义同字段影子类型（APP-02）。
import type {
  AppCapabilities as AppCapabilitiesWire,
  AppKind as AppKindWire,
  AppRuntimeState as AppRuntimeStateWire,
  AppView as AppViewWire,
  LocalProjectSpec as LocalProjectSpecWire,
  RegistrationOrigin as RegistrationOriginWire,
  SystemApplicationSpec as SystemApplicationSpecWire,
  WebApplicationSpec as WebApplicationSpecWire,
} from '@/types/generated';

export type WebApplicationSpec = WebApplicationSpecWire;
export type SystemApplicationSpec = SystemApplicationSpecWire;
export type LocalProjectSpec = LocalProjectSpecWire;

/**
 * AppView wire 契约 + 前端字面量收窄。
 *
 * 生成类型把 `kind`/`registrationOrigin` 声明为 `string`（Rust 侧为 String 字段，
 * 值域由 AppKind/RegistrationOrigin 保证）；这里用联合类型收窄，让组件侧
 * 的 `app.kind === 'web_application'` 等比较保有穷举/校验能力。
 */
export type AppView = Omit<AppViewWire, 'kind' | 'registrationOrigin'> & {
  kind: AppKind;
  registrationOrigin: RegistrationOrigin;
};

export type AppKind = AppKindWire;
export type RegistrationOrigin = RegistrationOriginWire;
export type AppRuntimeState = AppRuntimeStateWire;
export type AppCapabilities = AppCapabilitiesWire;

/**
 * App —— `applications` 表行（read-through）。
 * 与生成类型一致，但经 adapter 返回；保持独立声明以便与 wire 对齐演进。
 */
export type App = {
  id: string;
  source: string;
  sourceId: string;
  title: string;
  description?: string;
  icon?: string;
  version: string;
  createdAt: string;
  updatedAt: string;
};

/**
 * RuntimeInstance —— `runtime_instances` 表行。
 *
 * 注：本类型未纳入 ts-rs 收敛范围。`apps_list_instances` wire 字段含宿主侧
 * `ownershipMode` / `externalIdentity`（AppDetail 等组件正在消费，Rust model
 * 未投影或缺省）；字段变化应先回写 Rust `apps::model::RuntimeInstance` 再生成。
 */
export type RuntimeInstance = {
  id: string;
  applicationId: string;
  planId?: string;
  status: string;
  cleanupStatus?: string;
  ownerKind: string;
  pgid?: number;
  currentPort?: number;
  pid?: number;
  /** legacy/扩展字段：wire 可能缺失时 UI 侧已用可选链访问。 */
  ownershipMode?: string;
  externalIdentity?: string;
  failure?: string;
  createdAt: string;
  updatedAt: string;
};

/**
 * Surface —— `application_surfaces` 表行镜像（复用 creative_app 成熟类型）。
 * 未纳入 ts-rs 生成（Rust 侧为 `ApplicationSurface`，独立模块），
 * 这里保留 adapter 层声明，字段与 `apps_list_surfaces` 命令的 wire 对齐。
 */
export type Surface = {
  id: string;
  applicationId: string;
  kind: string;
  label: string;
  title?: string;
  url?: string;
  boundsJson?: string;
  createdAt: string;
  updatedAt: string;
};

export type AppHealthResult = {
  healthy: boolean;
  status: string;
  port: number | null;
};

export type SystemAppCandidate = {
  bundleId?: string;
  path: string;
  displayName: string;
  iconPath?: string;
};

export type SystemRunningState = {
  installed: boolean;
  running: boolean;
  pid?: number;
  hidden: boolean;
  active: boolean;
  bundlePath?: string;
  bundleId?: string;
  unobservable: boolean;
};

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

/** APPV2-T03：内容区矩形（与 Rust `BrowserBounds` camelCase 对齐）。 */
export type WebBounds = {
  x: number;
  y: number;
  width: number;
  height: number;
};

// ── 输入 DTO（Rust 侧定义 wire 形状；这里只做 TS 面声明对齐）─────────────────────────

export type RegisterSystemApplicationInput = {
  title: string;
  applicationPath: string;
  bundleIdentifier?: string;
  platform: string;
  launchPolicy?: string;
};

export type RegisterWebApplicationInput = {
  title: string;
  url: string;
  approvedOrigins?: string[];
  openBehavior?: string;
  keepAlive?: boolean;
};

export type UpdateAppMetadataInput = {
  appId: string;
  title?: string;
  description?: string;
  icon?: string;
  showInSidebar?: boolean;
  sidebarOrder?: number;
};

export type UpdateSystemApplicationSpecInput = {
  appId: string;
  applicationPath?: string;
  bundleIdentifier?: string;
  platform?: string;
  launchPolicy?: string;
};

export type UpdateWebApplicationSpecInput = {
  appId: string;
  url?: string;
  approvedOrigins?: string[];
  openBehavior?: string;
  keepAlive?: boolean;
};

// ── Trust-boundary 校验 ─────────────────────────────────────────────────────────────

/** 契约在哪一层违约的分类（供错误分类器/日志定位，不直接拼进 UI 文案）。 */
export type AppContractViolation =
  | 'EMPTY_APP_ID'
  | 'DUPLICATE_APP_IDS'
  | 'MALFORMED_VIEW';

/**
 * 结构化契约错误：adapter 边界校验到无效 AppView 数据时抛出。
 *
 * 命名与 `cmd` 的 `Error` 区分（后者是 IPC 失败），使上层可以按类别处理而
 * 不会静默吞掉宿主侧的契约漂移。`rawMessage` 保留可诊断信息（R-F5 前经
 * 错误分类器再展示，组件不直接弹原始信息）。
 */
export class AppContractError extends Error {
  readonly code: AppContractViolation;
  readonly appId?: string | undefined;
  readonly duplicateId?: string | undefined;

  constructor(code: AppContractViolation, message: string, opts?: { appId?: string; duplicateId?: string }) {
    super(`AppView contract violation [${code}]: ${message}`);
    this.name = 'AppContractError';
    this.code = code;
    this.appId = opts?.appId;
    this.duplicateId = opts?.duplicateId;
  }
}

/**
 * AppView trust-boundary 校验：`appId` 必须非空。
 *
 * 运行健全性底线（APP-02）：渲染路径依赖 `appId` 作为唯一键、navigation id、
 * IPC 目标（open/stop/restart/setSidebar 等）。空值会让这些路径在运行时
 * 静默指向错误目标——这里直接抛结构化错误，禁止把空 id 放行进 UI。
 */
export function assertNonEmptyAppId(value: unknown, context: string): asserts value is string {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new AppContractError(
      'EMPTY_APP_ID',
      `${context}: appId must be a non-empty string, received ${typeof value === 'string' ? 'empty string' : typeof value}`,
    );
  }
}

/** 单个 AppView 的完整 trust-boundary 校验（字段形状 + appId 非空）。 */
export function validateAppView(view: unknown, context: string): AppView {
  if (!view || typeof view !== 'object') {
    throw new AppContractError('MALFORMED_VIEW', `${context}: AppView must be an object`);
  }
  assertNonEmptyAppId((view as AppViewWire).appId, context);
  return view as AppView;
}

/**
 * 列表投影的 appId 唯一性校验（APP-02 身份健全性）。
 *
 * 同一注册表里重复出现同一 `appId` 意味着 ID 冲突（投影 bug 或数据损坏），
 * 会让 React key 冲突、导航歧义、操作作用到错误应用。检测到即抛错，
 * 不静默去重——去重会掩盖故障源。
 */
export function assertUniqueAppIds(views: AppView[], context: string): void {
  const seen = new Set<string>();
  for (const v of views) {
    if (seen.has(v.appId)) {
      throw new AppContractError('DUPLICATE_APP_IDS', `${context}: duplicate appId: ${v.appId}`, {
        duplicateId: v.appId,
      });
    }
    seen.add(v.appId);
  }
}

/** 把 wire AppView 数组规范化并做唯一性校验。 */
export function parseAppViews(value: unknown, context: string): AppView[] {
  if (!Array.isArray(value)) {
    throw new AppContractError('MALFORMED_VIEW', `${context}: 期望 AppView[]`);
  }
  const views = value.map((v) => validateAppView(v, context));
  assertUniqueAppIds(views, context);
  return views;
}

/** 单个 view（可能为 null）规范化；null 表示不存在，不做唯一性约束。 */
export function parseAppViewOrNull(value: unknown, context: string): AppView | null {
  if (value === null || value === undefined) return null;
  return validateAppView(value, context);
}

export const appsApi = {
  // ── Query ───────────────────────────────────────────────────────────────
  listViews: async () => parseAppViews(await cmd<AppViewWire[]>('apps_list_views'), 'apps_list_views'),
  get: (id: string) => cmd<App | null>('apps_get', { id }),
  getView: async (id: string) =>
    parseAppViewOrNull(await cmd<AppViewWire | null>('apps_get_view', { id }), 'apps_get_view'),
  getWebSpec: (applicationId: string) =>
    cmd<WebApplicationSpec | null>('apps_get_web_spec', { applicationId }),
  getSystemSpec: (applicationId: string) =>
    cmd<SystemApplicationSpec | null>('apps_get_system_spec', { applicationId }),
  getLocalSpec: (applicationId: string) =>
    cmd<LocalProjectSpec | null>('apps_get_local_spec', { applicationId }),
  listInstances: (applicationId: string) => cmd<RuntimeInstance[]>('apps_list_instances', { applicationId }),
  listSurfaces: (applicationId: string) => cmd<Surface[]>('apps_list_surfaces', { applicationId }),
  activeSpec: (applicationId: string) => cmd<unknown | null>('apps_active_spec', { applicationId }),
  systemDiscover: () => cmd<SystemAppCandidate[]>('apps_system_discover'),

  // ── Register ────────────────────────────────────────────────────────────
  registerSystem: async (input: RegisterSystemApplicationInput) =>
    parseAppViewOrNull(await cmd<AppViewWire>('apps_register_system', { input }), 'apps_register_system')!,
  registerWeb: async (input: RegisterWebApplicationInput) =>
    parseAppViewOrNull(await cmd<AppViewWire>('apps_register_web', { input }), 'apps_register_web')!,

  // ── Mutate ──────────────────────────────────────────────────────────────
  updateMetadata: async (input: UpdateAppMetadataInput) =>
    parseAppViewOrNull(await cmd<AppViewWire>('apps_update_metadata', { input }), 'apps_update_metadata')!,
  updateSystemSpec: async (input: UpdateSystemApplicationSpecInput) =>
    parseAppViewOrNull(await cmd<AppViewWire>('apps_update_system_spec', { input }), 'apps_update_system_spec')!,
  updateWebSpec: async (input: UpdateWebApplicationSpecInput) =>
    parseAppViewOrNull(await cmd<AppViewWire>('apps_update_web_spec', { input }), 'apps_update_web_spec')!,
  setSidebarVisibility: async (id: string, show: boolean) =>
    parseAppViewOrNull(await cmd<AppViewWire>('apps_set_sidebar_visibility', { id, show }), 'apps_set_sidebar_visibility')!,
  setSidebarOrder: async (id: string, order: number | null) =>
    parseAppViewOrNull(await cmd<AppViewWire>('apps_set_sidebar_order', { id, order }), 'apps_set_sidebar_order')!,
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
