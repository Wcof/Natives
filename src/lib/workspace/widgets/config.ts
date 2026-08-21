// ── Widget Config Pipeline（B-015） ──
// config version + Zod validate/migrate：
//   非法配置回退 last-valid，绝不把坏配置写回持久化层。
// base 字段（type/enabled/surface/order/size）由本模块统一校验；
// settings 字段由各 WidgetDefinition.configSchema 校验。

import { z } from 'zod';
import type { WidgetConfig, WidgetDefinition, WidgetSize, WidgetSurfaceKind } from './types';

export const WIDGET_SURFACE_KINDS = ['material', 'crystal', 'plain'] as const;
export const WIDGET_SIZES = ['small', 'medium', 'large'] as const;

/** 当前 config 结构版本（base 字段）。 */
export const WIDGET_CONFIG_VERSION = 1;

/** base config 结构（不含各 Widget 私有 settings）。 */
const WIDGET_CONFIG_BASE_SCHEMA = z.object({
  type: z.string().min(1),
  enabled: z.boolean(),
  surface: z.union([
    z.literal('auto'),
    z.literal('material'),
    z.literal('crystal'),
    z.literal('plain'),
  ]),
  order: z.number().int().finite(),
  size: z.enum(['small', 'medium', 'large']).optional(),
  settings: z.record(z.string(), z.unknown()),
});

export type ValidatedWidgetConfig = z.infer<typeof WIDGET_CONFIG_BASE_SCHEMA>;

/** 每个 Widget 的 last-valid config（非法输入回退目标）。 */
const lastValid = new Map<string, WidgetConfig>();

/**
 * 读取 raw config 时附带的版本标记。
 * 持久化层可能不带此字段（Rust 侧单独存 config_version）；
 * 迁移逻辑按 def.configVersion 与 raw.__version 差值判断。
 */
const VERSION_FIELD = '__version';

export function readConfigVersion(raw: unknown): number | null {
  if (raw && typeof raw === 'object') {
    const v = (raw as Record<string, unknown>)[VERSION_FIELD];
    if (typeof v === 'number' && Number.isInteger(v)) return v;
  }
  return null;
}

/** 由 definition 生成默认 config。 */
export function createDefaultConfig<TSettings extends Record<string, unknown>>(
  def: WidgetDefinition<unknown, TSettings>,
): WidgetConfig<TSettings> {
  return {
    type: def.type,
    enabled: def.defaultEnabled ?? true,
    surface: 'auto',
    order: 0,
    size: def.size,
    settings: def.defaultConfig,
  };
}

/** 记住某个 config 为 last-valid。 */
export function setLastValidConfig<TSettings extends Record<string, unknown>>(
  def: WidgetDefinition<unknown, TSettings>,
  config: WidgetConfig<TSettings>,
): void {
  lastValid.set(def.type, config as WidgetConfig);
}

/** 取某 Widget 的 last-valid config；无则 undefined。 */
export function getLastValidConfig<TSettings extends Record<string, unknown>>(
  def: WidgetDefinition<unknown, TSettings>,
): WidgetConfig<TSettings> | undefined {
  return lastValid.get(def.type) as WidgetConfig<TSettings> | undefined;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * 校验 base 字段 + 各 Widget settings schema。
 * 返回 null 表示非法。
 */
export function validateWidgetConfig<TSettings extends Record<string, unknown>>(
  def: WidgetDefinition<unknown, TSettings>,
  config: WidgetConfig<TSettings>,
): config is WidgetConfig<TSettings> {
  if (!isPlainObject(config)) return false;
  if (config.type !== def.type) return false;

  const base = WIDGET_CONFIG_BASE_SCHEMA.safeParse(config);
  if (!base.success) return false;

  // settings 必须是对象，且通过该 Widget 的 schema
  const settings = config.settings ?? {};
  if (!isPlainObject(settings)) return false;
  const parsed = def.configSchema.safeParse(settings);
  if (!parsed.success) return false;

  return true;
}

/**
 * 迁移 + 校验管线（B-015）。
 * 流程：版本迁移 → base/settings 校验 → 通过则记 last-valid 并返回；
 *       失败则回退 last-valid，再失败回退默认 config。绝不抛错。
 */
export function normalizeWidgetConfig<TSettings extends Record<string, unknown>>(
  def: WidgetDefinition<unknown, TSettings>,
  raw: unknown,
): WidgetConfig<TSettings> {
  const fallback = getLastValidConfig(def) ?? createDefaultConfig(def);

  // 1) 非对象 / 类型不匹配 → 直接回退
  if (!isPlainObject(raw) || raw.type !== def.type) {
    return fallback;
  }

  // 2) 版本迁移
  let candidate: unknown = raw;
  const rawVersion = readConfigVersion(raw);
  const needsMigrate =
    rawVersion !== null &&
    rawVersion !== def.configVersion &&
    typeof def.migrateConfig === 'function';
  if (needsMigrate) {
    try {
      const migrated = def.migrateConfig!(rawVersion, raw);
      candidate = { ...raw, settings: migrated, [VERSION_FIELD]: def.configVersion };
    } catch (err) {
      console.warn(`[widget-config] migrate failed for '${def.type}':`, err);
      return fallback;
    }
  }

  // 3) 规范化 base 字段默认值
  const base = WIDGET_CONFIG_BASE_SCHEMA.partial().safeParse(candidate);
  if (!base.success) return fallback;

  const merged: WidgetConfig<TSettings> = {
    type: def.type,
    enabled: (candidate as Record<string, unknown>).enabled === false ? false : true,
    surface: (((candidate as Record<string, unknown>).surface as WidgetSurfaceKind | undefined) ??
      'auto') as WidgetConfig<TSettings>['surface'],
    order: typeof (candidate as Record<string, unknown>).order === 'number'
      ? Number((candidate as Record<string, unknown>).order)
      : 0,
    size: (candidate as Record<string, unknown>).size as WidgetSize | undefined,
    settings: ((candidate as Record<string, unknown>).settings ?? {}) as TSettings,
  };

  // 4) 全量校验
  if (!validateWidgetConfig(def, merged)) {
    console.warn(
      `[widget-config] Invalid config for '${def.type}' — falling back to last-valid/default.`,
    );
    return fallback;
  }

  // 5) 记 last-valid 并返回
  setLastValidConfig(def, merged);
  return merged;
}

/**
 * 序列化：剥离内部版本字段，得到可持久化的纯对象。
 * Rust 侧用独立 config_version 列，不需要内嵌 __version。
 */
export function serializeWidgetConfig<TSettings extends Record<string, unknown>>(
  config: WidgetConfig<TSettings>,
): Record<string, unknown> {
  const { settings, ...rest } = config;
  const out: Record<string, unknown> = { ...rest };
  if (settings && Object.keys(settings).length > 0) {
    out.settings = settings;
  }
  return out;
}
