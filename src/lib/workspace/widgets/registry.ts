// ── Widget Registry（B-014） ──
// 代码注册（code-registered），非 Plugin Runtime。
// 唯一 type 校验：注册冲突显式失败（throw），不允许静默覆盖。

import type { WidgetDefinition } from './types';

/** 注册表：type → definition（同一进程内单一权威）。 */
const registry = new Map<string, WidgetDefinition>();

/**
 * 注册一个 Widget。
 * type 冲突时显式抛错（不静默覆盖）；同一定义对象重复注册（HMR 重执行）
 * 幂等跳过。
 */
export function registerWidget<TData, TSettings extends Record<string, unknown>>(
  def: WidgetDefinition<TData, TSettings>,
): void {
  if (!def || typeof def.type !== 'string' || def.type.length === 0) {
    throw new Error(`[widget-registry] WidgetDefinition requires a non-empty 'type'.`);
  }
  const existing = registry.get(def.type);
  if (existing) {
    // 同一定义对象重复注册（dev HMR 重执行）→ 幂等跳过。
    if (existing === (def as WidgetDefinition)) return;
    throw new Error(
      `[widget-registry] Duplicate widget type '${def.type}' — registration refused (explicit failure). ` +
        `Widget types must be globally unique within the workspace registry.`,
    );
  }
  if (!def.Component) {
    throw new Error(`[widget-registry] Widget '${def.type}' is missing a Component renderer.`);
  }
  if (typeof def.configSchema !== 'object' || def.configSchema === null) {
    throw new Error(`[widget-registry] Widget '${def.type}' must define a configSchema (Zod).`);
  }
  registry.set(def.type, def as WidgetDefinition);
}

/** 按 type 取定义；不存在返回 undefined。 */
export function getWidget(type: string): WidgetDefinition | undefined {
  return registry.get(type);
}

/** type 是否已注册。 */
export function hasWidget(type: string): boolean {
  return registry.has(type);
}

/** 全部已注册定义（注册顺序）。 */
export function getAllWidgets(): WidgetDefinition[] {
  return [...registry.values()];
}

/**
 * 批量注册（B-028）：按顺序注册多个 Widget。
 * 任一 type 冲突都会显式抛错并中止（不部分成功/部分失败地静默继续）。
 * 注：各 Widget 的 TData 不同（invariant），注册边界统一收窄为 WidgetDefinition。
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any -- 注册边界有意收窄异构泛型
export function registerWidgets(defs: readonly WidgetDefinition<any, any>[]): void {
  for (const def of defs) {
    registerWidget(def);
  }
}

/** 全部已注册 type。 */
export function getRegisteredTypes(): string[] {
  return [...registry.keys()];
}

/** 清空注册表（仅测试/热替换用；生产代码不要调用）。 */
export function clearWidgetRegistry(): void {
  registry.clear();
}
