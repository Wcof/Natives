import { z } from 'zod';

// ── AI Natives Design System V1.1 Theme Schema ──
// 全局中性色阶 · ADR 0010

// ── 15 级全局中性色板（固定，不随主题变化） ──
export const NEUTRAL_PALETTE = {
  0: '#010101',
  100: '#111113',
  150: '#18181A',
  200: '#202024',
  250: '#27262B',
  300: '#323137',
  400: '#48474D',
  500: '#646268',
  600: '#7F7D83',
  700: '#9B999E',
  800: '#B7B5BA',
  850: '#D4D3D7',
  900: '#E8E7EA',
  950: '#FAFAFC',
  1000: '#FFFFFF',
} as const;

// ── Chart volume 色阶（0-8，数据体量映射） ──
export const CHART_VOLUME_DARK: Record<number, string> = {
  0: '#202024',
  1: '#323137',
  2: '#48474D',
  3: '#646268',
  4: '#7F7D83',
  5: '#9B999E',
  6: '#B7B5BA',
  7: '#D4D3D7',
  8: '#FAFAFC',
};

export const CHART_VOLUME_LIGHT: Record<number, string> = {
  0: '#E8E7EA',
  1: '#D4D3D7',
  2: '#B7B5BA',
  3: '#9B999E',
  4: '#7F7D83',
  5: '#646268',
  6: '#48474D',
  7: '#323137',
  8: '#18181A',
};

