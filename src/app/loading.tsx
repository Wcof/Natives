'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';

export default function Loading() {
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
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      height: '100%', color: 'var(--vibe-btn-text)', fontSize: '0.8125rem',
    }}>
      {t(locale, 'common.loading')}
    </div>
  );
}
