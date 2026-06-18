'use client';

import { useState, useCallback } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';

interface UsernameOnboardingProps {
  locale: Locale;
  onComplete: (username: string) => void;
}

export default function UsernameOnboarding({ locale, onComplete }: UsernameOnboardingProps) {
  const [username, setUsername] = useState('');

  const handleSubmit = useCallback(async () => {
    const name = username.trim() || 'Developer';
    try {
      await window.nativesAPI?.db?.set?.('settings:username', name);
    } catch { /* ignore */ }
    onComplete(name);
  }, [username, onComplete]);

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 9999,
      background: 'var(--vibe-content-bg)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
    }}>
      <div style={{ width: 360, textAlign: 'center' }}>
        <div style={{ fontSize: 48, marginBottom: SPACING.lg }}>👋</div>
        <h1 style={{ fontSize: FONT_SIZE.heading, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
          {t(locale, 'common.welcome')}
        </h1>
        <p style={{ fontSize: FONT_SIZE.lg, color: 'var(--text-dim)', marginBottom: SPACING.xl }}>
          What should we call you?
        </p>
        <input
          type="text"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder={t(locale, 'common.namePlaceholder')}
          onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
          style={{
            width: '100%', padding: `px px`, fontSize: FONT_SIZE.xl, textAlign: 'center',
            background: 'var(--vibe-toolbar-bg)', border: '1px solid var(--vibe-btn-border)',
            borderRadius: BORDER_RADIUS.md, color: 'var(--text)', outline: 'none', marginBottom: SPACING.lg,
          }}
          autoFocus
        />
        <button className="btn btn-primary" onClick={handleSubmit} style={{ fontSize: FONT_SIZE.lg, width: '100%' }}>
          {t(locale, 'common.confirm')}
        </button>
      </div>
    </div>
  );
}
