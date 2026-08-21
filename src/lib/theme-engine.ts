import { z } from 'zod';
import {
  NEUTRAL_PALETTE,
  CHART_VOLUME_DARK,
  CHART_VOLUME_LIGHT,
  V2_TOKENS,
  THEMES,
  TERMINAL_THEMES,
  V2_THEME_NAMES,
  type V2ThemeId,
} from '@/lib/design-tokens';

// ── AI Natives Design System V2 Theme Engine ──
// 单一主题应用器：CSS 与 TS 不再维护两套漂移真值。
//   · 主题值唯一真值 = src/lib/design-tokens.ts 的 V2_TOKENS。
//   · 本文件只负责「解析 → 应用 → 偏好 fallback」。
//   · 内部 ID 固定 dark / light；旧名 terminal-volt / frosted-jasmine 只读兼容。
//   · glow 仅用于 focus/selected/status；reduced-motion / reduced-transparency
//     通过 data-* 属性 + CSS fallback 生效。

export { NEUTRAL_PALETTE, CHART_VOLUME_DARK, CHART_VOLUME_LIGHT, THEMES, TERMINAL_THEMES };

// ── 旧名 → 内部 ID 的只读兼容映射 ──
const LEGACY_THEME_ALIASES: Record<string, V2ThemeId> = {
  'terminal-volt': 'dark',
  'frosted-jasmine': 'light',
  dark: 'dark',
  light: 'light',
};

/** 解析任意旧 ID 到内部固定 ID（dark / light）。未知值兜底 dark。 */
export function normalizeThemeId(themeId: string | null | undefined): V2ThemeId {
  if (!themeId) return 'dark';
  const key = String(themeId).toLowerCase();
  return LEGACY_THEME_ALIASES[key] ?? 'dark';
}

// ── Zod 校验（R-U3）：仅 hex 核心子集必须校验；rgba/gradient 留在 CSS 层 ──
const ThemeCoreSchema = z.object({
  background: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  surface: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'surface-hover': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  sidebar: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  border: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'border-subtle': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  text: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'text-body': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'text-secondary': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'text-disabled': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  primary: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'primary-hover': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'primary-soft': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'primary-dark': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'control-bg': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'control-bg-hover': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'control-fg': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'control-selected-bg': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'control-selected-bg-hover': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'control-selected-fg': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  danger: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  warning: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  info: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  success: z.string().regex(/^#[0-9a-fA-F]{6}$/),
});

/** 校验 hex 核心子集（向后兼容入口；V2 全量 token 由 design-tokens 保证）。 */
export function validateTheme(theme: Record<string, unknown>): z.infer<typeof ThemeCoreSchema> {
  return ThemeCoreSchema.parse(theme);
}

/** 主题显示名 i18n key（设置页 / 命令面板共用）。 */
export function getThemeNames(id: string): { labelKey: string; descKey: string } {
  return V2_THEME_NAMES[normalizeThemeId(id)];
}

// ── 偏好 fallback：reduced motion / reduced transparency ──

type PreferenceKind = 'motion' | 'transparency';

function readPreference(kind: PreferenceKind): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false;
  const query =
    kind === 'motion'
      ? '(prefers-reduced-motion: reduce)'
      : '(prefers-reduced-transparency: reduce)';
  try {
    return window.matchMedia(query).matches;
  } catch {
    return false;
  }
}

function syncPreferenceAttribute(kind: PreferenceKind): void {
  const root = document.documentElement;
  const attr = kind === 'motion' ? 'data-reduced-motion' : 'data-reduced-transparency';
  root.setAttribute(attr, readPreference(kind) ? 'true' : 'false');
}

function subscribePreference(kind: PreferenceKind, cb: () => void): () => void {
  if (typeof window === 'undefined' || !window.matchMedia) return () => {};
  const query =
    kind === 'motion'
      ? '(prefers-reduced-motion: reduce)'
      : '(prefers-reduced-transparency: reduce)';
  try {
    const mql = window.matchMedia(query);
    const handler = () => {
      syncPreferenceAttribute(kind);
      cb();
    };
    mql.addEventListener('change', handler);
    return () => mql.removeEventListener('change', handler);
  } catch {
    return () => {};
  }
}

/** 是否启用 reduced motion（仅客户端有效）。 */
export function getReducedMotion(): boolean {
  return readPreference('motion');
}

/** 是否启用 reduced transparency（仅客户端有效）。 */
export function getReducedTransparency(): boolean {
  return readPreference('transparency');
}

