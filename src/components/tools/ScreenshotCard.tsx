'use client';

import { useState, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { t, type Locale } from '@/i18n';

export default function ScreenshotCard() {
  const [locale, setLocale] = useState<Locale>('zh');
  const [visible, setVisible] = useState(true);
  const [mounted, setMounted] = useState(false);

  useEffect(() => { setMounted(true); }, []);

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
  if (!mounted) return null;
  const root = document.getElementById('content-overlay-root');
  if (!root) return null;

  return createPortal(
    <div style={{
      position: 'absolute', bottom: 20, right: 20, zIndex: 1000,
      width: 280,
      background: 'var(--vibe-toolbar-bg)',
      backdropFilter: 'blur(20px) saturate(145%)',
      WebkitBackdropFilter: 'blur(20px) saturate(145%)',
      border: '0.0625rem solid var(--vibe-toolbar-border)',
      borderRadius: '0.75rem',
      overflow: 'hidden',
      boxShadow: 'var(--vibe-toolbar-shadow)',
      animation: 'slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
    }}>
      <div style={{ padding: 8, borderBottom: '0.0625rem solid var(--vibe-toolbar-border)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>{t(locale, 'screenshot.newScreenshot')}</span>
        <button className="btn btn-ghost" style={{ fontSize: 14, padding: '0 4px' }} onClick={() => setVisible(false)}><X size={14} /></button>
      </div>
      <div style={{ padding: 8, display: 'flex', gap: 6 }}>
        <button className="btn btn-primary" style={{ flex: 1, fontSize: 11 }}>{t(locale, 'screenshot.sendToTerminal')}</button>
        <button className="btn btn-ghost" style={{ flex: 1, fontSize: 11 }}>{t(locale, 'screenshot.saveToMaterial')}</button>
        <button className="btn btn-ghost" style={{ flex: 1, fontSize: 11 }}>{t(locale, 'screenshot.annotate')}</button>
      </div>
    </div>,
    root
  );
}
