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
    background: '#F7F7F5',
    surface: '#FFFFFF',
    'surface-hover': '#F3F4F6',
    sidebar: '#FAFAFA',
    border: '#E5E7EB',
    'border-subtle': '#F0F0F0',
    text: '#111827',
    'text-body': '#374151',
    'text-secondary': '#6B7280',
    'text-disabled': '#9CA3AF',
    primary: '#FF6B2C',
    'primary-hover': '#FF7A45',
    'primary-soft': '#FFF3EB',
    'primary-dark': '#C94A12',
    danger: '#EF4444',
    warning: '#EAB308',
    info: '#3B82F6',
    success: '#10B981',
    'diff-add': '#10B981',
    'diff-del': '#EF4444',
    'diff-mod': '#F59E0B',
  },
  dark: {
    background: '#0F1115',
    surface: '#171A21',
    'surface-hover': '#1D222B',
    sidebar: '#111318',
    border: '#252A33',
    'border-subtle': '#1F242D',
    text: '#F9FAFB',
    'text-body': '#E5E7EB',
    'text-secondary': '#9CA3AF',
    'text-disabled': '#667085',
    primary: '#FF6B2C',
    'primary-hover': '#FF7A45',
    'primary-soft': '#2A1810',
    'primary-dark': '#C94A12',
    danger: '#F87171',
    warning: '#FBBF24',
    info: '#60A5FA',
    success: '#34D399',
    'diff-add': '#34D399',
    'diff-del': '#F87171',
    'diff-mod': '#FBBF24',
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
