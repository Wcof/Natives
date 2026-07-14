import { z } from 'zod';

// ── AI Natives Design System V1.0 Theme Schema ──
// 纯色 Surface · 轻边框 · 双主题 (light / dark) · 无 backdrop-filter

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
  danger: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  warning: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  info: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  success: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-add': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-del': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-mod': z.string().regex(/^#[0-9a-fA-F]{6}$/),
});

export type Theme = z.infer<typeof ThemeSchema>;

// ── V1.0 Built-in Themes ──

export const THEMES: Record<string, Theme> = {
  light: {
    background: '#F4F4F2',
    surface: '#FFFFFF',
    'surface-hover': '#ECECEA',
    sidebar: '#EDEDEB',
    border: '#D5D5D2',
    'border-subtle': '#E2E2DF',
    text: '#111111',
    'text-body': '#333333',
    'text-secondary': '#666666',
    'text-disabled': '#999999',
    primary: '#111111',
    'primary-hover': '#333333',
    'primary-soft': '#E2E2DF',
    'primary-dark': '#000000',
    danger: '#DC2626',
    warning: '#D97706',
    info: '#2563EB',
    success: '#059669',
    'diff-add': '#059669',
    'diff-del': '#DC2626',
    'diff-mod': '#D97706',
  },
  dark: {
    background: '#080808',
    surface: '#151515',
    'surface-hover': '#1E1E1E',
    sidebar: '#0D0D0D',
    border: '#2A2A2A',
    'border-subtle': '#222222',
    text: '#F5F5F5',
    'text-body': '#D4D4D4',
    'text-secondary': '#A3A3A3',
    'text-disabled': '#666666',
    primary: '#F5F5F5',
    'primary-hover': '#D4D4D4',
    'primary-soft': '#262626',
    'primary-dark': '#FFFFFF',
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
  return 'light';
}

export function applyTheme(themeId: string): void {
  const resolvedId = normalizeThemeId(themeId);

  const theme = THEMES[resolvedId];
  if (!theme) {
    console.warn(`Theme '${themeId}' not found, falling back to light`);
    return applyTheme('light');
  }

  const root = document.documentElement;
  root.setAttribute('data-theme', resolvedId);

  // Apply CSS variables
  for (const [key, value] of Object.entries(theme)) {
    const cssVar = `--${key}`;
    root.style.setProperty(cssVar, value);
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
  return document.documentElement.getAttribute('data-theme') || 'light';
}
