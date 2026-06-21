'use client';

import { useState, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { Image, Terminal, FolderOpen, X } from 'lucide-react';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { useHydrated } from '@/hooks/useHydrated';

interface ScreenshotCardProps {
  locale: Locale;
  onSendToTerminal: (filePath: string) => void;
  onSaveToMaterial: (filePath: string) => void;
  onAnnotate: (filePath: string) => void;
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

  const handleSave = useCallback(() => {
    if (filePath) onSaveToMaterial(filePath);
    setVisible(false);
  }, [filePath, onSaveToMaterial]);

  const handleAnnotate = useCallback(() => {
    if (filePath) onAnnotate(filePath);
    setVisible(false);
  }, [filePath, onAnnotate]);

  if (!visible || !filePath) return null;

  if (!mounted) return null;
  const root = document.getElementById('content-overlay-root');
  if (!root) return null;

  return createPortal(
    (
      <div
        style={{
          position: 'absolute',
          bottom: 20,
          right: 20,
          zIndex: 9999,
          background: 'var(--vibe-toolbar-bg)',
          backdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
          WebkitBackdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
          border: '0.0625rem solid var(--vibe-toolbar-border)',
          borderRadius: '0.75rem',
          padding: `${SPACING.md}px ${SPACING.lg}px`,
          boxShadow: 'var(--vibe-toolbar-shadow)',
          minWidth: 220,
          animation: 'slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
          <Image size={18} style={{ color: 'var(--accent)' }} />
          <span style={{ fontSize: FONT_SIZE.md, color: 'var(--vibe-brand-text)', flex: 1 }}>
            {t(locale, 'screenshot.newScreenshot')}
          </span>
          <button
            onClick={() => { setVisible(false); onDismiss(); }}
            style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--vibe-btn-text)', padding: 2 }}
          >
            <X size={14} />
          </button>
        </div>

        <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--vibe-btn-text)', marginBottom: SPACING.sm, wordBreak: 'break-all' }}>
          {filePath.split('/').pop()}
        </div>

        <div style={{ display: 'flex', gap: 6 }}>
          <button className="btn btn-sm" onClick={handleSend} title={t(locale, 'screenshot.sendToTerminal')}>
            <Terminal size={14} /> {t(locale, 'screenshot.sendToTerminal')}
          </button>
          <button className="btn btn-sm" onClick={handleSave} title={t(locale, 'screenshot.saveToMaterial')}>
            <FolderOpen size={14} /> {t(locale, 'screenshot.saveToMaterial')}
          </button>
          <button className="btn btn-sm btn-primary" onClick={handleAnnotate} title={t(locale, 'screenshot.annotate')}>
            <Image size={14} /> {t(locale, 'screenshot.annotate')}
          </button>
        </div>
      </div>
    ),
    root
  );
}
