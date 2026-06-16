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
  'terminal-volt': {
    bg: '#0b0c0a',
    'bg-2': '#131410',
    'bg-3': '#1c1e17',
    panel: '#0e0f0c',
    border: '#262920',
    text: '#f2f2ea',
    'text-dim': '#9b9d8c',
    'text-faint': '#62655a',
    accent: '#cdf24b',
    'accent-ink': '#0b0c0a',
    radius: '4px',
    'font-display': 'ui-monospace, "SF Mono", "JetBrains Mono", monospace',
    danger: '#f24b4b',
    'danger-soft': '#f24b4b15',
    warning: '#e6b800',
    info: '#4bcdf2',
    'diff-add': '#4ec9b0',
    'diff-del': '#f24b4b',
    'diff-mod': '#5b9cf5',
  },
  'warm-archive': {
    bg: '#f5f0e8',
    'bg-2': '#ece4d6',
    'bg-3': '#e5dccd',
    panel: '#efe9df',
    border: '#d4c9b7',
    text: '#2c2418',
    'text-dim': '#6b5f4e',
    'text-faint': '#a0947d',
    accent: '#cc785c',
    'accent-ink': '#f5f0e8',
    radius: '9px',
    'font-display': '"Fraunces", Georgia, serif',
    danger: '#c0392b',
    'danger-soft': '#c0392b15',
    warning: '#b8860b',
    info: '#3a7ca5',
    'diff-add': '#5a8a5a',
    'diff-del': '#a04040',
    'diff-mod': '#5a6a9a',
  },
  'editorial': {
    bg: '#f4f1ea',
    'bg-2': '#ece8dd',
    'bg-3': '#e4dfd2',
    panel: '#f4f1ea',
    border: '#d6d1c3',
    text: '#0a0a0a',
    'text-dim': '#57534a',
    'text-faint': '#8a857a',
    accent: '#ff433d',
    'accent-ink': '#ffffff',
    radius: '0px',
    'font-display': 'var(--font-ui)',
    danger: '#ff433d',
    'danger-soft': '#ff433d15',
    warning: '#000000',
    info: '#0a0a0a',
    'diff-add': '#0a0a0a',
    'diff-del': '#ff433d',
    'diff-mod': '#5a5a5a',
  },
};

// ── Terminal ANSI Colors ──

export const TERMINAL_THEMES: Record<string, { background: string; foreground: string; cursor: string; selectionBackground?: string }> = {
  'terminal-volt': { background: '#0b0c0a', foreground: '#d6dac9', cursor: '#cdf24b', selectionBackground: '#cdf24b33' },
  'warm-archive': { background: '#ece2d2', foreground: '#4a3f30', cursor: '#cc785c', selectionBackground: '#cc785c33' },
  'editorial': { background: '#f4f1ea', foreground: '#0a0a0a', cursor: '#ff433d', selectionBackground: '#ff433d33' },
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
