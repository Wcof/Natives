// ── AI Natives Design System V2.0 Tokens ──
// Design System V2: Dark Glow（暗黑流光，低对比深色 + 克制 glow）+
// Liquid Crystal（晶透液态，独立浅色，禁止 dark 机械反色）。
//
// 单一真值源 (Single Source of Truth):
//   · V2_TOKENS 是本仓库唯一的「主题值」权威 —— theme-engine.ts 在运行时
//     把 V2_TOKENS 应用到 CSS 变量；src/app/styles/tokens.css 只保留
//     SSR 首帧 fallback（标记为 mirror），任何值改动都必须先改本文件。
//   · 组件一律消费语义 token（var(--surface) / var(--elevation-floating) …），
//     禁止调用方传 hex / 自建第二套颜色权威。

// ── 结构令牌：Spacing (4px base) ──
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

// ── 结构令牌：字号 ──
export const FONT_SIZE = {
  micro: '0.75rem',    // 12px Caption
  xs: '0.8125rem',     // 13px Label
  sm: '0.875rem',      // 14px Body (默认)
  md: '1rem',          // 16px Body Large
  lg: '1.125rem',      // 18px Heading
  xl: '1.5rem',        // 24px Title
  hero: '1.75rem',     // 28px Display
} as const;

// ── 结构令牌：圆角 ──
export const BORDER_RADIUS = {
  xs: 8,    // Small Button / Tag
  sm: 10,   // Button / Input
  md: 12,   // Card
  lg: 14,   // Large Card
  xl: 16,   // Window
  pill: 999,
} as const;

// ── 结构令牌：Motion（R-U13/14: one curve, three durations） ──
export const MOTION_EASING = 'cubic-bezier(0.16, 1, 0.3, 1)' as const;

export const TRANSITION = {
  fast: `120ms ${MOTION_EASING}`,
  normal: `200ms ${MOTION_EASING}`,
  slow: `300ms ${MOTION_EASING}`,
} as const;

// Continuous loading rotation is the only linear-easing exception (R-U13).
export const SPINNER_EASING = 'linear' as const;

// ── 结构令牌：布局 ──
export const LAYOUT = {
  readingWidth: 760,
  titlebarHeight: 48,
  controlCompact: 32,
  turnGap: 40,
} as const;

// ── 15 级全局中性色板（固定，深浅主题同值；主题切换只改语义映射） ──
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

// ── Chart volume 色阶（0-8，数据体量映射；0 = 真实零值） ──
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

// ════════════════════════════════════════════════════════════════════
// V2 主题 ID —— 内部固定 dark / light
// 旧名（terminal-volt / frosted-jasmine）只读兼容，由 theme-engine
// normalizeThemeId 映射；界面显示名走 i18n（设置页/命令面板）。
// ════════════════════════════════════════════════════════════════════

export type V2ThemeId = 'dark' | 'light';

export type ElevationLevel = 'base' | 'raised' | 'floating' | 'inset';

export const V2_THEME_NAMES: Record<V2ThemeId, { labelKey: string; descKey: string }> = {
  dark: { labelKey: 'settings.themeDarkGlow', descKey: 'settings.themeDescDarkGlow' },
  light: { labelKey: 'settings.themeLiquidCrystal', descKey: 'settings.themeDescLiquidCrystal' },
};

export const ELEVATION_LEVELS: ElevationLevel[] = ['base', 'raised', 'floating', 'inset'];

// ════════════════════════════════════════════════════════════════════
// V2 语义令牌（单一真值源）
//   dark  = Dark Glow  暗黑流光：低对比深色、克制 glow（仅 focus/selected/status）
//   light = Liquid Crystal 晶透液态：独立浅色（四条法则见 §7 contract）
// ════════════════════════════════════════════════════════════════════

export interface V2ThemeTokens {
  // ── Surfaces ──
  background: string;
  surface: string;
  'surface-hover': string;
  'surface-active': string;
  sidebar: string;
  canvas: string;
  raised: string;
  inset: string;
  composer: string;

  // ── Elevation（base/raised/floating/inset 四级） ──
  'elevation-base': string;
  'elevation-raised': string;
  'elevation-floating': string;
  'elevation-inset': string;
  'elev-shadow-base': string;
  'elev-shadow-raised': string;
  'elev-shadow-floating': string;
  'elev-shadow-inset': string;

  // ── Text ──
  text: string;
  'text-body': string;
  'text-secondary': string;
  'text-tertiary': string;
  'text-disabled': string;
  'text-ghost': string;
  selection: string;

