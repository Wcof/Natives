'use client';

import { useState, useCallback } from 'react';
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
      background: 'var(--bg,#0b0c0a)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
    }}>
      <div style={{ width: 360, textAlign: 'center' }}>
        <div style={{ fontSize: 48, marginBottom: 16 }}>👋</div>
        <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
          {t(locale, 'common.welcome')}
        </h1>
        <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 20 }}>
          What should we call you?
        </p>
        <input
          type="text"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder={t(locale, 'common.namePlaceholder')}
          onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
          style={{
            width: '100%', padding: '10px 12px', fontSize: 14, textAlign: 'center',
            background: 'var(--bg-2,#131410)', border: '1px solid var(--border,#262920)',
            borderRadius: 6, color: 'var(--text)', outline: 'none', marginBottom: 16,
          }}
          autoFocus
        />
        <button className="btn btn-primary" onClick={handleSubmit} style={{ fontSize: 13, width: '100%' }}>
          {t(locale, 'common.confirm')}
        </button>
      </div>
    </div>
  );
}
