'use client';

/**
 * Legacy home/widgets delegation to V2 Workspace widgets (B-D05).
 */

import type { ComponentType } from 'react';
import {
  greetingWidgetDefinition,
  recentFilesWidgetDefinition,
  appLauncherWidgetDefinition,
  todayUsageWidgetDefinition,
  aiStatusWidgetDefinition,
  storageOverviewWidgetDefinition,
  tokenMetricsWidgetDefinition,
  getWidget,
} from '@/components/workspace/widgets';

export {
  greetingWidgetDefinition as GreetingWidget,
  recentFilesWidgetDefinition as RecentFilesWidget,
  appLauncherWidgetDefinition as AppLauncherWidget,
  todayUsageWidgetDefinition as TodayUsageWidget,
  aiStatusWidgetDefinition as AiStatusWidget,
  storageOverviewWidgetDefinition as StorageOverviewWidget,
  tokenMetricsWidgetDefinition as TokenMetricsWidget,
  getWidget,
};

export function getWidgetComponent(widgetType: string): ComponentType | undefined {
  const def = getWidget(widgetType);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return def?.Component as ComponentType<any> | undefined;
}
