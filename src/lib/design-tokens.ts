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
  // Backgrounds
  bg: 'var(--bg, #0b0c0a)',
  bg2: 'var(--bg-2, #131410)',
  bg3: 'var(--bg-3, #1c1e17)',

  // Text
  text: 'var(--text, #f2f2ea)',
  textDim: 'var(--text-dim, #9b9d8c)',
  textFaint: 'var(--text-faint, #62655a)',

  // Accent
  accent: 'var(--accent, #FFF5E6)',
  accentSoft: 'var(--accent-soft, #FFF5E61f)',
  accentInk: 'var(--accent-ink, #0b0c0a)',

  // Borders
  border: 'var(--border, #262920)',

  // Surface
  surface: 'var(--surface, #131410)',
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
  background: 'var(--bg,#0b0c0a)',
  border: '1px solid var(--border,#262920)',
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
