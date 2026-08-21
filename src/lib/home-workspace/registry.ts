'use client';

/**
 * WidgetRegistry —— 代码注册的内置 Widget 定义（ADR-0020：Widget 不是 Plugin）。
 *
 * 每个 Widget 是普通 React Component + 定义 + 配置；不出现 Widget Runtime、
 * Worker、IPC Protocol、Event Bus、Package Manager（Home Patch 决策 3）。
 * 组件只消费现有 Domain query/facade 的 ViewModel（决策 7），不直接碰
 * SQLite / 文件扫描 / Provider / secret。
 */

import type { WidgetDescriptor } from './model';

export const WIDGET_REGISTRY: readonly WidgetDescriptor[] = [
  {
    id: 'greeting',
    titleKey: 'home.widgetGreeting',
    defaultSize: { w: 4, h: 2 },
    minSize: { w: 2, h: 2 },
    maxSize: { w: 12, h: 4 },
    defaultVisible: true,
  },
  {
    id: 'recent_files',
    titleKey: 'home.widgetRecentFiles',
    defaultSize: { w: 4, h: 4 },
    minSize: { w: 2, h: 3 },
    maxSize: { w: 12, h: 8 },
    defaultVisible: true,
  },
  {
    id: 'app_launcher',
    titleKey: 'home.widgetAppLauncher',
    defaultSize: { w: 4, h: 4 },
    minSize: { w: 2, h: 3 },
    maxSize: { w: 12, h: 8 },
    defaultVisible: true,
  },
  {
    id: 'today_usage',
    titleKey: 'home.widgetTodayUsage',
    defaultSize: { w: 4, h: 2 },
    minSize: { w: 2, h: 2 },
    maxSize: { w: 12, h: 4 },
    defaultVisible: true,
  },
  {
    id: 'ai_status',
    titleKey: 'home.widgetAiStatus',
    defaultSize: { w: 4, h: 2 },
    minSize: { w: 2, h: 2 },
    maxSize: { w: 12, h: 4 },
    defaultVisible: true,
  },
  {
    id: 'storage_overview',
    titleKey: 'home.widgetStorageOverview',
    defaultSize: { w: 6, h: 4 },
    minSize: { w: 4, h: 3 },
    maxSize: { w: 12, h: 8 },
    defaultVisible: false,
  },
  {
    id: 'token_metrics',
    titleKey: 'home.widgetTokenMetrics',
    defaultSize: { w: 4, h: 3 },
    minSize: { w: 3, h: 2 },
    maxSize: { w: 12, h: 6 },
    defaultVisible: false,
  },
];

export function getWidgetDescriptor(id: string): WidgetDescriptor | undefined {
  return WIDGET_REGISTRY.find((widget) => widget.id === id);
}
