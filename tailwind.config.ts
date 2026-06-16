import type { Config } from 'tailwindcss';

const config: Config = {
  content: ['./src/**/*.{ts,tsx}'],
  darkMode: ['class', '[data-theme="liquid-glass"]'],
  theme: {
    extend: {
      colors: {
        brand: {
          olive: '#262920',
          'olive-sidebar': '#1E211A',
          'olive-deep': '#161813',
          'jade-glow': '#F2FFD2',
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
        'liquid-edge': 'inset 0 1px 1px 0 rgba(255,255,255,0.25), inset 0 -1px 1px 0 rgba(0,0,0,0.3)',
        'glass-ambient': '0 10px 30px -5px rgba(0,0,0,0.25)',
        card: '0 1px 3px rgba(0,0,0,0.12)',
        elevated: '0 4px 12px rgba(0,0,0,0.15)',
        modal: '0 8px 32px rgba(0,0,0,0.2)',
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
        'drop-in': 'drop-in 0.16s cubic-bezier(0.2, 0.7, 0.3, 1)',
        'live-pulse': 'live-pulse 1.1s ease-in-out infinite',
        'skeleton-pulse': 'skeleton-pulse 1.5s ease-in-out infinite',
      },
    },
  },
  plugins: [],
};

export default config;