/** 订阅系统偏好变化；返回取消订阅函数。 */
export function onPreferenceChange(cb: () => void): () => void {
  const unsub1 = subscribePreference('motion', cb);
  const unsub2 = subscribePreference('transparency', cb);
  return () => {
    unsub1();
    unsub2();
  };
}

// ── 主题监听 ──

type ThemeListeners = (theme: V2ThemeId) => void;
const listeners = new Set<ThemeListeners>();

function notify(theme: V2ThemeId): void {
  listeners.forEach((cb) => cb(theme));
}

export function onThemeChange(cb: ThemeListeners): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function getThemeId(): V2ThemeId {
  if (typeof document === 'undefined') return 'dark';
  const raw = document.documentElement.getAttribute('data-theme');
  return normalizeThemeId(raw);
}

// ── 应用器 ──

/**
 * V2 单一主题应用器：把 design-tokens.ts 的 V2_TOKENS 应用到 CSS 变量。
 * 不维护第二套颜色真值 —— CSS 只保留首帧 fallback。
 */
export function applyTheme(themeId: string): void {
  const resolvedId = normalizeThemeId(themeId);

  const tokens = V2_TOKENS[resolvedId];
  if (!tokens) {
    console.warn(`[theme-engine] Theme '${themeId}' not found, falling back to dark`);
    return applyTheme('dark');
  }

  const root = document.documentElement;
  root.setAttribute('data-theme', resolvedId);

  // 1) Neutral palette（固定原始色板）
  for (const [key, value] of Object.entries(NEUTRAL_PALETTE)) {
    root.style.setProperty(`--neutral-${key}`, value);
  }

  // 2) V2 语义 token（单一真值源）
  for (const [key, value] of Object.entries(tokens)) {
    root.style.setProperty(`--${key}`, value);
  }

  // 3) Chart volume（0-8）
  const chartVolumes = resolvedId === 'dark' ? CHART_VOLUME_DARK : CHART_VOLUME_LIGHT;
  for (let i = 0; i <= 8; i++) {
    const val = chartVolumes[i];
    if (val) root.style.setProperty(`--chart-volume-${i}`, val);
  }

  // 4) Grayscale steps（兼容 V1 别名）
  const graySteps: Record<string, string> =
    resolvedId === 'dark'
      ? {
          '50': '#010101', '100': '#18181A', '200': '#202024', '300': '#27262B',
          '400': '#323137', '500': '#646268', '600': '#9B999E', '700': '#D4D3D7',
          '800': '#FAFAFC', '900': '#FFFFFF',
        }
      : {
          '50': '#FAFAFA', '100': '#F4F4F2', '200': '#EDEDEB', '300': '#E2E2DF',
          '400': '#D5D5D2', '500': '#999999', '600': '#666666', '700': '#333333',
          '800': '#1E1E1E', '900': '#111111',
        };
  for (const [key, value] of Object.entries(graySteps)) {
    root.style.setProperty(`--gray-${key}`, value);
  }

  // 5) Terminal ANSI（随皮肤联动）
  const terminalTheme = TERMINAL_THEMES[resolvedId];
  if (terminalTheme) {
    root.style.setProperty('--terminal-bg', terminalTheme.background);
    root.style.setProperty('--terminal-fg', terminalTheme.foreground);
    root.style.setProperty('--terminal-cursor', terminalTheme.cursor);
    if (terminalTheme.selectionBackground) {
      root.style.setProperty('--terminal-selection', terminalTheme.selectionBackground);
    }
  }

  // 6) 偏好 fallback 属性
  syncPreferenceAttribute('motion');
  syncPreferenceAttribute('transparency');

  // 7) 过渡类 + 通知
  root.classList.add('theme-transitioning');
  setTimeout(() => root.classList.remove('theme-transitioning'), 200);

  notify(resolvedId);
}

// ── 初始化：首次加载时同步偏好属性（幂等） ──
if (typeof document !== 'undefined') {
  syncPreferenceAttribute('motion');
  syncPreferenceAttribute('transparency');
  subscribePreference('motion', () => {});
  subscribePreference('transparency', () => {});
}

// ── Chart volume helper（向后兼容） ──
export function getChartVolumeLevel(value: number, visibleMax: number): 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 {
  if (value === 0 || visibleMax <= 0) return 0;
  const level = Math.ceil((value / visibleMax) * 8);
  if (level >= 8) return 8;
  if (level <= 1) return 1;
  return Math.max(1, Math.min(8, level)) as 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;
}
