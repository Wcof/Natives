'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';

export default function NotFound() {
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function load() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch {}
    }
    load();
  }, []);

  return (
    <div style={{
      display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
      height: '100%', padding: 40, textAlign: 'center',
    }}>
      <div style={{ fontSize: 48, marginBottom: 16 }}>404</div>
      <h1 style={{ fontSize: 18, fontWeight: 600, color: 'var(--text,#f2f2ea)', marginBottom: 8 }}>
        {t(locale, 'notFound.title')}
      </h1>
      <p style={{ fontSize: 13, color: 'var(--text-faint,#62655a)', marginBottom: 24 }}>
        {t(locale, 'notFound.description')}
      </p>
      <button className="btn btn-primary" onClick={() => window.location.href = '/'}>
        {t(locale, 'notFound.goHome')}
      </button>
    </div>
  );
}
