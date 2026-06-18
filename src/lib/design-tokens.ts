// ── Design Token System (TASK-019) ──
//
// Central source of truth for all visual tokens.
// Components should reference these tokens rather than inline values
// to ensure visual consistency across the application.

export const SPACING = {
  xs: 4,
  sm: 8,
  md: 12,
  lg: 16,
  xl: 24,
  xxl: 32,
} as const;

export const FONT_SIZE = {
  xs: 10,
  sm: 11,
  md: 12,
  lg: 13,
  xl: 14,
  heading: 17,
  title: 22,
} as const;

export const BORDER_RADIUS = {
  sm: 4,
  md: 6,
  lg: 8,
  xl: 10,
  pill: 999,
} as const;

export const TRANSITION = {
  fast: '0.12s cubic-bezier(0.16, 1, 0.3, 1)',
  normal: '0.2s cubic-bezier(0.16, 1, 0.3, 1)',
  slow: '0.3s cubic-bezier(0.16, 1, 0.3, 1)',
} as const;

export const SHADOW = {
  card: '0 1px 3px rgba(0,0,0,0.12)',
  elevated: '0 4px 12px rgba(0,0,0,0.15)',
  modal: '0 8px 32px rgba(0,0,0,0.2)',
} as const;

// ── Theme Token (maps to CSS variables) ──
// These are the semantic tokens that different themes can override.

export const THEME_TOKENS = {
  // Backgrounds (fallbacks match terminal-volt)
  bg: 'var(--bg, #0d0f12)',
  bg2: 'var(--bg-2, #15181d)',
  bg3: 'var(--bg-3, #1c2027)',

  // Text
  text: 'var(--text, #d4d7de)',
  textDim: 'var(--text-dim, #8b90a0)',
  textFaint: 'var(--text-faint, #555a66)',

  // Accent
  accent: 'var(--accent, #00ff9c)',
  accentSoft: 'var(--accent-soft, rgba(0,255,156,0.12))',
  accentInk: 'var(--accent-ink, #0d0f12)',

  // Borders
  border: 'var(--vibe-btn-border)',

  // Surface
  surface: 'var(--surface, #15181d)',
} as const;

// ── Component Token Presets ──

export const CARD_STYLE = {
  background: THEME_TOKENS.bg2,
  border: `1px solid ${THEME_TOKENS.border}`,
  borderRadius: BORDER_RADIUS.lg,
  padding: SPACING.lg,
  transition: TRANSITION.fast,
} as const;

export const INPUT_STYLE: React.CSSProperties = {
  width: '100%',
  padding: '8px 10px',
  background: 'var(--bg,#0d0f12)',
  border: '1px solid var(--vibe-btn-border)',
  borderRadius: BORDER_RADIUS.md,
  color: 'var(--text)',
  fontSize: FONT_SIZE.md,
  outline: 'none',
};

export const SECTION_TITLE_STYLE: React.CSSProperties = {
  fontSize: FONT_SIZE.xs,
  fontWeight: 600,
  color: 'var(--text-dim)',
  textTransform: 'uppercase',
  letterSpacing: 1,
  marginBottom: SPACING.md,
};

export const OVERLAY_STYLE: React.CSSProperties = {
  position: 'fixed',
  inset: 0,
  background: 'rgba(0,0,0,0.5)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  zIndex: 60,
};
