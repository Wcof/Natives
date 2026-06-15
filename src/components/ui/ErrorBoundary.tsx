'use client';

import React from 'react';

// i18n messages for ErrorBoundary (class component can't use hooks)
const MESSAGES = {
  en: { title: 'Something went wrong', fallback: 'An unexpected error occurred', retry: 'Try Again' },
  zh: { title: '出了点问题', fallback: '发生了意外错误', retry: '重试' },
};

function getLocale(): 'en' | 'zh' {
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

      const msg = MESSAGES[getLocale()] || MESSAGES.en;

      return (
        <div style={{
          display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
          height: '100%', padding: 32, gap: 12,
          color: 'var(--text, #f2f2ea)', background: 'var(--bg, #0b0c0a)',
        }}>
          <div style={{ fontSize: 32 }}>💥</div>
          <div style={{ fontSize: 14, fontWeight: 600 }}>{msg.title}</div>
          <div style={{ fontSize: 12, color: 'var(--text-dim, #9b9d8c)', textAlign: 'center', maxWidth: 400 }}>
            {this.state.error?.message || msg.fallback}
          </div>
          <button
            onClick={this.handleReset}
            style={{
              marginTop: 8, padding: '6px 16px', borderRadius: 6,
              background: 'var(--accent, #cdf24b)', color: 'var(--bg, #0b0c0a)',
              border: 'none', fontSize: 12, fontWeight: 600, cursor: 'pointer',
            }}
          >
            {msg.retry}
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
