// ── Workspace V2 Widget Contract（§6） ──
// 代码注册（code-registered），非 Plugin Runtime。
// WidgetDefinition / SurfacePolicy / WidgetInstance 三件套。
//
// 本轮（B-013..B-029）扩展：
//   · configVersion / defaultConfig / configSchema / migrateConfig —— B-015 config pipeline
//   · adapterKeyBuilder —— B-016 DataBroker 同 key 去重/共享
//   · subscribe —— B-026 AI Status 事件驱动刷新（DB change / domain event）
//   · WidgetShellProps 增加 editing / onRemove —— B-018/B-019 edit chrome

import type { ComponentType, ReactNode } from 'react';
import type { ZodType } from 'zod';
import type { ElevationLevel } from '@/lib/design-tokens';

/** 小组件尺寸（RGL 网格约定）。 */
export type WidgetSize = 'small' | 'medium' | 'large';

/** 允许的玻璃/材质策略 —— 禁止所有 Widget 强制同一种玻璃卡。 */
export type WidgetSurfaceKind = 'material' | 'crystal' | 'plain';

export interface SurfacePolicy {
  /** 允许使用的表面材质（按顺序优先）。 */
  surfaces: WidgetSurfaceKind[];
  /** 是否允许局部 backdrop blur（默认 false，克制使用）。 */
  allowBlur?: boolean;
  /** 是否允许 glow（仅 focus/selected/status，默认 false）。 */
  allowGlow?: boolean;
}

/** Workspace-level time range for time-aware widgets. */
export type TimeRange = 'today' | '7d' | '30d' | '90d';

/** DataBroker 上下文：loader 从现有 Domain query/facade 取数据。 */
export interface WidgetDataContext {
  signal: AbortSignal;
  timeRange?: TimeRange;
}

/** Widget 内容组件收到的 props。 */
export interface WidgetProps<TData = unknown, TSettings extends Record<string, unknown> = Record<string, unknown>> {
  data: TData | null;
  loading: boolean;
  error: Error | null;
  retry: () => void;
  config: WidgetConfig<TSettings>;
}

/** 用户/系统可持久化的配置。 */
export interface WidgetConfig<TSettings extends Record<string, unknown> = Record<string, unknown>> {
  type: string;
  enabled: boolean;
  /** auto = 按 SurfacePolicy.surfaces[0] 默认。 */
  surface: 'auto' | WidgetSurfaceKind;
  order: number;
  size?: WidgetSize;
  settings?: TSettings;
}

/** 空 settings（无配置的 Widget 用）。 */
export type NoopSettings = Record<string, never>;

/** Widget 定义 —— 代码注册的权威描述。 */
export interface WidgetDefinition<TData = unknown, TSettings extends Record<string, unknown> = Record<string, unknown>> {
  type: string;
  /** i18n key（标题）。 */
  titleKey: string;
  descriptionKey?: string;
  /** 当前 config schema 版本（B-015 迁移管线）。 */
  configVersion: number;
  /** 默认 settings（与 configSchema 匹配）。 */
  defaultConfig: TSettings;
  /** settings 的 Zod schema（B-015 校验）。 */
  configSchema: ZodType<TSettings>;
  /** 旧版本 config → 当前版本迁移（可选）。 */
  migrateConfig?: (version: number, raw: unknown) => TSettings;
  size: WidgetSize;
  surfacePolicy: SurfacePolicy;
  /** RGL 最小尺寸。 */
  minSize?: { w: number; h: number };
  defaultEnabled?: boolean;
  /** true → broker key 包含 timeRange，切换时段触发独立 fetch。 */
  timeAware?: boolean;
  /** DataBroker adapter key：同 key 的 Widget 共享一次查询/订阅（B-016）。 */
  adapterKeyBuilder?: (config: WidgetConfig<TSettings>) => string;
  /** DataBroker loader：返回 widget 所需 ViewModel。Renderer 不直连 IPC。 */
  load?: (ctx: WidgetDataContext) => Promise<TData>;
  /** 事件驱动刷新（DB change / domain event）；返回取消订阅函数。 */
  subscribe?: (emit: () => void) => (() => void) | void;
  Component: ComponentType<WidgetProps<TData, TSettings>>;
}

/** 已解析的运行实例（definition + config）。 */
export interface WidgetInstance<TData = unknown, TSettings extends Record<string, unknown> = Record<string, unknown>> {
  def: WidgetDefinition<TData, TSettings>;
  config: WidgetConfig<TSettings>;
}

/** WidgetShell 渲染插槽。 */
export interface WidgetShellProps<TData = unknown, TSettings extends Record<string, unknown> = Record<string, unknown>> {
  instance: WidgetInstance<TData, TSettings>;
  /** 面板右上角操作。 */
  actions?: ReactNode;
  children: ReactNode;
  /** 自定义 elevation（默认按 surface 策略推导）。 */
  elevation?: ElevationLevel;
  className?: string;
  /** 编辑态：显示 drag handle / edit chrome（B-019）。 */
  editing?: boolean;
  /** 编辑态移除回调（host 注入）。 */
  onRemove?: () => void;
}

export const DEFAULT_WIDGET_MIN_SIZE: Record<WidgetSize, { w: number; h: number }> = {
  small: { w: 2, h: 2 },
  medium: { w: 4, h: 3 },
  large: { w: 6, h: 4 },
};