  // ── Border ──
  border: string;
  'border-subtle': string;
  'border-strong': string;

  // ── Brand ──
  primary: string;
  'primary-hover': string;
  'primary-soft': string;
  'primary-dark': string;

  // ── Control ──
  'control-bg': string;
  'control-bg-hover': string;
  'control-fg': string;
  'control-selected-bg': string;
  'control-selected-bg-hover': string;
  'control-selected-fg': string;

  // ── Semantic ──
  danger: string;
  'danger-soft': string;
  warning: string;
  'warning-soft': string;
  info: string;
  'info-soft': string;
  success: string;
  'success-soft': string;

  // ── Glow（克制：仅 focus / selected / status，禁止霓虹墙） ──
  'glow-focus': string;
  'glow-selected': string;
  'glow-status-success': string;
  'glow-status-danger': string;

  // ── Crystal（晶透液态：顶层 Specular Highlight + 低 alpha 微阴影） ──
  'highlight-specular': string;
  'highlight-specular-strong': string;
  'crystal-border': string;
  'crystal-shadow-1': string;
  'crystal-shadow-2': string;
  'crystal-shadow-3': string;

  // ── Shadow（常规容器） ──
  'shadow-card': string;
  'shadow-popup': string;
  'shadow-modal': string;
  'shadow-thumb': string;

  // ── Overlay ──
  overlay: string;
  'overlay-soft': string;
  'overlay-medium': string;
  'overlay-strong': string;
  'overlay-dense': string;

  // ── Chart ──
  'chart-strong': string;
  'chart-medium': string;
  'chart-soft': string;
  'chart-empty': string;
  'chart-grid': string;
  'chart-axis': string;
  'chart-area-fill': string;

  // ── Terminal ANSI 联动 ──
  'terminal-bg': string;
  'terminal-fg': string;
  'terminal-cursor': string;
  'terminal-selection': string;
}

