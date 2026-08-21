'use client';

/**
 * Widget 渲染映射：widget_type → React 组件（代码注册，非 Plugin）。
 *
 * 每个组件只消费现有 Domain query/facade 的 ViewModel（决策 7），
 * 不直接碰 SQLite / 文件扫描 / Provider / secret / 自建高频 Timer。
 */

import type { ComponentType } from 'react';
import { AppLauncherWidget } from './AppLauncherWidget';
import { GreetingWidget } from './GreetingWidget';
import { RecentFilesWidget } from './RecentFilesWidget';
import { TodayUsageWidget } from './TodayUsageWidget';
import { AiStatusWidget } from './AiStatusWidget';
import { StorageOverviewWidget } from './StorageOverviewWidget';
import { TokenMetricsWidget } from './TokenMetricsWidget';

const WIDGET_COMPONENTS: Record<string, ComponentType> = {
  greeting: GreetingWidget,
  recent_files: RecentFilesWidget,
  app_launcher: AppLauncherWidget,
  today_usage: TodayUsageWidget,
  ai_status: AiStatusWidget,
  storage_overview: StorageOverviewWidget,
  token_metrics: TokenMetricsWidget,
};

export function getWidgetComponent(widgetType: string): ComponentType | undefined {
  return WIDGET_COMPONENTS[widgetType];
}
