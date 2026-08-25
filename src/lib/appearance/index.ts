'use client';

/**
 * appearance — 外观域 barrel（TH-02 单一协调权威）。
 *
 * 业务组件经此进口：coordinator（主题协调权威）+ 兼容类型。
 * 不要向该 barrel 导入 UI 原子（R-E3：ui 原子不得反向 import 业务模块）。
 */

export {
  createAppearanceCoordinator,
  getAppearanceCoordinator,
  resolveThemeHost,
  parseThemeId,
  classifyThemeError,
  ThemeCoordinatorError,
  DEFAULT_APPEARANCE_THEME,
  THEME_CHANNEL,
  applyTheme,
} from './coordinator';

export type {
  AppearanceCoordinator,
  AppearanceCoordinatorOptions,
  AppearanceSnapshot,
  ThemeHost,
  ThemeId,
  ThemeErrorPayload,
} from './coordinator';