export const V2_TOKENS: Record<V2ThemeId, V2ThemeTokens> = {
  // ── Dark Glow 暗黑流光：低对比深色、克制 glow、中性色板光度分层 ──
  dark: {
    background: NEUTRAL_PALETTE[0],
    surface: NEUTRAL_PALETTE[150],
    'surface-hover': NEUTRAL_PALETTE[200],
    'surface-active': NEUTRAL_PALETTE[250],
    sidebar: NEUTRAL_PALETTE[100],
    canvas: 'var(--background)',
    raised: NEUTRAL_PALETTE[200],
    inset: NEUTRAL_PALETTE[100],
    composer: NEUTRAL_PALETTE[150],

    'elevation-base': NEUTRAL_PALETTE[0],
    'elevation-raised': NEUTRAL_PALETTE[150],
    'elevation-floating': NEUTRAL_PALETTE[200],
    'elevation-inset': NEUTRAL_PALETTE[100],
    'elev-shadow-base': 'none',
    'elev-shadow-raised': '0 1px 2px rgba(0, 0, 0, 0.25)',
    'elev-shadow-floating': '0 8px 24px rgba(0, 0, 0, 0.35), 0 1px 3px rgba(0, 0, 0, 0.2)',
    'elev-shadow-inset': 'inset 0 1px 3px rgba(0, 0, 0, 0.3)',

    text: NEUTRAL_PALETTE[950],
    'text-body': NEUTRAL_PALETTE[850],
    'text-secondary': NEUTRAL_PALETTE[700],
    'text-tertiary': NEUTRAL_PALETTE[600],
    'text-disabled': NEUTRAL_PALETTE[500],
    'text-ghost': 'var(--text-disabled)',
    selection: 'color-mix(in srgb, var(--neutral-950) 24%, transparent)',

    border: NEUTRAL_PALETTE[300],
    'border-subtle': NEUTRAL_PALETTE[200],
    'border-strong': NEUTRAL_PALETTE[400],

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
    'danger-soft': 'rgba(239, 68, 68, 0.14)',
    warning: '#F59E0B',
    'warning-soft': 'rgba(245, 158, 11, 0.14)',
    info: '#60A5FA',
    'info-soft': 'rgba(96, 165, 250, 0.14)',
    success: '#34D399',
    'success-soft': 'rgba(52, 211, 153, 0.14)',

    'glow-focus': '0 0 0 3px rgba(250, 250, 252, 0.10), 0 0 12px rgba(250, 250, 252, 0.06)',
    'glow-selected': '0 0 0 1px var(--primary), 0 0 0 3px rgba(250, 250, 252, 0.06)',
    'glow-status-success': '0 0 12px rgba(52, 211, 153, 0.25)',
    'glow-status-danger': '0 0 12px rgba(239, 68, 68, 0.25)',

    'highlight-specular': 'rgba(255, 255, 255, 0.10)',
    'highlight-specular-strong': 'rgba(255, 255, 255, 0.16)',
    'crystal-border': 'rgba(255, 255, 255, 0.10)',
    'crystal-shadow-1': '0 1px 2px rgba(0, 0, 0, 0.35)',
    'crystal-shadow-2': '0 4px 14px rgba(0, 0, 0, 0.3)',
    'crystal-shadow-3': '0 16px 40px rgba(0, 0, 0, 0.45)',

    'shadow-card': '0 1px 2px rgba(0, 0, 0, 0.3)',
    'shadow-popup': '0 8px 24px rgba(0, 0, 0, 0.4)',
    'shadow-modal': '0 20px 48px rgba(0, 0, 0, 0.55)',
    'shadow-thumb': '0 1px 2px rgba(0, 0, 0, 0.4)',

    overlay: 'rgba(0, 0, 0, 0.55)',
    'overlay-soft': 'rgba(0, 0, 0, 0.35)',
    'overlay-medium': 'rgba(0, 0, 0, 0.45)',
    'overlay-strong': 'rgba(0, 0, 0, 0.65)',
    'overlay-dense': 'rgba(0, 0, 0, 0.85)',

    'chart-strong': NEUTRAL_PALETTE[850],
    'chart-medium': NEUTRAL_PALETTE[600],
    'chart-soft': NEUTRAL_PALETTE[300],
    'chart-empty': NEUTRAL_PALETTE[200],
    'chart-grid': 'rgba(250, 250, 252, 0.06)',
    'chart-axis': 'rgba(250, 250, 252, 0.14)',
    // Dark: 克制 fill，8% → 0%
    'chart-area-fill': 'linear-gradient(180deg, rgba(250, 250, 252, 0.08) 0%, rgba(250, 250, 252, 0) 100%)',

    'terminal-bg': NEUTRAL_PALETTE[150],
    'terminal-fg': NEUTRAL_PALETTE[950],
    'terminal-cursor': NEUTRAL_PALETTE[950],
    'terminal-selection': NEUTRAL_PALETTE[950] + '33',
  },

  // ── Liquid Crystal 晶透液态：独立浅色，非 dark 机械反色 ──
  // 法则 1: 顶层 Specular Highlight --highlight-specular
  // 法则 2: 2-3 层低 alpha 微阴影替代粗边框（--crystal-shadow-1..3 / --crystal-border）
  // 法则 3: 深石墨灰/中性灰文字层级（text=graphite, body/secondary=neutral gray）
  // 法则 4: 浅色图表 area fill 约 15% → 0% 渐变
  light: {
    background: NEUTRAL_PALETTE[950],
    surface: NEUTRAL_PALETTE[1000],
    'surface-hover': NEUTRAL_PALETTE[900],
    'surface-active': NEUTRAL_PALETTE[850],
    sidebar: NEUTRAL_PALETTE[900],
    canvas: 'var(--background)',
    raised: NEUTRAL_PALETTE[1000],
    inset: NEUTRAL_PALETTE[900],
    composer: NEUTRAL_PALETTE[1000],

    'elevation-base': NEUTRAL_PALETTE[950],
    'elevation-raised': NEUTRAL_PALETTE[1000],
    'elevation-floating': NEUTRAL_PALETTE[1000],
    'elevation-inset': NEUTRAL_PALETTE[900],
    'elev-shadow-base': 'none',
    'elev-shadow-raised': '0 1px 2px rgba(18, 18, 22, 0.05)',
    'elev-shadow-floating': '0 4px 16px rgba(18, 18, 22, 0.07), 0 1px 2px rgba(18, 18, 22, 0.04)',
    'elev-shadow-inset': 'inset 0 1px 3px rgba(18, 18, 22, 0.06)',

    text: NEUTRAL_PALETTE[150], // 深石墨灰
    'text-body': NEUTRAL_PALETTE[400],
    'text-secondary': NEUTRAL_PALETTE[500],
    'text-tertiary': NEUTRAL_PALETTE[600],
    'text-disabled': NEUTRAL_PALETTE[700],
    'text-ghost': 'var(--text-disabled)',
    selection: 'color-mix(in srgb, var(--neutral-150) 18%, transparent)',

    border: NEUTRAL_PALETTE[850],
    'border-subtle': NEUTRAL_PALETTE[900],
    'border-strong': NEUTRAL_PALETTE[800],

    primary: NEUTRAL_PALETTE[150], // 深石墨
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
    'danger-soft': 'rgba(220, 38, 38, 0.08)',
    warning: '#D97706',
    'warning-soft': 'rgba(217, 119, 6, 0.08)',
    info: '#2563EB',
    'info-soft': 'rgba(37, 99, 235, 0.08)',
    success: '#059669',
    'success-soft': 'rgba(5, 150, 105, 0.08)',

    'glow-focus': '0 0 0 3px rgba(24, 24, 26, 0.07)',
    'glow-selected': '0 0 0 1px var(--primary), 0 0 0 3px rgba(24, 24, 26, 0.05)',
    'glow-status-success': '0 0 10px rgba(5, 150, 105, 0.18)',
    'glow-status-danger': '0 0 10px rgba(220, 38, 38, 0.18)',

    // 法则 1: 顶层高光
    'highlight-specular': 'rgba(255, 255, 255, 0.85)',
    'highlight-specular-strong': 'rgba(255, 255, 255, 1)',
    // 法则 2: 极细发丝线 + 微阴影替代粗边框
    'crystal-border': 'rgba(24, 24, 26, 0.06)',
    'crystal-shadow-1': '0 1px 2px rgba(24, 24, 26, 0.04)',
    'crystal-shadow-2': '0 6px 20px rgba(24, 24, 26, 0.06)',
    'crystal-shadow-3': '0 20px 48px rgba(24, 24, 26, 0.10)',

    'shadow-card': '0 1px 3px rgba(24, 24, 26, 0.05)',
    'shadow-popup': '0 8px 24px rgba(24, 24, 26, 0.09)',
    'shadow-modal': '0 20px 48px rgba(24, 24, 26, 0.14)',
    'shadow-thumb': '0 1px 2px rgba(24, 24, 26, 0.10)',

    overlay: 'rgba(0, 0, 0, 0.5)',
    'overlay-soft': 'rgba(0, 0, 0, 0.3)',
    'overlay-medium': 'rgba(0, 0, 0, 0.4)',
    'overlay-strong': 'rgba(0, 0, 0, 0.6)',
    'overlay-dense': 'rgba(0, 0, 0, 0.85)',

    // 法则 3: 深石墨/中性灰图表层级
    'chart-strong': NEUTRAL_PALETTE[250],
    'chart-medium': NEUTRAL_PALETTE[500],
    'chart-soft': NEUTRAL_PALETTE[800],
    'chart-empty': NEUTRAL_PALETTE[900],
    'chart-grid': 'rgba(24, 24, 26, 0.05)',
    'chart-axis': 'rgba(24, 24, 26, 0.12)',
    // 法则 4: 浅色图表 area fill ≈ 15% → 0%
    'chart-area-fill': 'linear-gradient(180deg, rgba(24, 24, 26, 0.15) 0%, rgba(24, 24, 26, 0) 100%)',

    'terminal-bg': NEUTRAL_PALETTE[1000],
    'terminal-fg': NEUTRAL_PALETTE[150],
    'terminal-cursor': NEUTRAL_PALETTE[150],
    'terminal-selection': NEUTRAL_PALETTE[150] + '33',
  },
};

