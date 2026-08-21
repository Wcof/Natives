'use client';

/**
 * Workspace V2 Widgets barrel（B-021..B-028）。
 * 注册 7 个内置 Widget 到 lib registry（code-registered）。
 * 说明：registry.ts 是注册框架（type 唯一校验）；把 7 个内置定义的注册
 * 收敛在这里，避免 lib → components 的循环依赖（lib 不 import components）。
 * 任何模块 import 本 barrel 即完成注册；注册冲突会显式 throw。
 */

import {
  registerWidgets,
  getAllWidgets,
  getWidget,
  hasWidget,
} from '@/lib/workspace/widgets/registry';

import { greetingWidgetDefinition } from './GreetingWidget';
import { recentFilesWidgetDefinition } from './RecentFilesWidget';
import { appLauncherWidgetDefinition } from './AppLauncherWidget';
import { todayUsageWidgetDefinition } from './TodayUsageWidget';
import { tokenMetricsWidgetDefinition } from './TokenMetricsWidget';
import { aiStatusWidgetDefinition } from './AiStatusWidget';
import { storageOverviewWidgetDefinition } from './StorageOverviewWidget';

// ── 注册全部 7 个内置 Widget（重复 type 在此显式失败） ──
registerWidgets([
  greetingWidgetDefinition,
  recentFilesWidgetDefinition,
  appLauncherWidgetDefinition,
  todayUsageWidgetDefinition,
  tokenMetricsWidgetDefinition,
  aiStatusWidgetDefinition,
  storageOverviewWidgetDefinition,
] as const);

export { WidgetRenderer } from './WidgetRenderer';
export type { WidgetRendererProps } from './WidgetRenderer';
export { WidgetShell } from './WidgetShell';
export type { WidgetShellProps } from '@/lib/workspace/widgets';

export {
  greetingWidgetDefinition,
  recentFilesWidgetDefinition,
  appLauncherWidgetDefinition,
  todayUsageWidgetDefinition,
  tokenMetricsWidgetDefinition,
  aiStatusWidgetDefinition,
  storageOverviewWidgetDefinition,
};

/** 已注册内置 Widget 定义（registry 只读视图）。 */
export const BUILTIN_WIDGET_DEFINITIONS = getAllWidgets();

/** 按 type 取已注册内置定义。 */
export { getWidget, hasWidget };
