import type { Config } from 'tailwindcss';
import plugin from 'tailwindcss/plugin';

const config: Config = {
  content: ['./src/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: ['class', '[data-theme="dark"]'],
  theme: {
    extend: {
      colors: {
        // ── AI Natives V1.0 Brand ──
        brand: {
          DEFAULT: 'var(--primary)',
          hover: 'var(--primary-hover)',
          soft: 'var(--primary-soft)',
          dark: 'var(--primary-dark)',
          ink: 'var(--accent-ink)',
        },
        // ── Grayscale steps ──
        gray: {
          50: 'var(--gray-50)',
          100: 'var(--gray-100)',
          200: 'var(--gray-200)',
          300: 'var(--gray-300)',
          400: 'var(--gray-400)',
          500: 'var(--gray-500)',
          600: 'var(--gray-600)',
          700: 'var(--gray-700)',
          800: 'var(--gray-800)',
          900: 'var(--gray-900)',
        },
        // ── V1.0 Surfaces (Light/Dark 双主题) ──
        surface: {
          background: 'var(--background)',
          DEFAULT: 'var(--surface)',
          hover: 'var(--surface-hover)',
          sidebar: 'var(--sidebar)',
          border: 'var(--border)',
          'border-subtle': 'var(--border-subtle)',
        },
        // ── V1.0 Text ──
        content: {
          DEFAULT: 'var(--text)',
          body: 'var(--text-body)',
          secondary: 'var(--text-secondary)',
          disabled: 'var(--text-disabled)',
        },
        // ── V1.0 Semantic ──
        semantic: {
          danger: 'var(--danger)',
          warning: 'var(--warning)',
          info: 'var(--info)',
          success: 'var(--success)',
        },
        // ── Diff (代码变更) ──
        diff: {
          add: 'var(--diff-add)',
          del: 'var(--diff-del)',
          mod: 'var(--diff-mod)',
        },
        // ── 向下兼容别名 (旧 vibe-* 体系) ──
        accent: {
          DEFAULT: 'var(--primary)',
          soft: 'var(--primary-soft)',
          ink: 'var(--accent-ink)',
        },
        panel: 'var(--surface)',
        bg: 'var(--background)',
        'bg-2': 'var(--surface)',
        'bg-3': 'var(--surface-hover)',
      },
      fontFamily: {
        ui: ['var(--font-ui)'],
        mono: ['var(--font-mono)'],
        display: ['var(--font-display)'],
      },
      borderRadius: {
        xs: 'var(--radius-xs)',
        sm: 'var(--radius-sm)',
        md: 'var(--radius-md)',
        lg: 'var(--radius-lg)',
        xl: 'var(--radius-xl)',
        component: 'var(--radius)',
      },
      boxShadow: {
        card: 'var(--shadow-card)',
        popup: 'var(--shadow-popup)',
        modal: 'var(--shadow-modal)',
      },
      transitionDuration: {
        DEFAULT: '150ms',
        fast: '100ms',
        slow: '200ms',
      },
      keyframes: {
        'fade-in': {
          from: { opacity: '0', transform: 'translateY(4px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'drop-in': {
          from: { opacity: '0', transform: 'scale(0.96)' },
          to: { opacity: '1', transform: 'scale(1)' },
        },
        'slide-up': {
          from: { opacity: '0', transform: 'translateY(12px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'skeleton-pulse': {
          '0%, 100%': { opacity: '0.4' },
          '50%': { opacity: '0.8' },
        },
      },
      animation: {
        'fade-in': 'fade-in 150ms ease forwards',
        'drop-in': 'drop-in 150ms ease forwards',
        'slide-up': 'slide-up 200ms ease forwards',
        'skeleton-pulse': 'skeleton-pulse 1.5s ease infinite',
      },
    },
  },
  plugins: [
    plugin(({ matchUtilities }) => {
      matchUtilities(
        {
          'text-shadow': (value: string) => ({ textShadow: value }),
        },
        { values: { none: 'none' } },
      );
    }),
  ],
};

export default config;
