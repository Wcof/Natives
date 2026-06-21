'use client';

import { startTransition, useState, useEffect } from 'react';
import { Package, Upload, Check, AlertCircle } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { t, type Locale } from '@/i18n';
import Modal from '@/components/ui/Modal';

// ── Types ──

interface ReleaseWizardDialogProps {
  locale: Locale;
  isOpen: boolean;
  onClose: () => void;
}

type WizardStep = 'info' | 'assets' | 'publish' | 'done';

interface ReleaseInfo {
  version: string;
  releaseNotes: string;
  platform: 'all' | 'mac' | 'win' | 'linux';
}

// ── Component ──

export default function ReleaseWizardDialog({ locale, isOpen, onClose }: ReleaseWizardDialogProps) {
  const [step, setStep] = useState<WizardStep>('info');
  const [releaseInfo, setReleaseInfo] = useState<ReleaseInfo>({
    version: '',
    releaseNotes: '',
    platform: 'all',
  });
  const [publishing, setPublishing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState(0);

  // Reset state when dialog opens
  useEffect(() => {
    if (isOpen) {
      startTransition(() => { setStep('info'); });
    // eslint-disable-next-line react-hooks/set-state-in-effect
      setReleaseInfo({ version: '', releaseNotes: '', platform: 'all' });
      setPublishing(false);
      setError(null);
      setProgress(0);
    }
  }, [isOpen]);

  const handlePublish = async () => {
    setPublishing(true);
    setError(null);
    setStep('publish');

    // Simulate publish progress
    for (let i = 0; i <= 100; i += 10) {
      await new Promise(resolve => setTimeout(resolve, 200));
      setProgress(i);
    }

    setStep('done');
    setPublishing(false);
  };

  const handleClose = () => {
    if (!publishing) {
      onClose();
    }
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={handleClose}
      title={t(locale, 'release_wizard') || 'Release Wizard'}
      width={480}
      showCloseButton={!publishing}
      closeOnBackdropClick={!publishing}
      closeOnEscape={!publishing}
      contentClassName="!p-0 flex flex-col min-h-0 overflow-hidden"
    >
      {/* Content */}
      <div className="flex-1 overflow-y-auto p-5">
        {/* Step: Info */}
        {step === 'info' && (
          <div className="space-y-4">
            <div>
              <label className="mb-1.5 block text-xs font-medium text-[var(--vibe-btn-text)]">
                {t(locale, 'version') || 'Version'}
              </label>
              <input
                type="text"
                value={releaseInfo.version}
                onChange={e => setReleaseInfo(prev => ({ ...prev, version: e.target.value }))}
                placeholder="1.0.0"
                className="input w-full"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-medium text-[var(--vibe-btn-text)]">
                {t(locale, 'release_notes') || 'Release Notes'}
              </label>
              <textarea
                value={releaseInfo.releaseNotes}
                onChange={e => setReleaseInfo(prev => ({ ...prev, releaseNotes: e.target.value }))}
                placeholder={t(locale, 'release_notes_placeholder') || 'Describe what changed...'}
                rows={4}
                className="input w-full resize-none"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-medium text-[var(--vibe-btn-text)]">
                {t(locale, 'target_platform') || 'Target Platform'}
              </label>
              <div className="flex gap-2">
                {(['all', 'mac', 'win', 'linux'] as const).map(p => (
                  <button
                    key={p}
                    onClick={() => setReleaseInfo(prev => ({ ...prev, platform: p }))}
                    className={`btn ${releaseInfo.platform === p ? 'btn-primary' : ''}`}
                  >
                    {p === 'all' ? 'All' : p === 'mac' ? 'macOS' : p === 'win' ? 'Windows' : 'Linux'}
                  </button>
                ))}
              </div>
            </div>

            {error && (
              <div className="flex items-center gap-2 rounded-lg bg-red-500/10 px-3 py-2 text-xs text-red-400">
                <AlertCircle size={14} />
                {error}
              </div>
            )}
          </div>
        )}

        {/* Step: Assets */}
        {step === 'assets' && (
          <div className="space-y-4">
            <div className="rounded-lg border border-dashed border-[var(--border)] bg-[var(--bg-2)] p-8 text-center">
              <Upload size={24} className="mx-auto mb-2 text-[var(--text-faint)]" />
              <p className="text-xs text-[var(--text-dim)]">
                {t(locale, 'assets_auto_detected') || 'Build assets will be auto-detected'}
              </p>
              <p className="mt-1 text-[10px] text-[var(--text-faint)]">
                {releaseInfo.platform === 'all'
                  ? 'macOS (.dmg) · Windows (.exe) · Linux (.AppImage)'
                  : releaseInfo.platform === 'mac'
                    ? 'macOS (.dmg)'
                    : releaseInfo.platform === 'win'
                      ? 'Windows (.exe)'
                      : 'Linux (.AppImage)'}
              </p>
            </div>
          </div>
        )}

        {/* Step: Publish */}
        {step === 'publish' && (
          <div className="space-y-4 py-4 text-center">
            <MathCurveLoader size={48} />
            <p className="text-sm text-[var(--text)]">
              {t(locale, 'publishing') || 'Publishing...'}
            </p>
            <div className="mx-auto h-1.5 w-48 overflow-hidden rounded-full bg-[var(--bg-3)]">
              <div
                className="h-full rounded-full bg-[var(--accent)] transition-all duration-300"
                style={{ width: `${progress}%` }}
              />
            </div>
            <p className="text-xs text-[var(--text-dim)]">{progress}%</p>
          </div>
        )}

        {/* Step: Done */}
        {step === 'done' && (
          <div className="space-y-4 py-6 text-center">
            <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-green-500/10">
              <Check size={24} className="text-green-400" />
            </div>
            <div>
              <p className="text-sm font-medium text-[var(--text)]">
                {t(locale, 'release_published') || 'Release Published'}
              </p>
              <p className="mt-1 text-xs text-[var(--text-dim)]">
                v{releaseInfo.version}
              </p>
            </div>
          </div>
        )}
      </div>

      {/* Footer */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'end', gap: '8px', borderTop: '0.0625rem solid var(--border)', padding: '12px 20px', background: 'var(--bg-2)' }}>
        {step === 'info' && (
          <>
            <button onClick={handleClose} className="btn btn-ghost">
              {t(locale, 'cancel') || 'Cancel'}
            </button>
            <button
              onClick={() => setStep('assets')}
              disabled={!releaseInfo.version.trim()}
              className="btn btn-primary"
              style={{ opacity: !releaseInfo.version.trim() ? 0.4 : 1 }}
            >
              {t(locale, 'next') || 'Next'}
            </button>
          </>
        )}

        {step === 'assets' && (
          <>
            <button onClick={() => setStep('info')} className="btn btn-ghost">
              {t(locale, 'back') || 'Back'}
            </button>
            <button onClick={handlePublish} className="btn btn-primary">
              {t(locale, 'publish') || 'Publish'}
            </button>
          </>
        )}

        {step === 'done' && (
          <button onClick={handleClose} className="btn btn-primary">
            {t(locale, 'close') || 'Close'}
          </button>
        )}
      </div>
    </Modal>
  );
}