// ── CSS 语义引用（组件用它拿 var() 字符串） ──
export const THEME_TOKENS = {
  // Surfaces
  background: 'var(--background)',
  canvas: 'var(--canvas)',
  surface: 'var(--surface)',
  surfaceHover: 'var(--surface-hover)',
  surfaceActive: 'var(--surface-active)',
  raised: 'var(--raised)',
  inset: 'var(--inset)',
  composer: 'var(--composer)',
  sidebar: 'var(--sidebar)',

  // Elevation
  elevationBase: 'var(--elevation-base)',
  elevationRaised: 'var(--elevation-raised)',
  elevationFloating: 'var(--elevation-floating)',
  elevationInset: 'var(--elevation-inset)',
  elevationShadowBase: 'var(--elev-shadow-base)',
  elevationShadowRaised: 'var(--elev-shadow-raised)',
  elevationShadowFloating: 'var(--elev-shadow-floating)',
  elevationShadowInset: 'var(--elev-shadow-inset)',

  // Text
  text: 'var(--text)',
  textBody: 'var(--text-body)',
  textSecondary: 'var(--text-secondary)',
  textTertiary: 'var(--text-tertiary)',
  textGhost: 'var(--text-ghost)',
  textDisabled: 'var(--text-disabled)',
  selection: 'var(--selection)',

  // Brand
  primary: 'var(--primary)',
  primaryHover: 'var(--primary-hover)',
  primarySoft: 'var(--primary-soft)',
  primaryInk: 'var(--primary-dark)',

  // Border
  border: 'var(--border)',
  borderSubtle: 'var(--border-subtle)',
  borderStrong: 'var(--border-strong)',

  // Semantic
  danger: 'var(--danger)',
  warning: 'var(--warning)',
  info: 'var(--info)',
  success: 'var(--success)',

  // Glow
  glowFocus: 'var(--glow-focus)',
  glowSelected: 'var(--glow-selected)',
  glowStatusSuccess: 'var(--glow-status-success)',
  glowStatusDanger: 'var(--glow-status-danger)',

  // Crystal
  highlightSpecular: 'var(--highlight-specular)',
  highlightSpecularStrong: 'var(--highlight-specular-strong)',
  crystalBorder: 'var(--crystal-border)',
  crystalShadow1: 'var(--crystal-shadow-1)',
  crystalShadow2: 'var(--crystal-shadow-2)',
  crystalShadow3: 'var(--crystal-shadow-3)',

  // Shadow
  shadowCard: 'var(--shadow-card)',
  shadowPopup: 'var(--shadow-popup)',
  shadowModal: 'var(--shadow-modal)',

  // Chart
  chartStrong: 'var(--chart-strong)',
  chartMedium: 'var(--chart-medium)',
  chartSoft: 'var(--chart-soft)',
  chartEmpty: 'var(--chart-empty)',
  chartGrid: 'var(--chart-grid)',
  chartAxis: 'var(--chart-axis)',
  chartAreaFill: 'var(--chart-area-fill)',

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
} as const;

