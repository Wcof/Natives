'use client';

import { useState, useEffect, useCallback } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { t, type Locale } from '@/i18n';
import { EmptyState, ErrorState } from '@/components/ui/EmptyState';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';

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
  const { toast } = useToast();
  const [markingAll, setMarkingAll] = useState(false);
  const [markingIds, setMarkingIds] = useState<Set<number>>(new Set());

  const { data: notifications, loading, error, reload: loadNotifications } = useAsyncData(async () => {
    const api = window.nativesAPI?.notification;
    if (!api?.list) {
      throw new Error('Notification API unavailable');
    }
    const list = await api.list();
    if (Array.isArray(list)) return list as Notification[];
    return [];
  }, []);

  const apiUnavailableMsg = t(locale, 'notifications.apiUnavailable');
  const isApiUnavailable = (rawMessage?: string) =>
    rawMessage?.includes('Notification API unavailable') ?? false;

  // ── Listen for db-state-changed to refresh ──

  useEffect(() => {
    loadNotifications();
  }, [loadNotifications]);

  useEffect(() => {
    const unsub = window.nativesAPI?.onDbStateChanged?.(
      (_event: unknown, channel: string) => {
        if (channel === 'notification') {
          loadNotifications();
        }
      },
    );
    return () => {
      if (unsub) unsub();
    };
  }, [loadNotifications]);

  // ── Handlers ──

  const handleMarkRead = useCallback(async (id: number) => {
    const api = window.nativesAPI?.notification;
    if (!api?.markRead) {
      toast(apiUnavailableMsg, 'error');
      return;
    }
    setMarkingIds((prev) => new Set(prev).add(id));
    try {
      await api.markRead(id);
      toast(t(locale, 'notifications.markReadSuccess'), 'success');
      loadNotifications();
    } catch (err) {
      const classified = classifyError(err);
      toast(classified.userMessage, 'error');
    } finally {
      setMarkingIds((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
    }
  }, [locale, toast, loadNotifications, apiUnavailableMsg]);

  const handleMarkAllRead = useCallback(async () => {
    const api = window.nativesAPI?.notification;
    if (!api?.markAllAsRead) {
      toast(apiUnavailableMsg, 'error');
      return;
    }
    setMarkingAll(true);
    try {
      await api.markAllAsRead();
      toast(t(locale, 'notifications.markAllReadSuccess'), 'success');
      loadNotifications();
    } catch (err) {
      const classified = classifyError(err);
      toast(classified.userMessage, 'error');
    } finally {
      setMarkingAll(false);
    }
  }, [locale, toast, loadNotifications, apiUnavailableMsg]);

  const unreadCount = (notifications ?? []).filter((n) => !n.read).length;

  // ── Loading state ──

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center py-12 gap-3 text-sm text-[var(--text-disabled)]">
        <MathCurveLoader size={40} />
        <span>{t(locale, 'common.loading')}</span>
      </div>
    );
  }

  // ── Error state ──

  if (error) {
    const errorMsg = isApiUnavailable(error.rawMessage)
      ? apiUnavailableMsg
      : error.userMessage;
    return <ErrorState message={errorMsg} onRetry={loadNotifications} />;
  }

  // ── Normal render ──

  return (
    <div>
      {/* Header with actions */}
      <div className="flex items-center justify-between pb-3 mb-3 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold text-[var(--text-secondary)]">
            {t(locale, 'notifications.title')}
          </span>
          {unreadCount > 0 && (
            <span className="text-[0.625rem] font-semibold px-1.5 py-0.5 rounded bg-[var(--danger)] text-white">
              {unreadCount}
            </span>
          )}
        </div>
        <div className="flex gap-1">
          {unreadCount > 0 && (
            <button
              onClick={handleMarkAllRead}
              disabled={markingAll}
              className="text-[0.625rem] text-[var(--primary)] hover:text-[var(--primary)] transition-colors px-1.5 py-0.5 rounded hover:bg-[var(--surface-hover)] disabled:opacity-40"
            >
              {t(locale, 'notifications.markAllRead')}
            </button>
          )}
        </div>
      </div>

      {/* Notification list */}
      {(notifications ?? []).length === 0 ? (
        <EmptyState
          title={t(locale, 'notifications.noNotifications')}
          description={t(locale, 'notifications.emptyDescription')}
        />
      ) : (
        <div className="flex flex-col">
          {(notifications ?? []).map((notif) => (
            <div
              key={notif.id}
              className="py-2.5 border-b border-[var(--border-subtle)] last:border-b-0 transition-opacity"
              style={{ opacity: notif.read ? 0.45 : 1 }}
            >
              <div className="flex justify-between items-start gap-2">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5 mb-0.5">
                    <span
                      className="w-1.5 h-1.5 rounded-full shrink-0"
                      style={{ background: levelDot[notif.level] || levelDot.info }}
                    />
                    <span className="text-xs font-semibold text-[var(--text)] truncate">
                      {notif.title}
                    </span>
                  </div>
                  {notif.body && (
                    <div className="text-[0.6875rem] text-[var(--text-secondary)] ml-[18px] leading-relaxed">
                      {notif.body}
                    </div>
                  )}
                  <div className="text-[0.625rem] text-[var(--text-disabled)] ml-[18px] mt-0.5">
                    {notif.moduleId && <span className="mr-1.5">{notif.moduleId}</span>}
                    <span>{notif.createdAt}</span>
                  </div>
                </div>
                {!notif.read && (
                  <button
                    onClick={() => handleMarkRead(notif.id)}
                    disabled={markingIds.has(notif.id)}
                    title={t(locale, 'notifications.markRead')}
                    aria-label={t(locale, 'notifications.markRead')}
                    className="text-[0.625rem] text-[var(--primary)] hover:text-[var(--primary)] transition-colors px-1.5 py-0.5 rounded hover:bg-[var(--surface-hover)] shrink-0 disabled:opacity-40"
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

export function NotificationBadge({ locale: _locale }: { locale: Locale }) {
  const [count, setCount] = useState(0);

  const loadCount = useCallback(async () => {
    try {
      const list = await window.nativesAPI?.notification?.list?.(true);
      if (Array.isArray(list)) {
        setCount((list as Array<{ read?: number }>).filter((n) => !n.read).length);
      }
    } catch {
      // Silently degrade to 0
      setCount(0);
    }
  }, []);

  useEffect(() => {
    loadCount();
  }, [loadCount]);

  useEffect(() => {
    const unsub = window.nativesAPI?.onDbStateChanged?.(
      (_event: unknown, channel: string) => {
        if (channel === 'notification') {
          loadCount();
        }
      },
    );
    return () => {
      if (unsub) unsub();
    };
  }, [loadCount]);

  if (count === 0) return null;

  return (
    <span className="absolute -top-1 -right-1 min-w-[14px] h-[14px] flex items-center justify-center bg-[var(--danger)] text-white text-[9px] font-bold rounded-full px-[3px]">
      {count > 99 ? '99+' : count}
    </span>
  );
}
