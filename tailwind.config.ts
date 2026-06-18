import type { Config } from 'tailwindcss';
import plugin from 'tailwindcss/plugin';

const config: Config = {
  content: ['./src/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: ['class', '[data-theme="liquid-glass"]'],
  theme: {
    extend: {
      colors: {
        brand: {
          olive: 'var(--border)',
          'olive-sidebar': 'var(--panel)',
          'olive-deep': 'var(--bg)',
          'olive-modal': 'var(--bg-2)',
          'jade-glow': 'var(--accent)',
        },
        glass: {
          active: 'color-mix(in srgb, var(--accent) 80%, transparent)',
          edge: 'rgb(255 255 255 / <alpha-value>)',
        },
        surface: {
          bg: 'var(--bg)',
          'bg-2': 'var(--bg-2)',
          'bg-3': 'var(--bg-3)',
          panel: 'var(--panel)',
          border: 'var(--border)',
        },
        content: {
          text: 'var(--text)',
          'text-dim': 'var(--text-dim)',
          'text-faint': 'var(--text-faint)',
        },
        accent: {
          DEFAULT: 'var(--accent)',
          soft: 'var(--accent-soft)',
          ink: 'var(--accent-ink)',
        },
        semantic: {
          danger: 'var(--danger)',
          warning: 'var(--warning)',
          info: 'var(--info)',
        },
      },
      fontFamily: {
        ui: ['var(--font-ui)'],
        mono: ['var(--font-mono)'],
        display: ['var(--font-display)'],
      },
      borderRadius: {
        component: 'var(--radius)',
      },
      boxShadow: {
        'liquid-edge':
          'inset 0 1px 1px 0 rgba(255, 255, 255, 0.28), inset 0 -1px 1px 0 rgba(0, 0, 0, 0.35)',
        'glass-ambient': '0 10px 30px -5px rgba(0, 0, 0, 0.25)',
        card: '0 1px 3px rgba(0, 0, 0, 0.12)',
        elevated: '0 4px 12px rgba(0, 0, 0, 0.15)',
        modal: '0 8px 32px rgba(0, 0, 0, 0.2)',
      },
      textShadow: {
        jade: '0 0 6px color-mix(in srgb, var(--accent) 25%, transparent)',
      },
      transitionTimingFunction: {
        'ease-out-expo': 'cubic-bezier(0.16, 1, 0.3, 1)',
      },
      keyframes: {
        'slide-up': {
          from: { opacity: '0', transform: 'translateY(12px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'fade-in': {
          from: { opacity: '0', transform: 'translateY(4px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'drop-in': {
          from: { opacity: '0', transform: 'scale(0.96)' },
          to: { opacity: '1', transform: 'scale(1)' },
        },
        'liquid-surface-in': {
          from: { opacity: '0', transform: 'scale(0.985)' },
          to: { opacity: '1', transform: 'scale(1)' },
        },
        'live-pulse': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.4' },
        },
        'skeleton-pulse': {
          '0%, 100%': { opacity: '0.4' },
          '50%': { opacity: '0.8' },
        },
      },
      animation: {
        'slide-up': 'slide-up 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
        'fade-in': 'fade-in 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
        'drop-in': 'drop-in 0.16s cubic-bezier(0.16, 1, 0.3, 1)',
        'liquid-surface-in':
          'liquid-surface-in 0.2s cubic-bezier(0.16, 1, 0.3, 1)',
        'live-pulse': 'live-pulse 1.1s cubic-bezier(0.16, 1, 0.3, 1) infinite',
        'skeleton-pulse':
          'skeleton-pulse 1.5s cubic-bezier(0.16, 1, 0.3, 1) infinite',
      },
    },
  },
  plugins: [
    plugin(({ matchUtilities, theme }) => {
      matchUtilities(
        {
          'text-shadow': (value: string) => ({
            textShadow: value,
          }),
        },
        { values: theme('textShadow') },
      );
    }),
  ],
};

export default config;
