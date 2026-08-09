'use client';

import React from 'react';
import { AlertTriangle, RefreshCw } from 'lucide-react';
import { t } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

// i18n messages for ErrorBoundary (class component can't use hooks)
function getLocale(): string {
  try {
    const el = document.documentElement;
    const lang = el.getAttribute('lang') || el.dataset.locale || '';
    return lang.startsWith('zh') ? 'zh' : 'en';
  } catch { return 'en'; }
}

interface Props {
  children: React.ReactNode;
  fallback?: React.ReactNode;
  onReset?: () => void;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

/**
 * Error Boundary — catches rendering errors and shows a recovery UI
 * instead of crashing the entire application to a white screen.
 */
export default class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('[ErrorBoundary]', error, errorInfo);
  }

  handleReset = () => {
    this.setState({ hasError: false, error: null });
    this.props.onReset?.();
  };

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) return this.props.fallback;

      const locale = getLocale();

      return (
        <div style={{
          display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
          height: '100%', padding: SPACING.xxl, gap: SPACING.md,
          color: 'var(--text)', background: 'var(--surface)',
        }}>
          <AlertTriangle size={32} style={{ color: 'var(--danger)' }} />
<div style={{ fontSize: FONT_SIZE.xl, fontWeight: 600 }}>{t(locale, 'errorBoundary.title')}</div>
<div style={{ fontSize: FONT_SIZE.md, color: 'var(--text-secondary)', textAlign: 'center', maxWidth: 400 }}>
            {this.state.error?.message || t(locale, 'errorBoundary.fallback')}
          </div>
          <button
            onClick={this.handleReset}
            style={{
              marginTop: SPACING.sm, padding: `${SPACING.xs}px ${SPACING.lg}px`, borderRadius: BORDER_RADIUS.lg,
              background: 'var(--surface)', color: 'var(--text-secondary)',
              border: '0.0625rem solid var(--border)', fontSize: FONT_SIZE.md, fontWeight: 600, cursor: 'pointer',
              display: 'inline-flex', alignItems: 'center', gap: SPACING.xs,
            }}
          >
            <RefreshCw size={12} /> {t(locale, 'errorBoundary.retry')}
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
