'use client';

import { useState, useEffect, useCallback } from 'react';
import { Image, Terminal, FolderOpen, X } from 'lucide-react';
import { t, type Locale } from '@/i18n';

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

  return (
    <div
      style={{
        position: 'fixed',
        bottom: 80,
        right: 20,
        zIndex: 9999,
        background: 'var(--surface)',
        border: '1px solid var(--border)',
        borderRadius: 12,
        padding: '12px 16px',
        boxShadow: '0 8px 32px rgba(0,0,0,0.4)',
        minWidth: 220,
        animation: 'slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 }}>
        <Image size={18} style={{ color: 'var(--accent)' }} />
        <span style={{ fontSize: 12, color: 'var(--text)', flex: 1 }}>
          {t(locale, 'screenshot.newScreenshot')}
        </span>
        <button
          onClick={() => { setVisible(false); onDismiss(); }}
          style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-faint)', padding: 2 }}
        >
          <X size={14} />
        </button>
      </div>

      <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 10, wordBreak: 'break-all' }}>
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
  );
}
