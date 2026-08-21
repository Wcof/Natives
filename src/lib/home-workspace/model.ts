'use client';

/**
 * Home Workspace 文档模型（ADR-0020 / Home Patch 决策 3/8）。
 *
 * - `WidgetDescriptor`  —— 代码注册的 Widget 定义（不是 Plugin）。
 * - `WidgetInstance`    —— 用户放置的实例（widget_type + config + layout）。
 * - `HomeWorkspaceDocument` —— 单份版本化 JSON，存 settings K/V（决策 8），
 *   V1 不建 Workspace 表、不做多 Workspace。
 */

import type { ResponsiveLayouts } from 'react-grid-layout';

/** 每个 Widget 的注册元数据（WidgetRegistry 代码常量）。 */
export interface WidgetDescriptor {
  id: string;
  titleKey: string;
  /** 默认尺寸（网格单元）。 */
  defaultSize: { w: number; h: number };
  /** 允许的最小/最大尺寸。 */
  minSize: { w: number; h: number };
  maxSize: { w: number; h: number };
  /** 是否默认出现在初始布局。 */
  defaultVisible: boolean;
  /** 配置 JSON Schema 描述（V1 仅用于类型提示，不执行任意代码）。 */
  configSchema?: Record<string, unknown>;
}

/** 断点与列配置（与 HOME-P0-GRID-SPIKE 一致，已通过浏览器 Stage）。 */
export const BREAKPOINTS = { lg: 1000, md: 720, sm: 0 } as const;
export const COLUMNS = { lg: 12, md: 8, sm: 4 } as const;

export type HomeBreakpoint = keyof typeof BREAKPOINTS;

/** Home 使用的响应式布局类型（固定 lg/md/sm 三断点）。 */
export type HomeResponsiveLayouts = ResponsiveLayouts<HomeBreakpoint>;

/** 用户放置的一个 Widget 实例。 */
export interface WidgetInstance {
  /** 稳定实例 id（uuid，非 widget_type）。 */
  id: string;
  widget_type: string;
  /** 用户配置（序列化到文档，含选择器/阈值等非敏感设置）。 */
  config: Record<string, unknown>;
}

/** 单份版本化 Home 文档。 */
export interface HomeWorkspaceDocument {
  schemaVersion: 1;
  /** 默认布局恢复阈值：用户删除的 widget 保留在 hidden 列表而非实例销毁。 */
  hidden: string[];
  instances: WidgetInstance[];
  /** 每个断点的布局（lg/md/sm）。 */
  layouts: HomeResponsiveLayouts;
}

export const HOME_WORKSPACE_SCHEMA_VERSION = 1;

export const DEFAULT_DOCUMENT: HomeWorkspaceDocument = {
  schemaVersion: HOME_WORKSPACE_SCHEMA_VERSION,
  hidden: [],
  instances: [],
  layouts: { lg: [], md: [], sm: [] },
};
