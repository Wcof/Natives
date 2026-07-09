'use client';

import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
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
    <div className="doppelrand-outer h-full w-full flex items-center justify-center">
      <div className="doppelrand-inner flex flex-col items-center justify-center h-full w-full gap-6">
        <motion.div
          animate={{ scale: [1, 1.03, 1] }}
          transition={{ duration: 2.5, repeat: Infinity, ease: 'easeInOut' }}
        >
          <MathCurveLoader size={72} />
        </motion.div>
        <motion.span
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.2, duration: 0.4 }}
          className="text-xs tracking-widest text-[var(--text-disabled)]"
        >
          {t(locale, 'common.loading')}
        </motion.span>
      </div>
    </div>
  );
}