const ThemeSchema = z.object({
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
  'diff-add': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-del': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-mod': z.string().regex(/^#[0-9a-fA-F]{6}$/),
});

export type Theme = z.infer<typeof ThemeSchema>;

// ── V1.1 Built-in Themes (using NEUTRAL_PALETTE) ──

export const THEMES: Record<string, Theme> = {
  light: {
    background: NEUTRAL_PALETTE[950],
    surface: NEUTRAL_PALETTE[1000],
    'surface-hover': NEUTRAL_PALETTE[900],
    sidebar: NEUTRAL_PALETTE[900],
    border: NEUTRAL_PALETTE[850],
    'border-subtle': NEUTRAL_PALETTE[900],
    text: NEUTRAL_PALETTE[150],
    'text-body': NEUTRAL_PALETTE[400],
    'text-secondary': NEUTRAL_PALETTE[500],
    'text-disabled': NEUTRAL_PALETTE[700],
    primary: NEUTRAL_PALETTE[150],
    'primary-hover': NEUTRAL_PALETTE[300],
    'primary-soft': NEUTRAL_PALETTE[900],
    'primary-dark': NEUTRAL_PALETTE[0],
    'control-bg': NEUTRAL_PALETTE[900],
    'control-bg-hover': NEUTRAL_PALETTE[850],
    'control-fg': NEUTRAL_PALETTE[500],
    'control-selected-bg': NEUTRAL_PALETTE[150],
    'control-selected-bg-hover': NEUTRAL_PALETTE[300],
    'control-selected-fg': NEUTRAL_PALETTE[950],
    danger: '#DC2626',
    warning: '#D97706',
    info: '#2563EB',
    success: '#059669',
    'diff-add': '#059669',
    'diff-del': '#DC2626',
    'diff-mod': '#D97706',
  },
  dark: {
    background: NEUTRAL_PALETTE[0],
    surface: NEUTRAL_PALETTE[150],
    'surface-hover': NEUTRAL_PALETTE[200],
    sidebar: NEUTRAL_PALETTE[100],
    border: NEUTRAL_PALETTE[300],
    'border-subtle': NEUTRAL_PALETTE[200],
    text: NEUTRAL_PALETTE[950],
    'text-body': NEUTRAL_PALETTE[850],
    'text-secondary': NEUTRAL_PALETTE[700],
    'text-disabled': NEUTRAL_PALETTE[500],
    primary: NEUTRAL_PALETTE[950],
    'primary-hover': NEUTRAL_PALETTE[850],
    'primary-soft': NEUTRAL_PALETTE[250],
    'primary-dark': NEUTRAL_PALETTE[0],
    'control-bg': NEUTRAL_PALETTE[250],
    'control-bg-hover': NEUTRAL_PALETTE[300],
    'control-fg': NEUTRAL_PALETTE[700],
    'control-selected-bg': NEUTRAL_PALETTE[950],
    'control-selected-bg-hover': NEUTRAL_PALETTE[850],
    'control-selected-fg': NEUTRAL_PALETTE[0],
    danger: '#EF4444',
    warning: '#F59E0B',
    info: '#3B82F6',
    success: '#10B981',
    'diff-add': '#10B981',
    'diff-del': '#EF4444',
    'diff-mod': '#F59E0B',
  },
};

// ── Terminal ANSI Colors (per theme) ──

export const TERMINAL_THEMES: Record<string, { background: string; foreground: string; cursor: string; selectionBackground?: string }> = {
  light: {
    background: THEMES.light!.surface,
    foreground: THEMES.light!.text,
    cursor: THEMES.light!.primary,
    selectionBackground: THEMES.light!.primary + '33',
  },
  dark: {
    background: THEMES.dark!.surface,
    foreground: THEMES.dark!.text,
    cursor: THEMES.dark!.primary,
    selectionBackground: THEMES.dark!.primary + '33',
  },
  'terminal-volt': {
    background: THEMES.dark!.surface,
    foreground: THEMES.dark!.text,
    cursor: THEMES.dark!.primary,
    selectionBackground: THEMES.dark!.primary + '33',
  },
  'frosted-jasmine': {
    background: THEMES.light!.surface,
    foreground: THEMES.light!.text,
    cursor: THEMES.light!.primary,
    selectionBackground: THEMES.light!.primary + '33',
  },
};

// ── Theme Application ──

type ThemeListeners = (theme: string) => void;
const listeners = new Set<ThemeListeners>();

export function validateTheme(theme: Record<string, unknown>): Theme {
  return ThemeSchema.parse(theme);
}

export function normalizeThemeId(themeId: string | null | undefined): 'light' | 'dark' {
  if (themeId === 'terminal-volt' || themeId === 'dark') return 'dark';
  if (themeId === 'frosted-jasmine' || themeId === 'light') return 'light';
  return 'dark'; // New default is dark
}

export function applyTheme(themeId: string): void {
  const resolvedId = normalizeThemeId(themeId);

  const theme = THEMES[resolvedId];
  if (!theme) {
    console.warn(`Theme '${themeId}' not found, falling back to dark`);
    return applyTheme('dark');
  }

  const root = document.documentElement;
  root.setAttribute('data-theme', resolvedId);

  // Apply neutral palette CSS variables
  for (const [key, value] of Object.entries(NEUTRAL_PALETTE)) {
    root.style.setProperty(`--neutral-${key}`, value);
  }

  // Apply theme CSS variables
  for (const [key, value] of Object.entries(theme)) {
    const cssVar = `--${key}`;
    root.style.setProperty(cssVar, value);
  }

  // Apply chart volume CSS variables
  const chartVolumes = resolvedId === 'dark' ? CHART_VOLUME_DARK : CHART_VOLUME_LIGHT;
  for (let i = 0; i <= 8; i++) {
    const val = chartVolumes[i];
    if (val) root.style.setProperty(`--chart-volume-${i}`, val);
  }

  // Apply terminal ANSI colors
  const terminalTheme = TERMINAL_THEMES[resolvedId];
  if (terminalTheme) {
    root.style.setProperty('--terminal-bg', terminalTheme.background);
    root.style.setProperty('--terminal-fg', terminalTheme.foreground);
    root.style.setProperty('--terminal-cursor', terminalTheme.cursor);
  }

  // Add transitioning class
  root.classList.add('theme-transitioning');
  setTimeout(() => root.classList.remove('theme-transitioning'), 200);

  // Notify listeners
  listeners.forEach((cb) => cb(resolvedId));
}

export function onThemeChange(cb: ThemeListeners): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function getThemeId(): string {
  return document.documentElement.getAttribute('data-theme') || 'dark';
}

// ── Chart volume helper ──

/**
 * Map a value to a chart volume level (0-8).
 * value = 0 → 0
 * non-zero → max(1, ceil(value / visibleMax * 8))
 */
export function getChartVolumeLevel(value: number, visibleMax: number): 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 {
  if (value === 0 || visibleMax <= 0) return 0;
  const level = Math.ceil((value / visibleMax) * 8);
  if (level >= 8) return 8;
  if (level <= 1) return 1;
  return Math.max(1, Math.min(8, level)) as 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;
}