// ── 向后兼容：hex 核心子集（Zod 校验用；rgba/gradient/shadow 留在 CSS 层） ──
const V2_THEME_CORE = {
  dark: {
    background: V2_TOKENS.dark.background,
    surface: V2_TOKENS.dark.surface,
    'surface-hover': V2_TOKENS.dark['surface-hover'],
    sidebar: V2_TOKENS.dark.sidebar,
    border: V2_TOKENS.dark.border,
    'border-subtle': V2_TOKENS.dark['border-subtle'],
    text: V2_TOKENS.dark.text,
    'text-body': V2_TOKENS.dark['text-body'],
    'text-secondary': V2_TOKENS.dark['text-secondary'],
    'text-disabled': V2_TOKENS.dark['text-disabled'],
    primary: V2_TOKENS.dark.primary,
    'primary-hover': V2_TOKENS.dark['primary-hover'],
    'primary-soft': V2_TOKENS.dark['primary-soft'],
    'primary-dark': V2_TOKENS.dark['primary-dark'],
    'control-bg': V2_TOKENS.dark['control-bg'],
    'control-bg-hover': V2_TOKENS.dark['control-bg-hover'],
    'control-fg': V2_TOKENS.dark['control-fg'],
    'control-selected-bg': V2_TOKENS.dark['control-selected-bg'],
    'control-selected-bg-hover': V2_TOKENS.dark['control-selected-bg-hover'],
    'control-selected-fg': V2_TOKENS.dark['control-selected-fg'],
    danger: V2_TOKENS.dark.danger,
    warning: V2_TOKENS.dark.warning,
    info: V2_TOKENS.dark.info,
    success: V2_TOKENS.dark.success,
    'diff-add': V2_TOKENS.dark.success,
    'diff-del': V2_TOKENS.dark.danger,
    'diff-mod': V2_TOKENS.dark.warning,
  },
  light: {
    background: V2_TOKENS.light.background,
    surface: V2_TOKENS.light.surface,
    'surface-hover': V2_TOKENS.light['surface-hover'],
    sidebar: V2_TOKENS.light.sidebar,
    border: V2_TOKENS.light.border,
    'border-subtle': V2_TOKENS.light['border-subtle'],
    text: V2_TOKENS.light.text,
    'text-body': V2_TOKENS.light['text-body'],
    'text-secondary': V2_TOKENS.light['text-secondary'],
    'text-disabled': V2_TOKENS.light['text-disabled'],
    primary: V2_TOKENS.light.primary,
    'primary-hover': V2_TOKENS.light['primary-hover'],
    'primary-soft': V2_TOKENS.light['primary-soft'],
    'primary-dark': V2_TOKENS.light['primary-dark'],
    'control-bg': V2_TOKENS.light['control-bg'],
    'control-bg-hover': V2_TOKENS.light['control-bg-hover'],
    'control-fg': V2_TOKENS.light['control-fg'],
    'control-selected-bg': V2_TOKENS.light['control-selected-bg'],
    'control-selected-bg-hover': V2_TOKENS.light['control-selected-bg-hover'],
    'control-selected-fg': V2_TOKENS.light['control-selected-fg'],
    danger: V2_TOKENS.light.danger,
    warning: V2_TOKENS.light.warning,
    info: V2_TOKENS.light.info,
    success: V2_TOKENS.light.success,
    'diff-add': V2_TOKENS.light.success,
    'diff-del': V2_TOKENS.light.danger,
    'diff-mod': V2_TOKENS.light.warning,
  },
} as const;

