import { z } from 'zod';

// ── Theme Schema ──

const ThemeSchema = z.object({
  bg: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'bg-2': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'bg-3': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  panel: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  border: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  text: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'text-dim': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'text-faint': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  accent: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'accent-ink': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  radius: z.string(),
  'font-display': z.string().optional(),
  // 语义颜色令牌（Round 2 任务 2.2）
  danger: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'danger-soft': z.string().regex(/^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/).optional(),
  warning: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  info: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-add': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-del': z.string().regex(/^#[0-9a-fA-F]{6}$/),
  'diff-mod': z.string().regex(/^#[0-9a-fA-F]{6}$/),
});

export type Theme = z.infer<typeof ThemeSchema>;

// ── Built-in Themes ──

export const THEMES: Record<string, Theme> = {
  'frosted-jasmine': {
    bg: '#fdf6f0',
    'bg-2': '#f9eee4',
    'bg-3': '#f5e5d6',
    panel: '#fcf3ea',
    border: '#e8d5c0',
    text: '#2d1f14',
    'text-dim': '#7a6b5a',
    'text-faint': '#b8a594',
    accent: '#ff793f',
    'accent-ink': '#ffffff',
    radius: '12px',
    'font-display': '"Noto Serif SC", Georgia, serif',
    danger: '#f05b3f',
    'danger-soft': '#f05b3f20',
    warning: '#ffa466',
    info: '#be88ed',
    'diff-add': '#ff9856',
    'diff-del': '#f05b3f',
    'diff-mod': '#be88ed',
  },
  'terminal-volt': {
    bg: '#0d0f12',
    'bg-2': '#15181d',
    'bg-3': '#1c2027',
    panel: '#0d0f12',
    border: '#2a2f38',
    text: '#d4d7de',
    'text-dim': '#8b90a0',
    'text-faint': '#555a66',
    accent: '#00ff9c',
    'accent-ink': '#0d0f12',
    radius: '4px',
    'font-display': '"JetBrains Mono", "Fira Code", monospace',
    danger: '#ff3a4d',
    'danger-soft': '#ff3a4d15',
    warning: '#ffb545',
    info: '#45b5ff',
    'diff-add': '#00ff9c',
    'diff-del': '#ff3a4d',
    'diff-mod': '#45b5ff',
  },
};

// ── Terminal ANSI Colors ──

export const TERMINAL_THEMES: Record<string, { background: string; foreground: string; cursor: string; selectionBackground?: string }> = {
  'frosted-jasmine': { background: '#fdf6f0', foreground: '#2d1f14', cursor: '#ff793f', selectionBackground: '#ff793f33' },
  'terminal-volt': { background: '#0d0f12', foreground: '#d4d7de', cursor: '#00ff9c', selectionBackground: '#00ff9c33' },
};

// ── Theme Application ──

type ThemeListeners = (theme: string) => void;
const listeners = new Set<ThemeListeners>();

export function validateTheme(theme: Record<string, unknown>): Theme {
  return ThemeSchema.parse(theme);
}

export function applyTheme(themeId: string): void {
  const theme = THEMES[themeId];
  if (!theme) {
    console.warn(`Theme '${themeId}' not found, falling back to terminal-volt`);
    return applyTheme('terminal-volt');
  }

  const root = document.documentElement;
  root.setAttribute('data-theme', themeId);

  // Apply CSS variables
  for (const [key, value] of Object.entries(theme)) {
    const cssVar = `--${key}`;
    root.style.setProperty(cssVar, value);
  }

  // Apply terminal ANSI colors
  const terminalTheme = TERMINAL_THEMES[themeId];
  if (terminalTheme) {
    root.style.setProperty('--terminal-bg', terminalTheme.background);
    root.style.setProperty('--terminal-fg', terminalTheme.foreground);
    root.style.setProperty('--terminal-cursor', terminalTheme.cursor);
  }

  // Add transitioning class
  root.classList.add('theme-transitioning');
  setTimeout(() => root.classList.remove('theme-transitioning'), 300);

  // Notify listeners
  listeners.forEach((cb) => cb(themeId));
}

export function onThemeChange(cb: ThemeListeners): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function getThemeId(): string {
  return document.documentElement.getAttribute('data-theme') || 'terminal-volt';
}
