'use client';

import { useState, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { useHydrated } from '@/hooks/useHydrated';

export default function ScreenshotCard() {
  const [locale, setLocale] = useState<Locale>('zh');
  const [visible, setVisible] = useState(true);
  const mounted = useHydrated();

  

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
      backdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
      WebkitBackdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
      border: '0.0625rem solid var(--vibe-toolbar-border)',
      borderRadius: '0.75rem',
      overflow: 'hidden',
      boxShadow: 'var(--vibe-toolbar-shadow)',
      animation: 'slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
    }}>
      <div style={{ padding: SPACING.sm, borderBottom: '0.0625rem solid var(--vibe-toolbar-border)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>{t(locale, 'screenshot.newScreenshot')}</span>
        <button className="btn btn-ghost" style={{ fontSize: FONT_SIZE.xl, padding: `0 ${SPACING.xs}` }} onClick={() => setVisible(false)}><X size={14} /></button>
      </div>
      <div style={{ padding: SPACING.sm, display: 'flex', gap: SPACING.xs }}>
        <button className="btn btn-primary" style={{ flex: 1, fontSize: FONT_SIZE.sm }}>{t(locale, 'screenshot.sendToTerminal')}</button>
        <button className="btn btn-ghost" style={{ flex: 1, fontSize: FONT_SIZE.sm }}>{t(locale, 'screenshot.saveToMaterial')}</button>
        <button className="btn btn-ghost" style={{ flex: 1, fontSize: FONT_SIZE.sm }}>{t(locale, 'screenshot.annotate')}</button>
      </div>
    </div>,
    root
  );
}
