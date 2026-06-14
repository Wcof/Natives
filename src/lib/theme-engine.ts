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
  },
};

// ── Terminal ANSI Colors ──

export const TERMINAL_THEMES: Record<string, { background: string; foreground: string; cursor: string }> = {
  'terminal-volt': { background: '#0b0c0a', foreground: '#d6dac9', cursor: '#cdf24b' },
  'warm-archive': { background: '#ece2d2', foreground: '#4a3f30', cursor: '#cc785c' },
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
