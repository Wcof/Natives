// ── Inspector Section Schema（B-020） ──
// 供 C 的 Inspector host 消费：把 Widget 配置渲染为分区表单。
// 只描述 schema（控件类型 + 标签 + 可选选项），不直接渲染 UI，
// 避免 lib 层依赖特定 UI 库。

import type { ReactNode } from 'react';

export type InspectorFieldKind =
  | 'text'
  | 'number'
  | 'select'
  | 'toggle'
  | 'slider'
  | 'section'
  | 'custom';

export interface InspectorFieldOption {
  value: string | number;
  labelKey?: string;
  label?: ReactNode;
}

export interface InspectorField {
  key: string;
  kind: InspectorFieldKind;
  /** i18n key（label）。 */
  labelKey: string;
  hintKey?: string;
  placeholderKey?: string;
  options?: InspectorFieldOption[];
  /** slider min/max/step。 */
  min?: number;
  max?: number;
  step?: number;
  /** 自定义控件（kind === 'custom' 时由 host 消费该 key）。 */
  customRendererKey?: string;
}

export interface InspectorSection {
  id: string;
  titleKey: string;
  fields: InspectorField[];
}

/**
 * 由 WidgetDefinition 生成 Inspector 分区（默认实现）。
 * 目前覆盖基础分组（surface/size/order）；各 Widget 可自定义扩展。
 */
export function buildInspectorSections(def: { type: string }): InspectorSection[] {
  return [
    {
      id: `${def.type}:layout`,
      titleKey: 'inspector.sectionLayout',
      fields: [
        { key: 'surface', kind: 'select', labelKey: 'inspector.fieldSurface' },
        { key: 'size', kind: 'select', labelKey: 'inspector.fieldSize' },
        { key: 'order', kind: 'number', labelKey: 'inspector.fieldOrder' },
      ],
    },
  ];
}
