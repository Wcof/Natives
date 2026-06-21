'use client';

import { useState, useEffect } from 'react';
import { Check } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import Modal from '@/components/ui/Modal';

interface ReleaseCheckItem {
  label: string;
  ok: boolean;
  message: string;
}

export default function ReleaseWizardDialog({ onClose }: { onClose?: () => void }) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [version, setVersion] = useState('');
  const [checks, setChecks] = useState<ReleaseCheckItem[]>([
    { label: 'package.json', ok: false, message: t(locale, 'common.loading') },
    { label: 'Git Status', ok: false, message: t(locale, 'common.loading') },
    { label: 'CHANGELOG.md', ok: false, message: t(locale, 'common.loading') },
    { label: 'gh CLI', ok: false, message: t(locale, 'common.loading') },
  ]);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  return (
    <Modal
      isOpen={true}
      onClose={onClose || (() => {})}
      title={t(locale, 'release.title')}
      width={400}
    >
      {/* Version input */}
      <div style={{ marginBottom: SPACING.md }}>
        <label style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)', display: 'block', marginBottom: SPACING.xs }}>
          {t(locale, 'release.newVersion')}
        </label>
        <input
          className="input"
          type="text"
          placeholder="1.2.0"
          value={version}
          onChange={(e) => setVersion(e.target.value)}
          style={{ width: '100%' }}
        />
      </div>

      {/* Checklist */}
      <div style={{ marginBottom: SPACING.md }}>
        {checks.map((c, i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, padding: `${SPACING.xs}px 0`, fontSize: FONT_SIZE.md, color: 'var(--text-dim)' }}>
            <span style={{ display: 'inline-flex' }}>
              {c.ok ? (
                <Check size={12} style={{ color: 'var(--diff-add)' }} />
              ) : (
                <MathCurveLoader size={12} strokeWidth={1} particleCount={6} />
              )}
            </span>
            <span>{c.label}</span>
            <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)' }}>{c.message}</span>
          </div>
        ))}
      </div>

      {/* Actions */}
      <div style={{ display: 'flex', gap: SPACING.xs, justifyContent: 'flex-end', marginTop: SPACING.lg }}>
        <button className="btn btn-ghost" style={{ fontSize: FONT_SIZE.md }} onClick={onClose}>
          {t(locale, 'common.cancel')}
        </button>
        <button className="btn btn-primary" style={{ flex: 1, fontSize: FONT_SIZE.md }} disabled={!version}>
          {t(locale, 'release.runChecks')}
        </button>
      </div>
    </Modal>
  );
}
