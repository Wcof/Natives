'use client';

import { useState, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { Image, Terminal, FolderOpen, X } from 'lucide-react';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { useHydrated } from '@/hooks/useHydrated';

interface ScreenshotCardProps {
  locale: Locale;
  onSendToTerminal: (filePath: string) => void;
  onSaveToMaterial: (filePath: string) => Promise<boolean>;
  onAnnotate: (filePath: string) => Promise<boolean>;
  onDismiss: () => void;
}

export default function ScreenshotCard({
  locale,
  onSendToTerminal,
  onSaveToMaterial,
  onAnnotate,
  onDismiss,
}: ScreenshotCardProps) {
  const [filePath, setFilePath] = useState<string | null>(null);
  const [visible, setVisible] = useState(false);
  const [actionPending, setActionPending] = useState(false);
  const mounted = useHydrated();

  

  useEffect(() => {
    const api = window.nativesAPI;
    if (!api?.screenshot?.watch) return;

    const unregister = api.screenshot.watch((path: string) => {
      setFilePath(path);
      setVisible(true);
    });

    return () => unregister();
  }, []);

  const handleSend = useCallback(() => {
    if (filePath) onSendToTerminal(filePath);
    setVisible(false);
  }, [filePath, onSendToTerminal]);

  const handleSave = useCallback(async () => {
    if (!filePath || actionPending) return;
    setActionPending(true);
    try {
      if (await onSaveToMaterial(filePath)) setVisible(false);
    } finally {
      setActionPending(false);
    }
  }, [actionPending, filePath, onSaveToMaterial]);

  const handleAnnotate = useCallback(async () => {
    if (!filePath || actionPending) return;
    setActionPending(true);
    try {
      if (await onAnnotate(filePath)) setVisible(false);
    } finally {
      setActionPending(false);
    }
  }, [actionPending, filePath, onAnnotate]);

  if (!visible || !filePath) return null;

  if (!mounted) return null;
  const root = document.getElementById('content-overlay-root');
  if (!root) return null;

  return createPortal(
    (
      <div
        style={{
          position: 'absolute', bottom: 20, right: 20, zIndex: 9999,
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-md)',
          padding: `${SPACING.md}px ${SPACING.lg}px`,
          boxShadow: 'var(--shadow-popup)',
          minWidth: 220,
          animation: 'slideUp 200ms ease',
          pointerEvents: 'auto',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
          <Image size={18} style={{ color: 'var(--primary)' }} />
          <span style={{ fontSize: FONT_SIZE.md, color: 'var(--text)', flex: 1 }}>
            {t(locale, 'screenshot.newScreenshot')}
          </span>
          <button
            onClick={() => { setVisible(false); onDismiss(); }}
            style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-secondary)', padding: 2 }}
          >
            <X size={14} />
          </button>
        </div>

        <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', marginBottom: SPACING.sm, wordBreak: 'break-all' }}>
          {filePath.split('/').pop()}
        </div>

        <div style={{ display: 'flex', gap: 6 }}>
          <button className="btn btn-sm" onClick={handleSend} title={t(locale, 'screenshot.sendToTerminal')}>
            <Terminal size={14} /> {t(locale, 'screenshot.sendToTerminal')}
          </button>
          <button className="btn btn-sm" onClick={() => void handleSave()} disabled={actionPending} title={t(locale, 'screenshot.saveToMaterial')}>
            <FolderOpen size={14} /> {actionPending ? t(locale, 'screenshot.saving') : t(locale, 'screenshot.saveToMaterial')}
          </button>
          <button className="btn btn-sm btn-primary" onClick={() => void handleAnnotate()} disabled={actionPending} title={t(locale, 'screenshot.annotate')}>
            <Image size={14} /> {t(locale, 'screenshot.annotate')}
          </button>
        </div>
      </div>
    ),
    root
  );
}
