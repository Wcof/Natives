'use client';

import { useState, useEffect } from 'react';
import { Check, Loader2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { createPortal } from 'react-dom';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface ReleaseCheckItem {
  label: string;
  ok: boolean;
  message: string;
}

export default function ReleaseWizardDialog({ onClose }: { onClose?: () => void }) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [version, setVersion] = useState('');
  const [mounted, setMounted] = useState(false);
  const [checks, setChecks] = useState<ReleaseCheckItem[]>([
    { label: 'package.json', ok: false, message: t(locale, 'common.loading') },
    { label: 'Git Status', ok: false, message: t(locale, 'common.loading') },
    { label: 'CHANGELOG.md', ok: false, message: t(locale, 'common.loading') },
    { label: 'gh CLI', ok: false, message: t(locale, 'common.loading') },
  ]);

  useEffect(() => {
    setMounted(true);
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  if (!mounted) return null;
  const root = document.getElementById('content-overlay-root');
  if (!root) return null;

  return createPortal(
    <div style={{
      position: 'absolute', inset: 0, zIndex: 50,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      background: 'rgba(0,0,0,0.45)',
      backdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))',
      WebkitBackdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))',
      animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
    }} onClick={(e) => { if (e.target === e.currentTarget) onClose?.(); }}>
      <div style={{
        width: 400, background: 'var(--vibe-toolbar-bg)',
        backdropFilter: 'blur(var(--vibe-sidebar-blur, 28px)) saturate(var(--vibe-sidebar-saturation, 145%))',
        WebkitBackdropFilter: 'blur(var(--vibe-sidebar-blur, 28px)) saturate(var(--vibe-sidebar-saturation, 145%))',
        border: '0.0625rem solid var(--vibe-toolbar-border)',
        borderRadius: 'var(--radius)',
        boxShadow: 'var(--vibe-toolbar-shadow)',
        padding: SPACING.xl,
        animation: 'dropIn 0.16s cubic-bezier(0.2, 0.7, 0.3, 1)',
      }}>
        <div style={{ fontSize: FONT_SIZE.xl, fontWeight: 600, color: 'var(--vibe-brand-text)', marginBottom: SPACING.lg }}>
          {t(locale, 'release.title')}
        </div>

        {/* Version input */}
        <div style={{ marginBottom: SPACING.md }}>
          <label style={{ fontSize: FONT_SIZE.sm, color: 'var(--vibe-btn-text)', display: 'block', marginBottom: SPACING.xs }}>{t(locale, 'release.newVersion')}</label>
          <input className="input" type="text" placeholder="1.2.0" value={version} onChange={(e) => setVersion(e.target.value)} style={{ width: '100%' }} />
        </div>

        {/* Checklist */}
        <div style={{ marginBottom: SPACING.md }}>
          {checks.map((c, i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, padding: `${SPACING.xs}px 0`, fontSize: FONT_SIZE.md, color: 'var(--vibe-btn-text)' }}>
              <span style={{display:'inline-flex'}}>{c.ok ? <Check size={12} style={{color:'var(--diff-add)'}} /> : <Loader2 size={12} className='anim-livePulse' />}</span>
              <span>{c.label}</span>
              <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)' }}>{c.message}</span>
            </div>
          ))}
        </div>

        {/* Actions */}
        <div style={{ display: 'flex', gap: SPACING.xs }}>
          <button className="btn btn-primary" style={{ flex: 1, fontSize: FONT_SIZE.md }} disabled={!version}>
            {t(locale, 'release.runChecks')}
          </button>
          <button className="btn btn-ghost" style={{ fontSize: FONT_SIZE.md }} onClick={onClose}>{t(locale, 'common.cancel')}</button>
        </div>
      </div>
    </div>,
    root,
  );
}
