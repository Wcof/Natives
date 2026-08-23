'use client';

/**
 * Workspace V2 Widgets barrel (B-021..B-034).
 * Code-registered built-in widgets for the Workspace Framework.
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

// Wave 2 widgets
import { costMetricsWidgetDefinition } from './CostMetricsWidget';
import { workTimeWidgetDefinition } from './WorkTimeWidget';
import { distributionChartWidgetDefinition } from './DistributionChartWidget';
import { toolStatusWidgetDefinition } from './ToolStatusWidget';
import { proxyStatusWidgetDefinition } from './ProxyStatusWidget';
import { notesWidgetDefinition } from './NotesWidget';
import { promptSnippetsWidgetDefinition } from './PromptSnippetsWidget';
import { quickLinksWidgetDefinition } from './QuickLinksWidget';
import { dataViewWidgetDefinition } from './DataViewWidget';

// ── 注册全部 15 个内置 Widget ──
registerWidgets([
  greetingWidgetDefinition,
  recentFilesWidgetDefinition,
  appLauncherWidgetDefinition,
  todayUsageWidgetDefinition,
  tokenMetricsWidgetDefinition,
  aiStatusWidgetDefinition,
  storageOverviewWidgetDefinition,
  costMetricsWidgetDefinition,
  workTimeWidgetDefinition,
  distributionChartWidgetDefinition,
  toolStatusWidgetDefinition,
  proxyStatusWidgetDefinition,
  notesWidgetDefinition,
  promptSnippetsWidgetDefinition,
  quickLinksWidgetDefinition,
  dataViewWidgetDefinition,
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
  costMetricsWidgetDefinition,
  workTimeWidgetDefinition,
  distributionChartWidgetDefinition,
  toolStatusWidgetDefinition,
  proxyStatusWidgetDefinition,
  notesWidgetDefinition,
  promptSnippetsWidgetDefinition,
  quickLinksWidgetDefinition,
  dataViewWidgetDefinition,
};

/** 已注册内置 Widget 定义（registry 只读视图）。 */
export const BUILTIN_WIDGET_DEFINITIONS = getAllWidgets();

/** 按 type 取已注册内置定义。 */
export { getWidget, hasWidget };