export type Theme = (typeof V2_THEME_CORE)['dark'];

// ── THEMES（兼容旧引用；值派生自 V2_TOKENS 单一真值） ──
export const THEMES: Record<string, Theme> = {
  dark: { ...V2_THEME_CORE.dark },
  light: { ...V2_THEME_CORE.light },
  // 只读兼容旧名 —— 不参与写入，仅防旧代码读取 undefined
  'terminal-volt': { ...V2_THEME_CORE.dark },
  'frosted-jasmine': { ...V2_THEME_CORE.light },
};

// ── Terminal ANSI 配色（派生自 V2_TOKENS，随皮肤联动） ──
export const TERMINAL_THEMES: Record<string, { background: string; foreground: string; cursor: string; selectionBackground?: string }> = {
  dark: {
    background: V2_TOKENS.dark['terminal-bg'],
    foreground: V2_TOKENS.dark['terminal-fg'],
    cursor: V2_TOKENS.dark['terminal-cursor'],
    selectionBackground: V2_TOKENS.dark['terminal-selection'],
  },
  light: {
    background: V2_TOKENS.light['terminal-bg'],
    foreground: V2_TOKENS.light['terminal-fg'],
    cursor: V2_TOKENS.light['terminal-cursor'],
    selectionBackground: V2_TOKENS.light['terminal-selection'],
  },
  'terminal-volt': {
    background: V2_TOKENS.dark['terminal-bg'],
    foreground: V2_TOKENS.dark['terminal-fg'],
    cursor: V2_TOKENS.dark['terminal-cursor'],
    selectionBackground: V2_TOKENS.dark['terminal-selection'],
  },
  'frosted-jasmine': {
    background: V2_TOKENS.light['terminal-bg'],
    foreground: V2_TOKENS.light['terminal-fg'],
    cursor: V2_TOKENS.light['terminal-cursor'],
    selectionBackground: V2_TOKENS.light['terminal-selection'],
  },
};

// ════════════════════════════════════════════════════════════════════
// 组件级预置（全部消费语义 token，禁止硬编码）
// ════════════════════════════════════════════════════════════════════

/** Standard Card（elevation-raised 语义表面） */
export const CARD_STYLE: React.CSSProperties = {
  background: THEME_TOKENS.surface,
  border: `1px solid ${THEME_TOKENS.borderSubtle}`,
  borderRadius: BORDER_RADIUS.lg,
  padding: SPACING.xl,
  boxShadow: THEME_TOKENS.elevationShadowRaised,
  transition: `background-color ${TRANSITION.normal}, border-color ${TRANSITION.normal}, box-shadow ${TRANSITION.normal}`,
};

/** Standard Input */
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

/** Section title */
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

/** Modal/dialog overlay */
export const OVERLAY_STYLE: React.CSSProperties = {
  position: 'fixed',
  inset: 0,
  background: 'var(--overlay-medium)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  zIndex: 60,
};

/** Dialog surface（elevation-floating） */
export const DIALOG_SURFACE_STYLE: React.CSSProperties = {
  background: THEME_TOKENS.elevationFloating,
  border: `1px solid ${THEME_TOKENS.borderSubtle}`,
  borderRadius: BORDER_RADIUS.lg,
  boxShadow: THEME_TOKENS.elevationShadowFloating,
};

// ── Utility: merge inline styles ──
export function mergeStyles(base: React.CSSProperties, overrides: React.CSSProperties): React.CSSProperties {
  return { ...base, ...overrides };
}
