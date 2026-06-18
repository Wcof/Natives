'use client';

import { useState, useEffect } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { t, type Locale } from '@/i18n';
import { EmptyState } from '@/components/ui/EmptyState';

interface Notification {
  id: number;
  moduleId?: string;
  title: string;
  body?: string;
  level: string;
  read: number;
  createdAt: string;
}

interface NotificationPanelProps {
  locale: Locale;
}

const levelDot: Record<string, string> = {
  info: 'var(--mac-green)',
  warning: 'var(--mac-yellow)',
  error: 'var(--mac-red)',
};

export default function NotificationPanel({ locale }: NotificationPanelProps) {
  const { data: notifications, loading, error, reload: loadNotifications } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (api?.notification?.list) {
      const list = await api.notification.list();
      if (Array.isArray(list)) return list as Notification[];
    }
    return [];
  }, []);

  useEffect(() => {
    loadNotifications();
    const interval = setInterval(loadNotifications, 30000);
    return () => clearInterval(interval);
  }, [loadNotifications]);

  const handleMarkRead = async (id: number) => {
    try {
      await window.nativesAPI?.notification?.markRead(id);
      loadNotifications();
    } catch (err) {
      console.error('[Notifications] Mark read failed:', err);
    }
  };

  const handleMarkAllRead = async () => {
    try {
      await window.nativesAPI?.notification?.markAllAsRead();
      loadNotifications();
    } catch (err) {
      console.error('[Notifications] Mark all read failed:', err);
    }
  };

  const handleClear = async () => {
    await handleMarkAllRead();
  };

  const unreadCount = (notifications ?? []).filter((n) => !n.read).length;

  if (loading) {
    return (
      <div className="flex items-center justify-center py-10 text-sm text-[var(--text-faint)]">
        {t(locale, 'common.loading')}
      </div>
    );
  }

  return (
    <div>
      {/* Header with actions */}
      <div className="flex items-center justify-between pb-3 mb-3 border-b border-[var(--vibe-border-subtle)]">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold text-[var(--text-dim)]">
            {t(locale, 'notifications.title')}
          </span>
          {unreadCount > 0 && (
            <span className="text-[0.625rem] font-semibold px-1.5 py-0.5 rounded bg-[var(--vibe-notif-badge)] text-white">
              {unreadCount}
            </span>
          )}
        </div>
        <div className="flex gap-1">
          {unreadCount > 0 && (
            <button
              onClick={handleMarkAllRead}
              className="text-[0.625rem] text-[var(--vibe-accent-color)] hover:text-[var(--vibe-active-color)] transition-colors px-1.5 py-0.5 rounded hover:bg-[var(--vibe-notif-hover)]"
            >
              {t(locale, 'notifications.markAllRead')}
            </button>
          )}
          <button
            onClick={handleClear}
            className="text-[0.625rem] text-[var(--text-faint)] hover:text-[var(--text-dim)] transition-colors px-1.5 py-0.5 rounded hover:bg-[var(--vibe-notif-hover)]"
          >
            {t(locale, 'notifications.clear')}
          </button>
        </div>
      </div>

      {/* Notification list */}
      {(notifications ?? []).length === 0 ? (
        <EmptyState title={t(locale, 'notifications.empty')} />
      ) : (
        <div className="flex flex-col">
          {(notifications ?? []).map((notif) => (
            <div
              key={notif.id}
              className="py-2.5 border-b border-[var(--vibe-border-subtle)] last:border-b-0 transition-opacity"
              style={{ opacity: notif.read ? 0.45 : 1 }}
            >
              <div className="flex justify-between items-start gap-2">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5 mb-0.5">
                    <span
                      className="w-1.5 h-1.5 rounded-full shrink-0"
                      style={{ background: levelDot[notif.level] || levelDot.info }}
                    />
                    <span className="text-xs font-semibold text-[var(--vibe-brand-text)] truncate">
                      {notif.title}
                    </span>
                  </div>
                  {notif.body && (
                    <div className="text-[0.6875rem] text-[var(--text-dim)] ml-[18px] leading-relaxed">
                      {notif.body}
                    </div>
                  )}
                  <div className="text-[0.625rem] text-[var(--text-faint)] ml-[18px] mt-0.5">
                    {notif.moduleId && <span className="mr-1.5">{notif.moduleId}</span>}
                    <span>{notif.createdAt}</span>
                  </div>
                </div>
                {!notif.read && (
                  <button
                    onClick={() => handleMarkRead(notif.id)}
                    className="text-[0.625rem] text-[var(--vibe-accent-color)] hover:text-[var(--vibe-active-color)] transition-colors px-1.5 py-0.5 rounded hover:bg-[var(--vibe-notif-hover)] shrink-0"
                  >
                    ✓
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Notification Badge (for sidebar/header) ──

export function NotificationBadge({ locale }: { locale: Locale }) {
  const [count, setCount] = useState(0);

  useEffect(() => {
    async function loadCount() {
      try {
        const list = await window.nativesAPI?.notification?.list?.(true);
        if (Array.isArray(list)) {
          setCount((list as Array<{ read?: number }>).filter((n) => !n.read).length);
        }
      } catch { /* ignore */ }
    }
    loadCount();
    const interval = setInterval(loadCount, 30000);
    return () => clearInterval(interval);
  }, []);

  if (count === 0) return null;

  return (
    <span className="absolute -top-1 -right-1 min-w-[14px] h-[14px] flex items-center justify-center bg-[var(--vibe-notif-badge)] text-white text-[9px] font-bold rounded-full px-[3px]">
      {count > 99 ? '99+' : count}
    </span>
  );
}
