// ── AI Natives Design System V1.0 Tokens ──
// Modern Desktop Native UI · 纯色 Surface · 轻边框 · 双主题
// 中央 token 源：组件应引用这些 token，而非内联硬编码值。

// ── Spacing (4px base, V1.0 规范) ──
export const SPACING = {
  xxs: 2,
  xs: 4,    // space-1
  sm: 8,    // space-2
  md: 12,   // space-3
  lg: 16,   // space-4
  xl: 20,   // space-5
  xxl: 24,  // space-6
  xxxl: 32, // space-8
  huge: 40, // space-10
  massive: 48, // space-12
} as const;

// ── Font size (V1.0 规范字号体系) ──
export const FONT_SIZE = {
  micro: '0.75rem',    // 12px Caption
  xs: '0.8125rem',     // 13px Label
  sm: '0.875rem',      // 14px Body (默认)
  md: '1rem',          // 16px Body Large
  lg: '1.125rem',      // 18px Heading
  xl: '1.5rem',        // 24px Title
  hero: '1.75rem',     // 28px Display
} as const;

// ── Border radius (V1.0 规范) ──
export const BORDER_RADIUS = {
  xs: 8,    // Small Button / Tag
  sm: 10,   // Button / Input
  md: 12,   // Card
  lg: 14,   // Large Card
  xl: 16,   // Window
  pill: 999,
} as const;

// ── Transition (V1.0 规范：150ms ease) ──
export const TRANSITION = {
  fast: '100ms ease',
  normal: '150ms ease',
  slow: '200ms ease',
} as const;

// ── Shadow (V1.0 三级，深色尽量不用) ──
export const SHADOW = {
  card: '0 1px 3px rgba(0, 0, 0, 0.04)',
  popup: '0 8px 24px rgba(0, 0, 0, 0.08)',
  modal: '0 20px 40px rgba(0, 0, 0, 0.12)',
} as const;

// ── Theme Token (映射 V1.0 CSS 变量) ──
export const THEME_TOKENS = {
  // Backgrounds
  background: 'var(--background)',
  surface: 'var(--surface)',
  surfaceHover: 'var(--surface-hover)',
  sidebar: 'var(--sidebar)',

  // Grayscale steps
  gray50: 'var(--gray-50)',
  gray100: 'var(--gray-100)',
  gray200: 'var(--gray-200)',
  gray300: 'var(--gray-300)',
  gray400: 'var(--gray-400)',
  gray500: 'var(--gray-500)',
  gray600: 'var(--gray-600)',
  gray700: 'var(--gray-700)',
  gray800: 'var(--gray-800)',
  gray900: 'var(--gray-900)',

  // Text
  text: 'var(--text)',
  textBody: 'var(--text-body)',
  textSecondary: 'var(--text-secondary)',
  textDisabled: 'var(--text-disabled)',

  // Brand
  primary: 'var(--primary)',
  primaryHover: 'var(--primary-hover)',
  primarySoft: 'var(--primary-soft)',
  primaryInk: '#FFFFFF',

  // Border
  border: 'var(--border)',
  borderSubtle: 'var(--border-subtle)',

  // Semantic
  danger: 'var(--danger)',
  warning: 'var(--warning)',
  info: 'var(--info)',
  success: 'var(--success)',

  // Chart (grayscale)
  chartStrong: 'var(--chart-strong)',
  chartMedium: 'var(--chart-medium)',
  chartSoft: 'var(--chart-soft)',
  chartEmpty: 'var(--chart-empty)',
} as const;

// ── Component Token Presets ──

/** Standard Card (V1.0 规范第 7 节) */
export const CARD_STYLE: React.CSSProperties = {
  background: THEME_TOKENS.surface,
  border: `1px solid ${THEME_TOKENS.border}`,
  borderRadius: BORDER_RADIUS.lg,
  padding: SPACING.xl,
  transition: `all ${TRANSITION.normal}`,
};

/** Standard Input (V1.0 规范第 6 节) */
export const INPUT_STYLE: React.CSSProperties = {
  width: '100%',
  height: 40,
  padding: '0 12px',
  background: THEME_TOKENS.surface,
  border: `1px solid ${THEME_TOKENS.border}`,
  borderRadius: BORDER_RADIUS.sm,
  color: THEME_TOKENS.text,
  fontSize: '14px',
  outline: 'none',
  transition: `border-color ${TRANSITION.normal}, box-shadow ${TRANSITION.normal}`,
};

/** Section title (侧边栏分组、设置区块标题) */
export const SECTION_TITLE_STYLE: React.CSSProperties = {
  fontSize: '11px',
  fontWeight: 500,
  color: THEME_TOKENS.textDisabled,
  textTransform: 'uppercase',
  letterSpacing: '0.06em',
  marginBottom: '12px',
};

/** Section header with optional badge */
export const SECTION_HEADER_STYLE: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  padding: '8px 20px',
  borderBottom: `1px solid ${THEME_TOKENS.border}`,
};

/** Modal/dialog overlay (纯色半透明，无 backdrop-filter) */
export const OVERLAY_STYLE: React.CSSProperties = {
  position: 'fixed',
  inset: 0,
  background: 'rgba(0, 0, 0, 0.4)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  zIndex: 60,
};

/** Dialog surface (V1.0 Modal: surface + 1px border + modal shadow) */
export const DIALOG_SURFACE_STYLE: React.CSSProperties = {
  background: THEME_TOKENS.surface,
  border: `1px solid ${THEME_TOKENS.border}`,
  borderRadius: BORDER_RADIUS.lg,
  boxShadow: SHADOW.modal,
};

// ── Utility: merge inline styles ──
export function mergeStyles(base: React.CSSProperties, overrides: React.CSSProperties): React.CSSProperties {
  return { ...base, ...overrides };
}
