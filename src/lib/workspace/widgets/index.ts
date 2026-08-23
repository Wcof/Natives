// ── Workspace Widgets 库 barrel ──
// 供 Renderer / Inspector host / C 侧消费。

export * from './types';
export * from './registry';
export {
  createDefaultConfig,
  normalizeWidgetConfig,
  validateWidgetConfig,
  serializeWidgetConfig,
  setLastValidConfig,
  getLastValidConfig,
  readConfigVersion,
  WIDGET_CONFIG_VERSION,
} from './config';
export {
  WorkspaceDataBroker,
  workspaceDataBroker,
  buildWidgetBrokerKey,
} from './data-broker';
export { TimeRangeContext, useWorkspaceTimeRange } from './time-range-context';
export type { BrokerSnapshot, BrokerLoader, BrokerSubscribeOptions } from './data-broker';
export { buildInspectorSections } from './inspector';
export type { InspectorSection, InspectorField } from './inspector';

export type { GreetingData } from './adapters/greeting';
export type { RecentFilesData } from './adapters/recent-files';
export type { AppLauncherData } from './adapters/apps';
export type { UsageSummaryData } from './adapters/usage';
export type { AiStatusData } from './adapters/ai-status';
export type { StorageOverviewData } from './adapters/storage';
