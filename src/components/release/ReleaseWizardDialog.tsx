'use client';

import { useState, useEffect } from 'react';
import { X, Package, Upload, Check, Loader2, AlertCircle } from 'lucide-react';
import { t, type Locale } from '@/i18n';

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
      setStep('info');
      setReleaseInfo({ version: '', releaseNotes: '', platform: 'all' });
      setPublishing(false);
      setError(null);
      setProgress(0);
    }
  }, [isOpen]);

  if (!isOpen) return null;

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
    <div className="fixed inset-0 z-60 flex items-center justify-center">
      {/* Backdrop — Glass overlay */}
      <div
        className="absolute inset-0"
        onClick={handleClose}
        style={{
          background: 'rgba(0,0,0,0.45)',
          backdropFilter: 'blur(24px) saturate(150%)',
          WebkitBackdropFilter: 'blur(24px) saturate(150%)',
          animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
        }}
      />

      {/* Dialog */}
      <div
        className="relative z-10 w-full max-w-lg"
        style={{
          background: 'var(--vibe-toolbar-bg)',
          backdropFilter: 'blur(28px) saturate(145%)',
          WebkitBackdropFilter: 'blur(28px) saturate(145%)',
          border: '0.0625rem solid var(--vibe-toolbar-border)',
          borderRadius: '1rem',
          boxShadow: 'var(--vibe-toolbar-shadow)',
          animation: 'dropIn 0.16s cubic-bezier(0.2, 0.7, 0.3, 1)',
        }}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--vibe-toolbar-border)] px-5 py-4">
          <div className="flex items-center gap-2">
            <Package size={18} className="text-[var(--accent)]" />
            <h2 className="text-sm font-semibold text-[var(--vibe-brand-text)]">
              {t(locale, 'release_wizard') || 'Release Wizard'}
            </h2>
          </div>
          <button
            onClick={handleClose}
            disabled={publishing}
            className="rounded-md p-1.5 text-[var(--vibe-btn-text)] hover:bg-[var(--vibe-btn-bg)] hover:text-[var(--vibe-brand-text)] disabled:opacity-50"
          >
            <X size={16} />
          </button>
        </div>

        {/* Content */}
        <div className="p-5">
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
                  className="w-full rounded-lg border border-[var(--vibe-toolbar-border)] bg-[var(--vibe-btn-bg)] px-3 py-2 text-sm text-[var(--vibe-brand-text)] outline-none focus:border-[var(--accent)]"
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
                  className="w-full resize-none rounded-lg border border-[var(--vibe-toolbar-border)] bg-[var(--vibe-btn-bg)] px-3 py-2 text-sm text-[var(--vibe-brand-text)] outline-none focus:border-[var(--accent)]"
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
                      className={`rounded-lg px-3 py-1.5 text-xs font-medium transition-colors ${
                        releaseInfo.platform === p
                          ? 'bg-[var(--accent)] text-[var(--accent-ink)]'
                          : 'bg-[var(--vibe-btn-bg)] text-[var(--vibe-btn-text)] hover:text-[var(--vibe-brand-text)]'
                      }`}
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
              <div className="rounded-lg border border-dashed border-[var(--vibe-toolbar-border)] bg-[var(--vibe-btn-bg)] p-8 text-center">
                <Upload size={24} className="mx-auto mb-2 text-[var(--vibe-btn-text)]" />
                <p className="text-xs text-[var(--vibe-btn-text)]">
                  {t(locale, 'assets_auto_detected') || 'Build assets will be auto-detected'}
                </p>
                <p className="mt-1 text-[10px] text-[var(--vibe-btn-text)]">
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
              <Loader2 size={32} className="mx-auto animate-spin text-[var(--accent)]" />
              <p className="text-sm text-[var(--vibe-brand-text)]">
                {t(locale, 'publishing') || 'Publishing...'}
              </p>
              <div className="mx-auto h-1.5 w-48 overflow-hidden rounded-full bg-[var(--vibe-btn-bg)]">
                <div
                  className="h-full rounded-full bg-[var(--accent)] transition-all duration-300"
                  style={{ width: `${progress}%` }}
                />
              </div>
              <p className="text-xs text-[var(--vibe-btn-text)]">{progress}%</p>
            </div>
          )}

          {/* Step: Done */}
          {step === 'done' && (
            <div className="space-y-4 py-6 text-center">
              <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-green-500/10">
                <Check size={24} className="text-green-400" />
              </div>
              <div>
                <p className="text-sm font-medium text-[var(--vibe-brand-text)]">
                  {t(locale, 'release_published') || 'Release Published'}
                </p>
                <p className="mt-1 text-xs text-[var(--vibe-btn-text)]">
                  v{releaseInfo.version}
                </p>
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 border-t border-[var(--vibe-toolbar-border)] px-5 py-3">
          {step === 'info' && (
            <>
              <button
                onClick={handleClose}
                className="rounded-lg px-4 py-2 text-xs font-medium text-[var(--vibe-btn-text)] hover:text-[var(--vibe-brand-text)]"
              >
                {t(locale, 'cancel') || 'Cancel'}
              </button>
              <button
                onClick={() => setStep('assets')}
                disabled={!releaseInfo.version.trim()}
                className="rounded-lg bg-[var(--accent)] px-4 py-2 text-xs font-medium text-[var(--accent-ink)] transition-opacity hover:opacity-90 disabled:opacity-40"
              >
                {t(locale, 'next') || 'Next'}
              </button>
            </>
          )}

          {step === 'assets' && (
            <>
              <button
                onClick={() => setStep('info')}
                className="rounded-lg px-4 py-2 text-xs font-medium text-[var(--vibe-btn-text)] hover:text-[var(--vibe-brand-text)]"
              >
                {t(locale, 'back') || 'Back'}
              </button>
              <button
                onClick={handlePublish}
                className="rounded-lg bg-[var(--accent)] px-4 py-2 text-xs font-medium text-[var(--accent-ink)] transition-opacity hover:opacity-90"
              >
                {t(locale, 'publish') || 'Publish'}
              </button>
            </>
          )}

          {step === 'done' && (
            <button
              onClick={handleClose}
              className="rounded-lg bg-[var(--accent)] px-4 py-2 text-xs font-medium text-[var(--accent-ink)] transition-opacity hover:opacity-90"
            >
              {t(locale, 'close') || 'Close'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
