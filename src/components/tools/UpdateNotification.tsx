'use client';

import { useState, useEffect, useCallback, useRef } from 'react';
import { createPortal } from 'react-dom';
import { Download, X, Bell, Pause } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { useHydrated } from '@/hooks/useHydrated';

interface UpdateNotificationProps {
  locale: Locale;
}

interface UpdateInfo {
  currentVersion: string;
  latestVersion: string | null;
  updateAvailable: boolean;
  releaseUrl: string | null;
  publishedAt: string | null;
  body: string | null;
  sourceConfigured: boolean;
  message: string;
}

export default function UpdateNotification({ locale }: UpdateNotificationProps) {
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [checked, setChecked] = useState(false);
  const [checking, setChecking] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const mounted = useHydrated();

  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const checkForUpdate = useCallback(async () => {
    setChecking(true);
    try {
      const api = window.nativesAPI;
      if (!api?.update?.check) return;
      const result = (await api.update.check()) as UpdateInfo;
      if (result && result.updateAvailable) {
        setUpdate(result);
        setDismissed(false);
      } else {
        setUpdate(null);
        setChecked(true);
        setChecking(false);
        return;
      }
    } catch {
      // Silently fail
    } finally {
      setChecked(true);
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    const checkDismissed = async () => {
      try {
        const api = window.nativesAPI;
        if (!api?.update?.getDismissed) return;
        await api.update.getDismissed();
      } catch {
        // Ignore
      }
    };
    checkDismissed();
  }, []);

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

  const handleDismiss = useCallback(async () => {
    if (!update) return;
    try {
      const api = window.nativesAPI;
      if (api?.update?.dismiss) {
        await api.update.dismiss(update.latestVersion!);
      }
    } catch {
      // Silently fail
    }
    setDismissed(true);
    setUpdate(null);
  }, [update]);

  const handleMute = useCallback(async () => {
    if (!update) return;
    try {
      const api = window.nativesAPI;
      if (api?.update?.mute) {
        await api.update.mute(update.latestVersion!);
      }
    } catch {
      // Silently fail
    }
    setDismissed(true);
    setUpdate(null);
  }, [update]);

  if (!update || dismissed) return null;
  if (!mounted) return null;
  const root = document.getElementById('content-overlay-root');
  if (!root) return null;

  return createPortal(
    <>
      <div
        style={{
          position: 'absolute', bottom: 20, right: 20, zIndex: 9998,
          background: 'var(--vibe-toolbar-bg)',
          backdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
          WebkitBackdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
          border: '0.0625rem solid var(--vibe-toolbar-border)', borderRadius: '0.75rem',
          padding: '12px 16px', boxShadow: 'var(--vibe-toolbar-shadow)', maxWidth: 300,
          animation: 'slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
          <Bell size={16} style={{ color: 'var(--accent)' }} />
          <span style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--vibe-brand-text)', flex: 1 }}>
            {t(locale, 'update.newVersionAvailable').replace('{version}', update.latestVersion!)}
          </span>
          <button
            onClick={handleDismiss}
            title={t(locale, 'update.dismiss')}
            style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--vibe-btn-text)', padding: 2 }}
          >
            <X size={14} />
          </button>
        </div>
        <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--vibe-btn-text)', marginBottom: 10, maxHeight: 60, overflow: 'hidden' }}>
          {update.body ? (update.body.slice(0, 150) + (update.body.length > 150 ? '...' : '')) : t(locale, 'update.noReleaseNotes')}
        </div>
        <div style={{ display: 'flex', gap: SPACING.xs }}>
          <a
            href={update.releaseUrl!}
            target="_blank"
            rel="noopener noreferrer"
            className="btn btn-primary btn-sm"
            style={{ textDecoration: 'none', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          >
            <Download size={12} /> {t(locale, 'update.download')}
          </a>
          <button className="btn btn-sm" onClick={handleMute} title={t(locale, 'update.muteVersion')}>
            <Pause size={14} style={{ marginRight: 4 }} /> {t(locale, 'update.mute')}
          </button>
        </div>
      </div>
    </>,
    root
  );
}
