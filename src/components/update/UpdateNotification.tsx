'use client';

import { useState, useEffect, useCallback, useRef } from 'react';
import { Download, X, Bell } from 'lucide-react';
import { t, type Locale } from '@/i18n';

interface UpdateNotificationProps {
  locale: Locale;
}

interface UpdateInfo {
  latestVersion: string;
  releaseUrl: string;
  publishedAt: string;
  body: string;
}

export default function UpdateNotification({ locale }: UpdateNotificationProps) {
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [checked, setChecked] = useState(false);
  const [checking, setChecking] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const checkForUpdate = useCallback(async () => {
    setChecking(true);
    try {
      const api = window.nativesAPI;
      if (!api?.update?.check) return;

      const result = await api.update.check();
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
    // Initial check after 6 second delay
    const initialTimer = setTimeout(() => {
      checkForUpdate();
    }, 6000);

    // Periodic check every 2 hours
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
    <>
      {/* Bell icon indicator when update available */}
      <div
        style={{
          position: 'fixed',
          bottom: 40,
          right: 20,
          zIndex: 9998,
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 12,
          padding: '12px 16px',
          boxShadow: '0 8px 32px rgba(0,0,0,0.4)',
          maxWidth: 300,
          animation: 'slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
          <Bell size={16} style={{ color: 'var(--accent)' }} />
          <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', flex: 1 }}>
            {t(locale, 'update.newVersionAvailable').replace('{version}', update.latestVersion)}
          </span>
          <button
            onClick={handleDismiss}
            style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-faint)', padding: 2 }}
          >
            <X size={14} />
          </button>
        </div>

        <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 10, maxHeight: 60, overflow: 'hidden' }}>
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
    </>
  );
}
