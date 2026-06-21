'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';

export default function Loading() {
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function load() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* no-op */ }
    }
    load();
  }, []);

  return (
    <div style={{
      display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
      height: '100%', gap: '16px', color: 'var(--text-dim)', fontSize: '0.875rem',
    }}>
      <MathCurveLoader size={80} />
      <span style={{ letterSpacing: '0.05em' }}>{t(locale, 'common.loading')}</span>
    </div>
  );
}
