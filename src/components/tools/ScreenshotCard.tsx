'use client';

import { useState, useEffect } from 'react';
import { X } from 'lucide-react';
import { t, type Locale } from '@/i18n';

export default function ScreenshotCard() {
  const [locale, setLocale] = useState<Locale>('zh');
  const [visible, setVisible] = useState(true);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  if (!visible) return null;

  return (
    <div style={{
      position: 'fixed', bottom: 80, right: 20, zIndex: 1000,
      width: 280, background: 'var(--panel)', borderRadius: 'var(--radius)',
      border: '1px solid var(--border)', overflow: 'hidden',
      boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
    }}>
      <div style={{ padding: 8, borderBottom: '1px solid var(--border)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--text)' }}>{t(locale, 'screenshot.newScreenshot')}</span>
        <button className="btn btn-ghost" style={{ fontSize: 14, padding: '0 4px' }} onClick={() => setVisible(false)}><X size={14} /></button>
      </div>
      <div style={{ padding: 8, display: 'flex', gap: 6 }}>
        <button className="btn btn-primary" style={{ flex: 1, fontSize: 11 }}>{t(locale, 'screenshot.sendToTerminal')}</button>
        <button className="btn btn-ghost" style={{ flex: 1, fontSize: 11 }}>{t(locale, 'screenshot.saveToMaterial')}</button>
        <button className="btn btn-ghost" style={{ flex: 1, fontSize: 11 }}>{t(locale, 'screenshot.annotate')}</button>
      </div>
    </div>
  );
}
