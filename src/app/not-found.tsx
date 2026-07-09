'use client';

import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import { t, type Locale } from '@/i18n';
import { SPACING, BORDER_RADIUS } from '@/lib/design-tokens';

export default function NotFound() {
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
    <div className="doppelrand-outer h-full w-full flex items-center justify-center">
      <div className="doppelrand-inner flex flex-col items-center justify-center h-full w-full p-10">
        <div className="flex flex-col items-center text-center max-w-sm">
          {/* Large 404 with accent gradient */}
          <motion.div
            initial={{ scale: 0.9, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={{ duration: 0.4, delay: 0.1, ease: [0.16, 1, 0.3, 1] }}
            className="text-[5rem] font-extrabold leading-none tracking-tight mb-6"
            style={{
              fontFamily: 'var(--font-mono)',
              background: 'linear-gradient(135deg, var(--primary) 0%, var(--primary-soft) 100%)',
              WebkitBackgroundClip: 'text',
              WebkitTextFillColor: 'transparent',
            }}
          >
            404
          </motion.div>

          <motion.div
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.3, delay: 0.2 }}
          >
            <h1 className="text-base font-semibold text-[var(--text)] mb-2">
              {t(locale, 'notFound.title')}
            </h1>
            <p className="text-sm text-[var(--text-secondary)] mb-7 leading-relaxed max-w-xs">
              {t(locale, 'notFound.description')}
            </p>

            <button
              onClick={() => window.location.href = '/'}
              className="cta-pill"
            >
              {t(locale, 'notFound.goHome')}
              <span className="cta-icon-wrap">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                  <path d="M5 12h14M12 5l7 7-7 7"/>
                </svg>
              </span>
            </button>
          </motion.div>
        </div>
      </div>
    </div>
  );
}
