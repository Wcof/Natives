'use client';

import { useState, useEffect } from 'react';
import { Check, Loader2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { createPortal } from 'react-dom';

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
      backdropFilter: 'blur(24px) saturate(150%)',
      WebkitBackdropFilter: 'blur(24px) saturate(150%)',
      animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
    }} onClick={(e) => { if (e.target === e.currentTarget) onClose?.(); }}>
      <div style={{
        width: 400, background: 'var(--vibe-toolbar-bg)',
        backdropFilter: 'blur(28px) saturate(145%)',
        WebkitBackdropFilter: 'blur(28px) saturate(145%)',
        border: '0.0625rem solid var(--vibe-toolbar-border)',
        borderRadius: '1rem',
        boxShadow: 'var(--vibe-toolbar-shadow)',
        padding: 20,
        animation: 'dropIn 0.16s cubic-bezier(0.2, 0.7, 0.3, 1)',
      }}>
        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--vibe-brand-text)', marginBottom: 16 }}>
          {t(locale, 'release.title')}
        </div>

        {/* Version input */}
        <div style={{ marginBottom: 12 }}>
          <label style={{ fontSize: 11, color: 'var(--vibe-btn-text)', display: 'block', marginBottom: 4 }}>{t(locale, 'release.newVersion')}</label>
          <input className="input" type="text" placeholder="1.2.0" value={version} onChange={(e) => setVersion(e.target.value)} style={{ width: '100%' }} />
        </div>

        {/* Checklist */}
        <div style={{ marginBottom: 12 }}>
          {checks.map((c, i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '4px 0', fontSize: 12, color: 'var(--vibe-btn-text)' }}>
              <span style={{display:'inline-flex'}}>{c.ok ? <Check size={12} style={{color:'var(--diff-add)'}} /> : <Loader2 size={12} className='anim-livePulse' />}</span>
              <span>{c.label}</span>
              <span style={{ fontSize: 10, color: 'var(--vibe-btn-text)' }}>{c.message}</span>
            </div>
          ))}
        </div>

        {/* Actions */}
        <div style={{ display: 'flex', gap: 6 }}>
          <button className="btn btn-primary" style={{ flex: 1, fontSize: 12 }} disabled={!version}>
            {t(locale, 'release.runChecks')}
          </button>
          <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={onClose}>{t(locale, 'common.cancel')}</button>
        </div>
      </div>
    </div>,
    root,
  );
}
