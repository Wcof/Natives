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
    // Poll for new notifications every 30s
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
    // Mark all as read (soft clear)
    await handleMarkAllRead();
  };

  const levelColors: Record<string, string> = {
    info: 'var(--accent,#cdf24b)',
    warning: 'var(--warning)',
    error: 'var(--danger)',
  };

  const unreadCount = (notifications ?? []).filter((n) => !n.read).length;

  if (loading) {
    return (
      <div style={{ padding: '40px 16px', textAlign: 'center', color: 'var(--text-faint)', fontSize: 13 }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  return (
    <div>
      {/* Header with actions */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '0 0 12px', borderBottom: '1px solid var(--border,#262920)', marginBottom: 8,
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 12, color: 'var(--text-dim)', fontWeight: 600 }}>
            {t(locale, 'notifications.title')}
          </span>
          {unreadCount > 0 && (
            <span style={{
              fontSize: 10, padding: '1px 5px', borderRadius: 3,
              background: 'var(--accent,#cdf24b)', color: 'var(--accent-ink,#0b0c0a)',
              fontWeight: 600,
            }}>
              {unreadCount}
            </span>
          )}
        </div>
        <div style={{ display: 'flex', gap: 4 }}>
          {unreadCount > 0 && (
            <button
              className="btn-ghost"
              onClick={handleMarkAllRead}
              style={{ fontSize: 10, color: 'var(--accent,#cdf24b)', padding: '2px 6px' }}
            >
              {t(locale, 'notifications.markAllRead')}
            </button>
          )}
          <button
            className="btn-ghost"
            onClick={handleClear}
            style={{ fontSize: 10, color: 'var(--text-faint)', padding: '2px 6px' }}
          >
            {t(locale, 'notifications.clear')}
          </button>
        </div>
      </div>

      {/* Notification list */}
      {(notifications ?? []).length === 0 ? (
        <EmptyState title={t(locale, 'notifications.empty')} />
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          {(notifications ?? []).map((notif) => (
            <div
              key={notif.id}
              style={{
                padding: '10px 0',
                borderBottom: '1px solid var(--border,#262920)',
                opacity: notif.read ? 0.5 : 1,
                transition: 'opacity 0.15s',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 8 }}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 2 }}>
                    <span style={{
                      width: 7, height: 7, borderRadius: '50%', flexShrink: 0,
                      background: levelColors[notif.level] || levelColors.info,
                    }} />
                    <span style={{
                      fontSize: 12, fontWeight: 600, color: 'var(--text)',
                      overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                    }}>
                      {notif.title}
                    </span>
                  </div>
                  {notif.body && (
                    <div style={{ fontSize: 11, color: 'var(--text-dim)', marginLeft: 13, lineHeight: 1.4 }}>
                      {notif.body}
                    </div>
                  )}
                  <div style={{ fontSize: 10, color: 'var(--text-faint)', marginLeft: 13, marginTop: 3 }}>
                    {notif.moduleId && (
                      <span style={{ marginRight: 8 }}>{notif.moduleId}</span>
                    )}
                    <span>{notif.createdAt}</span>
                  </div>
                </div>
                {!notif.read && (
                  <button
                    className="btn-ghost"
                    onClick={() => handleMarkRead(notif.id)}
                    style={{ fontSize: 10, color: 'var(--accent)', padding: '2px 6px', flexShrink: 0 }}
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
    <span style={{
      position: 'absolute', top: -4, right: -4,
      minWidth: 14, height: 14,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      background: 'var(--danger)', color: '#fff',
      fontSize: 9, fontWeight: 700, borderRadius: 7,
      padding: '0 3px',
    }}>
      {count > 99 ? '99+' : count}
    </span>
  );
}
