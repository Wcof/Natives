'use client';

import { useState, useEffect, useCallback, useRef } from 'react';
import { Download, X, Bell } from 'lucide-react';
import { t, type Locale } from '@/i18n';

interface UpdateInfo {
  latestVersion: string;
  releaseUrl: string;
  publishedAt: string;
  body: string;
}

export default function UpdateNotification({ onClose }: { onClose?: () => void }) {
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [checked, setChecked] = useState(false);
  const [checking, setChecking] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [locale, setLocale] = useState<Locale>('zh');
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  const checkForUpdate = useCallback(async () => {
    setChecking(true);
    try {
      const api = window.nativesAPI;
      if (!api?.update?.check) return;

      const result = await api.update.check() as UpdateInfo | null;
      if (result) {
        setUpdate(result);
        setDismissed(false);
      } else {
        setUpdate(null);
      }
    } catch {
      // Silently fail
    } finally {
      setChecked(true);
      setChecking(false);
    }
  }, []);

  // Check on mount + periodic check
  useEffect(() => {
    const initialTimer = setTimeout(() => {
      checkForUpdate();
    }, 6000);

    intervalRef.current = setInterval(() => {
      checkForUpdate();
    }, 2 * 60 * 60 * 1000);

    return () => {
      clearTimeout(initialTimer);
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [checkForUpdate]);

  const handleMute = useCallback(async () => {
    if (!update) return;
    try {
      const api = window.nativesAPI;
      if (api?.update?.mute) {
        await api.update.mute(update.latestVersion);
      }
    } catch {
      // Silently fail
    }
    setDismissed(true);
  }, [update]);

  const handleDismiss = useCallback(() => {
    setDismissed(true);
  }, []);

  if (!update || dismissed) return null;

  return (
    <div style={{
      position: 'fixed', bottom: 20, right: 20, zIndex: 1000,
      padding: '12px 16px', background: 'var(--vibe-toolbar-bg)',
      backdropFilter: 'blur(20px) saturate(145%)',
      WebkitBackdropFilter: 'blur(20px) saturate(145%)',
      border: '1px solid var(--accent)', borderRadius: '0.75rem', maxWidth: 320,
      boxShadow: 'var(--vibe-toolbar-shadow)',
      animation: 'slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
        <Bell size={16} style={{ color: 'var(--accent)' }} />
        <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--vibe-brand-text)', flex: 1 }}>
          {t(locale, 'update.newVersionAvailable').replace('{version}', update.latestVersion)}
        </span>
        <button
          onClick={handleDismiss}
          style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--vibe-btn-text)', padding: 2 }}
        >
          <X size={14} />
        </button>
      </div>
      <div style={{ fontSize: 11, color: 'var(--vibe-btn-text)', marginBottom: 10, maxHeight: 60, overflow: 'hidden' }}>
        {update.body.slice(0, 150)}{update.body.length > 150 ? '...' : ''}
      </div>
      <div style={{ display: 'flex', gap: 6 }}>
        <a
          href={update.releaseUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="btn btn-primary btn-sm"
          style={{ textDecoration: 'none', display: 'inline-flex', alignItems: 'center', gap: 4 }}
        >
          <Download size={12} /> {t(locale, 'update.download')}
        </a>
        <button className="btn btn-sm" onClick={handleMute}>
          {t(locale, 'update.muteVersion')}
        </button>
      </div>
    </div>
  );
}
